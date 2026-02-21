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

**milestone 4b — parameterised IO presets**
- `FsPolicy` with path prefix matching and canonicalisation
- wrapper foreign functions for all 4 file-open primitives
- `.file_read(&[...])` / `.file_write(&[...])` builder api
- support presets (`FILE_READ_SUPPORT`, `FILE_WRITE_SUPPORT`) for port operations
- path traversal and symlink protection via `canonicalize()`

**r7rs standard environment**
- VFS + static libs + eval.c patches for embedded module loading
- `Context::new_standard()` / `ContextBuilder::standard_env()` API
- ~200 bindings (map, for-each, values, dynamic-wind, etc.)
- `ModulePolicy`: VFS-only import restriction in sandboxed standard-env contexts
- C-level interception in `sexp_find_module_file_raw` via `tein_module_allowed()`

### known limitations

1. **limited type coverage**
   - hash tables and ports are opaque (`Value::HashTable`, `Value::Port`) — no rich rust api
   - continuations surface as `Value::Procedure` (chibi uses the same type tag)

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
    sandbox.rs   — Preset type, FsPolicy, ModulePolicy, 16 const preset definitions
    timeout.rs   — TimeoutContext: wall-clock timeout via thread wrapper
  vendor/chibi-scheme/
    tein_shim.c  — exports chibi c macros as real functions, fuel control,
                   environment manipulation, module import policy
    vm.c         — 2-line patch: fuel budget consumption at timeslice boundary
  build.rs       — compiles chibi + shim, generates install.h, tein_vfs_data.h, tein_clibs.c
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

### IO policy flow

```
ContextBuilder with file_read/file_write:
  1. capture original file-open procs from full env before restriction
  2. store in thread-local ORIGINAL_PROCS (4 slots, one per open-*-file)
  3. register wrapper foreign fns in restricted env
  4. set FsPolicy thread-local with path prefixes
  5. on call: wrapper extracts filename → canonicalises path →
     checks prefix match → delegates to original proc or returns error
  6. on Context::drop(): clear FsPolicy and ORIGINAL_PROCS thread-locals
```

### module import policy

```
ContextBuilder with standard_env + presets:
  1. set MODULE_POLICY thread-local = VfsOnly
  2. set C-level tein_module_policy = 1 (vfs-only)
  3. load standard env (init-7, meta-7 via VFS — allowed under VfsOnly)
  4. apply sandbox restrictions (presets, IO wrappers)
  5. on (import ...): sexp_find_module_file_raw calls tein_module_allowed()
     → VFS paths (/vfs/lib/...) pass, filesystem paths blocked
  6. on Context::drop(): reset both thread-local and C-level to Unrestricted
```

**VFS safety contract**: VFS modules are safe by construction — tein curates
the embedded virtual filesystem to ensure no module can bypass the existing
safety layers (preset allowlists, FsPolicy, fuel/timeout). capabilities
exposed by VFS modules remain subject to these controls.

**security layers** (independent, composable):

| layer              | gates                                    |
|--------------------|------------------------------------------|
| module allowlist   | which libraries can be `import`ed        |
| preset allowlist   | which primitives/bindings are in scope   |
| FsPolicy           | which filesystem paths can be opened     |
| fuel/timeout       | resource exhaustion                      |

### thread safety

- `Context` is intentionally !Send + !Sync (chibi is not thread-safe)
- `TimeoutContext` wraps Context on a dedicated thread
- fuel counters are `__thread` (thread-local) so parallel tests don't interfere

### key design decisions

**GC safety — `ffi::GcRoot`**: chibi's conservative stack scanning is disabled in our build. the GC does NOT see rust locals — only objects reachable from the context's heap roots survive collection. any `sexp` held as a rust local across an allocation point must be rooted via `ffi::GcRoot`, an RAII guard that calls `sexp_preserve_object` on creation and `sexp_release_object` on drop.

allocating FFI calls (trigger GC, require rooting across):
- `sexp_make_flonum`, `sexp_c_str`, `sexp_intern` — create heap objects
- `sexp_cons`, `sexp_make_vector` — create containers
- `sexp_symbol_to_string` — allocates a string from a symbol
- `sexp_open_input_string`, `sexp_read`, `sexp_evaluate` — evaluation machinery
- `sexp_load_standard_env`, `sexp_make_null_env` — env construction
- `sexp_env_define`, `env_copy_named`, `sexp_define_foreign_proc` — env mutation
- `sexp_preserve_object` itself — allocates a cons cell on the preservatives list

non-allocating FFI calls (safe, no rooting needed):
- type predicates: `sexp_integerp`, `sexp_flonump`, `sexp_pairp`, etc.
- value extractors: `sexp_unbox_fixnum`, `sexp_flonum_value`, `sexp_string_data`, `sexp_car`, `sexp_cdr`, `sexp_vector_data`
- immediate constructors: `sexp_make_fixnum`, `sexp_make_boolean`, `get_null`, `get_void`
- `sexp_vector_set` — writes to an existing vector slot, no allocation

C-side equivalent: use `sexp_gc_var` / `sexp_gc_preserve` / `sexp_gc_release` (see eval.c patches).

**vendoring chibi**: source bundled, compiled via build.rs, zero external deps.

**shim layer**: chibi uses c macros extensively; `tein_shim.c` exports them as real functions for rust ffi.

**fuel implementation**: chibi's vm creates child contexts per eval, so context-level refuel doesn't work. thread-local counters + a 2-line vm.c patch intercept the timeslice boundary to implement true total-fuel budgeting. when fuel limiting is inactive, behaviour is identical to stock chibi.

**type checking order**: check `sexp_flonump` BEFORE `sexp_integerp`. the integer predicate includes `_or_integer_flonump` and matches floats like 4.0, producing garbage.

**VFS path prefix**: use `/vfs/lib` not `vfs://...` — chibi's `sexp_add_path` splits on `:`, so colons in paths break module resolution.

**`sexp_load_standard_env` signature**: the version parameter is `sexp` (a tagged fixnum via `sexp_make_fixnum`), NOT `sexp_uint_t`. this is a chibi API quirk.

**rename bindings in standard env**: the standard env stores most bindings as *renames* (via `SEXP_USE_RENAME_BINDINGS`), not direct bindings. `sexp_env_ref` with a bare symbol won't find them. `tein_env_copy_named` in `tein_shim.c` handles this by walking both direct bindings and renames with synclo unwrapping. note: the env parent chain terminates with NULL, and `sexp_envp(NULL)` segfaults because `sexp_pointerp(NULL)` returns true (`SEXP_POINTER_TAG == 0`). the env walk loop must guard against NULL explicitly.

**`import` in sandboxed envs**: `import` is not core syntax — it's a binding from `repl-import` in the meta env, spliced into the standard env during `sexp_load_standard_env`. it can be copied into the restricted null env via `.allow(&["import"])` like any other binding. the module policy (VFS-only) still applies, so only curated VFS modules are importable. both `source_env` and `null_env` must be GC-rooted during sandbox build, since `sexp_intern`, `env_copy_named`, and `sexp_define_foreign_proc` all allocate.

**`let` in sandboxed standard env**: closures from the standard env (e.g. `for-each`) reference the full env internally, but `let`-bound variables in user code live in the restricted null env. using `define` for top-level bindings works; `let` inside `for-each` callbacks does not. this is a scope chain issue specific to the null env sandbox approach.

## building & testing

```bash
cargo build                        # build (compiles vendored chibi-scheme)
cargo test                         # all tests (112 lib + 12 scheme_fn + 8 doc)
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
