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
just test                         # all tests (393 lib + 40 scheme + 58 vfs_module_tests (5 ignored) + tein-macros + ext_loading + doc-tests)
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

**sandboxing flow**: ContextBuilder with presets → set IS_SANDBOXED thread-local → build full standard env → arm FS policy gate (C-level `tein_fs_policy_gate` + rust thread-local `FS_GATE`) → seed `SANDBOX_ENV` (`{"TEIN_SANDBOX": "true"}` merged with `.environment_variables()`) + `SANDBOX_COMMAND_LINE` (`["tein", "--sandbox"]` or `.command_line()` override) → inject VFS shadow modules (`register_vfs_shadows()`: dynamic VFS overrides for scheme/file, scheme/repl, scheme/process-context, srfi/98) → resolve module allowlist from `Modules` variant via `VFS_REGISTRY` → set VFS gate + allowlist → GC-root source env + null env → create null env (syntax-only) → copy `import` via env_copy_named → register UX stubs for bindings not in the allowlist (each stub looks up providing module in `STUB_MODULE_MAP`) → set null env as active. IS_SANDBOXED + FS_GATE + SANDBOX_ENV + SANDBOX_COMMAND_LINE restored to previous values on drop. unsandboxed contexts ignore `.environment_variables()` / `.command_line()`.

**IO policy flow**: ContextBuilder with file_read/file_write → set FsPolicy thread-local → arm FS policy gate (sandboxed contexts only). `open-input-file` / `open-output-file` are chibi opcodes; eval.c patches F and G call `tein_fs_check_access()` before `fopen()` → C dispatcher checks gate level → if gate=1, calls rust callback `tein_fs_policy_check` → checks IS_SANDBOXED + FsPolicy prefix matching via canonicalisation → allows or denies. `file-exists?` and `delete-file` remain rust trampolines (no opcode equivalents).

