# VFS Shadow: (scheme file) + (scheme show) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Expand `(tein file)` to the full `(scheme file)` surface, inject a dynamic VFS shadow so sandboxed contexts resolve `(scheme file)` through our policy-checked trampolines, and enable `(scheme show)` / `(srfi 166)` in `Modules::Safe`.

**Architecture:** New rust trampolines for `open-input-file`, `open-binary-input-file`, `open-output-file`, `open-binary-output-file` follow the `file_exists_trampoline` pattern (check `IS_SANDBOXED` + `FsPolicy`, delegate to captured originals via `ORIGINAL_PROCS`). Original capture moves into `register_file_module(source_env)` (called in the sandbox build path where `source_env` is available) and also for the unsandboxed path. A new `register_vfs_shadows()` injects `scheme/file.sld` into the dynamic VFS (keyed as `/vfs/lib/scheme/file.sld`) before the VFS gate is armed. The old IO wrapper system (`check_and_delegate`, `wrapper_open_*`, `wrapper_fn_for`, `has_io` block) is removed. Four higher-order scheme wrappers live in `file.scm` in the chibi fork.

**Tech Stack:** Rust (unsafe FFI, thread-locals), chibi-scheme C FFI, Scheme (`.sld`/`.scm` in `emesal/chibi-scheme` branch `emesal-tein`), `just` for commands.

**Design doc:** `docs/plans/2026-03-01-vfs-shadow-scheme-file-design.md`

**Base branch:** `dev`
**Branch to create:** `just feature vfs-shadow-scheme-file-2603`

---

## Architecture notes (read before implementing)

### build() flow — two paths

The sandboxed context build path in `build()` is inside `if let Some(ref modules) = self.sandbox_modules.take()`. This block has access to `source_env` (captured just before sandbox restriction). After the sandbox block, `build()` creates the `Context` struct and then calls `register_file_module` etc. on it.

**Critical:** The 4 `open-*` originals must be captured **inside the sandbox block** (where `source_env` is live), not in `register_file_module` which runs after the env has been restricted.

### IO wrapper system — current vs target

**Current (to remove):**
- `check_and_delegate` — shared impl for 4 wrappers
- `wrapper_open_input_file` + 3 others — registered directly into `null_env` in `has_io` block
- `wrapper_fn_for` — dispatch table
- `has_io` block — captures originals from `source_env`, registers wrappers into `null_env`, sets `FS_POLICY`

**Target (new):**
- `open_file_trampoline(ctx, args, op)` — shared impl (reads `ORIGINAL_PROCS`)
- `open_input_file_trampoline` + 3 others — registered via `register_file_module`
- Original capture: done in `build()` sandbox block (via a helper fn or inline), and also for unsandboxed path
- `FS_POLICY` set in `build()` after context creation (moved out of `has_io` block, set unconditionally when prefixes are configured)

### VFS shadow registration timing

`register_vfs_shadows()` must be called **before** `VFS_GATE` is armed. The shadow `.sld` goes in as `/vfs/lib/scheme/file.sld` — the same key the chibi resolver would look up. The call site is in the sandbox block, after `IS_SANDBOXED` is set but before `VFS_GATE.with(|cell| cell.set(GATE_CHECK))`.

### ORIGINAL_PROCS capture — both paths

Unsandboxed contexts also need originals captured (for `open-input-file` etc. when called from `(tein file)` in non-sandboxed mode where we still delegate but without policy). The capture should happen in both the sandboxed and unsandboxed paths when `standard_env` is true.

Best approach: add a helper `capture_file_originals(ctx, env)` that captures from the given env into `ORIGINAL_PROCS`. Call it:
1. In the sandbox block before env restriction (captures from `source_env`)
2. In the unsandboxed path (captures from the default context env)

---

## Task 1: Create feature branch

```bash
cd /home/fey/projects/tein
just feature vfs-shadow-scheme-file-2603
```

---

## Task 2: Add 4 open-* trampolines to context.rs + add tests

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Write failing tests**

Add inside the `#[cfg(test)]` IO policy test section (find `IO_TEST_LOCK` — after `fn test_file_io_with_sandboxed_modules`). These test the trampolines via `(tein file)` import:

