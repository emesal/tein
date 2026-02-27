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
just test                         # all tests (211 lib + 12 tein_fn + 24 scheme + 8 tein_module_const + 4 tein_module_naming + 1 tein_module_parse + 11 tein_module_docs + 9 tein-macros + 11 ext_loading + 1 scheme_ext + doc-tests)
cargo test test_name               # single test by name
cargo test --lib -- --nocapture    # lib tests with stdout
just lint                          # lint (cargo fmt + cargo clippy)
cargo fmt --check                  # format check
cargo run --example basic          # run an example (basic|floats|ffi|debug|sandbox|foreign_types|managed)
cargo test --features debug-chibi   # tests with chibi GC instrumentation (slower)
just clean && cargo build         # nuclear option if ffi gets weird
cargo build -p tein-test-ext       # build test cdylib extension
cargo test -p tein -- ext          # run extension integration tests
```

## architecture

```
src/
  lib.rs       — public api re-exports (Context, ContextBuilder, TimeoutContext,
                 ThreadLocalContext, Mode, Value, Error,
                 ForeignType, MethodFn, MethodContext)
  context.rs   — Context, ContextBuilder: evaluation, fuel mgmt, env restriction, all tests;
                 load_extension(), build_ext_api(), ext trampolines, ExtApiGuard RAII
  value.rs     — Value enum: scheme↔rust conversion, cycle detection, Display
                 variants: Integer, Float, String, Symbol, Boolean, List, Pair,
                 Vector, Char, Bytevector, Port (opaque), HashTable (opaque,
                 falls to Other until runtime type detection added), Nil,
                 Unspecified, Procedure, Other, Foreign { handle_id, type_name }
  error.rs     — Error enum (EvalError, TypeError, InitError, Utf8Error, IoError,
                 StepLimitExceeded, Timeout, SandboxViolation)
  ffi.rs       — unsafe c bindings + safe wrappers, `raw` module for advanced users
  foreign.rs   — ForeignType trait, MethodFn/MethodContext, ForeignStore handle-map,
                 dispatch_foreign_call — the foreign type protocol engine;
                 ExtMethodEntry, ExtTypeEntry for dynamic ext-type registration,
                 MethodLookup (Static | Ext), find_method_any
  managed.rs   — ThreadLocalContext: persistent/fresh managed context on dedicated thread
  sandbox.rs   — Preset type, FsPolicy, ModulePolicy, 16 const preset definitions for env restriction
  thread.rs    — shared channel protocol (Request, Response, SendableValue, ForeignFnPtr)
  port.rs     — PortStore: Read/Write bridge via thread-local trampoline (custom ports)
  timeout.rs   — TimeoutContext: wall-clock timeout via dedicated thread
tein-ext/      — stable C ABI types for cdylib extensions (no chibi dependency):
  src/lib.rs   — TeinExtApi vtable, OpaqueCtx/OpaqueVal, TeinTypeDesc/TeinMethodDesc,
                 SexpFn/TeinMethodFn/TeinExtInitFn type aliases, error codes, API version
tein-test-ext/ — in-tree test cdylib extension (publish = false):
  src/lib.rs   — #[tein_module("testext", ext = true)] with free fns, consts, Counter type;
                 used by tein/tests/ext_loading.rs integration tests
target/chibi-scheme/  — fetched from emesal/chibi-scheme (branch emesal-tein) by build.rs
  tein_shim.c  — exports chibi c macros as real functions, fuel control, env manipulation,
                 env_copy_named (rename-aware binding copy), error construction,
                 module import policy (tein_module_allowed, tein_module_policy_set),
                 custom port creation, reader dispatch table (set/unset/get/chars/clear/reserved),
                 macro expansion hook (set/get/clear/active guard)
  eval.c       — 4 patches: VFS module lookup (A + module policy gate), VFS load (B), VFS open-input-file (C),
                 macro expansion hook call in analyze_macro_once (D)
  sexp.c       — 1 patch: reader dispatch table check before hardcoded # switch
  vm.c         — 2-line patch for fuel budget consumption at timeslice boundary
  lib/tein/foreign.sld — (tein foreign) library definition
  lib/tein/foreign.scm — pure-scheme predicates: foreign?, foreign-type, foreign-handle-id
  lib/tein/reader.sld — (tein reader) library definition + include-shared for C init
  lib/tein/reader.scm — module documentation
  lib/tein/reader.c   — C static library init: set-reader!, unset-reader!, reader-dispatch-chars
  lib/tein/macro.sld — (tein macro) library definition + include-shared for C init
  lib/tein/macro.scm — module documentation
  lib/tein/macro.c   — C static library init: set-macro-expand-hook!, unset-macro-expand-hook!, macro-expand-hook
  lib/tein/test.sld  — (tein test) library definition
  lib/tein/test.scm  — pure-scheme assertion framework: test-equal, test-true, test-false, test-error
