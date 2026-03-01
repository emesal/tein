## what is tein?

embeddable r7rs scheme interpreter for rust, built on vendored chibi-scheme 0.11. safe rust api wrapping unsafe c ffi. zero runtime dependencies. tein's scheme environment is designed with coding agents in mind.

## principles

- establish patterns now that scale well, refactor liberally when beneficial.
- backwards compatibility not a priority, legacy code unwanted.
- self-documenting code; keep symbols, comments, and docs consistent.
- missing or incorrect documentation including code comments are critical bugs.
- comprehensive tests including edge cases.

## important
- every public item has a docstring
- **base branch is dev** not main
- chibi-scheme C code changes: cargo hard resets chibi-scheme from remote on build; changes to upstream chibi-scheme must be pushed to the remote repo
- remember to GC root C vars where appropriate

## commands

```bash
cargo build                        # build (compiles vendored chibi-scheme via build.rs)
just test                         # all tests (299 lib + 12 tein_fn + 28 scheme + 8 tein_module_const + 4 tein_module_naming + 1 tein_module_parse + 11 tein_module_docs + 11 tein-macros + 11 ext_loading + 1 scheme_ext + doc-tests)
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
                 variants: Integer, Float, Bignum(String), Rational(Box<Value>, Box<Value>),
                 Complex(Box<Value>, Box<Value>), String, Symbol, Boolean, List, Pair,
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
  json.rs      — json_parse (JSON string → Value) + json_stringify_raw (raw sexp → JSON string);
                 registered as json-parse/json-stringify via trampolines in context.rs.
                 stringify works at raw sexp level to preserve alist structure through chibi round-trips
  toml.rs      — toml_parse (TOML string → Value) + toml_stringify_raw (raw sexp → TOML string);
                 datetimes as tagged lists (toml-datetime "iso-string"). registered via trampolines in context.rs.
                 feature-gated behind `toml` cargo feature
  sexp_bridge.rs — Value ↔ Sexp bidirectional conversion; shared layer for format modules (json, toml, yaml)
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
  eval.c       — 5 patches: VFS module lookup (A + module policy gate), VFS load (B), VFS open-input-file (C),
                 macro expansion hook call in analyze_macro_once (D),
                 suppress false "importing undefined variable" for rust-registered bindings (E)
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
  lib/tein/json.sld  — (tein json) library definition + exports json-parse, json-stringify
  lib/tein/json.scm  — module documentation (trampolines registered by rust runtime)
  lib/tein/toml.sld  — (tein toml) library definition + exports toml-parse, toml-stringify
  lib/tein/toml.scm  — module documentation (trampolines registered by rust runtime)
  lib/tein/file.sld  — (tein file) library definition + exports file-exists?, delete-file
  lib/tein/file.scm  — module documentation (trampolines registered by rust runtime)
  lib/tein/load.sld  — (tein load) library definition + includes load.scm
  lib/tein/load.scm  — (define load tein-load-vfs-internal); trampoline registered as tein-load-vfs-internal
  lib/tein/process.sld — (tein process) library definition + exports get-environment-variable,
                        get-environment-variables, command-line, exit (NOT in SAFE_MODULES)
  lib/tein/process.scm — module documentation (trampolines registered by rust runtime)
build.rs       — fetches chibi fork, compiles it, generates install.h, tein_vfs_data.h, tein_clibs.c into OUT_DIR
examples/      — basic.rs, floats.rs, ffi.rs, debug.rs, sandbox.rs, foreign_types.rs
tests/         — scheme_tests.rs (integration runner), scheme/*.scm (scheme-level tests)
```

**data flow**: rust code → `Context::evaluate()` → arm_fuel() → ffi.rs safe wrappers → tein_shim.c → chibi-scheme vm → tein_fuel_consume_slice() at timeslice boundary → sexp result → `Value::from_raw()` → check_fuel() → rust `Value` enum

**standard env flow**: ContextBuilder with `.standard_env()` → load_standard_env (init-7 + meta-7 via VFS) → load_standard_ports → ~200 bindings (map, for-each, values, dynamic-wind, etc.)

**sandboxing flow**: ContextBuilder with presets → set IS_SANDBOXED thread-local → get source env (primitive or standard) → GC-root both envs → create null env (syntax-only) → copy allowed bindings via env_copy_named (handles renames, NULL-safe parent walk) → set as active env. `.allow(&["import"])` enables idiomatic r7rs imports (VFS-only via module policy). IS_SANDBOXED is used by (tein file) trampolines to distinguish unsandboxed (allow all) from sandboxed (require FsPolicy); restored to previous value on drop.

**IO policy flow**: ContextBuilder with file_read/file_write → capture original file-open procs from full env → register wrapper foreign fns in restricted env → set FsPolicy thread-local → wrapper checks path prefix via canonicalisation → delegates to original proc or returns policy violation