```rust
#[test]
fn test_open_input_file_trampoline_allowed() {
    let _lock = IO_TEST_LOCK.lock().unwrap();
    let dir = io_test_dir("open_input_allowed");
    let file = dir.join("data.txt");
    std::fs::write(&file, "hello").expect("write");
    let canon_dir = dir.canonicalize().unwrap();
    let path = file.to_str().unwrap().to_string();
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .file_read(&[canon_dir.to_str().unwrap()])
        .build()
        .expect("builder");
    let code = format!(
        "(import (tein file)) (let ((p (open-input-file \"{path}\"))) (close-input-port p) #t)"
    );
    let r = ctx.evaluate(&code).expect("open-input-file allowed");
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_open_input_file_trampoline_denied() {
    let _lock = IO_TEST_LOCK.lock().unwrap();
    let dir = io_test_dir("open_input_denied");
    let file = dir.join("secret.txt");
    std::fs::write(&file, "no").expect("write");
    let path = file.to_str().unwrap().to_string();
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .file_read(&["/tmp/__nonexistent_prefix__/"])
        .build()
        .expect("builder");
    let code = format!("(import (tein file)) (open-input-file \"{path}\")");
    assert!(ctx.evaluate(&code).is_err(), "should be denied");
}

#[test]
fn test_open_output_file_trampoline_allowed() {
    let _lock = IO_TEST_LOCK.lock().unwrap();
    let dir = io_test_dir("open_output_allowed");
    let file = dir.join("out.txt");
    let canon_dir = dir.canonicalize().unwrap();
    let path = file.to_str().unwrap().to_string();
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .file_write(&[canon_dir.to_str().unwrap()])
        .build()
        .expect("builder");
    let code = format!(
        "(import (tein file)) (let ((p (open-output-file \"{path}\"))) (close-output-port p) #t)"
    );
    let r = ctx.evaluate(&code).expect("open-output-file allowed");
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_open_output_file_trampoline_denied() {
    let _lock = IO_TEST_LOCK.lock().unwrap();
    let dir = io_test_dir("open_output_denied");
    let path = dir.join("nope.txt").to_str().unwrap().to_string();
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .file_write(&["/tmp/__nonexistent_prefix__/"])
        .build()
        .expect("builder");
    let code = format!("(import (tein file)) (open-output-file \"{path}\")");
    assert!(ctx.evaluate(&code).is_err(), "should be denied");
}

#[test]
fn test_open_input_file_unsandboxed_passthrough() {
    // unsandboxed: open-input-file trampoline delegates to chibi original unconditionally
    let tmp = "/tmp/tein_open_unsandboxed_test.txt";
    std::fs::write(tmp, "test").expect("write");
    let ctx = Context::builder()
        .standard_env()
        .build()
        .expect("builder");
    let r = ctx.evaluate(&format!(
        "(import (tein file)) (let ((p (open-input-file \"{tmp}\"))) (close-input-port p) #t)"
    ));
    assert_eq!(r.expect("unsandboxed passthrough"), Value::Bool(true));
}
```

**Step 2: Run to confirm failure**
```bash
cargo test test_open_input_file_trampoline_allowed test_open_input_file_trampoline_denied test_open_output_file_trampoline_allowed test_open_output_file_trampoline_denied test_open_input_file_unsandboxed_passthrough 2>&1 | tail -20
```
Expected: FAIL — `(tein file)` doesn't export `open-input-file` yet (import error or unbound).

**Step 3: Add `capture_file_originals` helper + 4 trampolines + shared impl**

Find the `// --- (tein file) trampolines ---` comment block (around line 1187). Add below `delete_file_trampoline` (~line 1251):

