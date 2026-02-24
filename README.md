# tein

> embeddable r7rs scheme interpreter for rust

**tein** wraps [chibi-scheme](https://github.com/ashinn/chibi-scheme) in a safe rust API. zero runtime dependencies, full r7rs-small compliance, ~200kb footprint. evaluate scheme from rust, call rust from scheme, sandbox everything.

## quick start

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

## features

### sandboxing & resource limits

restrict the environment to exactly the primitives you need. combine presets, step limits, wall-clock timeouts, and file IO policies.

```rust
use tein::Context;

let ctx = Context::builder()
    .safe()                     // no filesystem, no eval
    .step_limit(50_000)         // terminate infinite loops
    .build()?;

let result = ctx.evaluate("(+ 1 2)")?;
assert_eq!(result, tein::Value::Integer(3));

// file IO blocked
assert!(ctx.evaluate(r#"(open-input-file "/etc/passwd")"#).is_err());
```

16 composable presets (`ARITHMETIC`, `LISTS`, `STRINGS`, `IO`, ...) plus convenience builders (`.pure_computation()`, `.safe()`). see the [`sandbox`](https://docs.rs/tein/latest/tein/sandbox/) module.

### `#[scheme_fn]` proc macro

define scheme-callable functions in pure rust with automatic type conversion and error handling.

```rust
use tein::{Context, scheme_fn};

#[scheme_fn]
fn square(n: i64) -> i64 {
    n * n
}

let ctx = Context::new()?;
ctx.define_fn_variadic("square", __tein_square)?;

let result = ctx.evaluate("(square 7)")?;
assert_eq!(result, tein::Value::Integer(49));
```

### foreign type protocol

expose rust types as first-class scheme objects with method dispatch, predicates, and introspection.

```rust
use tein::{Context, ForeignType, MethodFn, Value};

struct Counter { n: i64 }

impl ForeignType for Counter {
    fn type_name() -> &'static str { "counter" }
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

let ctx = Context::new_standard()?;
ctx.register_foreign_type::<Counter>()?;
// auto-generates: counter?, counter-increment, counter-get
```

### custom ports

bridge rust `Read`/`Write` into scheme's port system for streaming IO.

```rust
use tein::Context;

let ctx = Context::new_standard()?;
let json = r#"{"key": "value"}"#;
let port = ctx.open_input_port(std::io::Cursor::new(json))?;
let datum = ctx.read(&port)?;
```

### reader extensions

register custom `#` dispatch characters to extend scheme's reader at the syntax level.

```rust
let ctx = Context::new_standard()?;
ctx.register_reader('j', &ctx.evaluate(
    "(lambda (port) (read port))"  // #j<datum> → <datum>
)?)?;
```

or from scheme via `(import (tein reader))`:

```scheme
(import (tein reader))
(set-reader! #\j (lambda (port) (read port)))
```

### macro expansion hooks

intercept and transform macro expansions at analysis time — replace-and-reanalyse semantics.

```scheme
(import (tein macro))
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    expanded))  ; observe or transform
```

### managed contexts

thread-safe scheme evaluation via `ThreadLocalContext` — persistent state or fresh-per-evaluation.

```rust
use tein::Context;

let ctx = Context::builder()
    .standard_env()
    .step_limit(1_000_000)
    .build_managed(|ctx| {
        ctx.evaluate("(define counter 0)")?;
        Ok(())
    })?;

ctx.evaluate("(set! counter (+ counter 1))")?;
ctx.evaluate("(set! counter (+ counter 1))")?;
let result = ctx.evaluate("counter")?;
assert_eq!(result, tein::Value::Integer(2));

ctx.reset()?; // rebuild context, re-run init
```

## examples

| example | description |
|---------|-------------|
| `basic` | evaluate expressions, pattern-match on values |
| `floats` | floating-point arithmetic |
| `ffi` | `#[scheme_fn]` proc macro, calling rust from scheme and vice versa |
| `debug` | float/integer type inspection |
| `sandbox` | step limits, restricted environments, timeouts, file IO policies |
| `foreign_types` | foreign type protocol — registration, dispatch, introspection |
| `managed` | `ThreadLocalContext` persistent and fresh modes |
| `repl` | interactive scheme REPL with readline |

```bash
cargo run --example sandbox
```

## about

from old norse *tein* (teinn): **branch** — like the branches of an abstract syntax tree; **rune-stick** — carved wood for writing magical symbols. code as data, data as runes.

**why scheme?** homoiconic syntax, proper tail calls, hygienic macros, minimalist elegance. **why chibi?** tiny (~200kb), zero external deps, full r7rs-small, designed for embedding.

**why tein?** because embedding scheme in rust shouldn't require wrestling with raw FFI.

---

*carved with care, grown with intention*