**exit escape hatch flow**: `(import (tein process))` → `(exit)` / `(exit obj)` sets EXIT_REQUESTED + EXIT_VALUE thread-locals + returns exception to stop VM immediately → eval loop (`evaluate`/`evaluate_port`/`call`) intercepts via `check_exit()` → clears flags → converts EXIT_VALUE to `Value` → returns `Ok(value)` to rust caller. `(exit)` → 0, `(exit #t)` → 0, `(exit #f)` → 1, `(exit obj)` → obj. does not invoke dynamic-wind cleanup (emergency-exit semantics). EXIT_REQUESTED/EXIT_VALUE cleared on Context::drop().

**module policy flow**: ContextBuilder with standard_env + presets → resolve policy (explicit builder policy, or default Allowlist(SAFE_MODULES + IMPLICIT_DEPS) for sandboxed, Unrestricted otherwise) → set MODULE_POLICY level (u8) + MODULE_ALLOWLIST (Vec<String>) thread-locals + C-level tein_module_policy → sexp_find_module_file_raw calls tein_module_allowed() → policy 0: allow all, policy 1: VFS prefix check only, policy 2: .sld files checked via rust callback (tein_module_allowlist_check) strips /vfs/lib/ prefix and checks against MODULE_ALLOWLIST; .scm includes pass unconditionally (reachable only after .sld allowed) → policy + allowlist cleared on Context::drop() via RAII

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

**import warning suppression (eval.c patch E)**: `define_fn_variadic` registers bindings into the top-level env, not the library env. chibi's `sexp_env_import_op` would normally warn "importing undefined variable" for these because they're absent from the library's `.scm`. the fork patch suppresses the warning when `oldcell` (destination env lookup) is non-NULL — meaning the name is already reachable. NOTE: ext foreign type method convenience procs previously had doubled name prefixes (#69, fixed).

**type checking order**: `from_raw` checks in broadest-first order: `complex → ratio → bignum → flonum → integer`. the integer predicate includes `_or_integer_flonump` and will match floats like 4.0 (garbage integer values) — flonum must come before integer. similarly, ratio and bignum are heap-allocated numbers chibi checks before fixnums/flonums; complex is the broadest and must be outermost.

**numeric tower shim functions**: `sexp_bignum_to_string(ctx, x)` — opens a string port, writes bignum in decimal, returns string sexp (allocates). `sexp_string_to_number(ctx, str, base)` — parses a scheme string as a number (used for `Bignum::to_raw`). `sexp_make_ratio(ctx, num, den)` / `sexp_make_complex(ctx, real, imag)` — constructors for to_raw; the first argument must be GC-rooted before calling (both may allocate).

**chibi feature flags**: on linux, `SEXP_USE_GREEN_THREADS` defaults to 1, so the `threads` cond-expand feature is active (affects which VFS files are loaded, e.g. `srfi/39/syntax.scm` vs `syntax-no-threads.scm`). `full-unicode` is always enabled (affects `scheme/char.sld` path selection).

**json alist round-trip via chibi**: `Value::from_raw` collapses dotted pairs `(key . val)` into proper lists when `val` is itself a proper list — e.g. `("x" . (("y" . 1)))` becomes `Value::List(["x", Value::Pair("y",1)])`. this loses alist structure needed for json object detection. `json_stringify_raw` (used by the scheme trampoline) works directly at the raw sexp level to detect alist entries via `sexp_pairp + sexp_stringp(car)`, bypassing `from_raw`. the rust-only `json_stringify` path (test-only) via sexp_bridge remains correct since it operates on hand-built `Value`s that haven't been through chibi.

**load trampoline internal naming**: the VFS-restricted `load` function is registered globally as `tein-load-vfs-internal` (not `load`). chibi's built-in `load` is used by the module loader for `(include ...)` in `.sld` files — overriding it globally breaks all module imports. `(tein load)` exports it as `load` via `(define load tein-load-vfs-internal)` in `load.scm` (included by `load.sld`). NOTE: `(begin (define ...))` in a library body doesn't resolve globals — use `(include ...)` instead.

**SAFE_MODULES excludes (tein process)**: the module allowlist for `Preset::Safe` does not include `tein/process` because `command-line` leaks the host's argv. use `.allow_module("tein/process")` or `.vfs_all()` to enable it explicitly.

**edition 2024:** `unsafe fn` bodies need inner `unsafe { }` blocks

## adding a new scheme type

1. add predicate wrapper to `tein_shim.c` in the fork (emesal/chibi-scheme, branch emesal-tein)
2. add extern declaration + safe wrapper in `src/ffi.rs`
3. add variant to `Value` enum in `src/value.rs`
4. add extraction in `Value::from_raw()` (respect type check ordering!)
5. add `to_raw()` conversion
6. add Display impl
7. add test in `src/context.rs`

## license
- ISC
