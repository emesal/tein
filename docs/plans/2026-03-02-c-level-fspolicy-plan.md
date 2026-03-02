# C-level FsPolicy enforcement implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move `open-*-file` FsPolicy enforcement from rust trampolines into chibi's C-level opcode implementations, fixing library-to-library import resolution and eliminating code duplication.

**Architecture:** Add a C→rust callback `tein_fs_policy_check(path, is_read)` in `tein_shim.c`, following the existing `tein_vfs_gate_check` pattern. Patch `eval.c` opcodes to call it before `fopen()`. Remove all rust-side `open-*-file` trampolines + `ORIGINAL_PROCS` + `capture_file_originals`. Simplify `(tein file)` to import `(chibi)` for opcodes. Simplify `(scheme file)` shadow to pure re-export.

**Design doc:** `docs/plans/2026-03-02-c-level-fspolicy-design.md`

**Branch:** `feature/vfs-shadow-scheme-file-2603` (continuation)

**Base branch:** `dev`

---

## progress

- [x] Task 1: Add FS policy gate + callback to tein_shim.c
- [x] Task 2: Patch eval.c opcodes with FS policy check (pushed to chibi fork)
- [x] Task 3: Add rust callback + FFI wrappers
- [x] Task 4: Add FS_GATE thread-local + arm in build() + clear on drop()
- [x] Task 5: Remove open-*-file trampolines + ORIGINAL_PROCS + IoOp
- [x] Task 6: Update (tein file) scheme files in chibi fork (pushed to fork)
- [x] Task 7: Simplify (scheme file) shadow + update vfs_registry
- [ ] Task 8: Integration tests — srfi/166/columnar from-file
- [ ] Task 9: Docs — AGENTS.md + sandbox.rs comment + design doc
- [ ] Task 10: Final verification + plan update + handoff
- [ ] Task 11: PR creation

**status:** 356 lib tests pass, 0 failures. lint not yet run.

### notes for continued execution

- tasks 5+6+7 were committed together since they're tightly coupled (trampoline removal + scheme file update + shadow simplification + test updates must land atomically)
- added C-level policy denial → `SandboxViolation` error classification in `value.rs` (detects `"access denied by sandbox policy"` in exception messages from eval.c patches F/G)
- all 20 IO tests updated: sandboxed tests now `(import (tein file))` to get `open-input-file` / `open-output-file` in scope (no longer injected as top-level env trampolines)
- `test_file_read_without_policy` / `test_file_write_without_policy` semantics changed: previously tested "undefined variable", now tests "C gate denies" (same user-facing result: error)
- trampoline tests renamed: `test_open_input_file_trampoline_*` → `test_open_input_file_*` (no longer trampolines)
- `FS_GATE_OFF` is not imported in context.rs (only used in sandbox.rs const Cell default) — don't add it

### AGENTS.md notes to collect (task 9)

- **IO policy flow** needs update: remove "capture originals" / "wrapper foreign fns" / "delegates to original proc" language → describe C-level gate + callback
- **sandboxing flow** needs update: remove "capture_file_originals" step, add "arm FS policy gate"
- document eval.c patches F, G in the architecture section (alongside existing A-E)

---

## architecture notes (read before implementing)

### callback pattern (follow exactly)

the VFS gate callback is the template. it has three parts:
1. **C side** (`tein_shim.c`): thread-local gate level + extern callback declaration + dispatcher + setter
2. **C call site** (`eval.c`): gate check at decision point
3. **rust side** (`ffi.rs`): `#[unsafe(no_mangle)] extern "C"` callback + extern declaration for setter + safe wrapper

### gate semantics

the FS policy gate mirrors `IS_SANDBOXED`: gate=0 (off) for unsandboxed, gate=1 (check) for sandboxed. the rust callback (`check_fs_access`) handles all sub-cases:
- unsandboxed → allow (never reached because gate=0)
- sandboxed + policy configured → prefix check
- sandboxed + no policy → deny

### VFS path safety

`sexp_open_input_file_op` already handles VFS paths via `tein_vfs_lookup()` *before* reaching `fopen()`. the policy check is inserted between the VFS lookup (early return for VFS content) and `fopen()`. module loading is unaffected.

### binary variant inheritance

`sexp_open_binary_input_file` delegates to `sexp_open_input_file_op`. `sexp_open_binary_output_file` delegates to `sexp_open_output_file_op`. so patching the two text variants automatically covers binary.

### chibi fork workflow

