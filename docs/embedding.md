# embedding tein

Reference for embedding tein in a Rust application: context types, the builder API, the `Value` enum, calling Scheme from Rust, and custom I/O ports.

---

## context types

Three context types cover different deployment scenarios.

| | `Context` | `TimeoutContext` | `ThreadLocalContext` |
|---|---|---|---|
| thread safety | `!Send + !Sync` | `!Send + !Sync` | `Send + Sync` |
| timeout | step limit only | wall-clock deadline | step limit only |
| state | persists across calls | persists across calls | persistent or fresh |
| use case | single-threaded scripting | untrusted code with deadlines | shared across threads, servers |

`TimeoutContext` and `ThreadLocalContext` both run a `Context` on a dedicated thread and proxy requests over channels. `TimeoutContext` requires `step_limit` to be set — without it, the context thread cannot terminate after the timeout fires.

---

## constructors

Three ways to create a `Context`:

```rust
// primitive env — no (scheme base), just core Scheme special forms
let ctx = Context::new()?;

// full R7RS standard environment — equivalent to builder().standard_env().build()
let ctx = Context::new_standard()?;

// fluent builder for everything else
let ctx = Context::builder()
    .standard_env()
    .step_limit(100_000)
    .build()?;
```

---

## ContextBuilder API

`Context::builder()` returns a `ContextBuilder`. All methods take and return `self`, so calls chain. Call one of the `build*` terminal methods to produce a context.

### environment and limits

```rust
Context::builder()
    .standard_env()           // load (scheme base) + supporting modules from embedded VFS
    .step_limit(1_000_000)    // cap VM instructions per evaluate()/call(); returns Error::StepLimitExceeded
    .heap_size(8 * 1024 * 1024)   // initial heap (default 8 MiB)
    .heap_max(128 * 1024 * 1024)  // maximum heap (default 128 MiB)
    .build()?;
```

`standard_env()` is required for tein modules. Without it, `(import (tein json))` and similar will fail even if the module is in the allowlist.

### sandboxing

```rust
use tein::sandbox::Modules;

Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)             // module restriction + VFS gate
    .allow_module("tein/process")          // add a module + its transitive deps to the allowlist
    .file_read(&["/data/", "/config/"])   // allow reads under these absolute path prefixes
    .file_write(&["/tmp/output/"])        // allow writes under these absolute path prefixes
    .build()?;
```

`.sandboxed(modules)` requires `.standard_env()` — the full env must be loaded before restriction can be applied.

`.file_read()` and `.file_write()` auto-activate `sandboxed(Modules::Safe)` when no explicit `.sandboxed()` call precedes them. Paths are canonicalised before matching, so `..` traversals and symlinks are resolved.

See [sandboxing.md](sandboxing.md) for the full sandboxing model and `Modules` variants.

### terminal methods

```rust
// single-threaded context
let ctx: Context = builder.build()?;

// context with wall-clock timeout; requires step_limit
let ctx: TimeoutContext = builder
    .step_limit(1_000_000)
    .build_timeout(Duration::from_secs(5))?;

// managed context — persistent: state accumulates; reset() tears down and rebuilds
let ctx: ThreadLocalContext = builder.build_managed(|ctx| {
    ctx.evaluate("(define counter 0)")?;
    Ok(())
})?;

// managed context — fresh: context is rebuilt before every evaluation
let ctx: ThreadLocalContext = builder.build_managed_fresh(|ctx| {
    ctx.evaluate("(define x 42)")?;
    Ok(())
})?;
```

---

## Value enum

`Value` is the safe Rust representation of a Scheme value. Most variants own their data. `Procedure`, `Port`, and `HashTable` hold raw pointers that are only valid within the originating `Context` (enforced by `!Send + !Sync`).

