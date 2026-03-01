# tein

> Branch and rune-stick — embeddable R7RS Scheme for Rust

**tein** is an embeddable R7RS Scheme interpreter for Rust, built on vendored chibi-scheme 0.11. safe Rust API wrapping unsafe C FFI. zero runtime dependencies.

tein has a dual identity:

- **scheme embedded in rust** — add Scheme as a scripting or extension language to any Rust application. safe sandboxing, resource limits, bidirectional data exchange
- **scheme with rust inside** — Scheme programs get access to the Rust ecosystem via tein's module system: high-performance crates exposed as idiomatic R7RS libraries

long-term, tein aims to be a capable scheme in its own right — one that just happens to be exceptionally easy to embed in Rust, in the same spirit that chibi-scheme is easy to embed in C.

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

## What tein can do today

### Sandboxing & resource limits

Restrict the environment to exactly the modules you allow. Combine module sets, step limits, wall-clock timeouts, and file IO policies.

```rust
use tein::sandbox::Modules;

let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)   // no filesystem, no eval/repl
    .step_limit(50_000)         // terminate infinite loops
    .build()?;

// file IO blocked
assert!(ctx.evaluate(r#"(open-input-file "/etc/passwd")"#).is_err());
```

`Modules` enum (`Safe`, `All`, `None`, `only(&[...])`) controls which VFS modules are importable. Transitive dependencies are resolved automatically. See the [`sandbox`](https://docs.rs/tein/latest/tein/sandbox/) module.

### `#[tein_fn]` proc macro

Define Scheme-callable functions in pure Rust with automatic type conversion and error handling.

```rust
#[tein_fn]
fn square(n: i64) -> i64 { n * n }

ctx.define_fn_variadic("square", __tein_square)?;
assert_eq!(ctx.evaluate("(square 7)")?, Value::Integer(49));
```

### Foreign type protocol

Expose Rust types as first-class Scheme objects with method dispatch, predicates, and introspection.

```rust
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
                Ok(Value::Integer(obj.downcast_ref::<Counter>().unwrap().n))
            }),
        ]
    }
}

ctx.register_foreign_type::<Counter>()?;
// auto-generates: counter?, counter-increment, counter-get
```

### Custom ports

Bridge Rust `Read`/`Write` into Scheme's port system for streaming IO.

```rust
let port = ctx.open_input_port(std::io::Cursor::new(b"(+ 1 2)"))?;
let result = ctx.evaluate_port(&port)?;
```

### Reader extensions

Register custom `#` dispatch characters to extend Scheme's reader at the syntax level.

```scheme
(import (tein reader))
(set-reader! #\j (lambda (port) (list 'json (read port))))
;; #j(1 2 3) → (json (1 2 3))
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
let ctx = Context::builder()
    .standard_env()
    .step_limit(1_000_000)
    .build_managed(|ctx| {
        ctx.evaluate("(define counter 0)")?;
        Ok(())
    })?;

ctx.evaluate("(set! counter (+ counter 1))")?;
ctx.evaluate("(set! counter (+ counter 1))")?;
assert_eq!(ctx.evaluate("counter")?, Value::Integer(2));

ctx.reset()?;  // rebuild context, re-run init
```

## Examples

| example | description |
|---------|-------------|
| `basic` | evaluate expressions, pattern-match on values |
| `floats` | floating-point arithmetic |
| `ffi` | `#[tein_fn]` proc macro, Rust↔Scheme calls |
| `debug` | float/integer type inspection |
| `sandbox` | step limits, restricted environments, timeouts, file IO policies |
| `foreign_types` | foreign type protocol — registration, dispatch, introspection |
| `managed` | `ThreadLocalContext` persistent and fresh modes |
| `repl` | interactive Scheme REPL with readline |

```bash
cargo run --example sandbox
```

## What tein is becoming

the current feature set covers the "scheme embedded in rust" half of the dual identity well. the roadmap is about filling in the other half and then some.

### the rust ecosystem bridge (M8)

`(tein json)`, `(tein regex)`, `(tein crypto)`, `(tein uuid)` — high-value Rust crates exposed as idiomatic R7RS scheme modules. a `#[tein_module]` proc macro to make adding further modules fast and consistent. this is the "scheme with rust inside" story made real: scheme programs that import a regex library or a UUID generator and get a proper Rust implementation underneath.

### tein as a scheme (M9)

a standalone `tein` binary — a first-class scheme interpreter, not just a library. snow-fort package support with two trust tiers (vetted VFS packages available in sandboxed contexts; unvetted packages as an explicit capability). `(tein wisp)` for SRFI-119 indentation-based syntax. R5RS/R6RS compatibility layers. the goal is a scheme you could use on its own that also happens to embed into Rust trivially.

### capability modules (M10)

`(tein http)`, `(tein datetime)`, `(tein tracing)` — building on the module infrastructure from M8. scheme code that can make HTTP requests, manipulate datetimes, and emit structured traces into Rust's tracing ecosystem.

### stochastic runtime support (M12)

tein's long arc is toward hosting a stochastic programming language — a language where the fundamental primitive is not a value but a probability distribution. programs carry *intent*, not instructions. a stochastic binding like `(define~ meal (intent "comforting dinner, not heavy"))` names a typed cloud that collapses at runtime through the cheapest available strategy: deterministic resolution first, then algorithmic projection, then a small model, and only finally a full LLM call.

the stochastic language is not tein itself — it's a library implemented *in* tein R7RS modules, using tein as its substrate. tein already has every primitive it needs:

- **first-class continuations** — a residual node waiting for a model to fill in a value *is* a delimited continuation
- **macro expansion hook** — the deterministic compilation passes are macro transformations on the stochastic IR
- **foreign type protocol** — model handles, projection strategies, and knowledge base instances are Rust-side objects exposed to scheme
- **sandboxing** — the deterministic compilation phase runs isolated, model dispatch runs with appropriate capabilities granted
- **managed contexts** — persistent scheme contexts for holding compilation state and accumulated constraints across evaluations

M12 adds `(tein rat)` — a Rust-backed module wrapping a model gateway (chat, generate, embed, NLI, token counting) — and the stochastic core library: `define~`, `intent`, `narrow`, `project`, `monad`, `with-context`, `register-projection`. this is where tein's two identities converge: Rust ecosystem modules provide cheap algorithmic projections; the model bridge provides the LLM fallback; scheme coordinates and expresses intent.

---

## About

from Old Norse *tein* (teinn): **branch** — like the branches of an abstract syntax tree; **rune-stick** — carved wood for writing magical symbols. code as data, data as runes.

**why Scheme?** homoiconic syntax, proper tail calls, hygienic macros, minimalist elegance. **why Chibi?** tiny (~200kb), zero external deps, full R7RS-small, designed for embedding.

**why tein?** because embedding Scheme in Rust shouldn't require wrestling with raw FFI. and because a language that can carry intent deserves a host that can carry it faithfully.

*carved with care, grown with intention*

---

## License

ISC

make meow, not rawr
