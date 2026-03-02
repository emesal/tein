# Handoff: VFS shadow + (scheme file/repl/show) implementation

**branch:** `feature/vfs-shadow-scheme-file-2603`
**plan file:** `docs/plans/2026-03-01-vfs-shadow-scheme-file.md`
**date:** 2026-03-01

---

## progress

tasks 1–8 complete. C-level FsPolicy refactor (separate plan) complete. task 12 (PR) remains.

### completed
- **task 1:** branch created, GH issue #97 opened for deferred `(scheme eval)` + full REPL
- **task 2:** `VfsSource::Shadow` variant + `shadow_sld` field added to `VfsEntry`; all 123 existing entries updated with `shadow_sld: None`; `scheme/file` and `scheme/repl` shadow entries added; `scheme/file` added to `srfi/166/columnar` deps; `VfsSource::Shadow` arm added in `build.rs` export extraction; sandbox test updated (removed stale negative assertion for `scheme/repl`, added positive assertions for `scheme/file` and `scheme/repl`)
- **task 3:** `register_vfs_shadows()` added to `sandbox.rs` (has access to `VFS_REGISTRY` via `include!`); called in `build()` after `IS_SANDBOXED.with(|c| c.set(true))` and before `VFS_GATE` is armed
- **task 4:** `capture_file_originals()` unsafe fn + `open_file_trampoline()` shared impl + 4 `open_*_file_trampoline` extern "C" fns added; `register_file_module` updated to register all 6 trampolines; originals captured in both sandbox path (from `source_env`) and unsandboxed path (from `context.ctx` env)
- **task 5:** `check_and_delegate`, 4 `wrapper_open_*` fns, `wrapper_fn_for` removed; `has_io` block removed; `FsPolicy` setup relocated outside sandbox block (works for both paths)
- **task 6:** chibi fork updated (`~/forks/chibi-scheme` branch `emesal-tein`); `file.sld` exports 6 symbols; `file.scm` defines 4 higher-order wrappers; `(scheme file)` shadow updated; `capture_file_originals` skip-guard added; all 706 tests green
- **task 7:** VFS shadow integration tests written and passing (710/710). also fixed `(scheme file)` shadow — original `(define open-input-file open-input-file)` approach didn't work (chibi compiles free-var refs as UNDEF slots at library compile time). fix: register open-*-file trampolines under both R7RS names AND internal `tein-open-*-file` names; export internal names from `(tein file)` sld; shadow sld uses `(rename (tein file) ...)` to import them as R7RS names. fork updated and pushed (`d35f167e`).
- **task 8:** RESOLVED. root cause was missing `ClibEntry` for `srfi/27`, `srfi/95`, `srfi/98` — all three use `include-shared` (C extensions) but had `clib: None`. this was NOT sandbox-specific; they failed in unsandboxed contexts too. fix: added clibs + neutered `(tein process)` trampolines for sandbox + added `scheme/process-context` and `srfi/98` shadows. see batch 4 details below.

### completed via C-level FsPolicy plan (`docs/plans/2026-03-02-c-level-fspolicy-plan.md`)
- **task 9 (original):** `srfi/166/columnar` from-file integration tests — both pass with C-level enforcement
- **task 10 (original):** docs — AGENTS.md sandboxing/IO flows updated, sandbox.rs comment updated, design doc marked IMPLEMENTED
- **task 11 (original):** final verification — 714 tests pass, lint clean

### C-level FsPolicy refactor (completed)
moved `open-*-file` FsPolicy enforcement from rust trampolines to C-level opcodes:
- `tein_shim.c`: FS policy gate + callback dispatcher
- `eval.c`: patches F (open-input-file) and G (open-output-file)
- `ffi.rs`: `tein_fs_policy_check` C→rust callback
- `sandbox.rs`: `FS_GATE` thread-local, armed in sandboxed build
- removed: `ORIGINAL_PROCS`, `capture_file_originals`, `IoOp`, 4 open-*-file trampolines
- `(tein file)` simplified: imports `(chibi)` for opcodes
- `(scheme file)` shadow simplified: pure re-export from `(tein file)`