| Scheme type | Rust variant | `Display` |
|---|---|---|
| exact integer (fixnum) | `Value::Integer(i64)` | `42` |
| inexact float (flonum) | `Value::Float(f64)` | `3.14` |
| arbitrary-precision integer | `Value::Bignum(String)` | `123456789012345678901234567890` |
| exact rational | `Value::Rational(Box<Value>, Box<Value>)` | `1/3` |
| complex number | `Value::Complex(Box<Value>, Box<Value>)` | `1+2i` |
| boolean | `Value::Boolean(bool)` | `#t` |
| string | `Value::String(String)` | `"hello"` |
| symbol | `Value::Symbol(String)` | `foo` |
| proper list | `Value::List(Vec<Value>)` | `(1 2 3)` |
| improper pair | `Value::Pair(Box<Value>, Box<Value>)` | `(a . b)` |
| vector | `Value::Vector(Vec<Value>)` | `#(1 2 3)` |
| nil / empty list | `Value::Nil` | `()` |
| unspecified (void) | `Value::Unspecified` | `` (empty) |
| character | `Value::Char(char)` | `#\a` |
| bytevector | `Value::Bytevector(Vec<u8>)` | `#u8(1 2 3)` |
| port (opaque) | `Value::Port(sexp)` | `#<port>` |
| hash table (opaque) | `Value::HashTable(sexp)` | `#<hash-table>` |
| procedure / builtin | `Value::Procedure(sexp)` | `#<procedure>` |
| foreign object | `Value::Foreign { handle_id, type_name }` | `#<counter:1>` |
| unrecognised type | `Value::Other(String)` | (debug string) |

`Value` implements `Display` for scheme-readable output, and `PartialEq` for value comparison.

### extraction helpers

All helpers return `Option<T>` or `bool`.

```rust
value.as_integer()       // Option<i64>
value.as_float()         // Option<f64>
value.as_bignum()        // Option<&str>
value.as_bool()          // Option<bool>
value.as_string()        // Option<&str>  — borrows from Value
value.as_symbol()        // Option<&str>  — borrows from Value
value.as_list()          // Option<&[Value]>  — borrows from Value
value.as_pair()          // Option<(&Value, &Value)>
value.as_vector()        // Option<&[Value]>
value.as_char()          // Option<char>
value.as_bytevector()    // Option<&[u8]>
value.as_rational()      // Option<(&Value, &Value)>
value.as_complex()       // Option<(&Value, &Value)>
value.as_procedure()     // Option<sexp>  — raw pointer, advanced use only

value.is_nil()           // bool
value.is_unspecified()   // bool
value.is_procedure()     // bool
value.is_foreign()       // bool
value.is_port()          // bool
value.is_hash_table()    // bool
value.is_char()          // bool
value.is_bytevector()    // bool
```

**Borrow note:** `as_string()`, `as_symbol()`, `as_list()`, `as_vector()`, and `as_bytevector()` borrow from the `Value`. Bind the `Value` to a named variable before calling them — you cannot call them on a temporary.

```rust
// correct
let val = ctx.evaluate(r#""hello""#)?;
let s = val.as_string().unwrap();

// compile error — temporary dropped while borrowed
let s = ctx.evaluate(r#""hello""#)?.as_string().unwrap();
```

### constructing values

`Value` variants can be constructed directly and passed to `ctx.call()`:

```rust
Value::Integer(42)
Value::Float(3.14)
Value::Boolean(true)
Value::String("hello".into())
Value::Symbol("my-sym".into())
Value::List(vec![Value::Integer(1), Value::Integer(2)])
Value::Nil
Value::Char('a')
Value::Bytevector(vec![0x01, 0x02, 0x03])
```

---

## evaluating Scheme code

```rust
let ctx = Context::new_standard()?;

// evaluate a string — returns the last expression's value
let result = ctx.evaluate("(+ 1 2 3)")?;
assert_eq!(result, Value::Integer(6));

// load a file
ctx.load_file("/path/to/script.scm")?;

// state persists across calls
ctx.evaluate("(define x 10)")?;
let result = ctx.evaluate("(* x 2)")?;
assert_eq!(result, Value::Integer(20));
```

---

## calling Scheme procedures from Rust

`ctx.call()` invokes a Scheme procedure with Rust-constructed arguments.

```rust
let ctx = Context::new_standard()?;

ctx.evaluate("(define (add a b) (+ a b))")?;
let add_fn = ctx.evaluate("add")?;

let result = ctx.call(&add_fn, &[Value::Integer(3), Value::Integer(4)])?;
assert_eq!(result, Value::Integer(7));
```

`call()` accepts any `Value::Procedure`. It returns `Error::TypeError` if the value is not a procedure, or `Error::EvalError` if the Scheme call raises an exception.

---

## custom ports

