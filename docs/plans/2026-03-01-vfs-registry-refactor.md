# VFS registry refactor — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** replace the dual-list VFS module registry + preset sandbox system with a single-source-of-truth `VfsRegistry`, auto-extracted export stubs, and module-level sandboxing API.

**Architecture:** one `VfsEntry` registry (shared via `include!` between `sandbox.rs` and `build.rs`) declares files, deps, clib, feature gates, and safety tier per module. build.rs uses `tein-sexp` to parse `.sld` exports and generates a stub table. the sandbox starts from a null env + `import`, modules provide all bindings, UX stubs inform LLMs about missing imports.

**Tech Stack:** rust, tein-sexp (build dependency), chibi-scheme C FFI

**Design:** `docs/plans/2026-03-01-vfs-registry-refactor-design.md`

**Base branch:** dev

---

## overview

tasks are ordered for incremental compilation — each task produces a compiling, testable state. the refactor has four phases:

1. **registry data structure** (tasks 1–3): create `VfsEntry` registry, wire into build.rs
2. **export extraction** (tasks 4–5): add tein-sexp build dep, auto-extract exports
3. **sandbox API** (tasks 6–9): new `Modules` enum + `sandboxed()` builder, env construction, IO wrappers, UX stubs
4. **cleanup** (tasks 10–12): remove old code, update examples/docs/AGENTS.md, final test pass

### implementation notes

- `just feature refactor/vfs-registry-2603` to create the branch
- after each batch (indicated by `---` separators), run `just lint`, update this plan with progress notes, commit, and halt for context refresh
- **worktree not needed** — working directly on the branch
- the `VFS_FILES` list in build.rs and `VFS_MODULES_SAFE`/`VFS_MODULES_ALL` in sandbox.rs stay intact and functional until task 10 removes them — this keeps everything compiling between tasks
- `IS_SANDBOXED` thread-local stays — it's used by `(tein file)` trampolines to distinguish sandboxed from unsandboxed contexts and this concern is orthogonal to the preset removal
- `env_copy_named` FFI stays — still needed to copy `import` into the null env

---

## progress notes

**batch 2 complete (tasks 4–5).**

### implementation notes discovered

