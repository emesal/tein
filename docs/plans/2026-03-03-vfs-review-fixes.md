# VFS Module System Review Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Apply the 8 issues surfaced by post-refactor code review of the VFS module system (vfs_registry + sandbox + shadow stubs).

**Architecture:** Fixes are isolated — documentation corrections, build-time dedup, code comments, and one new test. No API changes. Tasks are ordered by dependency: doc fixes first, then code, then test.

**Tech Stack:** Rust (build.rs, src/sandbox.rs, src/context.rs, src/vfs_registry.rs), Markdown (AGENTS.md)

---

## Background / context

After the VFS registry refactor (PR #96) and module inventory completion (PR #107), a code review identified 4 important issues and 4 suggestions. None affect correctness or security. All fixes are small and self-contained.

Key files:
- `AGENTS.md` — project quirks/gotchas doc
- `tein/build.rs` — build script: VFS file list, `generate_vfs_data`, `feature_enabled`, export extractor
- `tein/src/sandbox.rs` — `Modules` enum, `feature_enabled`, shadow registration
- `tein/src/vfs_registry.rs` — central `VFS_REGISTRY` const (shared via `include!`)
- `tein/src/context.rs` — `allow_module()` method, all tests

---

### Task 1: Remove stale AGENTS.md tein/process quirk note

**Files:**
- Modify: `AGENTS.md`

The note at line 136 says `Modules::Safe` excludes `tein/process`. This was true in an earlier design but is now wrong — the registry marks `tein/process` as `default_safe: true` and it IS in the safe allowlist. The note actively misleads agents.

**Step 1: Delete the stale paragraph**

In `AGENTS.md`, find and remove this block (around line 136):

```
**Modules::Safe excludes (tein process)**: `registry_safe_allowlist()` does not include `tein/process` because `command-line` leaks the host's argv. use `.sandboxed(Modules::Safe).allow_module("tein/process")` to enable it explicitly.
```

**Step 2: Verify no other stale references**

```bash
grep -n "tein/process\|tein process" AGENTS.md
```

Expected: no output (or only unrelated mentions). If any remain, check they're accurate.

**Step 3: Commit**

```bash
git add AGENTS.md
git commit -m "docs: remove stale AGENTS.md note — Modules::Safe includes tein/process"
```

---

### Task 2: Dedup vfs_files before generate_vfs_data

**Files:**
- Modify: `tein/build.rs` around line 418

Multiple registry entries list the same `.scm` file (e.g. all 12 `srfi/160/*` sub-entries include `lib/srfi/160/uvector.scm`; several `srfi/166` entries share `.scm` files). Since `generate_vfs_data` iterates the full list, these are embedded multiple times in `tein_vfs_data.h`. The VFS lookup returns the first match, so correctness is unaffected — but it's ~120KB+ of redundant binary data.

The fix: sort + dedup `vfs_files` before passing to `generate_vfs_data`. Order doesn't matter for the VFS table (all paths are stored; first-match at lookup time).

**Step 1: Write a test that would catch regression**

There isn't a great unit test for this (it's a build artefact check). We'll verify manually after the fix. Skip to step 2.

**Step 2: Add dedup after building vfs_files**

In `tein/build.rs`, find the block (around line 412-418):

```rust
// build the combined VFS file list from the registry (replaces VFS_FILES + feature gates)
let mut vfs_files: Vec<&str> = BOOTSTRAP_FILES.to_vec();
vfs_files.extend(
    VFS_REGISTRY
        .iter()
        .filter(|e| e.source == VfsSource::Embedded && feature_enabled(e.feature))
        .flat_map(|e| e.files.iter().copied()),
);
```

Add dedup immediately after the `extend`:

```rust
// build the combined VFS file list from the registry (replaces VFS_FILES + feature gates)
let mut vfs_files: Vec<&str> = BOOTSTRAP_FILES.to_vec();
vfs_files.extend(
    VFS_REGISTRY
        .iter()
        .filter(|e| e.source == VfsSource::Embedded && feature_enabled(e.feature))
        .flat_map(|e| e.files.iter().copied()),
);
// dedup: multiple entries may share the same .scm file (e.g. srfi/160/* all include uvector.scm).
// first-match semantics at VFS lookup time means order doesn't matter; just drop duplicates.
vfs_files.sort_unstable();
vfs_files.dedup();
```

**Step 3: Verify build succeeds**

```bash
cargo build 2>&1 | tail -5
```

Expected: `Compiling tein ...` then `Finished`. No warnings about duplicate VFS entries.

**Step 4: Optionally verify dedup effect**

```bash
# count unique vs total files in the generated header
grep "tein_vfs_content_" target/debug/build/tein-*/out/tein_vfs_data.h | wc -l
# before fix this was ~250+; after it should match the number of unique files
```

