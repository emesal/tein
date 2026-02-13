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
cargo test                         # all tests (100 lib + 12 scheme_fn + 8 doc-tests)
cargo test test_name               # single test by name
cargo test --lib -- --nocapture    # lib tests with stdout
cargo clippy                       # lint
cargo fmt --check                  # format check
cargo run --example basic          # run an example (basic|floats|ffi|debug|sandbox)
cargo clean && cargo build         # nuclear option if ffi gets weird
```

## architecture

```
src/
  lib.rs       — public api re-exports (Context, ContextBuilder, TimeoutContext, Value, Error)
  context.rs   — Context, ContextBuilder: evaluation, fuel mgmt, env restriction, all tests
  value.rs     — Value enum: scheme↔rust conversion, cycle detection, Display
  error.rs     — Error enum (EvalError, TypeError, InitError, Utf8Error, IoError,
                 StepLimitExceeded, Timeout)
  ffi.rs       — unsafe c bindings + safe wrappers, `raw` module for advanced users
  sandbox.rs   — Preset type, FsPolicy, ModulePolicy, 16 const preset definitions for env restriction
  timeout.rs   — TimeoutContext: wall-clock timeout via dedicated thread
vendor/chibi-scheme/
  tein_shim.c  — exports chibi c macros as real functions, fuel control, env manipulation,
                 env_copy_named (rename-aware binding copy), error construction,
                 module import policy (tein_module_allowed, tein_module_policy_set)
  eval.c       — 3 patches: VFS module lookup (A + module policy gate), VFS load (B), VFS open-input-file (C)
  vm.c         — 2-line patch for fuel budget consumption at timeslice boundary
build.rs       — compiles chibi + shim, generates install.h, tein_vfs_data.h, tein_clibs.c
examples/      — basic.rs, floats.rs, ffi.rs, debug.rs, sandbox.rs
```

**data flow**: rust code → `Context::evaluate()` → arm_fuel() → ffi.rs safe wrappers → tein_shim.c → chibi-scheme vm → tein_fuel_consume_slice() at timeslice boundary → sexp result → `Value::from_raw()` → check_fuel() → rust `Value` enum

**standard env flow**: ContextBuilder with `.standard_env()` → load_standard_env (init-7 + meta-7 via VFS) → load_standard_ports → ~200 bindings (map, for-each, values, dynamic-wind, etc.)

**sandboxing flow**: ContextBuilder with presets → get source env (primitive or standard) → create null env (syntax-only) → copy allowed bindings via env_copy_named (handles renames) → set as active env

**IO policy flow**: ContextBuilder with file_read/file_write → capture original file-open procs from full env → register wrapper foreign fns in restricted env → set FsPolicy thread-local → wrapper checks path prefix via canonicalisation → delegates to original proc or returns policy violation

**module policy flow**: ContextBuilder with standard_env + presets → set MODULE_POLICY = VfsOnly (thread-local + C-level) → sexp_find_module_file_raw checks tein_module_allowed() → VFS paths pass, filesystem paths blocked → policy cleared on Context::drop()

**thread safety**: Context is intentionally !Send + !Sync. chibi contexts are not thread-safe. one context per thread. TimeoutContext wraps a Context on a dedicated thread for wall-clock deadlines. fuel counters are thread-local.

## critical gotchas

**type checking order**: check `sexp_flonump` BEFORE `sexp_integerp`. the integer predicate includes `_or_integer_flonump` and will match floats like 4.0, producing garbage integer values.

**chibi feature flags**: on linux, `SEXP_USE_GREEN_THREADS` defaults to 1, so the `threads` cond-expand feature is active (affects which VFS files are loaded, e.g. `srfi/39/syntax.scm` vs `syntax-no-threads.scm`). `full-unicode` is always enabled (affects `scheme/char.sld` path selection).

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