**exit escape hatch flow**: `(import (tein process))` → `(exit)` / `(exit obj)` sets EXIT_REQUESTED + EXIT_VALUE thread-locals + returns exception to stop VM immediately → eval loop (`evaluate`/`evaluate_port`/`call`) intercepts via `check_exit()` → clears flags → converts EXIT_VALUE to `Value` → returns `Ok(value)` to rust caller. `(exit)` → 0, `(exit #t)` → 0, `(exit #f)` → 1, `(exit obj)` → obj. EXIT_REQUESTED/EXIT_VALUE cleared on Context::drop(). **r7rs deviation**: both `exit` and `emergency-exit` have emergency-exit semantics — neither runs `dynamic-wind` "after" thunks. r7rs `exit` should run them; doing so requires an unwind continuation around `evaluate()` which tein does not currently establish (GH #101). a future standalone interpreter host is expected to handle this.

**VFS gate flow**: ContextBuilder with standard_env + sandboxed() → resolve gate (explicit builder gate, or default `Allow(registry_safe/all_allowlist())` for sandboxed, `Off` otherwise) → set `VFS_GATE` level (u8) + `VFS_ALLOWLIST` (Vec<String>) thread-locals + C-level `tein_vfs_gate` → `sexp_find_module_file_raw` calls `tein_module_allowed()` → gate 0: allow all, gate 1: rust callback `tein_vfs_gate_check` handles VFS `/vfs/lib/` prefix check, `..` traversal guard, `.scm` passthrough, allowlist prefix matching → gate + allowlist restored on `Context::drop()` via RAII. `allow_module()` resolves transitive deps from `VFS_REGISTRY` at builder time.

**foreign type protocol flow**: `ctx.register_foreign_type::<T>()` → registers `ForeignType::methods()` in `ForeignStore` → injects `foreign-call`/`foreign-types`/`foreign-methods`/`foreign-type-methods` as native fns + pure-scheme `foreign?`/`foreign-type`/`foreign-handle-id` → auto-generates `type-name?` and `type-name-method` convenience procs. `ctx.foreign_value(v)` → inserts into store → returns `Value::Foreign { handle_id, type_name }`. scheme calls `(type-name-method obj)` → convenience proc → `(apply foreign-call obj 'method args)` → `foreign_call_wrapper` (extern "C") → reads `FOREIGN_STORE_PTR` thread-local → `dispatch_foreign_call` → looks up method by type name + method name → calls `MethodFn` with `&mut dyn Any` → returns `Value`. `FOREIGN_STORE_PTR` is set by `evaluate()`/`call()` via `ForeignStoreGuard` RAII.

**managed context flow**: `ContextBuilder::build_managed(init)` → spawns dedicated thread → builds Context on that thread → runs init closure → signals ready. subsequent `evaluate()`/`call()` → send `Request` over channel → thread processes → sends `Response` back. `reset()` → sends `Request::Reset` → thread rebuilds context + reruns init. `build_managed_fresh()` → same, but rebuilds before every evaluation (no state leakage).

**custom port flow**: `ctx.open_input_port(reader)` → inserts `Box<dyn Read>` into `PortStore` → thread-local trampoline (`port_read_trampoline`) bridges chibi's buffer-fill callback to rust `Read`. output ports mirror via `port_write_trampoline`. `ctx.read(&port)` → one s-expression; `ctx.evaluate_port(&port)` → loops read+eval.

**reader dispatch flow**: `ctx.register_reader('j', &handler)` or `(import (tein reader)) (set-reader! #\j handler)` → stored in thread-local dispatch table. patched `sexp.c` checks table before hardcoded `#` switch; handler receives input port, returns datum. dispatch table cleared on `Context::drop()`. reserved r7rs chars (`#t`, `#f`, `#\`, `#(`, numeric prefixes) cannot be overridden.

**macro expansion hook flow**: `ctx.set_macro_expand_hook(&proc)` or `(import (tein macro)) (set-macro-expand-hook! proc)` → stored in thread-local with GC preservation. called in patched `analyze_macro_once()` with `(name unexpanded expanded env)`; return value replaces expanded form and is re-analysed (replace-and-reanalyse semantics). recursion guard prevents re-entrant hook calls. hook cleared on `Context::drop()`.

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
- **`CHIBI_MODULE_PATH` env var** — read by chibi's module resolver. our VFS gate blocks non-VFS paths at the C level so it can't escape the sandbox, but document that this env var exists.

## critical gotchas

**`#[tein_module]` inside tein itself**: requires `extern crate self as tein;` in `lib.rs` because the macro generates `tein::*` paths. the mod also needs `pub(crate)` visibility so `context.rs` can call the generated `register_module_*` fn via `crate::mod_name::inner::register_module_name(&ctx)`.

**`Value` arg in `#[tein_fn]` free fns**: use `value: Value` in the fn signature to accept any scheme value. extraction uses `Value::from_raw`; `Value` is brought into scope automatically by the macro. useful for predicates that accept heterogeneous input (e.g. `uuid?`).

**tein_const scheme naming**: constants get no module prefix — `#[tein_const] pub const GREETING` in module `"foo"` → scheme name `greeting`, not `foo-greeting`. free fns do get the prefix (`foo-greet`).

**Result::Err returns a scheme string**: `fn foo() -> Result<i64, String>` — the `Err` path returns `sexp_c_str(msg)` which becomes `Value::String(msg)` in rust. it's not an exception; `(test-error ...)` won't catch it. match on `Value::String` instead. same in internal and ext mode.

**import warning suppression (eval.c patch E)**: `define_fn_variadic` registers bindings into the top-level env, not the library env. chibi's `sexp_env_import_op` would normally warn "importing undefined variable" for these because they're absent from the library's `.scm`. the fork patch suppresses the warning when `oldcell` (destination env lookup) is non-NULL — meaning the name is already reachable. NOTE: ext foreign type method convenience procs previously had doubled name prefixes (#69, fixed).

**eval.c patch H — native proc import fallback**: `sexp_env_import_op` falls back to the top-level env when a name is absent from the source library env, importing it if it's a native procedure. required for any `define_fn_variadic`-registered proc to be importable as a transitive library dependency. if a new tein module exports native fns and needs to be used as a dep by pure-scheme libraries, patch H handles it automatically.

**`(srfi 19)` deviations from spec**: `time-process` and `time-thread` raise `unsupported-clock-type` (no process/thread CPU clock in tein). `time-gc` type removed entirely. leap second table is static, last entry 2017.

**`date->julian-day` reference bug fixed**: the reference implementation had `(/ ... (- offset))` which divides by zero for UTC (offset=0). tein fix: `(- (/ time-portion tm:sid) (/ offset tm:sid))`.

**type checking order**: `from_raw` checks in broadest-first order: `complex → ratio → bignum → flonum → integer`. the integer predicate includes `_or_integer_flonump` and will match floats like 4.0 (garbage integer values) — flonum must come before integer. similarly, ratio and bignum are heap-allocated numbers chibi checks before fixnums/flonums; complex is the broadest and must be outermost.

**json alist round-trip via chibi**: `Value::from_raw` collapses dotted pairs `(key . val)` into proper lists when `val` is itself a proper list — e.g. `("x" . (("y" . 1)))` becomes `Value::List(["x", Value::Pair("y",1)])`. this loses alist structure needed for json object detection. `json_stringify_raw` (used by the scheme trampoline) works directly at the raw sexp level to detect alist entries via `sexp_pairp + sexp_stringp(car)`, bypassing `from_raw`. the rust-only `json_stringify` path (test-only) via sexp_bridge remains correct since it operates on hand-built `Value`s that haven't been through chibi.

**load trampoline internal naming**: the VFS-restricted `load` function is registered globally as `tein-load-vfs-internal` (not `load`). chibi's built-in `load` is used by the module loader for `(include ...)` in `.sld` files — overriding it globally breaks all module imports. `(tein load)` exports it as `load` via `(export (rename tein-load-vfs-internal load))` in `load.sld`.

**GC rooting in rust FFI**: chibi's conservative stack scanning is disabled — the GC does NOT see rust locals. any `sexp` held across an allocating FFI call can be freed. use `ffi::GcRoot::new(ctx, sexp)` (RAII, calls `sexp_preserve_object`/`sexp_release_object`). root across: list/pair/vector building loops, `evaluate()`'s read/eval loop, `call()`'s arg accumulator, `build()`'s source_env + null_env. allocating calls: `sexp_make_flonum`, `sexp_c_str`, `sexp_intern`, `sexp_cons`, `sexp_make_vector`, `sexp_open_input_string`, `sexp_read`, `sexp_evaluate`, `sexp_load_standard_env`, `sexp_make_null_env`, `sexp_env_define`, `env_copy_named`, `sexp_define_foreign_proc`, `sexp_preserve_object`. in C code use `sexp_gc_var`/`sexp_gc_preserve`/`sexp_gc_release`.

**edition 2024:** `unsafe fn` bodies need inner `unsafe { }` blocks

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
- **shadow SLD rules**: never import other shadow libraries inside a shadow library body — each must be self-contained with `(chibi)` or `(scheme base)` only. never use `(define x x)` pattern — letrec* pre-binds to `#<unspecified>`.

## license
- ISC