**Step 5: Commit**

```bash
git add tein/build.rs
git commit -m "fix: dedup vfs_files before generate_vfs_data — removes ~120KB+ redundant binary data"
```

---

### Task 3: Cross-reference the duplicated feature_enabled functions

**Files:**
- Modify: `tein/build.rs` line ~192
- Modify: `tein/src/sandbox.rs` line ~377

`feature_enabled` exists identically in both `build.rs` (build-time) and `sandbox.rs` (runtime). They can't be merged (different compilation contexts). But without comments, anyone adding a new cargo feature will update one and miss the other.

**Step 1: Update the comment in build.rs**

Find in `tein/build.rs` (around line 192):

```rust
/// check whether a cargo feature is enabled at build time (mirrors sandbox.rs)
fn feature_enabled(feature: Option<&str>) -> bool {
```

Replace the doc comment:

```rust
/// check whether a cargo feature is enabled at build time.
///
/// **keep in sync with `feature_enabled` in `src/sandbox.rs`** — both must be updated
/// when adding or removing cargo features. they can't be merged because build.rs and
/// sandbox.rs run in different compilation contexts (`cfg!` resolves differently).
fn feature_enabled(feature: Option<&str>) -> bool {
```

**Step 2: Update the comment in sandbox.rs**

Find in `tein/src/sandbox.rs` (around line 377):

```rust
/// check whether a cargo feature gate is satisfied at runtime.
///
/// in sandbox.rs this is a compile-time check. build.rs uses the same check.
#[inline]
fn feature_enabled(feature: Option<&str>) -> bool {
```

Replace:

```rust
/// check whether a cargo feature gate is satisfied at compile time.
///
/// **keep in sync with `feature_enabled` in `build.rs`** — both must be updated
/// when adding or removing cargo features. they can't be merged because `cfg!` resolves
/// differently in build script vs lib contexts.
#[inline]
fn feature_enabled(feature: Option<&str>) -> bool {
```

**Step 3: Verify build + tests still pass**

```bash
cargo build 2>&1 | tail -3
just test 2>&1 | tail -5
```

Expected: no errors, all tests pass.

**Step 4: Commit**

```bash
git add tein/build.rs tein/src/sandbox.rs
git commit -m "docs: cross-reference duplicated feature_enabled in build.rs + sandbox.rs"
```

---

### Task 4: Fix binary-record-chicken.scm in VFS registry

**Files:**
- Modify: `tein/src/vfs_registry.rs` around line 2836

`binary-record-chicken.scm` is in the `files:` list for `chibi/binary-record` with a comment noting it's never loaded by chibi (only the `(cond-expand (chicken ...) (else ...))` branch on line 43-46 of the `.sld` loads it). It's dead weight in the VFS and the binary.

Removing it from `files:` means the VFS validator (`validate_sld_includes`) needs to not panic — check what that validator does first.

**Step 1: Check validate_sld_includes behaviour**

```bash
grep -n "validate_sld_includes\|binary.record.chicken" tein/build.rs
```

Find whether `validate_sld_includes` would panic if `binary-record-chicken.scm` is absent from the files list.

Read the relevant section: it checks that every `(include "file.scm")` in a `.sld` has a corresponding entry in the registry `files:` list. Since `binary-record-chicken.scm` is included inside `(cond-expand (chicken ...))` in the `.sld`, it WILL be flagged if removed from `files:`.

**Step 2: Check validate_sld_includes for cond-expand awareness**

```bash
grep -n -A 20 "fn validate_sld_includes" tein/build.rs
```

If the validator recurses into all `cond-expand` branches (like the export extractor), it will catch the chicken include. If it does NOT recurse into `cond-expand`, removing the file from `files:` is safe.