all changes to `eval.c` and `tein_shim.c` must be made in `~/forks/chibi-scheme` (branch `emesal-tein`) and pushed. `target/chibi-scheme` is hard-reset by `build.rs` on every `cargo build`.

### what stays unchanged

- `file-exists?` and `delete-file` rust trampolines — no opcode equivalents, stay as-is
- `FsAccess`, `check_fs_access()` in `context.rs` — reused by the C→rust callback
- `FsPolicy`, `FS_POLICY` in `sandbox.rs` — reused by `check_fs_access`
- `extract_string_arg` — still used by `file_exists_trampoline` / `delete_file_trampoline`
- `file.scm` higher-order wrappers — identical content, just updated header comment

### critical: existing test semantics

all existing IO policy tests (`test_file_read_allowed_path`, `test_file_write_allowed_path`, etc.) test via `(import (tein file))` which uses `file-exists?` and `delete-file` trampolines — these are unchanged. the `open-*-file` tests go through the opcode now. test assertions remain identical.

---

## Task 1: Add FS policy gate + callback to tein_shim.c

**Files:**
- Modify: `~/forks/chibi-scheme/tein_shim.c`

**Step 1: Add the FS policy gate section**

Add after the VFS gate section (after line 262, before `// environment manipulation`):

```c
// --- FS policy gate ---
//
// two-level gate for file IO policy enforcement:
//   0 = off (all file access allowed — unsandboxed)
//   1 = check (rust callback decides based on FsPolicy)
//
// patched into sexp_open_input_file_op / sexp_open_output_file_op
// in eval.c (patches F, G). VFS paths bypass this (handled before fopen).

TEIN_THREAD_LOCAL int tein_fs_policy_gate = 0;

// rust callback for FS policy checks (defined in ffi.rs).
// checks IS_SANDBOXED + FsPolicy prefix matching.
extern int tein_fs_policy_check(const char *path, int is_read);

// check if file access is allowed under the current gate.
// called from eval.c patches F and G.
int tein_fs_check_access(const char *path, int is_read) {
    if (tein_fs_policy_gate == 0) return 1;    /* off — allow everything */
    return tein_fs_policy_check(path, is_read); /* rust callback */
}

// set the FS policy gate level. called from rust ffi.
void tein_fs_policy_gate_set(int level) {
    tein_fs_policy_gate = level;
}
```

**Step 2: Verify no syntax errors**

```bash
cd ~/forks/chibi-scheme && gcc -fsyntax-only -I include tein_shim.c 2>&1 | head -10
```
Expected: no errors (linker errors about undefined refs are fine — the callback is in rust).

**Step 3: Commit (do NOT push yet — eval.c patches needed first)**

```bash
cd ~/forks/chibi-scheme
git add tein_shim.c
git commit -m "feat: add FS policy gate + callback for file IO enforcement"
```

---

## Task 2: Patch eval.c opcodes with FS policy check

**Files:**
- Modify: `~/forks/chibi-scheme/eval.c`

**Step 1: Add extern declaration for tein_fs_check_access**

Near the top of `eval.c`, find the existing VFS extern declarations (search for `extern const char *tein_vfs_lookup`). Add after:

```c
extern int tein_fs_check_access(const char *path, int is_read);
```

**Step 2: Patch sexp_open_input_file_op (patch F)**

Find `sexp_open_input_file_op` (line 1359). The current code after VFS lookup is:

```c
  }
  do {
    if (count != 0) sexp_gc(ctx, NULL);
    in = fopen(sexp_string_data(path), "r");
```

Insert the policy check between the VFS lookup closing brace and the `do` loop:

```c
  }
  /* tein: FS policy gate — deny file reads when sandboxed without permission (patch F).
   * VFS paths return early above; this only runs for real filesystem access. */
  if (!tein_fs_check_access(sexp_string_data(path), 1))
    return sexp_user_exception(ctx, self, "file read access denied by sandbox policy", path);
  do {
    if (count != 0) sexp_gc(ctx, NULL);
    in = fopen(sexp_string_data(path), "r");
```

**Step 3: Patch sexp_open_output_file_op (patch G)**

Find `sexp_open_output_file_op` (line 1392). Insert the policy check before the `do` loop:

```c
sexp sexp_open_output_file_op (sexp ctx, sexp self, sexp_sint_t n, sexp path) {
  FILE *out;
  int count = 0;
  sexp_assert_type(ctx, sexp_stringp, SEXP_STRING, path);
  /* tein: FS policy gate — deny file writes when sandboxed without permission (patch G).
   * output files have no VFS path — this always runs for real filesystem access. */
  if (!tein_fs_check_access(sexp_string_data(path), 0))
    return sexp_user_exception(ctx, self, "file write access denied by sandbox policy", path);
  do {
```