```rust
// --- open-*-file trampolines ---

/// Capture chibi's native `open-*-file` primitives from `env` into `ORIGINAL_PROCS`.
///
/// must be called before env restriction (sandbox) or on the full env (unsandboxed).
/// safe to call multiple times — later calls overwrite earlier ones.
///
/// # Safety
/// `ctx` and `env` must be valid chibi context and env pointers.
unsafe fn capture_file_originals(ctx: ffi::sexp, env: ffi::sexp) {
    unsafe {
        let undefined = ffi::get_void();
        for op in IoOp::ALL {
            let name = op.name();
            let c_name = CString::new(name).unwrap();
            let sym =
                ffi::sexp_intern(ctx, c_name.as_ptr(), name.len() as ffi::sexp_sint_t);
            let val = ffi::sexp_env_ref(ctx, env, sym, undefined);
            if val != undefined {
                ORIGINAL_PROCS.with(|procs| procs[op as usize].set(val));
            }
        }
    }
}

/// shared implementation for all 4 `open-*-file` trampolines.
///
/// checks `FsPolicy` (read for input ops, write for output ops), then delegates
/// to the captured original chibi primitive. if IS_SANDBOXED is false,
/// delegates unconditionally (unsandboxed passthrough).
///
/// # Safety
/// `ctx` and `args` must be valid sexp values.
unsafe fn open_file_trampoline(ctx: ffi::sexp, args: ffi::sexp, op: IoOp) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, op.name()) {
            Ok(s) => s,
            Err(e) => return e,
        };

        let access = if op.is_read() {
            FsAccess::Read
        } else {
            FsAccess::Write
        };
        if !check_fs_access(path, access) {
            let dir = if op.is_read() { "read" } else { "write" };
            let msg = format!("[sandbox:file] {} ({dir} not permitted)", path);
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        let original = ORIGINAL_PROCS.with(|procs| procs[op as usize].get());
        ffi::sexp_apply_proc(ctx, original, args)
    }
}

/// `open-input-file` trampoline: policy-checked textual input port opener.
unsafe extern "C" fn open_input_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { open_file_trampoline(ctx, args, IoOp::InputFile) }
}

/// `open-binary-input-file` trampoline: policy-checked binary input port opener.
unsafe extern "C" fn open_binary_input_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { open_file_trampoline(ctx, args, IoOp::BinaryInputFile) }
}

/// `open-output-file` trampoline: policy-checked textual output port opener.
unsafe extern "C" fn open_output_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { open_file_trampoline(ctx, args, IoOp::OutputFile) }
}

/// `open-binary-output-file` trampoline: policy-checked binary output port opener.
unsafe extern "C" fn open_binary_output_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { open_file_trampoline(ctx, args, IoOp::BinaryOutputFile) }
}
```

**Step 4: Update `register_file_module`**

Find `fn register_file_module` (~line 2938). Update docstring and add 4 new registrations:

```rust
/// Register all 6 `(tein file)` trampolines.
///
/// Called during `build()` after context creation. Originals are captured
/// separately via `capture_file_originals()` before env restriction.
fn register_file_module(&self) -> Result<()> {
    self.define_fn_variadic("file-exists?", file_exists_trampoline)?;
    self.define_fn_variadic("delete-file", delete_file_trampoline)?;
    self.define_fn_variadic("open-input-file", open_input_file_trampoline)?;
    self.define_fn_variadic("open-binary-input-file", open_binary_input_file_trampoline)?;
    self.define_fn_variadic("open-output-file", open_output_file_trampoline)?;
    self.define_fn_variadic("open-binary-output-file", open_binary_output_file_trampoline)?;
    Ok(())
}
```

**Step 5: Capture originals in both build paths**

In `build()`, the sandbox block starts at `if let Some(ref modules) = self.sandbox_modules.take()`. Inside, right after `let source_env = ffi::sexp_context_env(ctx)` (~line 1733), add:

```rust
// capture open-*-file originals before env restriction
capture_file_originals(ctx, source_env);
```

For the unsandboxed path: find `if self.standard_env { context.register_file_module()?; ... }` (~line 1898). This runs AFTER `Context` is created. The unsandboxed context env is the full standard env. Add capture before `register_file_module`:

```rust
if self.standard_env {
    // capture open-*-file originals from full standard env (unsandboxed)
    unsafe { capture_file_originals(ctx, ffi::sexp_context_env(ctx)); }
    context.register_file_module()?;
    context.register_load_module()?;
    context.register_process_module()?;
}
```

> **Note:** In the sandbox path, `register_file_module` is called on `context` after `sexp_context_env_set(ctx, null_env)` has already switched the env. So `define_fn_variadic` registers into the null env (the restricted env) — correct behaviour.

**Step 6: Run failing tests**
```bash
cargo test test_open_input_file_trampoline_allowed test_open_input_file_trampoline_denied test_open_output_file_trampoline_allowed test_open_output_file_trampoline_denied test_open_input_file_unsandboxed_passthrough 2>&1 | tail -20
```
These will still fail until `file.sld` exports are updated (task 4). **That's fine** — continue.

**Step 7: Check for compile errors**
```bash
cargo build 2>&1 | tail -20
```
Should compile clean.

