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

- [ ] **additional value types**
  - bytevectors, hash tables, ports, continuations (as opaque values)

### milestone 5 — reach

- [ ] **REPL example** — interactive scheme session with rustyline
- [ ] **WASM target** — chibi compiles via emscripten
- [ ] **serde data format** — s-expression ↔ rust structs via tein-sexp
- [ ] **macro expansion hooks**
- [ ] **custom reader extensions**

## ideas (unscheduled)

- **norse naming for modules?** core: `yggdrasil`, io: `bifrost`, macros: `galdr`, sexp: `runar`
- **scheme test harness** — run .scm files as cargo integration tests
- **context pooling / thread-local contexts** — ergonomic per-thread patterns
- **scheme-defined foreign type protocol** — rust types as opaque scheme objects
