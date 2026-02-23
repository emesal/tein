# tein todo list

## completed

- [x] **#1: implement pair/list value extraction**
- [x] **#2: add numeric types (floats)**
- [x] **#3: add vector support**
- [x] **#4: better error messages from exceptions**
- [x] **#5: rust→scheme ffi**

## completed milestones

### milestone 1 — ergonomics & round-trip

- [x] typed extraction helpers on Value
- [x] complete bidirectional value bridge
- [x] multi-expression evaluation
- [x] file evaluation

### milestone 2 — scheme as extension language

- [x] scheme→rust callbacks (procedures as values)
- [x] variadic foreign functions (`define_fn_variadic`)
- [x] `#[scheme_fn]` proc macro for ergonomic ffi

### milestone 3 — tein-sexp pure rust s-expression crate

- [x] `Sexp` AST with source spans
- [x] r7rs-compatible reader (lists, pairs, vectors, strings, chars, comments)
- [x] comment preservation mode
- [x] pretty printer with configurable output

### milestone 4a — sandboxing & resource limits

- [x] `ContextBuilder` with fluent api (heap sizes, step limits, presets)
- [x] fuel-based step limiting (thread-local counters + vm.c patch)
- [x] 14 allowlist-based sandbox presets (ARITHMETIC, MATH, LISTS, etc.)
- [x] convenience methods: `.pure_computation()`, `.safe()`
- [x] `TimeoutContext` for wall-clock deadlines via dedicated thread
- [x] `Error::StepLimitExceeded` and `Error::Timeout` variants
- [x] sandbox example (`examples/sandbox.rs`)

## roadmap

### milestone 4b — production hardening (continued)

- [x] **parameterised IO presets**
  - thread-local `FsPolicy` with path prefix matching + canonicalisation
  - wrapper foreign functions for `open-{input,output,binary-input,binary-output}-file`
  - `.file_read(&["/config/"])`, `.file_write(&["/tmp/"])` builder api
  - zero external dependencies (hand-rolled prefix matching)
  - support presets for port read/write operations
  - path traversal and symlink protection via canonicalisation

- [x] **r7rs standard environment**
  - [x] VFS + static libs + eval.c patches for module loading
  - [x] rust API + sandbox integration (Context::new_standard, ContextBuilder::standard_env)
  - [x] module import policy: VFS-only restriction in sandboxed standard-env contexts
    - C-level interception in sexp_find_module_file_raw via tein_module_allowed()
    - automatic: standard_env + any preset → VfsOnly, no explicit API needed
  - [x] import during standard env: GC rooting fix (default 4MB heap)
    - root cause: rust locals invisible to chibi GC (no conservative stack scanning)
    - fix: sexp_preserve_object in evaluate(), gc_preserve fix in sexp_load_op VFS patch
  - [x] sandboxed import: `.allow(&["import"])` enables idiomatic r7rs imports in sandbox
    - GC fix: root source_env in sandbox build (survives across null env allocation)
    - NULL safety: guard env parent chain walk in tein_env_copy_named
    - VFS-only policy blocks filesystem modules, VFS modules work normally
  - [x] test_module_policy_blocks_filesystem_import (sandboxed import test)
  - [x] test_standard_env_sandbox_allows_vfs_import (VFS import in sandbox)

- [x] **additional value types**
  - char, bytevector, port (opaque), hash table (opaque or Other fallback)
  - continuations already handled as Procedure (chibi uses same type tag)

### milestone 5 — reach

- [x] **REPL example** — interactive scheme session with rustyline
- [ ] **WASM target** — chibi compiles via emscripten
- [x] **serde data format** — s-expression ↔ rust structs via tein-sexp (hardened: alist fix, Sexp value type, IO API, attribute compat)
- [ ] **macro expansion hooks**
- [x] **custom ports** — rust `Read`/`Write` as scheme input/output ports
  - `open_input_port`/`open_output_port` → `PortStore` + thread-local trampoline
  - `read()` for single s-expression, `evaluate_port()` for read+eval loop
  - chibi's `fopencookie` + `sexp_cookie_reader`/`writer` callback protocol
- [x] **custom reader extensions**
  - `#x` hash dispatch via C-level thread-local table + patched sexp.c reader
  - `set-reader!`/`unset-reader!`/`reader-dispatch-chars` in standard env
  - `(tein reader)` VFS module for idiomatic imports
  - `Context::register_reader(char, &Value)` rust convenience API
  - reserved r7rs char protection, dispatch table cleared on context drop

### milestone 6 — foreign type protocol

- [x] **foreign type protocol**
  - `ForeignType` trait + `ForeignStore` handle-map per context
  - `Value::Foreign { handle_id, type_name }` with tagged-list wire format
  - `(tein foreign)` VFS module: `foreign?`, `foreign-type`, `foreign-handle-id`
  - `foreign-call` / `foreign-methods` / `foreign-types` / `foreign-type-methods` native fns
  - auto-generated `type-name?` predicates + `type-name-method` convenience procs
  - `FOREIGN_STORE_PTR` thread-local bridge with `ForeignStoreGuard` RAII
  - `ctx.foreign_value(v)`, `ctx.foreign_ref::<T>(&val)` rust-side API
  - LLM-friendly error messages (lists available methods on wrong-method call)
  - 22 tests: registration, round-trip, dispatch, introspection, predicates, cleanup

### milestone 7 — managed contexts

- [x] **ThreadLocalContext** — `Send + Sync` managed context on a dedicated thread
  - persistent mode: state accumulates, `reset()` tears down and rebuilds
  - fresh mode: context rebuilt before every evaluation, no state leakage
  - init closure: runs once (persistent) or before each call (fresh)
  - `ContextBuilder::build_managed(init)` / `build_managed_fresh(init)`
  - `ContextBuilder` gains `Clone` (required for fresh mode rebuild)
  - shared channel protocol extracted to `thread.rs` (generalises `TimeoutContext`)
  - 14 tests: evaluate, state accumulation, init, call, define_fn_variadic, reset, error handling

## ideas (unscheduled)

- **norse naming for modules?** core: `yggdrasil`, io: `bifrost`, macros: `galdr`, sexp: `runar`
- **scheme test harness** — run .scm files as cargo integration tests
- **context pool** — pool of `ThreadLocalContext` instances for high-throughput workloads
- **foreign type constructor macro** — ergonomic `make-type` registration from rust