### remaining
- **task 12:** PR creation (base branch: `dev`, closes #91)

---

## critical corrections (accumulated)

**`Value::Bool` doesn't exist — use `Value::Boolean`**

the plan's test code throughout uses `Value::Bool(true)` / `Value::Bool(false)`. the actual variant is `Value::Boolean(bool)`.

**`Value::Null` doesn't exist — use `Value::Nil`**

empty list `'()` is `Value::Nil`, not `Value::Null`.

**`(procedure? ...)` needs `(scheme base)` imported**

sandboxed null_env has no builtins — `(import (scheme base))` is required before any `let`, `close-input-port`, `procedure?`, etc.

**`(scheme file)` shadow must use import+rename, not `(define x x)`**

the original plan's shadow approach using `(begin (define open-input-file open-input-file) ...)` does NOT work. chibi compiles library bodies and free-var refs become UNDEF slots at compile time. fix: register trampolines under internal names AND R7RS names, export internal names from `(tein file)`, shadow uses `(rename (tein file) ...)`.

**`test_scheme_file_not_shadowed_unsandboxed` → renamed to `test_tein_file_not_shadowed_unsandboxed`**

`(scheme file)` is NOT available in unsandboxed contexts (tein's module path is VFS-only, shadow only registers in sandboxed mode). use `(tein file)` instead.

---

## batch 4: task 8 resolution — missing clibs + sandbox-safe (tein process)

### root cause analysis

the `(scheme show)` / `(srfi 166)` blocker was traced via systematic bisection:

```
(scheme show) → (srfi 166) → (srfi 166/base) → (srfi 165) → (srfi 128)
  → (srfi 27) [include-shared "27/rand" — NO ClibEntry!]
  → (srfi 95) [include-shared "95/qsort" — NO ClibEntry!]
  → (srfi 98) [include-shared "98/env" — NO ClibEntry!]
```

all three modules had `clib: None` in `vfs_registry.rs` despite their `.sld` files using `include-shared`. chibi's `include-shared` needs a statically-linked C library registered in `tein_clibs.c` (generated by `build.rs`). without the `ClibEntry`, the C code was never compiled, so chibi returned `EvalError("")` on load. this was NOT sandbox-specific — they failed in unsandboxed contexts too.

the previous blocker hypothesis about VFS gate / clib path handling / `(chibi ast)` was a red herring — `(chibi ast)` loads fine; the issue was three levels deeper in the dependency chain.

### changes (commit `3a80842`)

**`vfs_registry.rs`:**
- added `ClibEntry` for `srfi/27` (rand.c), `srfi/95` (qsort.c), `srfi/98` (env.c)
- flipped `tein/process` to `default_safe: true` (trampolines now sandbox-aware)
- added `scheme/process-context` shadow entry (`VfsSource::Shadow`) — re-exports from `(tein process)`, adds `emergency-exit` alias for `exit`
- added `srfi/98` shadow `.sld` — neutered `get-environment-variable` (returns `#f`) and `get-environment-variables` (returns `'()`) in sandbox

**`context.rs`:**
- `get_env_var_trampoline`: returns `#f` when `IS_SANDBOXED`
- `get_env_vars_trampoline`: returns `'()` when `IS_SANDBOXED`
- `command_line_trampoline`: returns `'("tein")` when `IS_SANDBOXED`
- `test_tein_process_blocked_by_default_sandbox` → renamed to `test_tein_process_safe_in_sandbox`, now asserts: importable, env vars neutered, command-line faked

**`sandbox.rs`:**
- updated module comment block to document all 4 shadow modules
- updated `registry_safe_allowlist_contains_expected_modules` test: added `tein/process`, `scheme/process-context`, `scheme/show`, `srfi/166`; removed negative `tein/process` assertion

**`ffi.rs`:**
- fixed pre-existing clippy `let_and_return` warning

### test state

712/712 tests pass, lint clean.

---

## architecture notes (all batches)

### (tein file) export architecture (post C-level FsPolicy)

**`(tein file)` exports 10 symbols**: `file-exists?`, `delete-file`, `open-input-file`, `open-binary-input-file`, `open-output-file`, `open-binary-output-file`, `call-with-input-file`, `call-with-output-file`, `with-input-from-file`, `with-output-to-file`.

`open-*-file` are chibi opcodes from `(chibi)` — imported by `(tein file)` and re-exported. FsPolicy enforcement is at the C opcode level (eval.c patches F, G). `file-exists?` and `delete-file` are rust trampolines. the 4 higher-order wrappers are scheme-defined in `file.scm`.

### (scheme file) shadow — pure re-export

the shadow `.sld` is a simple re-export from `(tein file)`:
```scheme
(define-library (scheme file)
  (import (tein file))
  (export file-exists? delete-file
          open-input-file open-binary-input-file
          open-output-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file))
```

### (scheme file) unsandboxed — NOT available

`(scheme file)` is ONLY available in sandboxed contexts (via shadow). in unsandboxed contexts, tein's module path is VFS-only and no shadow is registered. use `(tein file)` instead.

### (tein process) sandbox behaviour

trampolines check `IS_SANDBOXED` thread-local:
- `get-environment-variable` → `#f`
- `get-environment-variables` → `'()`
- `command-line` → `'("tein")`
- `exit` → unchanged (eval escape hatch, safe by design)

### shadow modules summary

| module | shadow strategy | deps |
|--------|----------------|------|
| `scheme/file` | import+rename from `(tein file)` | `tein/file` |
| `scheme/repl` | `(current-environment)` from `(chibi)` | none |
| `scheme/process-context` | re-exports from `(tein process)` + `emergency-exit` alias | `tein/process` |
| `srfi/98` | pure-scheme stubs (neutered) | `scheme/base` |

### chibi fork state

`~/forks/chibi-scheme` (branch `emesal-tein`):
- `tein_shim.c`: FS policy gate + `tein_fs_check_access` dispatcher + `tein_fs_policy_gate_set`
- `eval.c`: patches F (open-input-file) + G (open-output-file) — call `tein_fs_check_access` before `fopen()`
- `file.sld`: imports `(chibi)` for opcodes, exports 10 symbols (R7RS names)
- `file.scm`: 4 higher-order wrappers, updated header comment

---

## tests added (all batches)

in `context.rs`:
- `test_scheme_file_shadow_importable_in_sandbox` — sandbox + FsPolicy, (scheme file) works ✓
- `test_scheme_file_shadow_denies_without_policy` — sandbox no policy, denied ✓
- `test_tein_file_not_shadowed_unsandboxed` — unsandboxed uses (tein file) directly ✓
- `test_scheme_repl_shadow_returns_environment` — (scheme repl) returns env ✓
- `test_scheme_show_importable_in_sandbox` — (scheme show) works in sandbox ✓
- `test_srfi_166_base_importable_in_sandbox` — (srfi 166 base) works in sandbox ✓
- `test_tein_process_safe_in_sandbox` — (tein process) importable, env/argv neutered ✓

in `sandbox.rs`:
- `registry_safe_allowlist_contains_expected_modules` — updated with new safe modules ✓

---

## 714/714 tests green, lint clean. last commit: `600fe2b`.
