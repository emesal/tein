# tein

> Embeddable R7RS Scheme interpreter for Rust

**tein** wraps [chibi-scheme](https://github.com/ashinn/chibi-scheme) in a safe Rust API. Zero runtime dependencies, full r7rs-small compliance, ~200kb footprint. Evaluate Scheme from Rust, call Rust from Scheme, sandbox everything.

## Quick start

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

## Features

### Sandboxing & resource limits

Restrict the environment to exactly the primitives you need. Combine presets, step limits, wall-clock timeouts, and file IO policies.

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

16 composable presets (`ARITHMETIC`, `LISTS`, `STRINGS`, `IO`, ...) plus convenience builders (`.pure_computation()`, `.safe()`). See the [`sandbox`](https://docs.rs/tein/latest/tein/sandbox/) module.

### `#[scheme_fn]` proc macro

Define Scheme-callable functions in pure Rust with automatic type conversion and error handling.

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

### Foreign type protocol

Expose Rust types as first-class Scheme objects with method dispatch, predicates, and introspection.

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

### Custom ports

Bridge Rust `Read`/`Write` into Scheme's port system for streaming IO.

```rust
use tein::Context;

let ctx = Context::new_standard()?;
let json = r#"{"key": "value"}"#;
let port = ctx.open_input_port(std::io::Cursor::new(json))?;
let datum = ctx.read(&port)?;
```

### Reader extensions

Register custom `#` dispatch characters to extend Scheme's reader at the syntax level.

```rust
let ctx = Context::new_standard()?;
ctx.register_reader('j', &ctx.evaluate(
    "(lambda (port) (read port))"  // #j<datum> → <datum>
)?)?;
```

Or from Scheme via `(import (tein reader))`:

```scheme
(import (tein reader))
(set-reader! #\j (lambda (port) (read port)))
```

### Macro expansion hooks

Intercept and transform macro expansions at analysis time — replace-and-reanalyse semantics.

```scheme
(import (tein macro))
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    expanded))  ; observe or transform
```

### Managed contexts

Thread-safe Scheme evaluation via `ThreadLocalContext` — persistent state or fresh-per-evaluation.

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

## Examples

| example | description |
|---------|-------------|
| `basic` | Evaluate expressions, pattern-match on values |
| `floats` | Floating-point arithmetic |
| `ffi` | `#[scheme_fn]` proc macro, calling Rust from Scheme and vice versa |
| `debug` | Float/integer type inspection |
| `sandbox` | Step limits, restricted environments, timeouts, file IO policies |
| `foreign_types` | Foreign type protocol — registration, dispatch, introspection |
| `managed` | `ThreadLocalContext` persistent and fresh modes |
| `repl` | Interactive Scheme REPL with readline |

```bash
cargo run --example sandbox
```

## About

From Old Norse *tein* (teinn): **branch** — like the branches of an abstract syntax tree; **rune-stick** — carved wood for writing magical symbols. Code as data, data as runes.

**Why Scheme?** Homoiconic syntax, proper tail calls, hygienic macros, minimalist elegance. **Why Chibi?** Tiny (~200kb), zero external deps, full r7rs-small, designed for embedding.

**Why tein?** Because embedding Scheme in Rust shouldn't require wrestling with raw FFI.

---

*carved with care, grown with intention*
