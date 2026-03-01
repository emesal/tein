# tein guide

A walkthrough for developers embedding Scheme in Rust with tein. Covers every major
feature with context, worked examples, and decision guides.

**Prerequisites:** familiarity with Rust and basic Scheme syntax. No prior embedding experience required.

---

## Table of contents

1. [The big picture](#the-big-picture)
2. [Your first evaluation](#your-first-evaluation)
3. [Choosing a context type](#choosing-a-context-type)
4. [The virtual filesystem](#the-virtual-filesystem)
5. [Sandboxing and resource limits](#sandboxing-and-resource-limits)
6. [Calling Rust from Scheme](#calling-rust-from-scheme)
7. [Foreign type protocol](#foreign-type-protocol)
8. [Custom ports](#custom-ports)
9. [Reader extensions](#reader-extensions)
10. [Macro expansion hooks](#macro-expansion-hooks)
11. [Managed contexts](#managed-contexts)

---

## The big picture

tein gives you a full R7RS Scheme interpreter inside your Rust program. You write Scheme
expressions as strings (or load `.scm` files), evaluate them, and get back Rust values.
You can also go the other way: expose Rust functions and types to Scheme code.

Everything runs on a single Chibi-Scheme heap per `Context`. Chibi-Scheme is a compact,
embeddable Scheme interpreter (~200kb) with zero external dependencies — exactly the
right tool for scripting, configuration, DSLs, and sandboxed evaluation.

---

## Your first evaluation

```toml
[dependencies]
tein = { git = "https://github.com/emesal/tein" }
```

```rust
use tein::{Context, Value};

let ctx = Context::new()?;
let result = ctx.evaluate("(+ 1 2 3)")?;
assert_eq!(result, Value::Integer(6));
```

`Context::new()` gives you a minimal environment: core syntax only (`define`, `lambda`,
`if`, `let`, `begin`, `quote`, `set!`). If you need the full R7RS standard library
(`map`, `for-each`, `string-split`, etc.), use `Context::new_standard()` instead.

### Extracting values

`evaluate()` returns a `Value` — a Rust enum covering every Scheme type:

```rust
match ctx.evaluate("(list 1 2 3)")? {
    Value::List(items) => println!("got {} items", items.len()),
    Value::Nil         => println!("empty list"),
    other              => println!("unexpected: {other}"),
}
```

All variants have extraction helpers:

```rust
let n = ctx.evaluate("42")?.as_integer().unwrap();      // i64
let s = ctx.evaluate(r#""hello""#)?.as_string().unwrap(); // &str
let b = ctx.evaluate("#t")?.as_bool().unwrap();         // bool
```

See the [`value`](https://docs.rs/tein/latest/tein/value/) module for the full variant
table and conversion notes.

---

## Choosing a context type

tein offers three context types. Pick the simplest one that fits your needs:

| | `Context` | `TimeoutContext` | `ThreadLocalContext` |
|---|---|---|---|
| **Thread safety** | `!Send + !Sync` | `!Send + !Sync` | `Send + Sync` |
| **Timeout** | step limit only | wall-clock deadline | step limit only |
| **State** | persists | persists | persistent or fresh |
| **Use case** | single-threaded scripting | untrusted code with deadlines | shared across threads, servers |

**`Context`** is the baseline. Use it when your evaluation happens on one thread and you
control the code being run.

**`TimeoutContext`** wraps a `Context` on a dedicated thread and kills evaluation after
a wall-clock deadline. Requires `step_limit` to be set so the context thread can
terminate after the timeout fires.

```rust
use std::time::Duration;

let ctx = Context::builder()
    .step_limit(1_000_000)
    .build_timeout(Duration::from_secs(5))?;

let result = ctx.evaluate("(+ 1 2)")?;
```

**`ThreadLocalContext`** (in the [`managed`](https://docs.rs/tein/latest/tein/managed/)
module) is `Send + Sync` — safe to wrap in `Arc` and share across threads. Evaluation
requests are proxied over a channel to a dedicated thread that owns the context.

---

## The virtual filesystem

One of tein's key design decisions: standard library modules are **embedded directly
in the binary** via a virtual filesystem (VFS), not loaded from disk.

This matters for several reasons:

- **Portability**: your binary works without the host having Chibi-Scheme installed
- **Security**: sandboxed contexts can only import from the VFS — not arbitrary files
- **Auditability**: the VFS contains only curated modules that tein has vetted

### What's in the VFS

The VFS contains the full R7RS standard library (loaded when you call `.standard_env()`
or `Context::new_standard()`), plus tein's own extension modules:

- `(scheme base)`, `(scheme write)`, `(scheme file)`, `(scheme char)`, ... — R7RS standard
- `(tein foreign)` — predicates for foreign type objects
- `(tein reader)` — reader dispatch functions (`set-reader!`, etc.)
- `(tein macro)` — macro expansion hook functions

### Importing modules

In a standard-env context, Scheme code can use `import`:

```scheme
(import (scheme base))
(import (srfi 1))   ; SRFI-1 list library, also in the VFS
```

In a sandboxed context, `import` is available and restricted to vetted VFS modules:

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(tein::sandbox::Modules::Safe)
    .build()?;
```

```scheme
(import (scheme base))         ; works — in the safe module set
(import (scheme eval))         ; blocked — scheme/eval is not in Modules::Safe
```

The VFS gate is automatic: any context with `.standard_env()` + `.sandboxed(...)`
restricts `import` to vetted VFS modules only. Filesystem modules are always
blocked in sandboxed contexts.

---

## Sandboxing and resource limits

tein's sandboxing has four independent layers you can combine freely.

### Layer 1: Module restriction

`sandboxed(modules)` restricts which VFS modules Scheme code can import. The full
standard env is built first; then a restricted null env is constructed containing only
the bindings exported by the allowed modules.

```rust
use tein::sandbox::Modules;

// conservative safe set — scheme/base, scheme/write, scheme/read, srfi/*, tein/* (no eval/repl)
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .build()?;

// all vetted modules — superset of Safe, includes scheme/eval, scheme/repl
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::All)
    .build()?;

// syntax only — core syntax + import, but all module imports rejected by VFS gate
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::None)
    .build()?;

// explicit list — transitive deps resolved automatically
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::only(&["scheme/base", "scheme/write"]))
    .build()?;
```

With `Modules::None`, UX stubs are injected for every known binding — calling `map`
in a `None` context returns an informative error like `"sandbox: 'map' requires
(import (scheme base))"` rather than an opaque undefined-variable error.

You can add extra modules on top of any `Modules` variant:

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .allow_module("tein/process")  // explicitly enable process module
    .build()?;
```

### Layer 2: Step limits

Cap the total number of VM instructions:

```rust
let ctx = Context::builder()
    .step_limit(50_000)
    .build()?;

match ctx.evaluate("((lambda () (define (f) (f)) (f)))") {
    Err(tein::Error::StepLimitExceeded) => { /* expected */ }
    _ => panic!("should have hit limit"),
}
```

### Layer 3: File IO policy

Allow filesystem access only to specific path prefixes:

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .file_read(&["/data/config/"])    // read allowed under this prefix
    .file_write(&["/tmp/output/"])   // write allowed under this prefix
    .build()?;
```

File IO wrappers are injected directly into the restricted env — no `(import (scheme file))`
is needed in Scheme code. Path canonicalisation protects against `../` traversal and symlink
attacks.

### Layer 4: VFS gate

When `.sandboxed(...)` is used, `import` is automatically restricted to vetted VFS modules.
The gate is set at build time from the `Modules` configuration — no extra configuration
needed.

To widen to the full VFS (all registered modules regardless of safety tier), use
`.vfs_gate_all()`. To collapse to an empty allowlist (reject all imports), use
`.vfs_gate_none()`.

See the [`sandbox`](https://docs.rs/tein/latest/tein/sandbox/) module for the full API
reference.

---

## Calling Rust from Scheme

### With `#[tein_fn]`

The `#[tein_fn]` proc macro is the easiest way to expose a Rust function to Scheme:

```rust
use tein::{Context, tein_fn};

#[tein_fn]
fn square(n: i64) -> i64 {
    n * n
}

let ctx = Context::new()?;
ctx.define_fn_variadic("square", __tein_square)?;

assert_eq!(ctx.evaluate("(square 7)")?, tein::Value::Integer(49));
```

The macro generates a `__tein_<name>` wrapper with the Chibi FFI signature. Supported
argument and return types: `i64`, `f64`, `bool`, `String`. Functions can return
`Result<T, E>` where `E: Display` to signal Scheme errors:

```rust
#[tein_fn]
fn safe_div(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 { Err("division by zero".to_string()) }
    else { Ok(a / b) }
}
```

### Via `ctx.call()`

You can also retrieve a Scheme procedure and call it from Rust:

```rust
let ctx = Context::new()?;
ctx.evaluate("(define (add a b) (+ a b))")?;

let add_fn = ctx.evaluate("add")?;
let result = ctx.call(&add_fn, &[Value::Integer(3), Value::Integer(4)])?;
assert_eq!(result, Value::Integer(7));
```

`ctx.call()` takes `&[Value]` as arguments, so you can pass any Rust values that
implement the `to_raw()` conversion.

---

## Foreign type protocol

The foreign type protocol lets you expose full Rust types — with methods — as
first-class Scheme objects.

### Implementing `ForeignType`

```rust
use tein::{ForeignType, MethodFn, Value};

struct Counter { n: i64 }

impl ForeignType for Counter {
    fn type_name() -> &'static str { "counter" }  // kebab-case, used as Scheme name prefix
    fn methods() -> &'static [(&'static str, MethodFn)] {
        &[
            ("increment", |obj, _ctx, _args| {
                let c = obj.downcast_mut::<Counter>().unwrap();
                c.n += 1;
                Ok(Value::Integer(c.n))
            }),
            ("get", |obj, _ctx, _args| {
                let c = obj.downcast_ref::<Counter>().unwrap();
                Ok(Value::Integer(c.n))
            }),
        ]
    }
}
```

### Registering and using the type

```rust
let ctx = Context::new_standard()?;
ctx.register_foreign_type::<Counter>()?;

// auto-generated: counter?, counter-increment, counter-get
```

Create a foreign value and interact with it from Scheme:

```rust
let c = ctx.foreign_value(Counter { n: 0 })?;

// call methods from Rust
let inc = ctx.evaluate("counter-increment")?;
let get = ctx.evaluate("counter-get")?;
ctx.call(&inc, std::slice::from_ref(&c))?;
ctx.call(&inc, std::slice::from_ref(&c))?;
let result = ctx.call(&get, std::slice::from_ref(&c))?;
assert_eq!(result, Value::Integer(2));
```

Or from Scheme entirely:

```scheme
(counter-increment my-counter)
(counter-get my-counter)       ; => 1
(counter? my-counter)          ; => #t
```

### Introspection

Four built-in functions are available once any foreign type is registered:

```scheme
(foreign-types)               ; => ("counter") — all registered type names
(foreign-methods "counter")   ; => (increment get) — methods for a type
(foreign-type my-counter)     ; => "counter"
(foreign-handle-id my-counter); => 1  (monotonic handle ID)
```

Error messages are LLM-friendly — calling an unknown method lists available ones.

---

## Custom ports

Bridge Rust `Read`/`Write` objects into Scheme's port system.

### Input ports

```rust
use tein::Context;

let ctx = Context::new_standard()?;

// any type implementing std::io::Read works
let source = std::io::Cursor::new("(+ 1 2) (+ 3 4)");
let port = ctx.open_input_port(source)?;

// read one s-expression
let expr = ctx.read(&port)?;

// or read+eval everything
let result = ctx.evaluate_port(&port)?;
```

`ctx.read()` returns a `Value` without evaluating. `ctx.evaluate_port()` reads and
evaluates expressions in a loop, returning the last result.

### Output ports

```rust
let buf: Vec<u8> = Vec::new();
let port = ctx.open_output_port(std::io::Cursor::new(buf))?;
```

Custom ports work via Chibi's `fopencookie` mechanism — your Rust reader/writer is
called directly from within Scheme evaluation.

---

## Reader extensions

Extend Scheme's `#` reader syntax with custom dispatch characters.

### From Rust

```rust
let ctx = Context::new_standard()?;
let handler = ctx.evaluate("(lambda (port) 42)")?;
ctx.register_reader('j', &handler)?;

assert_eq!(ctx.evaluate("#j")?, Value::Integer(42));
```

The handler receives the input port positioned after the dispatch character. Read
further from the port if your syntax needs more input.

### From Scheme

```scheme
(set-reader! #\j (lambda (port) (list 'json (read port))))
;; #j(1 2 3) → (json (1 2 3))
```

Or with the `(tein reader)` module:

```scheme
(import (tein reader))
(set-reader! #\j (lambda (port) (read port)))
```

### Reserved characters

The following `#` dispatch characters cannot be overridden (R7RS reserved):
`t`, `f`, `\`, `(`, and numeric prefix characters.

`reader-dispatch-chars` returns the list of currently registered characters.

The dispatch table is thread-local and cleared when the `Context` drops.

---

## Macro expansion hooks

Intercept every macro expansion at analysis time. The hook receives the macro name,
the unexpanded form, the expanded form, and the syntactic environment. Its return
value replaces the expansion (replace-and-reanalyse semantics).

### From Scheme

```scheme
(import (tein macro))

;; observe expansions without changing them
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    (display name)
    expanded))  ; return expanded unchanged

;; or transform
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    (if (eq? name 'when)
      (list 'begin expanded '(display "when expanded\n"))
      expanded)))
```

### From Rust

```rust
let hook = ctx.evaluate("(lambda (name pre post env) post)")?;
ctx.set_macro_expand_hook(&hook)?;
// later:
ctx.unset_macro_expand_hook();
```

The recursion guard prevents the hook from triggering on its own macro usage.
The hook is cleared when the `Context` drops.

---

## Managed contexts

`ThreadLocalContext` is `Send + Sync` — safe to share across threads.

### Persistent mode

State accumulates across evaluations. `reset()` rebuilds the context and re-runs
the init closure.

```rust
let ctx = Context::builder()
    .standard_env()
    .step_limit(1_000_000)
    .build_managed(|ctx| {
        ctx.evaluate("(define counter 0)")?;
        Ok(())
    })?;

ctx.evaluate("(set! counter (+ counter 1))")?;
ctx.evaluate("(set! counter (+ counter 1))")?;
assert_eq!(ctx.evaluate("counter")?, tein::Value::Integer(2));

ctx.reset()?;
assert_eq!(ctx.evaluate("counter")?, tein::Value::Integer(0));  // back to init state
```

### Fresh mode

The context is rebuilt before every evaluation — no state leakage between calls.

```rust
let ctx = Context::builder()
    .step_limit(100_000)
    .build_managed_fresh(|ctx| {
        ctx.evaluate("(define x 10)")?;
        Ok(())
    })?;

ctx.evaluate("(set! x 99)")?;
assert_eq!(ctx.evaluate("x")?, tein::Value::Integer(10));  // x is back to 10
```

Fresh mode is ideal for deterministic evaluation where each call must see exactly
the same initial state — sandboxed user scripts, test runners, etc.

### Choosing a mode

- **Persistent** — REPL, stateful scripting, accumulated definitions
- **Fresh** — untrusted user scripts, request handlers, anything needing isolation