build.rs       — fetches chibi fork, compiles it, generates install.h, tein_vfs_data.h, tein_clibs.c into OUT_DIR
examples/      — basic.rs, floats.rs, ffi.rs, debug.rs, sandbox.rs, foreign_types.rs
tests/         — scheme_tests.rs (integration runner), scheme/*.scm (scheme-level tests)
```

**data flow**: rust code → `Context::evaluate()` → arm_fuel() → ffi.rs safe wrappers → tein_shim.c → chibi-scheme vm → tein_fuel_consume_slice() at timeslice boundary → sexp result → `Value::from_raw()` → check_fuel() → rust `Value` enum

**standard env flow**: ContextBuilder with `.standard_env()` → load_standard_env (init-7 + meta-7 via VFS) → load_standard_ports → ~200 bindings (map, for-each, values, dynamic-wind, etc.)

**sandboxing flow**: ContextBuilder with presets → get source env (primitive or standard) → GC-root both envs → create null env (syntax-only) → copy allowed bindings via env_copy_named (handles renames, NULL-safe parent walk) → set as active env. `.allow(&["import"])` enables idiomatic r7rs imports (VFS-only via module policy)

**IO policy flow**: ContextBuilder with file_read/file_write → capture original file-open procs from full env → register wrapper foreign fns in restricted env → set FsPolicy thread-local → wrapper checks path prefix via canonicalisation → delegates to original proc or returns policy violation

**module policy flow**: ContextBuilder with standard_env + presets → set MODULE_POLICY = VfsOnly (thread-local + C-level) → sexp_find_module_file_raw checks tein_module_allowed() → VFS paths pass, filesystem paths blocked → policy cleared on Context::drop()

**foreign type protocol flow**: `ctx.register_foreign_type::<T>()` → registers `ForeignType::methods()` in `ForeignStore` → injects `foreign-call`/`foreign-types`/`foreign-methods`/`foreign-type-methods` as native fns + pure-scheme `foreign?`/`foreign-type`/`foreign-handle-id` → auto-generates `type-name?` and `type-name-method` convenience procs. `ctx.foreign_value(v)` → inserts into store → returns `Value::Foreign { handle_id, type_name }`. scheme calls `(type-name-method obj)` → convenience proc → `(apply foreign-call obj 'method args)` → `foreign_call_wrapper` (extern "C") → reads `FOREIGN_STORE_PTR` thread-local → `dispatch_foreign_call` → looks up method by type name + method name → calls `MethodFn` with `&mut dyn Any` → returns `Value`. `FOREIGN_STORE_PTR` is set by `evaluate()`/`call()` via `ForeignStoreGuard` RAII.

**managed context flow**: `ContextBuilder::build_managed(init)` → spawns dedicated thread → builds Context on that thread → runs init closure → signals ready. subsequent `evaluate()`/`call()` → send `Request` over channel → thread processes → sends `Response` back. `reset()` → sends `Request::Reset` → thread rebuilds context + reruns init. `build_managed_fresh()` → same, but rebuilds before every evaluation (no state leakage).

**custom port flow**: `ctx.open_input_port(reader)` → inserts `Box<dyn Read>` into `PortStore` → creates scheme closure `(lambda (buf start end) (tein-port-read ID buf start end))` → `ffi::make_custom_input_port(ctx, closure)` → chibi's `fopencookie` + `sexp_cookie_reader` calls closure on buffer fill → `port_read_trampoline` (extern "C") reads from `PORT_STORE_PTR` thread-local → copies bytes into scheme string buffer → returns fixnum byte count. output ports mirror via `port_write_trampoline`. `ctx.read(&port)` calls `sexp_read` for one s-expression; `ctx.evaluate_port(&port)` loops read+eval.

**reader dispatch flow**: `ctx.register_reader('j', &handler)` or scheme `(import (tein reader)) (set-reader! #\j handler)` → `ffi::reader_dispatch_set(c, proc)` → stores proc in thread-local `tein_reader_dispatch[128]` table. when chibi's reader encounters `#j`, patched `sexp.c` calls `tein_reader_dispatch_get(c1)` → finds handler → `sexp_apply1(ctx, handler, in)` → handler receives input port, reads further if needed, returns datum → reader returns datum to evaluator. scheme-level fns are registered by the C static library init (`reader.c`) when `(import (tein reader))` loads the module via `include-shared`. dispatch table cleared on `Context::drop()`. reserved r7rs chars (`#t`, `#f`, `#\`, `#(`, numeric prefixes, etc.) cannot be overridden.

**macro expansion hook flow**: `ctx.set_macro_expand_hook(&proc)` or scheme `(import (tein macro)) (set-macro-expand-hook! proc)` → `ffi::macro_expand_hook_set(ctx, proc)` → stores proc in thread-local `tein_macro_expand_hook` with GC preservation. when chibi's `analyze_macro_once()` expands a macro (patched eval.c D), checks hook → if set and not already active, sets `tein_macro_expand_hook_active` recursion guard → calls `sexp_apply(ctx, hook, (name unexpanded expanded env))` → hook return value replaces expanded form → `goto loop` reanalyses (replace-and-reanalyse semantics). scheme-level fns are registered by the C static library init (`macro.c`) when `(import (tein macro))` loads the module via `include-shared`. hook cleared on `Context::drop()`.

**cdylib extension flow**: `ctx.load_extension(path)` → `libloading::Library::new(path)` → resolves `tein_ext_init` symbol → builds `TeinExtApi` vtable populated with trampolines into `ffi::*` → sets `FOREIGN_STORE_PTR` + `EXT_API` thread-locals → calls `tein_ext_init(ctx, &api)`. the extension's generated init fn (from `#[tein_module("name", ext = true)]`) checks API version, stores api pointer in `__TEIN_API` thread-local, calls `register_vfs_module`/`register_foreign_type`/`define_fn_variadic` through vtable. for ext foreign types, `ext_trampoline_register_type` builds `ExtTypeEntry` in `ForeignStore` with `TeinMethodFn` pointers; dispatch routes through `MethodLookup::Ext { func, is_mut }` passing `*mut c_void` and the api table. the shared library is leaked (no unload). `EXT_API` is also set during `evaluate()`/`call()` so ext method dispatch has access to the vtable at any time.

**dependency graph**: extension crates depend on `tein-ext` + `tein-macros`, never on `tein`. the macro emits `tein_ext::*` references resolved at extension compile time. the host (`tein`) depends on `tein-ext` for the vtable types.

**thread safety**: Context is intentionally !Send + !Sync. chibi contexts are not thread-safe. one context per thread. TimeoutContext wraps a Context on a dedicated thread for wall-clock deadlines. ThreadLocalContext generalises this pattern with persistent/fresh modes. fuel counters are thread-local.

## chibi safety invariants

tein mitigates known chibi-scheme bugs via configuration. if any of these change, review
`docs/plans/2026-02-25-chibi-scheme-review.md` for newly-exposed vulnerabilities.

- **`SEXP_USE_DL=0`** (build.rs) — disables dlopen, image loading, runtime type registration. mitigates GC finaliser bugs, image loading overflows, NULL-self finalisers.
- **`sexp_register_type` not exposed** (ffi.rs) — prevents C-level finaliser registration from rust side.
- **`sexp_exceptionp` checked after every allocation** (context.rs) — prevents writing into the shared global OOM object.
- **fuel always armed before eval** (context.rs) — bounds total operations, mitigates stack-exhaustion edge cases.
- **bytecode never user-supplied** — chibi compiles scheme→bytecode internally; no load-bytecode API exposed.
- **`heap_max` defaults to 128 MiB** (context.rs) — bounds heap growth, prevents memory exhaustion and strengthens heap-overflow mitigation.
- **version parameter hardcoded to 7** (context.rs) — chibi's `init_file[128]` does `version + '0'` unchecked; version >= 10 overflows.
- **`SEXP_G_STRICT_P` never set** — `sexp_warn` calls `exit(1)` in strict mode, bypassing all rust error handling. never enable strict mode.
- **module path list never user-modifiable** — `sexp_find_module_file_raw` reads `dir[-1]` on empty path (UB). safe because compiled-in defaults + VFS are never empty. never expose raw module path manipulation.
- **`SEXP_USE_STRICT_TOPLEVEL_BINDINGS=1`** (default) — must stay enabled; without it, `analyze_bind_syntax` has a potential NULL deref.
- **`CHIBI_MODULE_PATH` env var** — read by chibi's module resolver. our module policy gate blocks non-VFS paths at the C level so it can't escape the sandbox, but document that this env var exists.

## critical gotchas

**tein_const scheme naming**: constants get no module prefix — `#[tein_const] pub const GREETING` in module `"foo"` → scheme name `greeting`, not `foo-greeting`. free fns do get the prefix (`foo-greet`).

**Result::Err returns a scheme string**: `fn foo() -> Result<i64, String>` — the `Err` path returns `sexp_c_str(msg)` which becomes `Value::String(msg)` in rust. it's not an exception; `(test-error ...)` won't catch it. match on `Value::String` instead. same in internal and ext mode.

**type checking order**: check `sexp_flonump` BEFORE `sexp_integerp`. the integer predicate includes `_or_integer_flonump` and will match floats like 4.0, producing garbage integer values.

**chibi feature flags**: on linux, `SEXP_USE_GREEN_THREADS` defaults to 1, so the `threads` cond-expand feature is active (affects which VFS files are loaded, e.g. `srfi/39/syntax.scm` vs `syntax-no-threads.scm`). `full-unicode` is always enabled (affects `scheme/char.sld` path selection).

## adding a new scheme type

1. add predicate wrapper to `tein_shim.c` in the fork (emesal/chibi-scheme, branch emesal-tein)
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
- see ARCHITECTURE.md for full architecture docs, TODO.md for roadmap
- base branch is dev

## license
- ISC