**Step 3a: If validator is cond-expand-naive** (doesn't recurse into cond-expand branches)

Simply remove `"lib/chibi/binary-record-chicken.scm"` from the `files:` list in `vfs_registry.rs` and update the comment:

```rust
files: &[
    "lib/chibi/binary-record.sld",
    "lib/chibi/binary-types.scm",
    "lib/chibi/binary-record.scm",
    // binary-record-chicken.scm is listed in the .sld's (cond-expand (chicken ...))
    // branch but is never loaded by chibi — excluded from VFS to avoid dead embedding.
],
```

**Step 3b: If validator recurses into all cond-expand branches** (would panic without the file)

Add a comment explaining why it must stay, and why it's harmless:

```rust
files: &[
    "lib/chibi/binary-record.sld",
    "lib/chibi/binary-types.scm",
    "lib/chibi/binary-record.scm",
    // chicken-compat alternative for define-binary-record-type. listed in the .sld's
    // (cond-expand (chicken ...) (else ...)) block; chibi always takes the else branch
    // so this file is never loaded. kept here only because validate_sld_includes
    // recurses into all cond-expand branches and would panic if it were absent.
],
```

**Step 4: Build to confirm no panics from validate_sld_includes**

```bash
cargo build 2>&1 | grep -E "panic|validate|binary-record|error\[" | head -10
```

Expected: no output (clean build).

**Step 5: Commit**

```bash
git add tein/src/vfs_registry.rs
git commit -m "fix: clarify binary-record-chicken.scm in VFS registry — never loaded by chibi"
# or if removed:
git commit -m "fix: remove binary-record-chicken.scm from VFS files — never loaded by chibi"
```

---

### Task 5: Add allow_module dep-resolution cross-reference comment

**Files:**
- Modify: `tein/src/context.rs` around line 1519

The `allow_module` docstring says "dependencies are resolved automatically from the registry — callers never need to think about transitive imports." This is true, but the mechanism is non-obvious: it happens at `build()` time via `Modules::Only(list)` → `registry_resolve_deps`. An agent reading only `allow_module`'s body sees just a `Vec::push` and wouldn't guess resolution happens elsewhere.

**Step 1: Add inline cross-reference to the docstring**

Find in `tein/src/context.rs` (around line 1519):

```rust
    /// starts from the current `Modules` variant's resolved allowlist, appends
    /// the new module, and sets `Modules::Only(extended_list)`. dependencies are
    /// resolved automatically from the registry — callers never need to think about
    /// transitive imports.
```

Replace with:

```rust
    /// starts from the current `Modules` variant's resolved allowlist, appends
    /// the new module, and sets `Modules::Only(extended_list)`. dependencies are
    /// resolved automatically from the registry — callers never need to think about
    /// transitive imports.
    ///
    /// note: dep resolution happens at [`ContextBuilder::build`] time, not here.
    /// `allow_module` only appends to the list; `build()` calls `registry_resolve_deps`
    /// on the final `Modules::Only(list)` to expand transitive deps before arming the gate.
```

**Step 2: Verify build + doc tests**

```bash
cargo build 2>&1 | tail -3
cargo test --doc 2>&1 | tail -5
```

Expected: clean.

**Step 3: Commit**

```bash
git add tein/src/context.rs
git commit -m "docs: clarify allow_module dep resolution happens at build() time"
```

---

### Task 6: Document chibi/channel default_safe: true despite runtime failure

**Files:**
- Modify: `tein/src/vfs_registry.rs` around line 1963

`chibi/channel` is `default_safe: true` but its dep `srfi/18` (threads) is `default_safe: false` and non-functional in tein (`SEXP_USE_GREEN_THREADS=0`). So importing `chibi/channel` in a sandbox succeeds (no `SandboxViolation`), then fails at runtime. The intent is arguably correct (no security risk), but the `default_safe: true` surprises readers.

**Step 1: Add a comment above the chibi/channel entry**

Find in `tein/src/vfs_registry.rs` (around line 1963):

```rust
    VfsEntry {
        path: "chibi/channel",
        deps: &["srfi/9", "srfi/18"],
        files: &["lib/chibi/channel.sld", "lib/chibi/channel.scm"],
        clib: None,
        default_safe: true,
```

Replace with:

```rust
    // chibi/channel: default_safe: true because the module itself has no OS-touching code.
    // however, its dep srfi/18 (threads) is non-functional in tein (SEXP_USE_GREEN_THREADS=0),
    // so importing chibi/channel succeeds in a sandbox but channel operations fail at runtime.
    // this is intentional: allow the import, let the runtime error surface naturally.
    VfsEntry {
        path: "chibi/channel",
        deps: &["srfi/9", "srfi/18"],
        files: &["lib/chibi/channel.sld", "lib/chibi/channel.scm"],
        clib: None,
        default_safe: true,
```

**Step 2: Also add a comment for scheme/mapping/hash (same pattern as srfi/146/hash)**

Find in `tein/src/vfs_registry.rs` (around line 909):

```rust
    VfsEntry {
        path: "scheme/mapping/hash",
        deps: &["srfi/146/hash"],
        files: &["lib/scheme/mapping/hash.sld"],
        clib: None,
        default_safe: false,
```

Replace with:

```rust
    // scheme/mapping/hash: default_safe: false — depends on srfi/146/hash which pulls in
    // hamt-map (a heavier, less-tested dependency). same reasoning as srfi/146/hash.
    VfsEntry {
        path: "scheme/mapping/hash",
        deps: &["srfi/146/hash"],
        files: &["lib/scheme/mapping/hash.sld"],
        clib: None,
        default_safe: false,
```

**Step 3: Build to verify no issues**

```bash
cargo build 2>&1 | tail -3
```

**Step 4: Commit**

```bash
git add tein/src/vfs_registry.rs
git commit -m "docs: add comments explaining chibi/channel and scheme/mapping/hash safety rationale"
```

---

### Task 7: Document export extractor cond-expand limitation

**Files:**
- Modify: `tein/build.rs` around line 354

The export extractor in `collect_exports_from_sexps` recurses into ALL list children, including `cond-expand` branches that chibi won't execute. This means exports from else-branches (e.g. `defrec`, `define-auxiliary-syntax` from `chibi/binary-record`'s chicken-compat branch) appear in the exports table. These ARE real exports for chibi (the `else` branch runs on chibi), so this is less a bug than an imprecise description.