- `vfs_registry.rs` uses `#[allow(dead_code)]` on each struct/enum, not `#![allow(dead_code)]` — the inner attr form doesn't work in `include!`'d files
- `registry_safe_allowlist` / `registry_all_allowlist` have `#[allow(dead_code)]` until task 7 wires them into context.rs
- `validate_sld_includes` in build.rs caught 6 missing includes in the initial registry (char/ascii.scm, srfi/1/immutable's 9 scm files, srfi/27/constructors.scm, srfi/41.scm, srfi/95/sort.scm, srfi/135/kernel8.body.scm) — the validator is working as intended
- old `VFS_FILES` / `CLIB_ENTRIES` constants removed from build.rs immediately (were unused); old `VFS_MODULES_SAFE` / `VFS_MODULES_ALL` in sandbox.rs stay until task 10
- `feature_enabled` duplicated between build.rs and sandbox.rs (acceptable — they live in different compilation contexts; the include!'d vfs_registry.rs can't define it since it needs `cfg!()` which is context-dependent)
- registry now embeds significantly more files than old VFS_FILES (220 vs ~64) — all vetted modules get embedded, not just the minimal set. this is correct and intentional.
- srfi/18 clib entry added to registry (was previously absent; srfi/18 is pulled in by scheme/time but only used in the "all" set)
- **task 4**: `collect_exports_from_sexps` recurses via an inner `fn walk()` — `use SexpKind` must be in the outer fn scope for the inner fn to see it. `#[allow(dead_code)]` on the generated `MODULE_EXPORTS` const goes into the generated file itself (the `include!` macro doesn't accept attributes on itself)
- **task 4**: alias libraries (`scheme/bitwise`, `scheme/box`) use `(alias-for ...)` with no `(export ...)` — they produce empty exports lists. this is correct; they have no top-level names of their own to stub
- **task 5**: `map` appears in `srfi/1/immutable` and `srfi/101` as well as `scheme/base` — stub tests should use `+` or `number->string` (scheme/base-only) for precise assertions
- **task 5**: `#[allow(dead_code)]` on `include!` is silently ignored by rustc; instead put the allow attr in the generated file content

### next batch

bootstrap context: this plan file + branch `feature/refactor/vfs-registry-2603`. start at task 6.

---

## phase 1: registry data structure

### task 1: create `vfs_registry.rs` with `VfsEntry` and the registry

**files:**
- create: `tein/src/vfs_registry.rs`

create the shared registry file. this file must be self-contained (no imports from `tein` types) because build.rs will `include!` it.

define:
```rust
/// source type for a VFS module entry
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VfsSource {
    /// .sld/.scm files embedded at build time
    Embedded,
    /// registered at runtime via #[tein_module] — no files to embed
    Dynamic,
}

/// C static library backing for a module
#[derive(Clone, Copy, Debug)]
struct ClibEntry {
    /// C source file relative to chibi dir
    source: &'static str,
    /// init function suffix for the static lib table
    init_suffix: &'static str,
    /// VFS key for static lib lookup
    vfs_key: &'static str,
}

/// a single module in the VFS registry
#[derive(Clone, Debug)]
struct VfsEntry {
    /// module path, e.g. "scheme/char", "srfi/1"
    path: &'static str,
    /// transitive module deps (resolved at builder time)
    deps: &'static [&'static str],
    /// files to embed relative to chibi lib/
    files: &'static [&'static str],
    /// C static library, if native-backed
    clib: Option<ClibEntry>,
    /// in the default safe set?
    default_safe: bool,
    /// embedded or runtime-registered
    source: VfsSource,
    /// required cargo feature (None = always)
    feature: Option<&'static str>,
}
```

then populate `const VFS_REGISTRY: &[VfsEntry] = &[...]` with ALL modules currently in `VFS_MODULES_SAFE` + `VFS_MODULES_ALL`, plus clib info from `CLIB_ENTRIES`, plus file lists from `VFS_FILES`. each entry gets its `files` list from the current `VFS_FILES` entries for that module path prefix.

key decisions for specific entries:
- `scheme/eval`: `default_safe: false` (exports `eval` + `environment`)
- `scheme/repl`: `default_safe: false` (exports `interaction-environment`)
- `scheme/time`: `default_safe: false` (depends on unvetted modules)
- `scheme/show`: `default_safe: false` (depends on unvetted modules)
- `tein/process`: `default_safe: false` (leaks host argv)
- `tein/uuid`: `source: VfsSource::Dynamic, feature: Some("uuid"), files: &[]`
- `tein/time`: `source: VfsSource::Dynamic, feature: Some("time"), files: &[]`
- `tein/json`: `feature: Some("json")`
- `tein/toml`: `feature: Some("toml")`
- clib entries: `chibi/ast`, `chibi/io`, `srfi/39`, `srfi/69`, `srfi/151`, `tein/reader`, `tein/macro`
- bootstrap files (`init-7.scm`, `meta-7.scm`) are NOT in the registry

this is the largest single task — the registry has ~80 entries. tedious but mechanical: translate existing data into the new struct.

**step 1:** create `tein/src/vfs_registry.rs` with the structs and full registry
**step 2:** `cargo check -p tein` — file exists but isn't referenced yet, should compile
**step 3:** commit

### task 2: wire registry into sandbox.rs

**files:**
- modify: `tein/src/sandbox.rs`

add `include!("vfs_registry.rs")` at the top of sandbox.rs (after the existing imports).

add helper functions that read from the new registry, parallel to the existing ones:
```rust
/// resolve transitive deps from the VFS_REGISTRY
pub fn registry_resolve_deps(paths: &[&str]) -> Vec<String> { ... }

/// build default safe allowlist from VFS_REGISTRY
pub(crate) fn registry_safe_allowlist() -> Vec<String> { ... }

/// build full allowlist from VFS_REGISTRY
pub(crate) fn registry_all_allowlist() -> Vec<String> { ... }

/// get all VFS files to embed from VFS_REGISTRY (for build.rs)
fn registry_vfs_files() -> Vec<&'static str> { ... }

/// get all clib entries from VFS_REGISTRY (for build.rs)
fn registry_clib_entries() -> Vec<&'static ClibEntry> { ... }
```

these new functions coexist with the old `resolve_module_deps` / `vfs_safe_allowlist` / `vfs_all_allowlist`. old code still works, new code is testable.

**step 1:** add `include!` and helper functions
**step 2:** add unit tests: `registry_safe_allowlist()` produces the expected module set, `registry_all_allowlist()` is a superset, `registry_resolve_deps` resolves transitive deps correctly
**step 3:** `cargo test -p tein -- registry` — tests pass
**step 4:** commit

### task 3: wire registry into build.rs

**files:**
- modify: `tein/build.rs`
- modify: `tein/Cargo.toml` (add tein-sexp build dependency)

add `tein-sexp` as a build dependency in `tein/Cargo.toml`:
```toml
[build-dependencies]
cc = "1.0"
tein-sexp = { path = "../tein-sexp" }
```

in build.rs, `include!("src/vfs_registry.rs")` and replace the three existing data sources:
- `VFS_FILES` → `VFS_REGISTRY.iter().filter(|e| e.source == VfsSource::Embedded).flat_map(|e| e.files)`
  - also handle feature gates: skip entries whose `feature` doesn't match `cfg!(feature = ...)`
  - keep bootstrap files (`init-7.scm`, `meta-7.scm`) as separate hardcoded entries
- `VFS_FILES_JSON` / `VFS_FILES_TOML` → subsumed by the feature-gated registry entries
- `CLIB_ENTRIES` → `VFS_REGISTRY.iter().filter_map(|e| e.clib.as_ref())`

add `.sld` include-validation pass: for each embedded `.sld` in the registry, use `tein_sexp::parser::parse_all` to parse it, walk the tree to find `(include ...)` / `(include-ci ...)` directives (including inside `cond-expand` branches), and verify all referenced files appear in the entry's `files` list. panic with a clear message if mismatched.

leave `generate_vfs_data`, `generate_clibs`, `generate_install_h` structure intact — only change the input data they iterate over.

**step 1:** add tein-sexp build dep to Cargo.toml
**step 2:** add `include!` and replace data sources in build.rs
**step 3:** add `.sld` include-validation pass
**step 4:** `cargo build -p tein` — build succeeds, VFS data identical
**step 5:** commit

---

*batch 1 checkpoint: `just lint`, update plan, commit, halt*

---

## phase 2: export extraction

### task 4: add export extraction to build.rs

**files:**
- modify: `tein/build.rs`

add a function `extract_exports(chibi_dir, registry) -> HashMap<String, Vec<String>>` that:
1. for each registry entry with `source: VfsSource::Embedded`, finds the `.sld` file in `files`
2. parses it with `tein_sexp::parser::parse_all`
3. walks the parsed tree to find `(export ...)` forms
4. extracts symbol names, handling `(rename old new)` → takes `new`
5. for dynamic modules (uuid, time), hardcode exports:
   - `tein/uuid`: `["make-uuid", "uuid?", "uuid-nil"]`
   - `tein/time`: `["current-second", "current-jiffy", "jiffies-per-second"]`

generate `tein_exports.rs` in `OUT_DIR` containing:
```rust
/// auto-generated by build.rs — module path → exported binding names
const MODULE_EXPORTS: &[(&str, &[&str])] = &[
    ("scheme/base", &["*", "+", "-", ...]),
    ("scheme/char", &["char-alphabetic?", ...]),
    // ...
];
```

**step 1:** implement `extract_exports` function
**step 2:** implement `generate_exports_rs` function that writes the const table
**step 3:** call both from `main()` after include-validation
**step 4:** `cargo build -p tein` — generates `tein_exports.rs` in OUT_DIR
**step 5:** commit

### task 5: include generated exports in sandbox.rs

**files:**
- modify: `tein/src/sandbox.rs`

add to sandbox.rs:
```rust
include!(concat!(env!("OUT_DIR"), "/tein_exports.rs"));
```

add helper functions:
```rust
/// look up exports for a module path
pub(crate) fn module_exports(path: &str) -> Option<&'static [&'static str]> {
    MODULE_EXPORTS.iter()
        .find(|(p, _)| *p == path)
        .map(|(_, exports)| *exports)
}

/// collect all exports from modules NOT in the given allowlist.
/// returns (binding_name, module_path) pairs for stub registration.
pub(crate) fn unexported_stubs(allowed_modules: &[String]) -> Vec<(&'static str, &'static str)> {
    let mut stubs = Vec::new();
    for (path, exports) in MODULE_EXPORTS.iter() {
        if !allowed_modules.iter().any(|a| a == path) {
            for name in exports.iter() {
                stubs.push((*name, *path));
            }
        }
    }
    stubs
}
```

add tests:
- `module_exports("scheme/base")` returns Some with `"+"` in it
- `module_exports("nonexistent")` returns None
- `unexported_stubs` with `["scheme/base"]` allowed does NOT contain `"+"`
- `unexported_stubs` with empty allowlist contains bindings from all modules

**step 1:** add include + helper functions
**step 2:** add tests
**step 3:** `cargo test -p tein -- module_exports` + `cargo test -p tein -- unexported_stubs`
**step 4:** commit

---

*batch 2 checkpoint: `just lint`, update plan, commit, halt*

---

## phase 3: sandbox API

### task 6: add `Modules` enum and `sandboxed()` builder method

**files:**
- modify: `tein/src/sandbox.rs`
- modify: `tein/src/context.rs`

add the `Modules` enum to sandbox.rs (after the existing code — don't remove old code yet):
```rust
/// module set configuration for sandboxed contexts
#[derive(Clone, Debug)]
pub enum Modules {
    /// conservative safe set — default for sandboxed contexts
    Safe,
    /// all vetted modules in the registry
    All,
    /// no modules — syntax + import only
    None,
    /// custom explicit module list (deps resolved automatically)
    Only(Vec<String>),
}

impl Modules {
    /// construct a custom module list
    pub fn only(modules: &[&str]) -> Self {
        Modules::Only(modules.iter().map(|s| s.to_string()).collect())
    }
}

impl Default for Modules {
    fn default() -> Self { Modules::Safe }
}
```

add to `ContextBuilder`:
- new field: `sandbox_modules: Option<Modules>`
- new method: `pub fn sandboxed(mut self, modules: Modules) -> Self`
  - sets `sandbox_modules = Some(modules)`
  - does NOT touch `allowed_primitives` — the two systems coexist for now

the `sandboxed()` method does not yet affect `build()` — that comes in task 7. this task only adds the data structure and builder method.

**step 1:** add `Modules` enum to sandbox.rs
**step 2:** add `sandbox_modules` field and `sandboxed()` method to ContextBuilder
**step 3:** add test: `Context::builder().standard_env().sandboxed(Modules::Safe)` compiles and produces a ContextBuilder
**step 4:** commit

### task 7: implement new sandbox env construction in `build()`

**files:**
- modify: `tein/src/context.rs`

add a new code path in `build()` that activates when `sandbox_modules` is `Some(...)`. this path runs *instead of* the existing `allowed_primitives` path — but the old path remains for now (guarded by `else if`).

new sandbox build flow:
```
if let Some(ref modules) = self.sandbox_modules {
    1. IS_SANDBOXED.with(|c| c.set(true))
    2. let source_env = sexp_context_env(ctx)  // full standard env
    3. let null_env = sexp_make_null_env(ctx, 7)
    4. GC root both envs
    5. copy "import" from source_env to null_env via env_copy_named
    6. resolve allowlist from Modules variant:
       - Safe → registry_safe_allowlist()
       - All → registry_all_allowlist()
       - None → vec![]
       - Only(list) → registry_resolve_deps(list)
    7. compute UX stubs via unexported_stubs(allowlist)
    8. register stubs as foreign procs in null_env (informative error message)
    9. set VFS gate with allowlist
   10. if file_read/file_write configured:
       - capture original IO procs from source_env
       - register wrapper fns in null_env
       - set FsPolicy
   11. sexp_context_env_set(ctx, null_env)
}
```

the UX stub function signature is the same as the old `sandbox_stub` but with a more informative message. define a new trampoline that reads the binding name from its registration and looks it up in the exports table to produce a message like: `"sandbox: 'map' requires (import (scheme base))"`.

since we can't easily pass per-binding data through the C function pointer, one approach: register each stub with a unique name via `sexp_define_foreign_proc` where the `name` parameter encodes the module info. the stub callback can extract it from `self` (the opcode). alternatively, generate a small lookup table in a thread-local.

simplest approach: use the same `sandbox_stub` callback but change the message format. the stub already receives the opcode name from chibi. replace the error message to include module info by building a thread-local `HashMap<String, String>` (binding name → module path) at sandbox build time, and looking it up in the stub callback.

**step 1:** add thread-local `STUB_MODULE_MAP: RefCell<HashMap<String, String>>` near existing thread-locals
**step 2:** implement the new sandbox build path (steps 1–11 above)
**step 3:** update the stub callback to look up module info from `STUB_MODULE_MAP`
**step 4:** add tests:
  - `sandboxed(Modules::Safe)` + `(import (scheme base)) (+ 1 2)` → 3
  - `sandboxed(Modules::None)` + `(+ 1 2)` → error mentioning scheme/base
  - `sandboxed(Modules::only(&["scheme/base"]))` + `(import (scheme base)) (+ 1 2)` → 3
  - `sandboxed(Modules::All)` + `(import (scheme write)) (begin (write 1) #t)` → #t
  - `sandboxed(Modules::Safe)` + `(import (scheme eval))` → import error (not in safe set)
**step 5:** commit

### task 8: integrate `file_read` / `file_write` with new sandbox path

**files:**
- modify: `tein/src/context.rs`

update `file_read()` and `file_write()` builder methods: when `sandbox_modules` is set, don't touch `allowed_primitives` or presets. instead, just record the prefixes. the new build path (task 7) already handles IO wrapper registration.

when `sandbox_modules` is not set and `file_read`/`file_write` is called without `sandboxed()`, auto-set `sandbox_modules = Some(Modules::Safe)` to activate the new path. this means `file_read`/`file_write` implies sandboxing (which is the current behaviour).

**step 1:** update `file_read()` / `file_write()` to work with `sandbox_modules`
**step 2:** add tests:
  - `sandboxed(Modules::Safe).file_read(&["/tmp/"])` + reading from /tmp/ works
  - `sandboxed(Modules::Safe).file_read(&["/tmp/"])` + reading from /etc/ blocked
  - `file_read(&["/tmp/"])` without explicit `sandboxed()` still works (auto-sandbox)
**step 3:** commit

### task 9: ensure `allow_module()` works with new sandbox path

**files:**
- modify: `tein/src/context.rs`

update `allow_module()` to work with the `sandbox_modules` field:
- if `sandbox_modules` is `Some(...)`, resolve deps from `registry_resolve_deps` instead of the old `resolve_module_deps`
- `vfs_gate_all()` and `vfs_gate_none()` work the same (they set explicit VFS gates)

**step 1:** update `allow_module()` to use registry functions
**step 2:** add tests:
  - `sandboxed(Modules::Safe).allow_module("tein/process")` + `(import (tein process)) (exit 0)` → Value::Integer(0)
  - `sandboxed(Modules::Safe).allow_module("scheme/eval")` + `(import (scheme eval))` works
**step 3:** commit

---

*batch 3 checkpoint: `just lint`, update plan, commit, halt*

---

## phase 4: cleanup

### task 10: remove old preset system

**files:**
- modify: `tein/src/sandbox.rs` — remove: `Preset` struct, all 16 preset constants, `ALL_PRESETS`, `ALWAYS_STUB`, `VFS_MODULES_SAFE`, `VFS_MODULES_ALL`, old `resolve_module_deps`, `vfs_safe_allowlist`, `vfs_all_allowlist`, `VfsModule` struct. keep: `FsPolicy`, `VfsGate`, `FsAccess`, thread-locals, `Modules` enum, new registry helpers
- modify: `tein/src/context.rs` — remove: `allowed_primitives` field, `preset()`, `allow()`, `pure_computation()`, `safe()` methods, old sandbox build path in `build()`, `has_io_wrappers` field. update: `file_read()`/`file_write()` to no longer reference presets. update `Context` struct: remove `has_io_wrappers`
- modify: `tein/src/lib.rs` — update `pub mod sandbox` re-exports if needed, update module docstring
- modify: `tein/src/ffi.rs` — keep `env_copy_named` (still needed for `import`)

**step 1:** remove old code from sandbox.rs (keep FsPolicy, VfsGate, thread-locals, Modules, registry)
**step 2:** remove old builder methods and build path from context.rs
**step 3:** `cargo check -p tein` — fix all compilation errors from removed items
**step 4:** commit (compiles but tests broken — expected)

### task 11: update all tests

**files:**
- modify: `tein/src/context.rs` — all test functions using `.preset()`, `.safe()`, `.allow()`, `.pure_computation()`
- modify: `tein/tests/scheme_tests.rs` — sandboxed test helper
- modify: `tein/tests/tein_uuid.rs` — `.safe().allow(&["import"])` → `.sandboxed(Modules::Safe)`
- modify: `tein/tests/tein_time.rs` — same
- modify: other test files referencing old API

migration pattern:
```rust
// old                                          // new
.safe()                                         .sandboxed(Modules::Safe)
.safe().allow(&["import"])                      .sandboxed(Modules::Safe)
.preset(&ARITHMETIC)                            .sandboxed(Modules::only(&["scheme/base"]))
.preset(&ARITHMETIC).allow(&["import"])         .sandboxed(Modules::only(&["scheme/base"]))
.preset(&ARITHMETIC).preset(&LISTS)             .sandboxed(Modules::only(&["scheme/base"]))
.pure_computation()                             .sandboxed(Modules::only(&["scheme/base"]))
```

note: many old tests tested fine-grained preset behaviour (e.g. "cons blocked in ARITHMETIC-only"). these tests should be replaced with module-level equivalents (e.g. "cons requires `(import (scheme base))`"). the sandbox-stub tests become UX-stub tests.

tests that tested `ALWAYS_STUB` behaviour (eval escape attempts) should become tests that verify `scheme/eval` and `scheme/repl` are not importable in `Modules::Safe`.

**step 1:** update all test functions (mechanical migration)
**step 2:** `just test` — all tests pass
**step 3:** commit

### task 12: update examples, docs, AGENTS.md

**files:**
- modify: `tein/examples/sandbox.rs` — rewrite to use `sandboxed(Modules::Safe)` etc.
- modify: `AGENTS.md` — update sandboxing flow, remove preset references, update architecture
- modify: `ARCHITECTURE.md` — update sandbox description
- modify: `README.md` — update sandbox feature description
- modify: `docs/guide.md` — update sandbox examples
- modify: `tein/src/sandbox.rs` — update module docstring
- modify: `tein/src/context.rs` — update module docstring and ContextBuilder docstring

**step 1:** rewrite sandbox example
**step 2:** update AGENTS.md sandboxing flow + architecture section
**step 3:** update other docs
**step 4:** `cargo test --doc -p tein` — doc tests pass
**step 5:** `just lint` — all clean
**step 6:** commit

---

*batch 4 checkpoint: `just test` (full suite), final review, update plan with completion notes and any AGENTS.md gotchas discovered during implementation*

---

## completion checklist

- [ ] `just test` passes (all lib + integration + doc tests)
- [ ] `just lint` clean
- [ ] `cargo run --example sandbox` works
- [ ] no references to `Preset`, `ALL_PRESETS`, `ALWAYS_STUB`, `VFS_MODULES_SAFE`, `VFS_MODULES_ALL` remain in src/
- [ ] `scheme/eval` and `scheme/repl` not importable in `Modules::Safe`
- [ ] UX stubs produce informative messages naming the providing module
- [ ] build.rs validates `.sld` includes against registry
- [ ] build.rs validates `.sld` exports against extracted data
- [ ] AGENTS.md updated with new sandboxing flow
- [ ] collect any new AGENTS.md gotchas from implementation