**Step 8: Commit**
```bash
git add tein/src/context.rs
git commit -m "feat: add open-*-file trampolines + capture_file_originals to (tein file)"
```

---

## Task 3: Remove the old IO wrapper system from context.rs

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Identify + delete dead code**

Remove these items (search by name):
- `unsafe fn check_and_delegate(...)` (~40 lines)
- `unsafe extern "C" fn wrapper_open_input_file(...)` (~10 lines)
- `unsafe extern "C" fn wrapper_open_binary_input_file(...)` (~10 lines)
- `unsafe extern "C" fn wrapper_open_output_file(...)` (~10 lines)
- `unsafe extern "C" fn wrapper_open_binary_output_file(...)` (~10 lines)
- `fn wrapper_fn_for(...)` (~10 lines)

**Step 2: Remove / rework the `has_io` block**

Find the block starting `// IO wrappers: capture original procs from source env, register wrappers` (~line 1812). This block:
1. Takes `file_read_prefixes` / `file_write_prefixes` from `self`
2. Captures originals into `ORIGINAL_PROCS` (now done by `capture_file_originals` earlier)
3. Registers wrapper fns into `null_env` (now done by `register_file_module`)
4. Sets `FS_POLICY`

**Remove** the entire `has_io` block (`let file_read_prefixes = ...; let file_write_prefixes = ...; let has_io = ...; if has_io { ... }`).

**Then add** FsPolicy setup **after** the sandbox block closes (i.e., right before `let context = Context { ... }`), unconditional on `has_io`:

```rust
// set FsPolicy if file_read() or file_write() was configured
{
    let file_read_prefixes = self.file_read_prefixes.take();
    let file_write_prefixes = self.file_write_prefixes.take();
    if file_read_prefixes.is_some() || file_write_prefixes.is_some() {
        FS_POLICY.with(|cell| {
            *cell.borrow_mut() = Some(FsPolicy {
                read_prefixes: file_read_prefixes.unwrap_or_default(),
                write_prefixes: file_write_prefixes.unwrap_or_default(),
            });
        });
    }
}
```

> This way `FS_POLICY` is set for **both** sandboxed and (future) unsandboxed-with-policy scenarios.

**Step 3: Keep** `IoOp` enum, `IoOp::ALL`, `IoOp::name()`, `IoOp::is_read()`, `ORIGINAL_PROCS` — still needed by new trampolines.

**Step 4: Compile check**
```bash
cargo build 2>&1 | tail -20
```
Fix any compile errors.

**Step 5: Run full test suite**
```bash
just test 2>&1 | tail -30
```
The IO policy tests (`test_file_read_allowed_path`, `test_file_write_allowed_path`, etc.) must still pass — the policy mechanism is unchanged, just the registration path changed.

**Step 6: Lint**
```bash
just lint
```

**Step 7: Commit**
```bash
git add tein/src/context.rs
git commit -m "refactor: remove IO wrapper system, policy enforcement via (tein file) trampolines"
```

---

## Task 4: Expand (tein file) scheme files in the chibi fork

**IMPORTANT:** Changes go in `target/chibi-scheme/` then **must be pushed** to `emesal/chibi-scheme` branch `emesal-tein` before `cargo build` (which hard-resets from remote).

**Files:**
- Modify: `target/chibi-scheme/lib/tein/file.sld`
- Modify: `target/chibi-scheme/lib/tein/file.scm`

**Step 1: Write failing scheme test**

Create `tein/tests/scheme/tein_file_open.scm`:
```scheme
(import (tein test) (tein file) (scheme base))

(test-group "tein file higher-order wrappers"
  (test "call-with-input-file is procedure"
        #t (procedure? call-with-input-file))
  (test "call-with-output-file is procedure"
        #t (procedure? call-with-output-file))
  (test "with-input-from-file is procedure"
        #t (procedure? with-input-from-file))
  (test "with-output-to-file is procedure"
        #t (procedure? with-output-to-file)))
```

**Step 2: Run to confirm failure**
```bash
cargo test tein_file_open 2>&1 | tail -20
```
Expected: FAIL — `call-with-input-file` not exported from `(tein file)`.

**Step 3: Update `file.sld`**

Replace `target/chibi-scheme/lib/tein/file.sld`:
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

**Step 4: Update `file.scm`**

