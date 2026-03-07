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
just test                         # all tests (439 lib + 40 scheme + 58 vfs_module_tests (5 ignored) + tein-macros + ext_loading + doc-tests + 11 tein-bin)
cargo test -p tein --test vfs_module_tests  # chibi/srfi test suite integration (58 pass, 5 ignored)
cargo test test_name               # single test by name
cargo test --lib -- --nocapture    # lib tests with stdout
just lint                          # lint (cargo fmt + cargo clippy)
cargo fmt --check                  # format check
cargo run --example basic          # run an example (basic|floats|ffi|debug|sandbox|foreign_types|managed)
cargo test --features debug-chibi   # tests with chibi GC instrumentation (slower)
just clean && cargo build         # nuclear option if ffi gets weird
cargo build -p tein-test-ext       # build test cdylib extension
cargo test -p tein -- ext          # run extension integration tests
cargo build -p tein-bin            # build the tein binary
cargo run -p tein-bin              # run REPL
cargo run -p tein-bin -- script.scm  # run script
cargo test -p tein --features http  # http module tests
cargo test -p tein-bin             # unit tests (arg parsing, shebang, paren_depth)
```

## architecture

```
src/
  lib.rs         — public api re-exports
  context.rs     — Context, ContextBuilder: eval, fuel, env restriction, all tests;
                   load_extension(), build_ext_api(), ext trampolines, ExtApiGuard RAII
  value.rs       — Value enum: scheme↔rust conversion, cycle detection, Display
  error.rs       — Error enum
  ffi.rs         — unsafe c bindings + safe wrappers, GcRoot, `raw` module
  foreign.rs     — ForeignType trait, ForeignStore, dispatch_foreign_call;
                   ExtMethodEntry/ExtTypeEntry, MethodLookup (Static | Ext), find_method_any
  sandbox.rs     — Modules enum, FsPolicy, VFS_REGISTRY helpers, UX stub generation
  managed.rs     — ThreadLocalContext (persistent/fresh) on dedicated thread
  port.rs        — PortStore: Read/Write bridge via thread-local trampoline
  timeout.rs     — TimeoutContext: wall-clock timeout via dedicated thread
  json.rs        — json_parse + json_stringify_raw (raw sexp level, preserves alist)
  toml.rs        — toml_parse + toml_stringify_raw; datetimes as (toml-datetime "iso"). feature=toml
  uuid.rs        — #[tein_module]: make-uuid, uuid?, uuid-nil. feature=uuid
  time.rs        — #[tein_module]: current-second, current-jiffy, jiffies-per-second, timezone-offset-seconds. feature=time
  http.rs        — HTTP_SLD/HTTP_SCM constants, do_http_request (ureq), http_request_trampoline. feature=http
  safe_regexp.rs — #[tein_module("safe-regexp")]: regexp, regexp?, regexp-search, regexp-matches, regexp-replace,
                   regexp-split, regexp-extract, regexp-fold, match accessors. feature=regex
  sexp_bridge.rs — Value ↔ Sexp; shared layer for format modules
tein-ext/        — stable C ABI vtable for cdylib extensions (no chibi dependency)
tein-test-ext/   — in-tree test extension (publish=false); used by tests/ext_loading.rs
target/chibi-scheme/  — fetched from emesal/chibi-scheme (branch emesal-tein) by build.rs
  tein_shim.c    — chibi macro shims, fuel control, env_copy_named, VFS gate,
                   FS policy gate, custom ports, reader dispatch table, macro expansion hook
  eval.c         — 8 patches: VFS lookup+gate (A), VFS load (B), VFS open-input-file (C),
                   macro hook in analyze_macro_once (D), suppress false import warning (E),
                   FS policy gate in open-input-file (F), FS policy gate in open-output-file (G),
                   top-level native fn fallback in sexp_env_import_op (H)
  sexp.c         — 1 patch: reader dispatch before hardcoded # switch
  vm.c           — 2-line patch: fuel consumption at timeslice boundary
  lib/tein/      — tein scheme modules: foreign, reader, macro, test, json, toml,
                   uuid, time, file, load, process (see each .sld/.scm for exports)