**Step 1: Update the comment at the recursion site**

Find in `tein/build.rs` (around line 354):

```rust
        } else {
            // recurse into all list children (handles define-library, cond-expand, etc.)
            for item in items {
                walk(item, out);
            }
```

Replace the comment:

```rust
        } else {
            // recurse into all list children: handles define-library, cond-expand, begin, etc.
            // note: we recurse into ALL cond-expand branches, including (chicken ...) or
            // implementation-specific arms that chibi won't execute. for chibi, the (else ...)
            // branch always runs, so any (export ...) inside it is a real export — but
            // implementation-specific branches (like chicken) will produce false positives.
            // currently only chibi/binary-record's chicken branch is affected; its exports
            // (defrec, define-auxiliary-syntax) appear in MODULE_EXPORTS but are harmless.
            for item in items {
                walk(item, out);
            }
```

**Step 2: Build to verify**

```bash
cargo build 2>&1 | tail -3
```

**Step 3: Commit**

```bash
git add tein/build.rs
git commit -m "docs: document cond-expand limitation in collect_exports_from_sexps"
```

---

### Task 8: Add test — shadow stubs absent from unsandboxed context

**Files:**
- Modify: `tein/src/context.rs` — add test near line 7671 (`test_tein_file_not_shadowed_unsandboxed`)

There's no test verifying that shadow stubs are NOT registered in unsandboxed contexts. The code path is safe (register_vfs_shadows is only called inside the sandbox block), but a regression test closes the loop.

The test should verify that an unsandboxed context loading `chibi/filesystem` gets the REAL chibi module (i.e. an actual function, not a stub that raises a sandbox error). Since `chibi/filesystem` has OS operations, we can't call them safely in tests — but we can check the import succeeds and that the binding is a procedure (not our stub error-raiser).

**Step 1: Write the failing test**

Find `test_tein_file_not_shadowed_unsandboxed` in `tein/src/context.rs` (around line 7671). Add the new test immediately after it:

```rust
#[test]
fn test_shadow_stubs_not_registered_in_unsandboxed_context() {
    // unsandboxed contexts must NOT have shadow stubs injected — the real chibi module
    // should be used. verify by importing chibi/filesystem and checking that list-directory
    // is a real procedure, not a stub that raises a sandbox error.
    let ctx = Context::builder()
        .standard_env()
        .build()
        .expect("unsandboxed context");
    // import succeeds and list-directory is a procedure (not a stub error-raiser)
    let result = ctx.evaluate("(import (chibi filesystem)) (procedure? list-directory)");
    assert_eq!(
        result.expect("import should succeed in unsandboxed context"),
        Value::Boolean(true),
        "list-directory should be the real chibi proc, not a stub"
    );
}
```

**Step 2: Run just this test to verify it fails first**

```bash
cargo test test_shadow_stubs_not_registered_in_unsandboxed_context -- --nocapture 2>&1 | tail -20
```

Expected: test not found (we haven't added it yet) OR passes already (which would mean the behaviour is already correct and the test is a regression guard).

**Step 3: Add the test to context.rs**

Use the Edit tool to add the test block immediately after `test_tein_file_not_shadowed_unsandboxed`.

**Step 4: Run the test**

```bash
cargo test test_shadow_stubs_not_registered_in_unsandboxed_context -- --nocapture 2>&1 | tail -20
```

Expected: PASS. The real `chibi/filesystem` module loads and `list-directory` is a procedure.

**Step 5: Run the full test suite**

```bash
just test 2>&1 | tail -10
```

Expected: all tests pass (same count as before + 1 new).

**Step 6: Commit**

```bash
git add tein/src/context.rs
git commit -m "test: verify shadow stubs absent from unsandboxed context"
```

---

## Batch checkpoint

After all 8 tasks:

```bash
just lint
just test 2>&1 | tail -10
```

Expected: clean lint, all tests pass. Then collect any notes for AGENTS.md (there shouldn't be any from these fixes — they're all clarifications of existing behaviour).