Replace `target/chibi-scheme/lib/tein/file.scm`:
```scheme
;;; (tein file) — safe file IO with FsPolicy enforcement
;;;
;;; file-exists?, delete-file, open-input-file, open-binary-input-file,
;;; open-output-file, open-binary-output-file are rust trampolines registered
;;; by register_file_module() in context.rs. policy:
;;;   - unsandboxed: allow all (delegate to chibi original)
;;;   - sandboxed + policy: check prefix, then delegate
;;;   - sandboxed + no policy: deny (sandbox violation)
;;;
;;; the 4 higher-order wrappers below delegate to the above primitives —
;;; policy enforcement happens at open-* (single point of check).

(define (call-with-input-file filename proc)
  (let ((port (open-input-file filename)))
    (dynamic-wind
      (lambda () #f)
      (lambda () (proc port))
      (lambda () (close-input-port port)))))

(define (call-with-output-file filename proc)
  (let ((port (open-output-file filename)))
    (dynamic-wind
      (lambda () #f)
      (lambda () (proc port))
      (lambda () (close-output-port port)))))

(define (with-input-from-file filename thunk)
  (let ((port (open-input-file filename)))
    (dynamic-wind
      (lambda () #f)
      (lambda ()
        (parameterize ((current-input-port port))
          (thunk)))
      (lambda () (close-input-port port)))))

(define (with-output-to-file filename thunk)
  (let ((port (open-output-file filename)))
    (dynamic-wind
      (lambda () #f)
      (lambda ()
        (parameterize ((current-output-port port))
          (thunk)))
      (lambda () (close-output-port port)))))
```

**Step 5: Push chibi fork changes**
```bash
cd /home/fey/projects/tein/target/chibi-scheme
git add lib/tein/file.sld lib/tein/file.scm
git commit -m "feat: expand (tein file) to full (scheme file) surface — 10 exports"
git push
cd /home/fey/projects/tein
```

**Step 6: Rebuild**
```bash
just clean && cargo build 2>&1 | tail -20
```

**Step 7: Run scheme test**
```bash
cargo test tein_file_open 2>&1 | tail -20
```
Expected: PASS.

**Step 8: Run all earlier trampoline tests now (they need updated .sld)**
```bash
cargo test test_open_input_file_trampoline_allowed test_open_input_file_trampoline_denied test_open_output_file_trampoline_allowed test_open_output_file_trampoline_denied test_open_input_file_unsandboxed_passthrough 2>&1 | tail -20
```
Expected: PASS.

**Step 9: Full test suite**
```bash
just test 2>&1 | tail -30
```

**Step 10: Commit tein side**
```bash
git add tein/tests/scheme/tein_file_open.scm
git commit -m "test: scheme test for (tein file) higher-order wrappers"
```

---

## Task 5: VFS shadow for (scheme file)

**Files:**
- Modify: `tein/src/context.rs`
- Modify: `tein/src/vfs_registry.rs`

**Step 1: Write failing tests**

Add to the sandboxed tests section in context.rs (find `// --- task 8:` around line 7495):

```rust
#[test]
fn test_scheme_file_shadow_importable_in_sandbox() {
    use crate::sandbox::Modules;
    let tmp = "/tmp/tein_shadow_file_test.txt";
    std::fs::write(tmp, "shadowed").expect("write");
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .file_read(&["/tmp/"])
        .build()
        .expect("builder");
    // (scheme file) in sandbox should resolve to our shadow
    let r = ctx.evaluate(&format!(
        "(import (scheme file)) (let ((p (open-input-file \"{tmp}\"))) (close-input-port p) #t)"
    ));
    assert_eq!(r.expect("scheme file shadow works"), Value::Bool(true));
}

#[test]
fn test_scheme_file_shadow_denies_without_policy() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        // no file_read configured
        .build()
        .expect("builder");
    let r = ctx.evaluate("(import (scheme file)) (open-input-file \"/etc/passwd\")");
    assert!(r.is_err(), "scheme/file open-input-file denied without policy");
}

#[test]
fn test_scheme_file_not_shadowed_unsandboxed() {
    // unsandboxed: (scheme file) resolves to chibi's native, still works
    let tmp = "/tmp/tein_unsandboxed_scheme_file.txt";
    std::fs::write(tmp, "native").expect("write");
    let ctx = Context::builder()
        .standard_env()
        .build()
        .expect("builder");
    let r = ctx.evaluate(&format!(
        "(import (scheme file)) (let ((p (open-input-file \"{tmp}\"))) (close-input-port p) #t)"
    ));
    assert_eq!(r.expect("unsandboxed scheme file works"), Value::Bool(true));
}
```

