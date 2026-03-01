# Handoff: VFS shadow + (scheme file/repl/show) implementation

**branch:** `feature/vfs-shadow-scheme-file-2603`
**plan file:** `docs/plans/2026-03-01-vfs-shadow-scheme-file.md`
**date:** 2026-03-01

---

## progress

tasks 1–5 complete. tasks 6–12 remain.

### completed
- **task 1:** branch created, GH issue #97 opened for deferred `(scheme eval)` + full REPL
- **task 2:** `VfsSource::Shadow` variant + `shadow_sld` field added to `VfsEntry`; all 123 existing entries updated with `shadow_sld: None`; `scheme/file` and `scheme/repl` shadow entries added; `scheme/file` added to `srfi/166/columnar` deps; `VfsSource::Shadow` arm added in `build.rs` export extraction; sandbox test updated (removed stale negative assertion for `scheme/repl`, added positive assertions for `scheme/file` and `scheme/repl`)
- **task 3:** `register_vfs_shadows()` added to `sandbox.rs` (has access to `VFS_REGISTRY` via `include!`); called in `build()` after `IS_SANDBOXED.with(|c| c.set(true))` and before `VFS_GATE` is armed
- **task 4:** `capture_file_originals()` unsafe fn + `open_file_trampoline()` shared impl + 4 `open_*_file_trampoline` extern "C" fns added; `register_file_module` updated to register all 6 trampolines; originals captured in both sandbox path (from `source_env`) and unsandboxed path (from `context.ctx` env)
- **task 5:** `check_and_delegate`, 4 `wrapper_open_*` fns, `wrapper_fn_for` removed; `has_io` block removed; `FsPolicy` setup relocated outside sandbox block (works for both paths)

### remaining (tasks 6–12)
- **task 6:** expand `(tein file)` scheme files in chibi fork — update `file.sld` + `file.scm` in `target/chibi-scheme`, **push to remote**, rebuild
- **task 7:** VFS shadow integration tests for `(scheme file)` + `(scheme repl)` in `context.rs`
- **task 8:** flip `default_safe` flags for `scheme/show` + `srfi/166` tree; update sandbox.rs test to add `scheme/show` + `srfi/166` assertions
- **task 9:** `srfi/166/columnar` from-file integration tests with/without FsPolicy
- **task 10:** docs update — sandbox.rs comment block, AGENTS.md sandboxing flow, design doc status
- **task 11:** final verification + lint + plan update commit
- **task 12:** PR creation

---

## critical corrections to the plan

**`Value::Bool` doesn't exist — use `Value::Boolean`**

the plan's test code throughout uses `Value::Bool(true)` / `Value::Bool(false)`. the actual variant is `Value::Boolean(bool)`. every test that asserts a boolean return value must use `Value::Boolean(true)` instead. affects tasks 6, 7, 8, 9.

**`(procedure? ...)` needs `(scheme base)` imported**

the plan's `test_scheme_repl_shadow_importable_in_sandbox` test uses `(procedure? ...)` without importing `(scheme base)`. fixed to:
```rust
"(import (scheme base) (scheme repl)) (procedure? interaction-environment)"
```
similar care needed in task 7's shadow tests.

**sandbox.rs negative assertion for `scheme/repl` was removed in task 2**

the plan deferred updating the sandbox test to task 8 step 5, but we already did it in task 2 (it was a test failure from making `scheme/repl` `default_safe: true`). task 8 step 5 only needs to add the `scheme/show` and `srfi/166` positive assertions — the `scheme/file` and `scheme/repl` ones are already there.

---

## architecture notes (what's been built)

- `register_vfs_shadows()` lives in `sandbox.rs` (not `context.rs`) because `VFS_REGISTRY` and `VfsSource` are in scope there via `include!("vfs_registry.rs")`
- capture call in unsandboxed path: `capture_file_originals(context.ctx, ffi::sexp_context_env(context.ctx))` — no inner `unsafe {}` needed because it's already inside the outer `unsafe` build block
- `FsPolicy` is set unconditionally outside the sandbox block (previously inside `has_io`)
- the 4 new `open_*_file_trampoline` fns and `capture_file_originals` + `open_file_trampoline` live between `delete_file_trampoline` and `// --- (tein load) trampoline ---`

---

## task 6 — chibi fork changes (IMPORTANT)

files to modify in `target/chibi-scheme/lib/tein/`:

**`file.sld`** — replace with:
```scheme
(define-library (tein file)
  (import (scheme base))
  (export file-exists? delete-file
          open-input-file open-binary-input-file
          open-output-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file)
  (include "file.scm"))
```

**`file.scm`** — replace with the 4 higher-order wrappers (call-with-input-file, call-with-output-file, with-input-from-file, with-output-to-file) using `dynamic-wind` + `parameterize`. see plan task 6 for exact content.

**must push to remote before `cargo build`** — build.rs hard-resets chibi-scheme from `emesal/chibi-scheme` branch `emesal-tein` on every build. changes not pushed will be lost.

```bash
cd /home/fey/projects/tein/target/chibi-scheme
git add lib/tein/file.sld lib/tein/file.scm
git commit -m "feat: expand (tein file) to full (scheme file) surface — 10 exports"
git push
cd /home/fey/projects/tein
just clean && cargo build
```

---

## currently failing tests (expected, unblocked by task 6)

- `test_open_input_file_trampoline_allowed` — `open-input-file` not yet in `(tein file)` exports
- scheme test `tein_file_open` — not written yet (task 6 writes it)

all other tests were green after task 5.
