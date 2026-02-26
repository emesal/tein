# tein todo list

## Completed

- [x] **#1: Implement pair/list value extraction**
- [x] **#2: Add numeric types (floats)**
- [x] **#3: Add vector support**
- [x] **#4: Better error messages from exceptions**
- [x] **#5: Rust→Scheme FFI**

## Completed milestones

### Milestone 1 — ergonomics & round-trip

- [x] Typed extraction helpers on Value
- [x] Complete bidirectional value bridge
- [x] Multi-expression evaluation
- [x] File evaluation

### Milestone 2 — Scheme as extension language

- [x] Scheme→Rust callbacks (procedures as values)
- [x] Variadic foreign functions (`define_fn_variadic`)
- [x] `#[scheme_fn]` proc macro for ergonomic FFI

### Milestone 3 — tein-sexp pure Rust s-expression crate

- [x] `Sexp` AST with source spans
- [x] R7RS-compatible reader (lists, pairs, vectors, strings, chars, comments)
- [x] Comment preservation mode
- [x] Pretty printer with configurable output

### Milestone 4a — sandboxing & resource limits

- [x] `ContextBuilder` with fluent API (heap sizes, step limits, presets)
- [x] Fuel-based step limiting (thread-local counters + vm.c patch)
- [x] 14 allowlist-based sandbox presets (ARITHMETIC, MATH, LISTS, etc.)
- [x] Convenience methods: `.pure_computation()`, `.safe()`
- [x] `TimeoutContext` for wall-clock deadlines via dedicated thread
- [x] `Error::StepLimitExceeded` and `Error::Timeout` variants
- [x] Sandbox example (`examples/sandbox.rs`)

## Roadmap

### Milestone 4b — production hardening (continued)

- [x] **Parameterised IO presets**
  - Thread-local `FsPolicy` with path prefix matching + canonicalisation
  - Wrapper foreign functions for `open-{input,output,binary-input,binary-output}-file`
  - `.file_read(&["/config/"])`, `.file_write(&["/tmp/"])` builder API
  - Zero external dependencies (hand-rolled prefix matching)
  - Support presets for port read/write operations
  - Path traversal and symlink protection via canonicalisation

- [x] **R7RS standard environment**
  - [x] VFS + static libs + eval.c patches for module loading
  - [x] Rust API + sandbox integration (`Context::new_standard`, `ContextBuilder::standard_env`)
  - [x] Module import policy: VFS-only restriction in sandboxed standard-env contexts
    - C-level interception in `sexp_find_module_file_raw` via `tein_module_allowed()`
    - Automatic: `standard_env` + any preset → VfsOnly, no explicit API needed
  - [x] Import during standard env: GC rooting fix (default 8MB heap)
    - Root cause: Rust locals invisible to Chibi GC (no conservative stack scanning)
    - Fix: `sexp_preserve_object` in `evaluate()`, gc_preserve fix in sexp_load_op VFS patch
  - [x] Sandboxed import: `.allow(&["import"])` enables idiomatic R7RS imports in sandbox
    - GC fix: root `source_env` in sandbox build (survives across null env allocation)
    - NULL safety: guard env parent chain walk in `tein_env_copy_named`
    - VFS-only policy blocks filesystem modules, VFS modules work normally
  - [x] `test_module_policy_blocks_filesystem_import` (sandboxed import test)
  - [x] `test_standard_env_sandbox_allows_vfs_import` (VFS import in sandbox)

- [x] **Additional value types**
  - Char, bytevector, port (opaque), hash table (opaque or Other fallback)
  - Continuations already handled as Procedure (Chibi uses same type tag)

### Milestone 5 — Reach

- [x] **REPL example** — interactive Scheme session with rustyline
- [ ] **WASM target** — Chibi compiles via emscripten
- [x] **Serde data format** — s-expression ↔ Rust structs via tein-sexp (hardened: alist fix, Sexp value type, IO API, attribute compat)
- [x] **Macro expansion hooks**
  - Thread-local hook in `tein_shim.c` + eval.c patch in `analyze_macro_once()`
  - `(tein macro)` VFS module: `set-macro-expand-hook!`, `unset-macro-expand-hook!`, `macro-expand-hook`
  - `Context::set_macro_expand_hook`, `unset_macro_expand_hook`, `macro_expand_hook` Rust API
  - 3 individual native fns registered via `register_protocol_fns`, replace-and-reanalyse semantics, recursion guard, GC-safe
  - 13 tests: observation, transformation, reanalyse, unset, recursion, errors, introspection, cleanup, sandbox, Rust API, via import
- [x] **Custom ports** — Rust `Read`/`Write` as Scheme input/output ports
  - `open_input_port`/`open_output_port` → `PortStore` + thread-local trampoline
  - `read()` for single s-expression, `evaluate_port()` for read+eval loop
  - Chibi's `fopencookie` + `sexp_cookie_reader`/`writer` callback protocol
- [x] **Custom reader extensions**
  - `#x` hash dispatch via C-level thread-local table + patched sexp.c reader
  - `set-reader!`/`unset-reader!`/`reader-dispatch-chars` in standard env
  - `(tein reader)` VFS module for idiomatic imports
  - `Context::register_reader(char, &Value)` Rust convenience API
  - Reserved R7RS char protection, dispatch table cleared on context drop

### Milestone 6 — Foreign type protocol

- [x] **Foreign type protocol**
  - `ForeignType` trait + `ForeignStore` handle-map per context
  - `Value::Foreign { handle_id, type_name }` with tagged-list wire format
  - `(tein foreign)` VFS module: `foreign?`, `foreign-type`, `foreign-handle-id`
  - `foreign-call` / `foreign-methods` / `foreign-types` / `foreign-type-methods` native fns
  - Auto-generated `type-name?` predicates + `type-name-method` convenience procs
  - `FOREIGN_STORE_PTR` thread-local bridge with `ForeignStoreGuard` RAII
  - `ctx.foreign_value(v)`, `ctx.foreign_ref::<T>(&val)` Rust-side API
  - LLM-friendly error messages (lists available methods on wrong-method call)
  - 22 tests: registration, round-trip, dispatch, introspection, predicates, cleanup

### Milestone 7 — Managed contexts

- [x] **ThreadLocalContext** — `Send + Sync` managed context on a dedicated thread
  - Persistent mode: state accumulates, `reset()` tears down and rebuilds
  - Fresh mode: context rebuilt before every evaluation, no state leakage
  - Init closure: runs once (persistent) or before each call (fresh)
  - `ContextBuilder::build_managed(init)` / `build_managed_fresh(init)`
  - `ContextBuilder` gains `Clone` (required for fresh mode rebuild)
  - Shared channel protocol extracted to `thread.rs` (generalises `TimeoutContext`)
  - 14 tests: evaluate, state accumulation, init, call, `define_fn_variadic`, reset, error handling

## Ideas (unscheduled)

- **Norse naming for modules?** core: `yggdrasil`, io: `bifrost`, macros: `galdr`, sexp: `runar`
- **Scheme test harness** — run .scm files as cargo integration tests
- **Context pool** — pool of `ThreadLocalContext` instances for high-throughput workloads
- **Foreign type constructor macro** — ergonomic `make-type` registration from Rust
