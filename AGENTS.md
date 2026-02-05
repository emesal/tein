## what is tein?

embeddable r7rs scheme interpreter for rust, built on vendored chibi-scheme 0.11. safe rust api wrapping unsafe c ffi. zero runtime dependencies.

## principles

- establish patterns now that scale well, refactor liberally when beneficial.
- backwards compatibility not a priority, legacy code unwanted. (pre-alpha.)
- self-documenting code; keep symbols, comments, and docs consistent.
- missing or incorrect documentation including code comments are critical bugs.
- comprehensive tests including edge cases.

## commands

```bash
cargo build                        # build (compiles vendored chibi-scheme via build.rs)
cargo test                         # all tests (23 unit + 3 doc-tests)
cargo test test_name               # single test by name
cargo test --lib -- --nocapture    # lib tests with stdout
cargo clippy                       # lint
cargo fmt --check                  # format check
cargo run --example basic          # run an example (basic|floats|ffi|debug)
cargo clean && cargo build         # nuclear option if ffi gets weird
```

## architecture

```
src/
  lib.rs       — public api re-exports
  context.rs   — Context: evaluation entry point, foreign fn registration, all tests
  value.rs     — Value enum: scheme↔rust conversion, cycle detection, Display
  error.rs     — Error enum (EvalError, TypeError, InitError, Utf8Error)
  ffi.rs       — unsafe c bindings + safe wrappers, `raw` module for advanced users
vendor/chibi-scheme/
  tein_shim.c  — exports chibi c macros as real functions (rust ffi can't call macros)
build.rs       — compiles chibi + shim, generates install.h
examples/      — basic.rs, floats.rs, ffi.rs, debug.rs
```

**data flow**: rust code → `Context::evaluate()` → ffi.rs safe wrappers → tein_shim.c → chibi-scheme → sexp result → `Value::from_raw()` → rust `Value` enum

**thread safety**: Context is intentionally !Send + !Sync. chibi contexts are not thread-safe. one context per thread.

## critical gotcha

**type checking order**: check `sexp_flonump` BEFORE `sexp_integerp`. the integer predicate includes `_or_integer_flonump` and will match floats like 4.0, producing garbage integer values.

## adding a new scheme type

1. add predicate wrapper to `vendor/chibi-scheme/tein_shim.c`
2. add extern declaration + safe wrapper in `src/ffi.rs`
3. add variant to `Value` enum in `src/value.rs`
4. add extraction in `Value::from_raw()` (respect type check ordering!)
5. add `to_raw()` conversion
6. add Display impl
7. add test in `src/context.rs`

## conventions

- edition 2024: `unsafe fn` bodies need inner `unsafe { }` blocks
- every public item has a docstring
- comments explain *why*, code shows *what*
- lowercase style, casual but precise
- norse mythology naming theme
- see DEVELOPMENT.md for full architecture docs, TODO.md for roadmap