**Step 4: Push chibi fork**

```bash
cd ~/forks/chibi-scheme
git add eval.c
git commit -m "feat: FS policy gate in open-input/output-file opcodes (patches F, G)"
git push
```

**Step 5: Rebuild tein to pull fork changes**

```bash
cd ~/projects/tein
just clean && cargo build 2>&1 | tail -20
```

Expected: build succeeds. linker may warn about `tein_fs_policy_check` being undefined until we add the rust callback in task 3.

---

## Task 3: Add rust callback + FFI wrappers

**Files:**
- Modify: `tein/src/ffi.rs`

**Step 1: Add extern declaration for `tein_fs_policy_gate_set`**

Find the `extern "C"` block in `ffi.rs` (around line 204 where `tein_vfs_gate_set` is declared). Add:

```rust
    pub fn tein_fs_policy_gate_set(level: c_int);
```

**Step 2: Add safe wrapper for `tein_fs_policy_gate_set`**

Find `pub unsafe fn vfs_gate_set` (line 730). Add after it:

```rust
/// Set the C-level FS policy gate level.
///
/// 0 = off (all file access allowed), 1 = check via rust callback.
/// # Safety
/// Must be called from the same thread as the chibi context.
pub unsafe fn fs_policy_gate_set(level: i32) {
    unsafe { tein_fs_policy_gate_set(level as c_int) }
}
```

**Step 3: Add the `tein_fs_policy_check` callback**

Find `tein_vfs_gate_check` (line 744). Add the new callback after it (after line 777):

```rust
/// C→rust callback for FS policy enforcement.
///
/// Called from `tein_fs_check_access` in `tein_shim.c` when the FS policy
/// gate is armed (sandboxed contexts). Delegates to `check_fs_access()`
/// which checks `IS_SANDBOXED` + `FS_POLICY` thread-locals.
///
/// Returns 1 (allow) or 0 (deny).
#[unsafe(no_mangle)]
extern "C" fn tein_fs_policy_check(path: *const c_char, is_read: c_int) -> c_int {
    use crate::context::{check_fs_access, FsAccess};

    let path_str = unsafe { CStr::from_ptr(path) }.to_str().unwrap_or("");
    let access = if is_read != 0 {
        FsAccess::Read
    } else {
        FsAccess::Write
    };
    if check_fs_access(path_str, access) {
        1
    } else {
        0
    }
}
```

**Step 4: Make `check_fs_access` and `FsAccess` visible to ffi.rs**

In `tein/src/context.rs`, change the visibility of `FsAccess` and `check_fs_access` from private to `pub(crate)`:

```rust
/// FsPolicy access direction for [`check_fs_access`].
pub(crate) enum FsAccess {
    Read,
    Write,
}

/// Check FsPolicy access for `path`.
/// ...
pub(crate) fn check_fs_access(path: &str, access: FsAccess) -> bool {
```

**Step 5: Build**

```bash
cargo build 2>&1 | tail -20
```

Expected: compiles clean. the callback links with the C extern.

**Step 6: Commit**

```bash
git add tein/src/ffi.rs tein/src/context.rs
git commit -m "feat: tein_fs_policy_check C→rust callback + FFI wrappers"
```

---

## Task 4: Add FS_GATE thread-local + arm in build() + clear on drop()

**Files:**
- Modify: `tein/src/sandbox.rs`
- Modify: `tein/src/context.rs`

**Step 1: Add FS_GATE constants and thread-local to sandbox.rs**

Find `pub(crate) static VFS_ALLOWLIST` (line 140). Add after the VFS_ALLOWLIST thread-local, inside the same `thread_local!` block, or add a new block right after:

```rust
/// numeric FS policy gate level for C interop. mirrors `tein_fs_policy_gate` in `tein_shim.c`.
pub(crate) const FS_GATE_OFF: u8 = 0;
/// numeric FS policy gate level — rust callback checks IS_SANDBOXED + FsPolicy.
pub(crate) const FS_GATE_CHECK: u8 = 1;

thread_local! {
    /// FS policy gate level (0=off, 1=check). set during Context::build(), cleared on drop.
    pub(crate) static FS_GATE: Cell<u8> = const { Cell::new(FS_GATE_OFF) };
}
```