build.rs         — fetches chibi fork, compiles it, generates install.h, tein_vfs_data.h, tein_clibs.c
examples/        — basic, floats, ffi, debug, sandbox, foreign_types, managed
tests/           — scheme_tests.rs (integration runner), scheme/*.scm
```

**data flow**: rust code → `Context::evaluate()` → arm_fuel() → ffi.rs safe wrappers → tein_shim.c → chibi-scheme vm → tein_fuel_consume_slice() at timeslice boundary → sexp result → `Value::from_raw()` → check_fuel() → rust `Value` enum

**standard env flow**: ContextBuilder with `.standard_env()` → load_standard_env (init-7 + meta-7 via VFS) → load_standard_ports → ~200 bindings (map, for-each, values, dynamic-wind, etc.)

**sandboxing flow**: ContextBuilder with presets → set IS_SANDBOXED thread-local → build full standard env → arm FS policy gate (C-level `tein_fs_policy_gate` + rust thread-local `FS_GATE`) → seed `SANDBOX_ENV` (`{"TEIN_SANDBOX": "true"}` merged with `.environment_variables()`) + `SANDBOX_COMMAND_LINE` (`["tein", "--sandbox"]` or `.command_line()` override) → inject VFS shadow modules (`register_vfs_shadows()`: dynamic VFS overrides for scheme/eval, scheme/load, scheme/repl, scheme/file, scheme/process-context, srfi/98) → resolve module allowlist from `Modules` variant via `VFS_REGISTRY` → set VFS gate + allowlist → GC-root source env + null env → create null env (syntax-only) → copy `import` via env_copy_named → register UX stubs for bindings not in the allowlist (each stub looks up providing module in `STUB_MODULE_MAP`) → set null env as active. IS_SANDBOXED + FS_GATE + SANDBOX_ENV + SANDBOX_COMMAND_LINE restored to previous values on drop. unsandboxed contexts ignore `.environment_variables()` / `.command_line()`.

**IO policy flow**: ContextBuilder with file_read/file_write → set FsPolicy thread-local → arm FS policy gate (sandboxed contexts only). `open-input-file` / `open-output-file` are chibi opcodes; eval.c patches F and G call `tein_fs_check_access()` before `fopen()` → C dispatcher checks gate level → if gate=1, calls rust callback `tein_fs_policy_check` → checks IS_SANDBOXED + FsPolicy prefix matching via canonicalisation → allows or denies. `file-exists?` and `delete-file` remain rust trampolines (no opcode equivalents).

**exit escape hatch flow**: `(import (tein process))` → `(exit)` / `(exit obj)` unwinds the `%dk` dynamic-wind stack via `travel-to-point!` (runs all "after" thunks innermost-first), flushes current output/error ports (r7rs requires flush, not close — closing may raise on custom ports), then calls `emergency-exit` (rust trampoline). `emergency-exit` sets EXIT_REQUESTED + EXIT_VALUE thread-locals + returns exception to stop VM immediately → eval loop (`evaluate`/`evaluate_port`/`call`) intercepts via `check_exit()` → clears flags → converts EXIT_VALUE to `Value` → returns `Ok(Value::Exit(n))` to rust caller. `(exit)` → 0, `(exit #t)` → 0, `(exit #f)` → 1, `(exit obj)` → obj. EXIT_REQUESTED/EXIT_VALUE cleared on Context::drop(). `emergency-exit` is r7rs-compliant: immediate halt, no cleanup. `exit` is r7rs-compliant: runs `dynamic-wind` "after" thunks and flushes ports before halting.

**VFS gate flow**: ContextBuilder with standard_env + sandboxed() → resolve gate (explicit builder gate, or default `Allow(registry_safe/all_allowlist())` for sandboxed, `Off` otherwise) → set `VFS_GATE` level (u8) + `VFS_ALLOWLIST` (Vec<String>) thread-locals + C-level `tein_vfs_gate` → `sexp_find_module_file_raw` calls `tein_module_allowed()` → gate 0: allow all, gate 1: rust callback `tein_vfs_gate_check` handles VFS `/vfs/lib/` prefix check, `..` traversal guard, `.scm` passthrough, allowlist prefix matching → gate + allowlist restored on `Context::drop()` via RAII. `allow_module()` resolves transitive deps from `VFS_REGISTRY` at builder time.

**sandboxed eval/load/repl flow**: `(scheme eval)` / `(scheme load)` / `(scheme repl)` are VFS shadow SLDs that wrap tein-specific trampolines. trampolines are registered into the primitive env BEFORE `load_standard_env` (in `build()`): `init-7.scm` builds `*chibi-env*` by importing all primitive-env bindings, so the trampolines end up in `*chibi-env*` and are available to any library body that `(import (chibi))`. `tein-environment-internal` (variadic, `extern "C"`) validates each import spec against `VFS_ALLOWLIST` in sandboxed mode, then calls chibi's `mutable-environment` via the meta env and marks the result immutable (r7rs: `environment` returns immutable envs). `tein-interaction-environment-internal` returns `sexp_context_env(ctx)` — the sandbox's own active env — GC-rooted and cached in `INTERACTION_ENV` thread-local on first call; subsequent calls return the same env so definitions accumulate across evals. **r7rs deviation**: `interaction-environment` returns the context env directly rather than a separate isolated env; correct for tein's single-context-per-scope embedding model. INTERACTION_ENV is released via `sexp_release_object` and cleared in `Context::drop()`.

**foreign type protocol flow**: `ctx.register_foreign_type::<T>()` → registers `ForeignType::methods()` in `ForeignStore` → injects `foreign-call`/`foreign-types`/`foreign-methods`/`foreign-type-methods` as native fns + pure-scheme `foreign?`/`foreign-type`/`foreign-handle-id` → auto-generates `type-name?` and `type-name-method` convenience procs. `ctx.foreign_value(v)` → inserts into store → returns `Value::Foreign { handle_id, type_name }`. scheme calls `(type-name-method obj)` → convenience proc → `(apply foreign-call obj 'method args)` → `foreign_call_wrapper` (extern "C") → reads `FOREIGN_STORE_PTR` thread-local → `dispatch_foreign_call` → looks up method by type name + method name → calls `MethodFn` with `&mut dyn Any` → returns `Value`. `FOREIGN_STORE_PTR` is set by `evaluate()`/`call()` via `ForeignStoreGuard` RAII.

**managed context flow**: `ContextBuilder::build_managed(init)` → spawns dedicated thread → builds Context on that thread → runs init closure → signals ready. subsequent `evaluate()`/`call()` → send `Request` over channel → thread processes → sends `Response` back. `reset()` → sends `Request::Reset` → thread rebuilds context + reruns init. `build_managed_fresh()` → same, but rebuilds before every evaluation (no state leakage).

**custom port flow**: `ctx.open_input_port(reader)` → inserts `Box<dyn Read>` into `PortStore` → thread-local trampoline (`port_read_trampoline`) bridges chibi's buffer-fill callback to rust `Read`. output ports mirror via `port_write_trampoline`. `ctx.read(&port)` → one s-expression; `ctx.evaluate_port(&port)` → loops read+eval. `ctx.set_current_output_port(&port)` / `set_current_input_port` / `set_current_error_port` replace the default port parameter so all subsequent scheme IO goes through the custom port. uses `sexp_set_parameter` under the hood. **buffering**: chibi custom ports (non-`SEXP_USE_STRING_STREAMS`) use a 4096-byte buffer — the rust write proc is only called during `sexp_buffered_flush`, not on every scheme write. callers MUST call `(flush-output (current-output-port))` after scheme code that produces output, otherwise bytes stay in the buffer and the write proc never fires.

**reader dispatch flow**: `ctx.register_reader('j', &handler)` or `(import (tein reader)) (set-reader! #\j handler)` → stored in thread-local dispatch table. patched `sexp.c` checks table before hardcoded `#` switch; handler receives input port, returns datum. dispatch table cleared on `Context::drop()`. reserved r7rs chars (`#t`, `#f`, `#\`, `#(`, numeric prefixes) cannot be overridden.

**macro expansion hook flow**: `ctx.set_macro_expand_hook(&proc)` or `(import (tein macro)) (set-macro-expand-hook! proc)` → stored in thread-local with GC preservation. called in patched `analyze_macro_once()` with `(name unexpanded expanded env)`; return value replaces expanded form and is re-analysed (replace-and-reanalyse semantics). recursion guard prevents re-entrant hook calls. hook cleared on `Context::drop()`.

**cdylib extension flow**: `ctx.load_extension(path)` → `libloading::Library::new(path)` → resolves `tein_ext_init` symbol → builds `TeinExtApi` vtable populated with trampolines into `ffi::*` → sets `FOREIGN_STORE_PTR` + `EXT_API` thread-locals → calls `tein_ext_init(ctx, &api)`. the extension's generated init fn (from `#[tein_module("name", ext = true)]`) checks API version, stores api pointer in `__TEIN_API` thread-local, calls `register_vfs_module`/`register_foreign_type`/`define_fn_variadic` through vtable. for ext foreign types, `ext_trampoline_register_type` builds `ExtTypeEntry` in `ForeignStore` with `TeinMethodFn` pointers; dispatch routes through `MethodLookup::Ext { func, is_mut }` passing `*mut c_void` and the api table. the shared library is leaked (no unload). `EXT_API` is also set during `evaluate()`/`call()` so ext method dispatch has access to the vtable at any time.

**dynamic module registration flow**: `ctx.register_module(source)` → sexp_read to parse define-library → extract library name → collision check via `tein_vfs_lookup_static` (rejects built-in modules) → reject `(include ...)` → register source as `/vfs/lib/<path>.sld` via `tein_vfs_register` → append to live `VFS_ALLOWLIST`. scheme-side: `(tein modules)` exports `register-module` (trampoline via CONTEXT_PTR → `ctx.register_module()`) and `module-registered?`. gated in sandbox via `.allow_dynamic_modules()` (= `.allow_module("tein/modules")`). chibi caches modules after first import — re-registration does not invalidate.

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
- **module path list and the empty-path UB** — `sexp_find_module_file_raw` reads `dir[-1]` on empty path (UB). user-supplied paths via `ContextBuilder::module_path()` / `TEIN_MODULE_PATH` / `-I` are safe because each is run through `std::path::Path::canonicalize()` (which always returns a non-empty absolute path) and empty env-var tokens are filtered before reaching `sexp_add_module_directory`. never add paths to chibi's module list without canonicalization.
- **`SEXP_USE_STRICT_TOPLEVEL_BINDINGS=1`** (default) — must stay enabled; without it, `analyze_bind_syntax` has a potential NULL deref.
- **`CHIBI_MODULE_PATH` env var** — read by chibi's module resolver. our VFS gate blocks non-VFS paths at the C level so it can't escape the sandbox, but document that this env var exists.

## critical gotchas

**`#[tein_module]` inside tein itself**: requires `extern crate self as tein;` in `lib.rs` because the macro generates `tein::*` paths. the mod also needs `pub(crate)` visibility so `context.rs` can call the generated `register_module_*` fn via `crate::mod_name::inner::register_module_name(&ctx)`.

**`Value` arg in `#[tein_fn]` free fns**: use `value: Value` in the fn signature to accept any scheme value. extraction uses `Value::from_raw`; `Value` is brought into scope automatically by the macro. useful for predicates that accept heterogeneous input (e.g. `uuid?`).

**`#[tein_fn]` supported return types**: `i64`, `f64`, `String`, `bool`, `Value`, `()`, and `Result<T, E>` where T is any of those. `Vec<u8>` is **not** supported — return `Value::Bytevector(vec)` instead. `Value` is the escape hatch for any scheme type not directly mapped (bytevectors, booleans from complex logic, etc.).

**tein_const scheme naming**: constants get no module prefix — `#[tein_const] pub const GREETING` in module `"foo"` → scheme name `greeting`, not `foo-greeting`. free fns do get the prefix (`foo-greet`).

**Result::Err raises a scheme exception**: `fn foo() -> Result<i64, String>` — the `Err` path calls `make_error(msg)` which creates a proper r7rs error object. in scheme, catch with `(guard (exn ((error-object? exn) (error-object-message exn))) ...)`. in rust, `evaluate()` returns `Err(Error::EvalError(msg))`. same in internal and ext mode.

**import warning suppression (eval.c patch E)**: `define_fn_variadic` registers bindings into the top-level env, not the library env. chibi's `sexp_env_import_op` would normally warn "importing undefined variable" for these because they're absent from the library's `.scm`. the fork patch suppresses the warning when `oldcell` (destination env lookup) is non-NULL — meaning the name is already reachable. NOTE: ext foreign type method convenience procs previously had doubled name prefixes (#69, fixed).

**eval.c patch H — native proc import fallback**: `sexp_env_import_op` falls back to the top-level env when a name is absent from the source library env, importing it if it's a native procedure. required for any `define_fn_variadic`-registered proc to be importable as a transitive library dependency. if a new tein module exports native fns and needs to be used as a dep by pure-scheme libraries, patch H handles it automatically.

**`(srfi 19)` deviations from spec**: `time-process` and `time-thread` raise `unsupported-clock-type` (no process/thread CPU clock in tein). `time-gc` type removed entirely. leap second table is static, last entry 2017.

**`date->julian-day` reference bug fixed**: the reference implementation had `(/ ... (- offset))` which divides by zero for UTC (offset=0). tein fix: `(- (/ time-portion tm:sid) (/ offset tm:sid))`.

**type checking order**: `from_raw` checks in broadest-first order: `complex → ratio → bignum → flonum → integer`. the integer predicate includes `_or_integer_flonump` and will match floats like 4.0 (garbage integer values) — flonum must come before integer. similarly, ratio and bignum are heap-allocated numbers chibi checks before fixnums/flonums; complex is the broadest and must be outermost.

**json alist round-trip via chibi**: `Value::from_raw` collapses dotted pairs `(key . val)` into proper lists when `val` is itself a proper list — e.g. `("x" . (("y" . 1)))` becomes `Value::List(["x", Value::Pair("y",1)])`. this loses alist structure needed for json object detection. `json_stringify_raw` (used by the scheme trampoline) works directly at the raw sexp level to detect alist entries via `sexp_pairp + sexp_stringp(car)`, bypassing `from_raw`. the rust-only `json_stringify` path (test-only) via sexp_bridge remains correct since it operates on hand-built `Value`s that haven't been through chibi.

**load trampoline internal naming**: the VFS-restricted `load` function is registered globally as `tein-load-vfs-internal` (not `load`). chibi's built-in `load` is used by the module loader for `(include ...)` in `.sld` files — overriding it globally breaks all module imports. `(tein load)` exports it as `load` via `(export (rename tein-load-vfs-internal load))` in `load.sld`.

**GC rooting in rust FFI**: chibi's conservative stack scanning is disabled — the GC does NOT see rust locals. any `sexp` held across an allocating FFI call can be freed. use `ffi::GcRoot::new(ctx, sexp)` (RAII, calls `sexp_preserve_object`/`sexp_release_object`). root across: list/pair/vector building loops, `evaluate()`'s read/eval loop, `call()`'s arg accumulator, `build()`'s source_env + null_env. allocating calls: `sexp_make_flonum`, `sexp_c_str`, `sexp_intern`, `sexp_cons`, `sexp_make_vector`, `sexp_open_input_string`, `sexp_read`, `sexp_evaluate`, `sexp_load_standard_env`, `sexp_make_null_env`, `sexp_env_define`, `env_copy_named`, `sexp_define_foreign_proc`, `sexp_preserve_object`. in C code use `sexp_gc_var`/`sexp_gc_preserve`/`sexp_gc_release`.

**edition 2024:** `unsafe fn` bodies need inner `unsafe { }` blocks

**`Value::Exit(i32)` is not a scheme type**: produced only by `check_exit()` when `EXIT_REQUESTED` thread-local is set. `Value::from_raw()` never produces it — do not add it to the type-checking dispatch in `from_raw`. `to_raw()` and `sexp_bridge` both return `Err` for `Exit`. used by embedders (and `tein-bin`) to distinguish an `(exit n)` escape from a normal return value.

**`tein-bin` crate**: binary crate (`publish = false`), produces the `tein` executable. `rustyline` is a regular dep of `tein-bin`, not a dev-dep of `tein`.

**`CONTEXT_PTR` thread-local**: raw `*const Context` set during `evaluate()`/`call()`/`evaluate_port()`/`read()` alongside `FOREIGN_STORE_PTR`. lets trampolines call `Context` methods directly (e.g. `register_module`). cleared via `ContextPtrGuard` RAII on all exit paths. NOT set during `load_extension()`.

**`(tein modules)` is `default_safe: false`**: must use `.allow_dynamic_modules()` to make it available in sandboxed contexts. without it, the VFS gate blocks `(import (tein modules))`.

**chibi module cache vs dynamic re-registration**: `register_module` updates the VFS entry but chibi caches module environments after first `(import ...)`. a second `(import (my tool))` in the same context returns the cached (old) version. fresh context or `ManagedContext::reset()` required for updated imports.

**`register_module` collision check**: rejects if module `.sld` exists in the *static* VFS table (built-in modules). dynamic-over-dynamic is allowed (update semantics). collision check uses `tein_vfs_lookup_static` which skips the dynamic linked list.

**native fn visibility in VFS library bodies**: native fns registered via `define_fn_variadic` into the top-level env are NOT visible to library bodies loaded via `(import ...)` — chibi creates a fresh env per library. if a library's `.scm` body uses a native fn as a free variable, the trampoline must be registered into the **primitive env** (via `register_native_trampoline`) BEFORE `load_standard_env` so it ends up in `*chibi-env*`, and the `.sld` must `(import (chibi))`. this applies to `(tein http)`'s `http-request-internal`. native fns that are directly exported (like `json-parse`) work via eval.c patch H without this.

**`register_module` requires `(begin ...)`**: `(include ...)`, `(include-ci ...)`, and `(include-library-declarations ...)` are rejected. dynamically registered modules must be self-contained.

**`register-module` trampoline owns source string**: the trampoline copies the scheme string arg to a rust `String` before calling `register_module`, because `register_module` calls `sexp_read` which may trigger GC and relocate the original scheme string.

**`FS_MODULE_PATHS` thread-local**: populated during `Context::build()` for contexts with `module_path()` dirs or `TEIN_MODULE_PATH` env var. read by `tein_vfs_gate_check` to allow imports from user-supplied directories, and by `check_fs_access` to allow `open-input-file` reads during module loading. saved/restored on build/drop like all other gate thread-locals. orthogonal to `FsPolicy` — module search paths grant no runtime file IO write access and no read access outside the registered dirs.

**`TEIN_MODULE_PATH` env var**: colon-separated list of module search dirs, read during `build()`. lower priority than builder `module_path()` calls (env paths prepended first, builder paths prepended after — chibi searches last-prepended first). consistent with `CHIBI_MODULE_PATH` convention. works in sandboxed and unsandboxed contexts.

## adding a new scheme type

1. add predicate wrapper to `tein_shim.c` in the fork (emesal/chibi-scheme, branch emesal-tein)
2. add extern declaration + safe wrapper in `src/ffi.rs`
3. add variant to `Value` enum in `src/value.rs`
4. add extraction in `Value::from_raw()` (respect type check ordering!)
5. add `to_raw()` conversion
6. add Display impl
7. add test in `src/context.rs`

**`JIFFY_EPOCH` is process-global**: `time.rs` uses a `static OnceLock<Instant>` shared across all `Context` instances. `current-jiffy` values are process-relative, not context-relative — epoch is set on first call anywhere in the process. this is correct per r7rs ("constant within a single run of the program") but means two separate contexts share the same jiffy epoch.

**`(tein safe-regexp)` byte offsets**: match vector start/end values are byte offsets (rust regex semantics), not character offsets. for multi-byte unicode, these differ from scheme's char-indexed `substring`. use `regexp-match-submatch` for text extraction rather than raw offsets.

**`(tein safe-regexp)` naming**: the `Regexp` foreign type is named `"safe-regexp"`, so auto-generated method names are `safe-regexp-search`, `safe-regexp-matches`, etc. the user-facing API (`regexp-search`, `regexp-matches`, etc.) is native `#[tein_fn]` free fns with string-or-regexp dispatch via `ensure_regexp`. `regexp?` is also a manual free fn — `#[tein_type(name = "safe-regexp")]` auto-generates `safe-regexp?` with the type prefix but not the user-facing `regexp?` name.

**`(tein safe-regexp)` dynamic exports**: `tein/safe-regexp` uses `VfsSource::Dynamic` (no `.sld` for the scanner to parse). its exports are declared in `DYNAMIC_MODULE_EXPORTS` in `build.rs` — if new exports are added to the rust module, this table must be updated too or sandbox UX stubs will be wrong.

**`(tein safe-regexp)` `regexp-fold` native fn**: `regexp-fold` is a hand-written `unsafe extern "C" fn` (not macro-generated) registered via `define_fn_variadic`. it calls scheme closures via `ffi::sexp_apply_proc`. the minimal `.scm` is comment-only — all fns are native.

## vfs module test harness

`tests/vfs_module_tests.rs` wires chibi's bundled srfi/chibi test suites into cargo test.
- `run_chibi_test(module)` builds a `standard_env().with_vfs_shadows()` context, imports the module, calls `(run-tests)`, checks `(test-failure-count)`.
- `(chibi test)` is in the VFS as `default_safe: false` — not available in sandboxed contexts.
- **5 tests permanently ignored** (see `#[ignore]` annotations in source):
  - `srfi_33`: `bitwise-merge` implementation quirk in chibi
  - `srfi_35`: imports `(chibi repl)` — not in VFS
  - `srfi_166`: needs real `delete-file` — `chibi/filesystem` stub blocks it
  - `chibi_diff`: `edits->string/color` needs real TERM env var
  - `chibi_weak`: `(gc)` in test body → SIGSEGV in embedded chibi
- **excluded from harness entirely**: `chibi/regexp-test` (needs pcre), crypto/mime/memoize (fs/network), filesystem/process/system-test (OS-level), `srfi/179/231` (fuel concerns)
- **shadow SLD rules**: `VfsSource::Shadow` bodies must not import other `VfsSource::Shadow` modules (circular/ordering hazard). importing `VfsSource::Embedded` tein modules (e.g. `(tein load)`, `(tein time)`) is fine. allowed imports in shadow bodies: `(chibi)`, `(scheme base)`, and `VfsSource::Embedded` tein modules. **exception**: `scheme/file` imports `(only (chibi filesystem) ...)` — `chibi/filesystem` is a `VfsSource::Shadow` (generated safe stub in sandboxed contexts). this is intentional: the stub only exposes `delete-file` and `file-exists?` with safe semantics; no real FS access. never use `(define x x)` pattern — letrec* pre-binds to `#<unspecified>`.

## license
- ISC