**Step 2: Run to confirm failure**
```bash
cargo test test_scheme_file_shadow_importable_in_sandbox test_scheme_file_shadow_denies_without_policy test_scheme_file_not_shadowed_unsandboxed 2>&1 | tail -20
```
Expected: FAIL — `(scheme file)` blocked by VFS gate in sandbox (no shadow yet).

**Step 3: Add `register_vfs_shadows()` to context.rs**

Add as a free fn near `register_file_module` (or just before it):

```rust
/// Inject VFS shadow modules for sandboxed contexts.
///
/// Registers replacement `.sld` files into the dynamic VFS under their
/// canonical `/vfs/lib/` paths so chibi's module resolver finds our
/// policy-checked wrappers instead of native implementations.
///
/// Must be called before the VFS gate is armed (before VFS_GATE is set to GATE_CHECK).
///
/// current shadows:
/// - `scheme/file` → re-exports all 10 names from `(tein file)`
fn register_vfs_shadows() {
    const SCHEME_FILE_SLD: &str = "\
(define-library (scheme file)
  (import (tein file))
  (export open-input-file open-output-file
          open-binary-input-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file
          file-exists? delete-file))
";
    let path = c"/vfs/lib/scheme/file.sld";
    unsafe {
        ffi::tein_vfs_register(
            path.as_ptr(),
            SCHEME_FILE_SLD.as_ptr() as *const std::ffi::c_char,
            SCHEME_FILE_SLD.len() as std::ffi::c_uint,
        );
    }
}
```

**Step 4: Call `register_vfs_shadows()` in `build()` sandbox path**

In `build()`, inside the `if let Some(ref modules) = self.sandbox_modules.take()` block, find where `IS_SANDBOXED` is set to `true` (line ~1731):

```rust
IS_SANDBOXED.with(|c| c.set(true));
```

Add the shadow call **immediately after** this (before `VFS_GATE.with(|cell| cell.set(GATE_CHECK))`):

```rust
IS_SANDBOXED.with(|c| c.set(true));
register_vfs_shadows(); // inject scheme/file shadow before gate is armed
```

**Step 5: Add `scheme/file` to `vfs_registry.rs`**

`scheme/file` needs to be in the allowlist so the VFS gate permits it. It's a shadow-only module (no static files — registered dynamically). Add a `VfsSource::Dynamic` entry in the appropriate section (near other `scheme/*` entries — after `scheme/show` or in the r7rs section):

```rust
// scheme/file: VFS shadow — registered dynamically by register_vfs_shadows()
// in sandboxed contexts. resolves to (tein file) trampolines for FsPolicy enforcement.
// unsandboxed contexts use chibi's native scheme/file directly.
VfsEntry {
    path: "scheme/file",
    deps: &["tein/file"],
    files: &[],
    clib: None,
    default_safe: true,
    source: VfsSource::Dynamic,
    feature: None,
},
```

Also update `tein/file` deps to include `scheme/file` so transitive resolution works bidirectionally (any allowlist entry that includes `tein/file` automatically pulls `scheme/file` too):

Find the `tein/file` entry and update its deps:
```rust
VfsEntry {
    path: "tein/file",
    deps: &["scheme/base", "scheme/file"],  // scheme/file added
    ...
}
```

> **Careful:** this creates a cycle `tein/file` ↔ `scheme/file`. The `registry_resolve_deps` function handles cycles via `seen` set — it's safe.

**Step 6: Run tests**
```bash
cargo test test_scheme_file_shadow_importable_in_sandbox test_scheme_file_shadow_denies_without_policy test_scheme_file_not_shadowed_unsandboxed 2>&1 | tail -20
```
Expected: PASS.

**Step 7: Update `registry_safe_allowlist_contains_expected_modules` test in `sandbox.rs`**

Add assertions inside the test:
```rust
assert!(
    safe.iter().any(|m| m == "scheme/file"),
    "scheme/file missing from safe (shadow dep)"
);
```

**Step 8: Full test suite**
```bash
just test 2>&1 | tail -30
```

**Step 9: Lint**
```bash
just lint
```