**Step 2: Save previous FS gate in build()**

Find where `prev_vfs_gate` is saved (line 1734):

```rust
            let prev_vfs_gate = VFS_GATE.with(|cell| cell.get());
```

Add after:

```rust
            let prev_fs_gate = FS_GATE.with(|cell| cell.get());
```

Add the necessary import at the top of the block — find where `GATE_CHECK` is imported (around line 1744) and add `FS_GATE, FS_GATE_CHECK, FS_GATE_OFF` to the same `use crate::sandbox::` import.

**Step 3: Arm FS gate when IS_SANDBOXED is set**

Find where `IS_SANDBOXED` is set to true (line 1749):

```rust
                IS_SANDBOXED.with(|c| c.set(true));
```

Add right after:

```rust
                // arm FS policy gate — C opcodes will call tein_fs_policy_check
                FS_GATE.with(|cell| cell.set(FS_GATE_CHECK));
                unsafe { ffi::fs_policy_gate_set(FS_GATE_CHECK as i32) };
```

**Step 4: Add prev_fs_gate to Context struct**

Find `prev_vfs_gate: u8` (line 1955). Add after:

```rust
    /// previous FS_GATE level, restored on drop
    prev_fs_gate: u8,
```

**Step 5: Pass prev_fs_gate in Context construction**

Find the `Context { ... }` construction (around line 1852). Add the new field alongside `prev_vfs_gate`:

```rust
                prev_fs_gate,
```

