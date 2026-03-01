# Handoff: VFS shadow + (scheme file/repl/show) implementation

**branch:** `feature/vfs-shadow-scheme-file-2603`
**plan file:** `docs/plans/2026-03-01-vfs-shadow-scheme-file.md`
**date:** 2026-03-01

---

## progress

tasks 1–6 complete. tasks 7–12 remain.

### completed
- **task 1:** branch created, GH issue #97 opened for deferred `(scheme eval)` + full REPL
- **task 2:** `VfsSource::Shadow` variant + `shadow_sld` field added to `VfsEntry`; all 123 existing entries updated with `shadow_sld: None`; `scheme/file` and `scheme/repl` shadow entries added; `scheme/file` added to `srfi/166/columnar` deps; `VfsSource::Shadow` arm added in `build.rs` export extraction; sandbox test updated (removed stale negative assertion for `scheme/repl`, added positive assertions for `scheme/file` and `scheme/repl`)
- **task 3:** `register_vfs_shadows()` added to `sandbox.rs` (has access to `VFS_REGISTRY` via `include!`); called in `build()` after `IS_SANDBOXED.with(|c| c.set(true))` and before `VFS_GATE` is armed
- **task 4:** `capture_file_originals()` unsafe fn + `open_file_trampoline()` shared impl + 4 `open_*_file_trampoline` extern "C" fns added; `register_file_module` updated to register all 6 trampolines; originals captured in both sandbox path (from `source_env`) and unsandboxed path (from `context.ctx` env)
- **task 5:** `check_and_delegate`, 4 `wrapper_open_*` fns, `wrapper_fn_for` removed; `has_io` block removed; `FsPolicy` setup relocated outside sandbox block (works for both paths)
- **task 6:** chibi fork updated (`~/forks/chibi-scheme` branch `emesal-tein`); `file.sld` exports 6 symbols; `file.scm` defines 4 higher-order wrappers; `(scheme file)` shadow updated; `capture_file_originals` skip-guard added; all 706 tests green

### remaining (tasks 7–12)
- **task 7:** VFS shadow integration tests for `(scheme file)` + `(scheme repl)` in `context.rs`
- **task 8:** flip `default_safe` flags for `scheme/show` + `srfi/166` tree; update sandbox.rs test to add `scheme/show` + `srfi/166` assertions
- **task 9:** `srfi/166/columnar` from-file integration tests with/without FsPolicy
- **task 10:** docs update — sandbox.rs comment block, AGENTS.md sandboxing flow, design doc status
- **task 11:** final verification + lint + plan update commit
- **task 12:** PR creation

---

## critical corrections to the plan

**`Value::Bool` doesn't exist — use `Value::Boolean`**

the plan's test code throughout uses `Value::Bool(true)` / `Value::Bool(false)`. the actual variant is `Value::Boolean(bool)`. every test that asserts a boolean return value must use `Value::Boolean(true)` instead. affects tasks 7, 8, 9.

**`(procedure? ...)` needs `(scheme base)` imported**

sandboxed null_env has no builtins — `(import (scheme base))` is required before any `let`, `close-input-port`, `procedure?`, etc. the plan's test snippets often omit this. confirmed pattern:
```rust
"(import (scheme base) (scheme repl)) (procedure? interaction-environment)"
```

**sandbox.rs negative assertion for `scheme/repl` was removed in task 2**

the plan deferred updating the sandbox test to task 8 step 5, but we already did it in task 2. task 8 step 5 only needs to add the `scheme/show` and `srfi/166` positive assertions — the `scheme/file` and `scheme/repl` ones are already there.

**task 7 tests — `(import (scheme file))` with FsPolicy**

the `test_scheme_file_shadow_importable_in_sandbox` test uses `(import (scheme file)) (let ((p (open-input-file ...)))...)`. needs `(import (scheme base))` for `let` and `close-input-port`. use:
```rust
"(import (scheme base) (scheme file)) (let ((p (open-input-file \"{tmp}\"))) (close-input-port p) #t)"
```

---

## architecture notes (what's been built)