**Step 10: Commit**
```bash
git add tein/src/context.rs tein/src/vfs_registry.rs
git commit -m "feat: VFS shadow for (scheme file) — sandboxed contexts delegate to (tein file)"
```

---

## Task 6: Enable (scheme show) / (srfi 166) in Modules::Safe

With the shadow in place, `(scheme file)` is resolvable in sandboxed contexts. Flip the relevant registry entries.

**Files:**
- Modify: `tein/src/vfs_registry.rs`

**Step 1: Write failing tests**

Add to context.rs tests:

```rust
#[test]
fn test_scheme_show_importable_in_sandbox() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .step_limit(10_000_000)
        .build()
        .expect("builder");
    let r = ctx.evaluate("(import (scheme show)) (show #f \"hello\")");
    assert!(r.is_ok(), "scheme show importable in sandbox: {:?}", r);
}

#[test]
fn test_srfi_166_base_importable_in_sandbox() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .step_limit(10_000_000)
        .build()
        .expect("builder");
    let r = ctx.evaluate("(import (srfi 166 base)) (show #f (displayed \"test\"))");
    assert!(r.is_ok(), "srfi/166/base importable in sandbox: {:?}", r);
}
```

**Step 2: Run to confirm failure**
```bash
cargo test test_scheme_show_importable_in_sandbox test_srfi_166_base_importable_in_sandbox 2>&1 | tail -20
```
Expected: FAIL — `(scheme show)` blocked by gate.

**Step 3: Update registry**

In `tein/src/vfs_registry.rs`:

1. Find `scheme/show` entry (~line 551). Flip `default_safe: false` → `default_safe: true`. Update comment:
```rust
// scheme/show: (scheme file) dep satisfied via VFS shadow → (tein file)
VfsEntry {
    path: "scheme/show",
    deps: &["srfi/166"],
    ...
    default_safe: true,
    ...
}
```

2. Find `srfi/166` entry (~line 1096). Flip `default_safe: false` → `default_safe: true`.

3. Find `srfi/166/columnar` entry (~line 1155). Add `"scheme/file"` to its deps and flip `default_safe: false` → `default_safe: true`:
```rust
VfsEntry {
    path: "srfi/166/columnar",
    deps: &[
        "scheme/base",
        "scheme/char",
        "scheme/file",   // shadow dep — resolves via (tein file) in sandbox
        "srfi/1",
        "srfi/117",
        "srfi/130",
        "srfi/166/base",
        "chibi/optional",
    ],
    ...
    default_safe: true,
    ...
}
```

**Step 4: Run tests**
```bash
cargo test test_scheme_show_importable_in_sandbox test_srfi_166_base_importable_in_sandbox 2>&1 | tail -20
```
Expected: PASS.

**Step 5: Update `registry_safe_allowlist_contains_expected_modules` in `sandbox.rs`**

Add:
```rust
assert!(
    safe.iter().any(|m| m == "scheme/show"),
    "scheme/show missing from safe"
);
assert!(
    safe.iter().any(|m| m == "srfi/166"),
    "srfi/166 missing from safe"
);
```

**Step 6: Full test suite**
```bash
just test 2>&1 | tail -30
```

**Step 7: Lint**
```bash
just lint
```

**Step 8: Commit**
```bash
git add tein/src/vfs_registry.rs
git commit -m "feat: enable (scheme show) / (srfi 166) in Modules::Safe via (scheme file) shadow"
```

---

## Task 7: Integration test — srfi/166/columnar from-file with FsPolicy

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Write test**

```rust
#[test]
fn test_srfi_166_columnar_from_file_with_policy() {
    use crate::sandbox::Modules;
    let tmp = "/tmp/tein_from_file_test.txt";
    std::fs::write(tmp, "line1\nline2\n").expect("write");
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .file_read(&["/tmp/"])
        .step_limit(10_000_000)
        .build()
        .expect("builder");
    let r = ctx.evaluate(&format!(
        "(import (srfi 166 columnar)) (show #f (from-file \"{tmp}\"))"
    ));
    assert!(r.is_ok(), "from-file with read policy: {:?}", r);
}

#[test]
fn test_srfi_166_columnar_from_file_denied_without_policy() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .step_limit(10_000_000)
        .build()
        .expect("builder");
    // from-file calls open-input-file which hits the policy check
    let r = ctx.evaluate("(import (srfi 166 columnar)) (show #f (from-file \"/tmp/x\"))");
    assert!(r.is_err(), "from-file without policy should fail");
}
```

