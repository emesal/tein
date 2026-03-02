# tein

> Branch and rune-stick — embeddable R7RS Scheme for Rust

**tein** is an embeddable R7RS Scheme interpreter for Rust, built on vendored chibi-scheme 0.11. Safe Rust API wrapping unsafe C FFI. Zero runtime dependencies.

tein has a dual identity: **scheme embedded in rust** — add Scheme as a scripting or extension language to any Rust application, with safe sandboxing, resource limits, and bidirectional data exchange — and **scheme with rust inside** — Scheme programs get access to the Rust ecosystem via tein's module system, with high-performance crates exposed as idiomatic R7RS libraries.

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

- **[Sandboxing](docs/sandboxing.md)** — restrict the module set, cap step counts, set wall-clock timeouts, and enforce file IO policies (allow/deny by path prefix)
- **[Rust↔Scheme bridge](docs/rust-scheme-bridge.md)** — `#[tein_fn]` and `#[tein_module]` proc macros for zero-boilerplate Rust functions and modules; `ForeignType` protocol for first-class Rust objects in Scheme with method dispatch and introspection
- **[Built-in modules](docs/modules.md)** — `(tein json)`, `(tein toml)`, `(tein uuid)`, `(tein time)`, `(tein process)`, `(tein docs)` ship with the crate
- **[cdylib extensions](docs/extensions.md)** — load Rust extensions at runtime via a stable C ABI vtable; no chibi dependency in the extension crate
- **[Managed contexts](docs/embedding.md)** — `ThreadLocalContext` gives you Send+Sync Scheme evaluation with persistent state or fresh-per-evaluation semantics
- **[Custom ports](docs/embedding.md)** — bridge any Rust `Read`/`Write` into Scheme's port system for streaming IO
- **[Reader extensions](docs/rust-scheme-bridge.md)** — register custom `#` dispatch characters to extend the Scheme reader at the syntax level
- **[Macro expansion hooks](docs/rust-scheme-bridge.md)** — intercept and transform macro expansions at analysis time with replace-and-reanalyse semantics

## Docs

| doc | description |
|-----|-------------|
| [guide](docs/guide.md) | index and reading order |
| [quickstart](docs/quickstart.md) | working code in 5 minutes |
| [embedding](docs/embedding.md) | context types, Value enum, builder API |
| [sandboxing](docs/sandboxing.md) | module policy, file IO, resource limits |
| [rust-scheme bridge](docs/rust-scheme-bridge.md) | `#[tein_fn]`, `#[tein_module]`, foreign types |
| [modules](docs/modules.md) | built-in `(tein *)` modules |
| [extensions](docs/extensions.md) | cdylib extension system |
| [tein for agents](docs/tein-for-agents.md) | tein as an agent execution platform |
| [reference](docs/reference.md) | Value types, feature flags, VFS modules, quirks |

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

## About

from Old Norse *tein* (teinn): **branch** — like the branches of an abstract syntax tree; **rune-stick** — carved wood for writing magical symbols. code as data, data as runes.

**why Scheme?** homoiconic syntax, proper tail calls, hygienic macros, minimalist elegance. **why Chibi?** tiny (~200kb), zero external deps, full R7RS-small, designed for embedding.

**why tein?** because embedding Scheme in Rust shouldn't require wrestling with raw FFI. and because a language that can carry intent deserves a host that can carry it faithfully.

*carved with care, grown with intention*

---

## License

ISC

make meow, not rawr
