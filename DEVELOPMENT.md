# tein development handoff

> *branch and rune-stick* — embeddable chibi-scheme for rust

## project status

### completed milestones

**milestone 1 — core types & ergonomics**
- vendored chibi-scheme 0.11 with custom build system
- c ffi shim layer (`tein_shim.c`) for macro-based apis
- safe rust wrappers around unsafe c functions
- all core value types: integers, floats, strings, symbols, booleans, lists, pairs, vectors, nil, procedures
- typed extraction helpers (`as_integer()`, `as_list()`, `is_procedure()`, etc.)
- bidirectional value bridge (`Value::to_raw()` ↔ `Value::from_raw()`)
- multi-expression evaluation, file loading
- tortoise-and-hare cycle detection, depth limits

**milestone 2 — scheme as extension language**
- procedures as values via `sexp_applicablep`
- `ctx.call(proc, &[args])` for rust→scheme callbacks
- `define_fn_variadic` for registering rust functions
- `#[scheme_fn]` proc macro for ergonomic ffi
- panic safety at ffi boundary

**milestone 3 — tein-sexp pure rust s-expression crate**
- separate workspace crate, no chibi dependency
- `Sexp` AST with source spans
- r7rs-compatible lexer and parser
- comment preservation mode
- pretty printer with configurable output

**milestone 4a — sandboxing & resource limits**
- `ContextBuilder` with fluent api for heap sizes, step limits, and environment restriction
- fuel-based step limiting via thread-local counters + vm.c patch
- allowlist-based sandbox presets using chibi's null env (14 presets)
- `TimeoutContext` for wall-clock deadlines via dedicated thread
- `Error::StepLimitExceeded` and `Error::Timeout` variants

### known limitations

1. **no r7rs standard environment**
   - running with chibi primitives only (arithmetic, cons/car/cdr, define, if, lambda, etc.)
   - missing: most r7rs standard library functions
   - requires static library embedding or dynamic module loading

2. **limited type coverage**
   - no hash tables, ports, continuations, bytevectors as Value variants

## architecture

### directory structure
```
tein/
  src/
    lib.rs       — public api re-exports
    context.rs   — Context, ContextBuilder, evaluation, fuel mgmt, all tests
    value.rs     — Value enum: scheme↔rust conversion, cycle detection, Display
    error.rs     — Error enum (EvalError, TypeError, InitError, Utf8Error,
                   IoError, StepLimitExceeded, Timeout)
    ffi.rs       — unsafe c bindings + safe wrappers, `raw` module
    sandbox.rs   — Preset type + 14 const preset definitions
    timeout.rs   — TimeoutContext: wall-clock timeout via thread wrapper
  vendor/chibi-scheme/
    tein_shim.c  — exports chibi c macros as real functions, fuel control,
                   environment manipulation
    vm.c         — 2-line patch: fuel budget consumption at timeslice boundary
  build.rs       — compiles chibi + shim, generates install.h
  examples/      — basic.rs, floats.rs, ffi.rs, debug.rs, sandbox.rs
tein-macros/     — #[scheme_fn] proc macro crate
tein-sexp/       — pure rust s-expression parser/printer
```

### data flow

```
rust code → Context::evaluate()
  → arm_fuel() (if step limit configured)
  → ffi.rs safe wrappers → tein_shim.c → chibi-scheme vm
  → tein_fuel_consume_slice() at each timeslice boundary
  → sexp result → Value::from_raw() → check_fuel()
  → rust Value enum (or Error::StepLimitExceeded)
```

### sandboxing flow

```
ContextBuilder::build() with presets:
  1. create context with full primitive env
  2. create null env (syntax-only: define, if, lambda, begin, quote)
  3. for each allowed primitive: look up in primitive env, copy to null env
  4. set null env as active → only allowed primitives accessible
```

### thread safety

- `Context` is intentionally !Send + !Sync (chibi is not thread-safe)
- `TimeoutContext` wraps Context on a dedicated thread
- fuel counters are `__thread` (thread-local) so parallel tests don't interfere

### key design decisions

**vendoring chibi**: source bundled, compiled via build.rs, zero external deps.

**shim layer**: chibi uses c macros extensively; `tein_shim.c` exports them as real functions for rust ffi.

**fuel implementation**: chibi's vm creates child contexts per eval, so context-level refuel doesn't work. thread-local counters + a 2-line vm.c patch intercept the timeslice boundary to implement true total-fuel budgeting. when fuel limiting is inactive, behaviour is identical to stock chibi.

**type checking order**: check `sexp_flonump` BEFORE `sexp_integerp`. the integer predicate includes `_or_integer_flonump` and matches floats like 4.0, producing garbage.

## building & testing

```bash
cargo build                        # build (compiles vendored chibi-scheme)
cargo test                         # all tests (88 lib + 12 scheme_fn + 8 doc)
cargo test test_name               # single test by name
cargo test --lib -- --nocapture    # lib tests with stdout
cargo clippy                       # lint
cargo fmt --check                  # format check
cargo run --example basic          # run an example
cargo run --example sandbox        # sandboxing demo
cargo clean && cargo build         # nuclear option if ffi gets weird
```

## adding a new scheme type

1. add predicate wrapper to `vendor/chibi-scheme/tein_shim.c`
2. add extern declaration + safe wrapper in `src/ffi.rs`
3. add variant to `Value` enum in `src/value.rs`
4. add extraction in `Value::from_raw()` (respect type check ordering!)
5. add `to_raw()` conversion
6. add Display impl
7. add test in `src/context.rs`

## registering rust functions in scheme

**via proc macro (recommended):**
```rust
#[scheme_fn]
fn add(a: i64, b: i64) -> i64 { a + b }

ctx.define_fn_variadic("add", __tein_add)?;
```

**via raw ffi:**
```rust
unsafe extern "C" fn my_fn(
    ctx: raw::sexp, _self: raw::sexp,
    _n: raw::sexp_sint_t, args: raw::sexp,
) -> raw::sexp { ... }

ctx.define_fn_variadic("my-fn", my_fn)?;
```

## conventions

- edition 2024: `unsafe fn` bodies need inner `unsafe { }` blocks
- every public item has a docstring
- comments explain *why*, code shows *what*
- lowercase style, casual but precise
- norse mythology naming theme
- see TODO.md for roadmap