- `register_vfs_shadows()` lives in `sandbox.rs` (not `context.rs`) because `VFS_REGISTRY` and `VfsSource` are in scope there via `include!("vfs_registry.rs")`
- `FsPolicy` is set unconditionally outside the sandbox block (previously inside `has_io`)
- the 4 new `open_*_file_trampoline` fns and `capture_file_originals` + `open_file_trampoline` live between `delete_file_trampoline` and `// --- (tein load) trampoline ---`

### (tein file) export architecture — CRITICAL

**`(tein file)` exports 6 symbols**: `file-exists?`, `delete-file`, `call-with-input-file`, `call-with-output-file`, `with-input-from-file`, `with-output-to-file`.

**the 4 `open-*-file` trampolines are NOT exported from `(tein file)`**. they live directly in the context env (null_env for sandbox, top-level for unsandboxed). no import is required to use them — they're always in scope.

**why not export them**: chibi compiles library body free-variable references as static slots, not dynamic env lookups. if `open-input-file` were exported from `(tein file)` AND referenced in `file.scm` body, the library compiler would create an UNDEF slot at compile time. at runtime the slot stays UNDEF — the top-level trampoline is never found. similarly, if `file.scm` defines scheme wrappers that call `tein-file-open-*-file` (internal names), those names are also compiled as UNDEF slots since they're not in the module env chain. any name referenced in a library body must be available in the library's compile-time env chain.

**`(scheme file)` shadow** re-exports from `(tein file)` for the 6 library items, and uses `(begin (define open-input-file open-input-file) ...)` to alias the 4 primitives from the context env at shadow-load time. this works because shadow library body runs in the null_env context where the trampolines are registered.

**the `capture_file_originals` skip-guard**: the sandboxed path calls `capture_file_originals(ctx, source_env)` BEFORE env restriction. the later call in `if self.standard_env { ... }` must NOT re-run for sandboxed contexts (null_env has no originals — would overwrite with null pointers). fixed with `if !IS_SANDBOXED.with(|c| c.get()) { capture_file_originals(...) }`.

### trampoline test pattern (sandboxed)

```rust
// open-*-file is in env directly — import scheme base for let/close-*
let code = format!(
    "(import (scheme base)) (let ((p (open-input-file \"{path}\"))) (close-input-port p) #t)"
);
```

---

## chibi fork workflow (IMPORTANT — learned this session)

changes must be made and pushed from **`~/forks/chibi-scheme`** (branch `emesal-tein`), NOT from `target/chibi-scheme`. build.rs hard-resets `target/chibi-scheme` from remote on every build.

```bash
# correct workflow:
cd ~/forks/chibi-scheme
# edit files...
git add lib/tein/file.sld lib/tein/file.scm
git commit -m "..."
git push
cd /home/fey/projects/tein
just clean && cargo build
```

### current chibi fork state

`~/forks/chibi-scheme` is at `2b95710b` (pushed this session). `target/chibi-scheme` will pull from there on next `just clean && cargo build`.

current `file.sld` (6 exports, no primitives):
```scheme
(define-library (tein file)
  (import (scheme base))
  (export file-exists? delete-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file)
  (include "file.scm"))
```

current `file.scm`: defines 4 higher-order wrappers calling `open-input-file` / `open-output-file` (free var refs that resolve from context env at call time).

---

## tests green after task 6

all 706 tests pass. last commit: `414d834`.

trampoline tests now in context.rs (search `test_open_input_file_trampoline_allowed`):
- `test_open_input_file_trampoline_allowed` — allowed path, sandboxed ✓
- `test_open_input_file_trampoline_denied` — denied path, sandboxed ✓
- `test_open_output_file_trampoline_allowed` — allowed path, sandboxed ✓
- `test_open_output_file_trampoline_denied` — denied path, sandboxed ✓
- `test_open_input_file_unsandboxed_passthrough` — unsandboxed, delegates to chibi original ✓

scheme test: `tein/tests/scheme/tein_file_open.scm` + `test_scheme_tein_file_open` ✓
