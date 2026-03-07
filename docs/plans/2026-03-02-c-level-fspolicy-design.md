# C-level FsPolicy enforcement in chibi's open-*-file opcodes

**status: IMPLEMENTED**
**date:** 2026-03-02
**branch:** `feature/vfs-shadow-scheme-file-2603` (continuation)

---

## problem

rust trampolines for `open-*-file` register into the top-level env via `define_fn_variadic`. library envs have NULL parent chains (`make-environment` → no inheritance from top-level). when `(srfi 166 columnar)` imports `open-input-file` through `(scheme file)` shadow → `(tein file)`, chibi can't resolve the name at the library level — it compiles as UNDEF, failing at runtime with "undefined variable".

## solution

move FsPolicy enforcement into the C-level opcode implementations (`sexp_open_input_file_op` / `sexp_open_output_file_op` in `eval.c`). add a C→rust callback `tein_fs_policy_check(path, is_read)` following the existing `tein_vfs_gate_check` pattern.

opcodes live in the core env, visible to ALL code — libraries, user code, modules. single enforcement point, zero duplication.

## changes

### added

**`tein_shim.c`** (~/forks/chibi-scheme):
- `TEIN_THREAD_LOCAL int tein_fs_policy_gate = 0;` — two-level gate (0=off, 1=check)
- `extern int tein_fs_policy_check(const char *path, int is_read);` — rust callback
- `int tein_fs_check_access(const char *path, int is_read)` — dispatcher (gate 0 → allow, gate 1 → callback)
- `void tein_fs_policy_gate_set(int level);` — called from rust

**`eval.c`** (~/forks/chibi-scheme):
- patch F: `sexp_open_input_file_op` — after VFS lookup returns NULL, before `fopen()`: call `tein_fs_check_access(path, 1)`. deny → return error exception
- patch G: `sexp_open_output_file_op` — same with `is_read=0`
- binary variants (`sexp_open_binary_input_file`, `sexp_open_binary_output_file`) delegate to these → inherit enforcement

**`ffi.rs`**:
- `#[unsafe(no_mangle)] extern "C" fn tein_fs_policy_check(path, is_read) -> c_int` — calls existing `check_fs_access()`
- extern declaration + safe wrapper for `tein_fs_policy_gate_set`

**`sandbox.rs`**:
- `FS_GATE` thread-local `Cell<u8>` + `FS_GATE_OFF` / `FS_GATE_CHECK` constants

**`context.rs` (build)**:
- set `FS_GATE` + C-level `tein_fs_policy_gate` when sandboxed + has file policy
- clear on `Context::drop()` (RAII)

### removed

**`context.rs`**:
- `ORIGINAL_PROCS` thread-local
- `capture_file_originals()`
- `open_file_trampoline()` shared impl
- 4 `open_*_file_trampoline` extern "C" fns
- `IoOp` enum
- all `tein-open-*` registrations from `register_file_module`
- `capture_file_originals()` call sites in both build paths

### simplified

**`register_file_module`**: only registers `file-exists?` and `delete-file`

**`(tein file)` file.sld** (chibi fork):
- import `(chibi)` instead of `(scheme base)` — opcodes enter library env
- export all 10 names (open-* from `(chibi)`, higher-order from `file.scm`)
- remove `tein-open-*` exports

**`(scheme file)` shadow .sld** (vfs_registry.rs):
```scheme
(define-library (scheme file)
  (import (tein file))
  (export file-exists? delete-file
          open-input-file open-binary-input-file
          open-output-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file))
```
pure re-export from `(tein file)` — zero duplication.

## invariants

- **VFS paths bypass policy check**: VFS lookup (`tein_vfs_lookup`) returns early with a string port before `fopen()` is reached. module loading is unaffected.
- **unsandboxed**: gate=0, callback never called. opcodes behave as chibi's native. no performance impact.
- **binary variants**: delegate to text variants → inherit enforcement automatically.
- **`file-exists?` / `delete-file`**: remain as rust trampolines (no opcode equivalents). `file-exists?` checks `IS_SANDBOXED` + `FsPolicy` at the rust level. `delete-file` same.
- **gate RAII**: `FS_GATE` + `tein_fs_policy_gate` cleared on `Context::drop()`, same pattern as `VFS_GATE`.

## callback flow

```
scheme code calls (open-input-file "/some/path")
  → sexp_open_input_file_op (eval.c opcode)
    → tein_vfs_lookup(path) → NULL (not VFS)
    → tein_fs_check_access(path, 1) (tein_shim.c)
      → gate == 0? allow
      → gate == 1? tein_fs_policy_check(path, 1) (ffi.rs rust callback)
        → check_fs_access(path, FsAccess::Read) (context.rs)
          → IS_SANDBOXED? no → allow
          → IS_SANDBOXED? yes → FS_POLICY.check_read(path)
    → allowed? fopen(path, "r") → port
    → denied? sexp_user_exception("file access denied")
```