**Step 2: Run**
```bash
cargo test test_srfi_166_columnar_from_file_with_policy test_srfi_166_columnar_from_file_denied_without_policy 2>&1 | tail -20
```
Expected: PASS.

**Step 3: Full test suite**
```bash
just test 2>&1 | tail -30
```

**Step 4: Commit**
```bash
git add tein/src/context.rs
git commit -m "test: srfi/166/columnar from-file integration with FsPolicy"
```

---

## Task 8: Docs — sandbox.rs comment + AGENTS.md

**Files:**
- Modify: `tein/src/sandbox.rs`
- Modify: `AGENTS.md`

**Step 1: Update sandbox.rs comment block**

Find `// modules NOT in the VFS registry:` (~line 111). Update:
```rust
// unsandboxed-blocked modules:
//
// - `scheme/file` — in the registry as `VfsSource::Dynamic` (shadow-only). in sandboxed
//   contexts, register_vfs_shadows() injects a replacement .sld that re-exports from
//   (tein file), providing FsPolicy enforcement. unsandboxed contexts use chibi's native
//   scheme/file directly (no shadow registered).
// - `scheme/process-context` — `exit`/`emergency-exit` from `(chibi process)` kills the
//   host process, bypassing rust error handling. use `(tein process)` instead.
// - `scheme/load` — loads arbitrary files. use `(tein load)` instead.
// - `scheme/r5rs` — re-exports scheme/file, scheme/load, scheme/process-context.
```

**Step 2: Update AGENTS.md sandboxing flow**

Find: `set IS_SANDBOXED thread-local →`

Change to:
```
set IS_SANDBOXED thread-local → register_vfs_shadows() (injects scheme/file.sld → (tein file)) →
```

**Step 3: Commit**
```bash
git add tein/src/sandbox.rs AGENTS.md
git commit -m "docs: update sandbox comment + AGENTS.md for scheme/file shadow"
```

---

## Task 9: Final verification + PR

**Step 1: Full test suite**
```bash
just test 2>&1 | tail -40
```
Expected: all pass.

**Step 2: Lint**
```bash
just lint
```

**Step 3: Create PR**
```bash
gh pr create \
  --base dev \
  --title "feat: (scheme file) VFS shadow + (scheme show) in Modules::Safe" \
  --body "$(cat <<'EOF'
## summary

- expands `(tein file)` to full `(scheme file)` surface: 4 new `open-*-file` trampolines + 4 higher-order scheme wrappers (`call-with-*`, `with-*-from/to-file`)
- dynamic VFS shadow: sandboxed contexts inject `scheme/file.sld` re-exporting from `(tein file)` — policy-checked and gate-permitted
- removes old IO wrapper system (`check_and_delegate`, `wrapper_open_*`, `wrapper_fn_for`, `has_io` block) — enforcement unified in trampolines
- enables `(scheme show)` / `(srfi 166)` + sub-modules in `Modules::Safe`
- `srfi/166/columnar` `from-file` works in sandboxed contexts with `file_read` policy

closes #91

## test plan

- [ ] `test_open_input_file_trampoline_*` — new trampoline policy tests pass
- [ ] `test_open_input_file_unsandboxed_passthrough` — unsandboxed delegation works
- [ ] `test_scheme_file_shadow_*` — shadow resolution + policy checks
- [ ] `test_scheme_show_importable_in_sandbox` — scheme/show in safe set
- [ ] `test_srfi_166_columnar_from_file_*` — from-file with/without policy
- [ ] scheme/tein_file_open.scm — higher-order wrappers
- [ ] all existing IO policy tests still pass
- [ ] `just test` green
EOF
)"
```

---

## Appendix: unsandboxed path for `file_read`/`file_write`

Currently `file_read`/`file_write` only work in sandboxed contexts (the `has_io` block only runs in the sandbox path). After this refactor, `FS_POLICY` is set outside the sandbox path too — **this is a behaviour change**: unsandboxed contexts with `file_read`/`file_write` configured will now enforce policy via the trampolines.

This is actually *correct* and desirable behaviour (the builder docs say file_read auto-activates `sandboxed(Modules::Safe)` when no explicit `sandboxed()` call is made, but the policy check should still apply). No test changes needed — the new tests confirm it works in sandbox. Just be aware during review.