For the unsandboxed build path, find the other `Context { ... }` construction and add `prev_fs_gate` there too (it should be `FS_GATE_OFF` since unsandboxed doesn't arm the gate — but we save/restore anyway for consistency). Check if there's a single construction site or two.

**Step 6: Restore FS gate on drop**

Find where `VFS_GATE` is restored in `drop()` (line 3086). Add before or after:

```rust
        FS_GATE.with(|cell| cell.set(self.prev_fs_gate));
        unsafe { ffi::fs_policy_gate_set(self.prev_fs_gate as i32) };
```

**Step 7: Build + test**

```bash
cargo build 2>&1 | tail -20
cargo test -p tein --lib 2>&1 | tail -10
```

Expected: compiles and existing tests pass. the FS gate is now armed but the old trampolines are still registered (task 5 removes them).

**Step 8: Commit**

```bash
git add tein/src/sandbox.rs tein/src/context.rs
git commit -m "feat: FS_GATE thread-local — arm in sandboxed build, clear on drop"
```

---

## Task 5: Remove open-*-file trampolines + ORIGINAL_PROCS + IoOp

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Remove ORIGINAL_PROCS thread-local**

Delete lines 72-89 (the `ORIGINAL_PROCS` thread-local and its comment block).

**Step 2: Remove IoOp enum**

Delete lines 950-983 (the `IoOp` enum and its impl block).

**Step 3: Remove capture_file_originals**

Delete lines 1155-1177 (the `capture_file_originals` function).

**Step 4: Remove open_file_trampoline + 4 individual trampolines**

Delete lines 1179-1249 (shared `open_file_trampoline` + 4 `open_*_file_trampoline` fns).

**Step 5: Remove capture_file_originals call sites**

In the sandboxed build path, find and remove (around line 1753-1754):
```rust
                // capture open-*-file originals before env restriction
                capture_file_originals(ctx, source_env);
```

In the unsandboxed build path, find and remove (around lines 1890-1896):
```rust
                // capture open-*-file originals from the current env.
                // sandboxed contexts already captured from source_env above (before
                // env restriction); re-capturing from null_env would overwrite with
                // null pointers (null_env has no open-*-file). skip for sandboxed.
                if !IS_SANDBOXED.with(|c| c.get()) {
                    capture_file_originals(context.ctx, ffi::sexp_context_env(context.ctx));
                }
```

**Step 6: Simplify register_file_module**

Replace the current `register_file_module` with:

```rust
    /// Register `file-exists?` and `delete-file` trampolines.
    ///
    /// `open-*-file` enforcement is handled at the C opcode level
    /// (eval.c patches F, G) via the FS policy gate callback.
    fn register_file_module(&self) -> Result<()> {
        self.define_fn_variadic("file-exists?", file_exists_trampoline)?;
        self.define_fn_variadic("delete-file", delete_file_trampoline)?;
        Ok(())
    }
```

**Step 7: Remove unused imports (if any)**

Check for any now-unused imports related to `ORIGINAL_PROCS`, `IoOp`, etc. clean up dead `use` statements.

**Step 8: Build + test**

```bash
cargo build 2>&1 | tail -20
cargo test -p tein --lib 2>&1 | tail -10
```

Expected: compiles. existing `file-exists?` / `delete-file` tests pass. `open-*-file` tests may fail until the scheme side is updated (task 6).

**Step 9: Commit**

```bash
git add tein/src/context.rs
git commit -m "refactor: remove open-*-file trampolines — enforcement at C opcode level"
```

---

## Task 6: Update (tein file) scheme files in chibi fork

**Files:**
- Modify: `~/forks/chibi-scheme/lib/tein/file.sld`
- Modify: `~/forks/chibi-scheme/lib/tein/file.scm`

**Step 1: Update file.sld**

Replace `~/forks/chibi-scheme/lib/tein/file.sld` with:

```scheme
(define-library (tein file)
  (import (chibi))
  (export file-exists? delete-file
          open-input-file open-binary-input-file
          open-output-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file)
  (include "file.scm"))
```

Key change: `(import (chibi))` instead of `(import (scheme base))`. `(chibi)` provides the core env including all opcodes (`open-input-file`, `open-output-file`, etc.) as proper library-level bindings. removed `tein-open-*` exports — no longer needed.

**Step 2: Update file.scm header comment**

Replace the header comment in `~/forks/chibi-scheme/lib/tein/file.scm`:

```scheme
;;; (tein file) — safe file IO with FsPolicy enforcement
;;;
;;; open-input-file, open-binary-input-file, open-output-file,
;;; open-binary-output-file are chibi opcodes (core env). policy enforcement
;;; happens at the C level in eval.c (patches F, G) via tein_fs_check_access.
;;; the FS policy gate is armed for sandboxed contexts; unsandboxed = allow all.
;;;
;;; file-exists? and delete-file are rust trampolines registered by
;;; register_file_module() in context.rs — they check IS_SANDBOXED + FsPolicy.
;;;
;;; the 4 higher-order wrappers below call open-input-file / open-output-file.
;;; policy enforcement flows through the C-level opcode check.
```

The 4 wrapper function bodies remain identical.

**Step 3: Push chibi fork**

```bash
cd ~/forks/chibi-scheme
git add lib/tein/file.sld lib/tein/file.scm
git commit -m "refactor: (tein file) imports (chibi) for opcode-level open-*-file"
git push
```

**Step 4: Rebuild tein**

```bash
cd ~/projects/tein
just clean && cargo build 2>&1 | tail -20
```

**Step 5: Run tests**

```bash
cargo test -p tein --lib 2>&1 | tail -20
```

Expected: existing tests pass.

**Step 6: Commit tein side (if any tein files changed)**

No tein-side changes in this task — the fork push is the commit.

---

## Task 7: Simplify (scheme file) shadow + update vfs_registry

**Files:**
- Modify: `tein/src/vfs_registry.rs`

**Step 1: Update scheme/file shadow entry**

Find the `scheme/file` shadow entry (line 576-603). Replace:

```rust
    // scheme/file: VFS shadow — sandboxed contexts re-export from (tein file).
    // policy enforcement for open-*-file is at the C opcode level (eval.c patches F, G).
    // unsandboxed contexts use chibi's native scheme/file directly.
    VfsEntry {
        path: "scheme/file",
        deps: &["tein/file"],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: Some("\
(define-library (scheme file)
  (import (tein file))
  (export file-exists? delete-file
          open-input-file open-binary-input-file
          open-output-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file))
"),
    },
```

**Step 2: Build**

```bash
cargo build 2>&1 | tail -20
```

**Step 3: Run full test suite**

```bash
just test 2>&1 | tail -30
```

Expected: all tests pass.

**Step 4: Commit**

```bash
git add tein/src/vfs_registry.rs
git commit -m "refactor: simplify (scheme file) shadow — pure re-export from (tein file)"
```

---

## Task 8: Integration tests — srfi/166/columnar from-file

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Verify existing columnar tests pass**

The two `srfi/166/columnar` tests were already added earlier in the session. Run them:

```bash
cargo test -p tein --lib -- test_srfi_166_columnar 2>&1 | tail -20
```

Expected: BOTH pass now — `from-file` calls `open-input-file` (the opcode) which goes through the C-level policy check.

**Step 2: If tests fail, investigate**

If `test_srfi_166_columnar_from_file_with_policy` still fails, check:
1. the C gate is armed (task 4)
2. the callback allows reads for the configured prefix
3. `(scheme file)` shadow resolves correctly

**Step 3: Run full test suite**

```bash
just test 2>&1 | tail -30
```

**Step 4: Lint**

```bash
just lint
```

**Step 5: Commit (only if tests were modified)**

```bash
git add tein/src/context.rs
git commit -m "test: srfi/166/columnar from-file integration with C-level FsPolicy"
```

---

## Task 9: Docs — AGENTS.md + sandbox.rs comment + design doc

**Files:**
- Modify: `AGENTS.md`
- Modify: `tein/src/sandbox.rs`
- Modify: `docs/plans/2026-03-02-c-level-fspolicy-design.md`

**Step 1: Update AGENTS.md sandboxing flow**

Find the sandboxing flow section. Update to reflect the new architecture:
- remove mention of "capture originals" step
- add "arm FS policy gate" after "IS_SANDBOXED set"
- update IO policy flow to describe C-level enforcement

**Step 2: Update sandbox.rs module comment**

Update the comment block at the top of `sandbox.rs` (or wherever the shadow module docs are) to remove references to "trampolines" for open-*-file and instead reference "C-level opcode enforcement (eval.c patches F, G)".

**Step 3: Mark design doc as IMPLEMENTED**

Update `docs/plans/2026-03-02-c-level-fspolicy-design.md` status from `APPROVED` to `IMPLEMENTED`.

**Step 4: Commit**

```bash
git add AGENTS.md tein/src/sandbox.rs docs/plans/2026-03-02-c-level-fspolicy-design.md
git commit -m "docs: update AGENTS.md + sandbox docs for C-level FsPolicy enforcement"
```

---

## Task 10: Final verification + plan update + handoff

**Step 1: Full test suite**

```bash
just test 2>&1 | tail -40
```

Expected: all pass.

**Step 2: Lint**

```bash
just lint
```

**Step 3: Update handoff doc**

Update `docs/plans/2026-03-01-vfs-shadow-scheme-file-handoff.md` to reflect:
- task 9 complete (columnar from-file tests)
- C-level FsPolicy refactor complete
- tasks 10-12 from original plan remain (docs, verification, PR)

**Step 4: Commit**

```bash
git add docs/plans/
git commit -m "docs: final verification + handoff update"
```

**Step 5: Halt for context clear**

---

## Task 11: PR creation

After context clear and final verification:

```bash
gh pr create \
  --base dev \
  --title "feat: VFS shadow system + C-level FsPolicy + (scheme file/repl/show) in sandbox" \
  --body "$(cat <<'EOF'
## summary

- introduces `VfsSource::Shadow` — data-driven VFS shadow system in the module registry. shadow `.sld` content declared inline in `VFS_REGISTRY` entries; `register_vfs_shadows()` injects them at sandbox build time
- **C-level FsPolicy enforcement** (eval.c patches F, G): `open-*-file` opcodes call `tein_fs_check_access()` before `fopen()`, which dispatches to rust callback `tein_fs_policy_check`. fixes library-to-library import resolution — opcodes are in the core env, visible to all code
- removes rust-side `open-*-file` trampolines (`ORIGINAL_PROCS`, `capture_file_originals`, `IoOp`, 4 trampoline fns) — single enforcement point at C level
- `(tein file)` imports `(chibi)` for opcodes; `(scheme file)` shadow is pure re-export from `(tein file)` — zero code duplication
- `scheme/repl` shadow: neutered `interaction-environment` via `(current-environment)`
- `scheme/process-context` shadow + `srfi/98` shadow: sandbox-safe process info
- enables `(scheme show)` / `(srfi 166)` + all sub-modules in `Modules::Safe`
- `srfi/166/columnar` `from-file` works in sandbox with `file_read` policy

closes #91

## test plan

- [ ] C-level policy: open-input-file allowed/denied in sandbox
- [ ] C-level policy: open-output-file allowed/denied in sandbox
- [ ] unsandboxed passthrough (gate=0, no check)
- [ ] scheme/file shadow resolution in sandbox
- [ ] scheme/repl shadow returns environment
- [ ] scheme/show importable in sandbox
- [ ] srfi/166/columnar from-file with/without policy
- [ ] higher-order wrappers (call-with-*, with-*-from/to-file)
- [ ] file-exists? / delete-file rust trampolines unchanged
- [ ] registry safe allowlist updated
- [ ] `just test` green + `just lint` clean
EOF
)"
```