Custom ports bridge Rust `Read`/`Write` into Scheme's port system. Any type implementing `std::io::Read` or `std::io::Write` works.

### input ports

```rust
use std::io::Cursor;

let ctx = Context::new_standard()?;
let source = Cursor::new("(+ 1 2) (+ 3 4)");
let port = ctx.open_input_port(source)?;

// read one s-expression without evaluating
let expr = ctx.read(&port)?;
// => Value::List([Symbol("+"), Integer(1), Integer(2)])

// read and evaluate expressions until EOF; returns the last result
let result = ctx.evaluate_port(&port)?;
// => Value::Integer(7)
```

`ctx.read()` parses one datum from the port. `ctx.evaluate_port()` loops read+eval until EOF and returns the final result.

### output ports

```rust
use std::sync::{Arc, Mutex};

struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

let ctx = Context::new_standard()?;
let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
let port = ctx.open_output_port(SharedWriter(Arc::clone(&buf)))?;

// pass the port Value to Scheme's display/write/newline
let add_fn = ctx.evaluate("display")?;
ctx.call(&add_fn, &[Value::String("hello".into()), port])?;

let written = buf.lock().unwrap();
assert_eq!(&*written, b"hello");
```

The backing `Read`/`Write` is stored in the context's port store and lives until the `Context` is dropped. There is no explicit close API — for resources that must be released promptly, drop the `Context` or use a wrapper with a completion flag.

---

## TimeoutContext

`TimeoutContext` enforces wall-clock deadlines. It requires `step_limit` — the context thread uses the step limit to terminate after a timeout fires.

```rust
use tein::Context;
use std::time::Duration;

let ctx = Context::builder()
    .standard_env()
    .step_limit(1_000_000)
    .build_timeout(Duration::from_secs(5))?;

let result = ctx.evaluate("(+ 1 2 3)")?;
assert_eq!(result, Value::Integer(6));

// state persists between evaluations
ctx.evaluate("(define x 42)")?;
assert_eq!(ctx.evaluate("x")?, Value::Integer(42));
```

When the deadline is exceeded, `evaluate()` returns `Error::Timeout`. The context thread continues running until its fuel is exhausted, then terminates. The `TimeoutContext` itself remains valid — you can issue new calls after a timeout.

---

## ThreadLocalContext

`ThreadLocalContext` is `Send + Sync`, making it safe to share via `Arc` across threads. It runs a `Context` on a dedicated thread and proxies requests over bounded channels.

Two modes:

- **persistent** — state accumulates across calls; `reset()` tears down and rebuilds from the init closure
- **fresh** — context is rebuilt before every call; `reset()` is a no-op

### persistent mode

Use for REPLs, stateful scripting, or any scenario where accumulated state is intentional.

```rust
use tein::Context;
use std::sync::Arc;

let ctx = Arc::new(
    Context::builder()
        .standard_env()
        .step_limit(1_000_000)
        .build_managed(|ctx| {
            ctx.evaluate("(define counter 0)")?;
            Ok(())
        })?
);

ctx.evaluate("(set! counter (+ counter 1))")?;
ctx.evaluate("(set! counter (+ counter 1))")?;
assert_eq!(ctx.evaluate("counter")?, Value::Integer(2));

// reset tears down and re-runs the init closure
ctx.reset()?;
assert_eq!(ctx.evaluate("counter")?, Value::Integer(0));
```

### fresh mode

Use for untrusted or deterministic evaluation where each call must start from a clean slate.

```rust
let ctx = Context::builder()
    .standard_env()
    .step_limit(100_000)
    .build_managed_fresh(|ctx| {
        ctx.evaluate("(define x 42)")?;
        Ok(())
    })?;

// x is always 42 regardless of what earlier calls did
assert_eq!(ctx.evaluate("x")?, Value::Integer(42));
ctx.evaluate("(set! x 99)")?;
assert_eq!(ctx.evaluate("x")?, Value::Integer(42)); // fresh context, 99 is gone
```

The init closure runs before every call in fresh mode, so keep it lightweight — context construction involves heap allocation and VFS loading.

---

## see also

- [sandboxing.md](sandboxing.md) — module restriction, VFS gate, file IO policies
- [rust-scheme-bridge.md](rust-scheme-bridge.md) — `#[tein_fn]`, `ForeignType`, exposing Rust types to Scheme
