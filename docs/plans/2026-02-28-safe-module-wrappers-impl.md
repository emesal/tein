# safe module wrappers implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** implement `(tein file)`, `(tein load)`, `(tein process)` modules per issue #87

**Architecture:** scheme-heavy approach — scheme wrappers for higher-order file ops (composing over policy-checked primitives), rust trampolines only for `file-exists?`, `delete-file`, `load`, env access, and `exit`. exit uses exception + thread-local flag mechanism for immediate return to rust caller.

**Tech Stack:** rust (context.rs, sandbox.rs, ffi.rs, build.rs), scheme (.sld/.scm VFS files), C (tein_shim.c — only `tein_vfs_lookup` exposure)

**Design doc:** `docs/plans/2026-02-28-safe-module-wrappers-design.md`

## progress (branch: feature/safe-module-wrappers-2602)

- ✅ task 1 — SAFE_MODULES blanket replaced (commit fa6cbee)
- ✅ task 2 — `tein_vfs_lookup` + `sexp_voidp`/`sexp_truep` added to ffi.rs (commit 9aa29f6)
- ✅ task 3 — exit thread-locals + eval intercepts in evaluate/evaluate_port/call (commit 5203d14)
- ✅ task 4 — VFS files pushed to emesal/chibi-scheme emesal-tein (commits 943ab8f + chibi fork eaff5d3a/3419bda8)
- ✅ task 5 — build.rs VFS_FILES updated (commit 3948c82)
- ✅ task 6 — (tein file) trampolines implemented (commit 127a96c)
- 🔴 task 7 — (tein load) trampoline BLOCKED: `(import (tein load))` fails, see blocker notes below
- ✅ task 8 — (tein process) trampolines implemented (commit 127a96c)
- ✅ task 9 — scheme integration tests added (commit 8893cf4)
- ✅ task 10 — AGENTS.md + sandbox.rs doc updated (commit ff0b91d)
- ⏳ task 11 — resolve (tein load) import blocker, then final lint + full test run
- ⏳ task 12 — final lint + full test run (blocked by task 11)

## notes for next session

**BLOCKER: (tein load) import fails**

`(import (tein load))` returns `EvalError("")` (chibi's silent error). root cause not yet identified.

Findings:
- `tein-load-vfs-internal` IS accessible in the global env (confirmed via test: `Ok(Procedure(...))`)
- `load.scm` in VFS contains `(define load tein-load-vfs-internal)` and the VFS data is correct
- `load.sld` uses `(include "load.scm")` — same pattern as `json.sld` and `test.sld`
- json import works fine; load doesn't — something specific to `load.scm`
- When DEBUG tested with manual `(define load tein-load-vfs-internal)` before import, got `"undefined variable: (include)"` and `WARNING: exception inside undefined operator: define-library`

Hypothesis: the `(define load tein-load-vfs-internal)` inside `load.scm` is being evaluated **without** the library environment context, possibly because chibi's `include` mechanism for library bodies doesn't give access to `tein-load-vfs-internal` which is in the top-level env (not the library's import chain). The library only imports `(scheme base)` so `tein-load-vfs-internal` may be invisible.

**Option A**: Move `tein-load-vfs-internal` registration into a chibi static library (like `reader.c` / `macro.c`) that gets loaded at library import time. Complex.

**Option B**: Export `tein-load-vfs-internal` from `load.sld` directly (same pattern as json/toml exports) and have scheme code alias it: but scheme callers would need to do `(define load tein-load-vfs-internal)` themselves.

**Option C**: Register `tein-load-vfs-internal` at the scheme level in `load.scm` via eval — but that requires interaction-environment access.

**Option D (simplest to try)**: Remove `load.scm` content. Instead, have `load.sld` export `tein-load-vfs-internal` as `load` directly using chibi's `rename` in export:
```scheme
(define-library (tein load)
  (import (scheme base))
  (export (rename tein-load-vfs-internal load)))
```
This would work if chibi can find `tein-load-vfs-internal` in the global env during export resolution.

**Option E**: Don't export `load` at all from `(tein load)`. Instead, let users do `(define load tein-load-vfs-internal)` manually. Ugly.

**Current state of tests**: all tests pass EXCEPT the 3 `test_tein_load_*` tests. `just test` fails. Need to resolve before PR.

**chibi fork location**: `~/forks/chibi-scheme` (NOT `target/chibi-scheme` which gets cargo-reset). changes must be committed + pushed from `~/forks/chibi-scheme`.

**IS_SANDBOXED thread-local**: added to context.rs to distinguish unsandboxed (allow all) from sandboxed (deny without FsPolicy). set when presets are applied in build(). prev_is_sandboxed stored in Context struct, restored on drop.

**file module simplified**: `(tein file)` exports only `file-exists?` and `delete-file`. open-* wrappers dropped from exports — available from standard env already. `file.scm` is just doc comments.

**load module trampoline name**: registered as `tein-load-vfs-internal` (not `load`) to avoid breaking chibi's module loader. the intention is for `load.scm` to alias it as `load` in the library body — but this is currently failing.

---

### Task 1: replace `"tein/"` blanket with explicit SAFE_MODULES entries

**Files:**
- Modify: `tein/src/sandbox.rs:222-224` (SAFE_MODULES const)

**Step 1: write a test that `(tein process)` is blocked by default sandbox**

add to `tein/src/context.rs` tests section (near other sandbox tests):

```rust
#[test]
fn test_tein_process_blocked_by_default_sandbox() {
    use crate::sandbox::*;
    let ctx = Context::builder()
        .standard_env()
        .preset(&SAFE)
        .allow(&["import", "define", "if", "lambda", "begin", "quote"])
        .step_limit(5_000_000)
        .build()
        .expect("sandboxed context");
    let r = ctx.evaluate("(import (tein process))");
    assert!(r.is_err() || matches!(r, Ok(Value::String(ref s)) if s.contains("couldn't find")),
        "expected (tein process) to be blocked in default sandbox, got: {:?}", r);
}

#[test]
fn test_tein_process_allowed_with_allow_module() {
    use crate::sandbox::*;
    let ctx = Context::builder()
        .standard_env()
        .preset(&SAFE)
        .allow(&["import", "define", "if", "lambda", "begin", "quote"])
        .allow_module("tein/process")
        .step_limit(5_000_000)
        .build()
        .expect("sandboxed context");
    // module import should succeed once trampolines are registered
    let r = ctx.evaluate("(import (tein process))");
    assert!(r.is_ok(), "expected (tein process) import to succeed: {:?}", r);
}
```

**Step 2: run tests to verify the first test FAILS (blanket `"tein/"` still allows it)**

Run: `cargo test test_tein_process_blocked -- --nocapture`
Expected: FAIL (currently `"tein/"` allows all tein modules)

**Step 3: replace the blanket in SAFE_MODULES**

in `tein/src/sandbox.rs:222-224`, replace:
```rust
pub const SAFE_MODULES: &[&str] = &[
    "tein/",
```
with:
```rust
pub const SAFE_MODULES: &[&str] = &[
    // tein modules (explicit — tein/process excluded, leaks host argv)
    "tein/foreign",
    "tein/reader",
    "tein/macro",
    "tein/test",
    "tein/docs",
    "tein/json",
    "tein/toml",
    "tein/file",
    "tein/load",
```

update the doc comment above SAFE_MODULES to note the change and rationale.

**Step 4: run tests**

Run: `just test`
Expected: all existing tests pass; `test_tein_process_blocked_by_default_sandbox` now passes; `test_tein_process_allowed_with_allow_module` fails (VFS files don't exist yet — expected)

**Step 5: commit**

```
feat: replace tein/ blanket with explicit SAFE_MODULES entries (#87)

tein/process intentionally excluded — command-line leaks host argv.
available via .vfs_all() or .allow_module("tein/process").
```

---

### Task 2: expose `tein_vfs_lookup` in ffi.rs

**Files:**
- Modify: `tein/src/ffi.rs:230-232` (extern block, near existing `tein_vfs_register`)

**Step 1: add extern declaration and safe wrapper**

in the extern block (after `tein_vfs_register`, before `}`):
```rust
    pub fn tein_vfs_lookup(
        full_path: *const c_char,
        out_length: *mut c_uint,
    ) -> *const c_char;
```

add safe wrapper below the existing VFS functions:
```rust
/// look up a VFS path, returning the embedded content and length.
///
/// returns `None` if the path is not in the VFS. the returned slice
/// borrows from static (compiled-in) or thread-local (dynamic) storage
/// and is valid for the lifetime of the context.
#[inline]
pub unsafe fn vfs_lookup(path: &CStr) -> Option<&[u8]> {
    unsafe {
        let mut len: c_uint = 0;
        let ptr = tein_vfs_lookup(path.as_ptr(), &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(std::slice::from_raw_parts(ptr as *const u8, len as usize))
        }
    }
}
```

**Step 2: run `just lint`**

Expected: passes

**Step 3: commit**

```
feat: expose tein_vfs_lookup in ffi.rs (#87)

needed by (tein load) trampoline to resolve VFS content for load.
```

---

### Task 3: add exit mechanism thread-locals and intercept

**Files:**
- Modify: `tein/src/context.rs` (thread-locals, check_exit helper, evaluate, evaluate_port, call)

**Step 1: add thread-locals and helper**

near the existing thread-locals (around line 84, after `ORIGINAL_PROCS`):

```rust
// exit escape hatch — (exit) / (exit obj) in scheme sets these, and
// the eval loop intercepts the resulting exception to return Ok(value).
thread_local! {
    static EXIT_REQUESTED: Cell<bool> = const { Cell::new(false) };
    static EXIT_VALUE: Cell<ffi::sexp> = const { Cell::new(std::ptr::null_mut()) };
}
```

add a helper method on `Context`:

```rust
/// Check if `(exit)` was called during evaluation.
///
/// If the exit flag is set, clears it, releases the GC root on the
/// stashed value, converts it to a `Value`, and returns `Some(Ok(value))`.
/// Returns `None` if no exit was requested.
fn check_exit(&self) -> Option<Result<Value>> {
    if EXIT_REQUESTED.with(|c| c.replace(false)) {
        let raw = EXIT_VALUE.with(|c| c.replace(std::ptr::null_mut()));
        if !raw.is_null() {
            unsafe { ffi::sexp_release_object(self.ctx, raw) };
        }
        // null means (exit) with no args — return 0
        if raw.is_null() || unsafe { ffi::sexp_voidp(raw) != 0 } {
            return Some(Ok(Value::Integer(0)));
        }
        Some(Value::from_raw(self.ctx, raw))
    } else {
        None
    }
}
```

**Step 2: intercept in `evaluate()`**

in `evaluate()` (around line 1839), BEFORE the exception check, add exit intercept:

change:
```rust
                // evaluation error
                if ffi::sexp_exceptionp(result) != 0 {
                    return Value::from_raw(self.ctx, result);
                }
```
to:
```rust
                // exit escape hatch — (exit) returns an exception to stop
                // the VM immediately, but we intercept it here to return
                // Ok(value) to the rust caller.
                if ffi::sexp_exceptionp(result) != 0 {
                    if let Some(exit_result) = self.check_exit() {
                        return exit_result;
                    }
                    return Value::from_raw(self.ctx, result);
                }
```

**Step 3: intercept in `evaluate_port()`**

same pattern in `evaluate_port()` (around line 2528):

change:
```rust
                if ffi::sexp_exceptionp(result) != 0 {
                    return Value::from_raw(self.ctx, result);
                }
```
to:
```rust
                if ffi::sexp_exceptionp(result) != 0 {
                    if let Some(exit_result) = self.check_exit() {
                        return exit_result;
                    }
                    return Value::from_raw(self.ctx, result);
                }
```

**Step 4: intercept in `call()`**

same pattern in `call()` (around line 2661):

change:
```rust
            if ffi::sexp_exceptionp(result) != 0 {
                return Value::from_raw(self.ctx, result);
            }
```
to:
```rust
            if ffi::sexp_exceptionp(result) != 0 {
                if let Some(exit_result) = self.check_exit() {
                    return exit_result;
                }
                return Value::from_raw(self.ctx, result);
            }
```

**Step 5: add cleanup in `Drop` impl**

in the `Drop` impl for `Context` (around line 2708), add cleanup before `sexp_destroy_context`:

```rust
        // clear any pending exit request (defensive — should be consumed by eval loop)
        EXIT_REQUESTED.with(|c| c.set(false));
        let stashed = EXIT_VALUE.with(|c| c.replace(std::ptr::null_mut()));
        if !stashed.is_null() {
            unsafe { ffi::sexp_release_object(self.ctx, stashed) };
        }
```

**Step 6: check `sexp_voidp` exists in ffi.rs**

verify `sexp_voidp` is available. if not, we can check `raw == ffi::get_void()` instead. adjust `check_exit` accordingly.

**Step 7: run `just lint` and `just test`**

Expected: all pass (no trampolines registered yet — exit mechanism is inert)

**Step 8: commit**

```
feat: add exit escape hatch thread-locals and eval intercept (#87)

EXIT_REQUESTED + EXIT_VALUE thread-locals with intercept in evaluate(),
evaluate_port(), and call(). when (exit) sets the flag and returns an
exception, the eval loop returns Ok(value) instead of propagating the error.
```

---

### Task 4: VFS files for all three modules (chibi fork)

**Files:**
- Create: `target/chibi-scheme/lib/tein/file.sld`
- Create: `target/chibi-scheme/lib/tein/file.scm`
- Create: `target/chibi-scheme/lib/tein/load.sld`
- Create: `target/chibi-scheme/lib/tein/load.scm`
- Create: `target/chibi-scheme/lib/tein/process.sld`
- Create: `target/chibi-scheme/lib/tein/process.scm`

**NOTE:** these files live in the chibi fork (emesal/chibi-scheme, branch emesal-tein). they're fetched by build.rs into `target/chibi-scheme/`. for development, create them locally in `target/chibi-scheme/lib/tein/` and then push to the fork before the final PR. for now, create locally so we can iterate.

**Step 1: create `lib/tein/file.sld`**

```scheme
(define-library (tein file)
  (import (scheme base))
  (export open-input-file open-output-file
          open-binary-input-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file
          file-exists? delete-file)
  (include "file.scm"))
```

**Step 2: create `lib/tein/file.scm`**

```scheme
;;; (tein file) — safe file operations through FsPolicy
;;;
;;; open-input-file, open-output-file, open-binary-input-file,
;;; open-binary-output-file are re-exported from the environment (already
;;; policy-wrapped by the rust runtime when sandboxed).
;;;
;;; file-exists? and delete-file are rust trampolines registered via
;;; define_fn_variadic that check FsPolicy independently.
;;;
;;; the higher-order wrappers below compose over the open-* procs,
;;; inheriting their policy safety.

(define (call-with-input-file path proc)
  (let ((port (open-input-file path)))
    (dynamic-wind
      (lambda () #f)
      (lambda () (proc port))
      (lambda () (close-input-port port)))))

(define (call-with-output-file path proc)
  (let ((port (open-output-file path)))
    (dynamic-wind
      (lambda () #f)
      (lambda () (proc port))
      (lambda () (close-output-port port)))))

(define (with-input-from-file path thunk)
  (let ((port (open-input-file path)))
    (dynamic-wind
      (lambda () #f)
      (lambda ()
        (parameterize ((current-input-port port))
          (thunk)))
      (lambda () (close-input-port port)))))

(define (with-output-to-file path thunk)
  (let ((port (open-output-file path)))
    (dynamic-wind
      (lambda () #f)
      (lambda ()
        (parameterize ((current-output-port port))
          (thunk)))
      (lambda () (close-output-port port)))))
```

**Step 3: create `lib/tein/load.sld`**

```scheme
(define-library (tein load)
  (import (scheme base))
  (export load)
  (include "load.scm"))
```

**Step 4: create `lib/tein/load.scm`**

```scheme
;;; (tein load) — VFS-restricted load
;;;
;;; load is a rust trampoline registered by the runtime. it accepts only
;;; VFS paths (/vfs/...) and evaluates the embedded content. non-VFS paths
;;; return a sandbox violation error.
```

**Step 5: create `lib/tein/process.sld`**

```scheme
(define-library (tein process)
  (import (scheme base))
  (export get-environment-variable get-environment-variables
          command-line exit)
  (include "process.scm"))
```

**Step 6: create `lib/tein/process.scm`**

```scheme
;;; (tein process) — process context access
;;;
;;; get-environment-variable, get-environment-variables, command-line,
;;; and exit are rust trampolines registered by the runtime.
;;;
;;; NOT in SAFE_MODULES — command-line leaks host argv. available via
;;; .vfs_all() or .allow_module("tein/process").
;;;
;;; exit is an eval escape hatch: (exit) or (exit obj) immediately returns
;;; to the rust caller with the given value. does not invoke dynamic-wind
;;; cleanup (emergency-exit semantics).
```

**Step 7: commit**

```
feat: add VFS module files for (tein file), (tein load), (tein process) (#87)

scheme wrappers for higher-order file ops in file.scm.
load.scm and process.scm are doc stubs — trampolines registered by rust.
```

---

### Task 5: add VFS files to build.rs

**Files:**
- Modify: `tein/build.rs:86-88` (end of VFS_FILES const)

**Step 1: add the 6 new files to VFS_FILES**

after the `tein/docs` entries (line 87), before the closing `];`:

```rust
    // tein file operations (safe wrappers via FsPolicy)
    "lib/tein/file.sld",
    "lib/tein/file.scm",
    // tein VFS-restricted load
    "lib/tein/load.sld",
    "lib/tein/load.scm",
    // tein process context access (NOT in SAFE_MODULES)
    "lib/tein/process.sld",
    "lib/tein/process.scm",
```

**Step 2: rebuild to verify VFS generation**

Run: `cargo build`
Expected: compiles successfully, new VFS entries generated

**Step 3: commit**

```
feat: add (tein file/load/process) to VFS build (#87)
```

---

### Task 6: implement `(tein file)` trampolines + registration

**Files:**
- Modify: `tein/src/context.rs` (trampolines, registration, build() call)

**Step 1: write rust tests for file-exists? and delete-file**

add to context.rs test section:

```rust
#[test]
fn test_tein_file_exists() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein file))").unwrap();
    // test with a file that exists (Cargo.toml in workspace root)
    let r = ctx.evaluate("(file-exists? \"Cargo.toml\")").unwrap();
    assert_eq!(r, Value::Boolean(true));
    // test with a file that doesn't exist
    let r = ctx.evaluate("(file-exists? \"/nonexistent/path/xyz\")").unwrap();
    assert_eq!(r, Value::Boolean(false));
}

#[test]
fn test_tein_file_delete() {
    use std::io::Write;
    let dir = std::env::temp_dir();
    let path = dir.join("tein_test_delete_file.txt");
    // create a temp file
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(b"test").unwrap();
    drop(f);

    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein file))").unwrap();
    let code = format!("(delete-file \"{}\")", path.display());
    let r = ctx.evaluate(&code);
    assert!(r.is_ok(), "delete-file failed: {:?}", r);
    assert!(!path.exists(), "file should be deleted");
}

#[test]
fn test_tein_file_exists_sandboxed() {
    use crate::sandbox::*;
    let ctx = Context::builder()
        .standard_env()
        .preset(&SAFE)
        .allow(&["import", "define", "if", "lambda", "begin", "quote"])
        .step_limit(5_000_000)
        .build()
        .unwrap();
    ctx.evaluate("(import (tein file))").unwrap();
    // no FsPolicy configured, but file-exists? should return a policy error
    let r = ctx.evaluate("(file-exists? \"/etc/passwd\")");
    // should be an error (sandbox violation)
    assert!(r.is_err(), "expected sandbox violation: {:?}", r);
}

#[test]
fn test_tein_file_exists_with_read_policy() {
    use crate::sandbox::*;
    let ctx = Context::builder()
        .standard_env()
        .preset(&SAFE)
        .allow(&["import", "define", "if", "lambda", "begin", "quote"])
        .file_read(&["/"])
        .step_limit(5_000_000)
        .build()
        .unwrap();
    ctx.evaluate("(import (tein file))").unwrap();
    let r = ctx.evaluate("(file-exists? \"Cargo.toml\")").unwrap();
    assert_eq!(r, Value::Boolean(true));
}
```

**Step 2: run tests to verify they fail**

Run: `cargo test test_tein_file -- --nocapture`
Expected: FAIL (trampolines not registered yet)

**Step 3: implement trampolines**

add to context.rs (near existing IoOp wrappers, around line 1078):

```rust
/// file-exists? trampoline: checks FsPolicy read access, returns boolean.
///
/// when no FsPolicy is set (unsandboxed context), allows unconditionally.
/// in sandboxed contexts without file_read configured, returns a policy
/// violation exception.
unsafe extern "C" fn file_exists_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let first_arg = ffi::sexp_car(args);
        if ffi::sexp_stringp(first_arg) == 0 {
            let msg = "file-exists?: expected string argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let c_str = ffi::sexp_string_data(first_arg);
        let len = ffi::sexp_string_size(first_arg) as usize;
        let path =
            std::str::from_utf8(std::slice::from_raw_parts(c_str as *const u8, len)).unwrap_or("");

        // policy check: None = unsandboxed (allow), Some = check read prefixes
        let allowed = FS_POLICY.with(|cell| {
            let policy = cell.borrow();
            match &*policy {
                Some(p) => p.check_read(path),
                None => true, // unsandboxed — allow
            }
        });

        if !allowed {
            let msg = format!("[sandbox:file] {} (read not permitted)", path);
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        if std::path::Path::new(path).exists() {
            ffi::get_true()
        } else {
            ffi::get_false()
        }
    }
}

/// delete-file trampoline: checks FsPolicy write access, deletes file.
///
/// when no FsPolicy is set (unsandboxed context), allows unconditionally.
/// returns void on success, exception on policy violation or IO error.
unsafe extern "C" fn delete_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let first_arg = ffi::sexp_car(args);
        if ffi::sexp_stringp(first_arg) == 0 {
            let msg = "delete-file: expected string argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let c_str = ffi::sexp_string_data(first_arg);
        let len = ffi::sexp_string_size(first_arg) as usize;
        let path =
            std::str::from_utf8(std::slice::from_raw_parts(c_str as *const u8, len)).unwrap_or("");

        // policy check: None = unsandboxed (allow), Some = check write prefixes
        let allowed = FS_POLICY.with(|cell| {
            let policy = cell.borrow();
            match &*policy {
                Some(p) => p.check_write(path),
                None => true,
            }
        });

        if !allowed {
            let msg = format!("[sandbox:file] {} (write not permitted)", path);
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        match std::fs::remove_file(path) {
            Ok(()) => ffi::get_void(),
            Err(e) => {
                let msg = format!("delete-file: {}", e);
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}
```

**Step 4: add registration function**

near `register_json_module` (around line 2580):

```rust
/// Register `file-exists?` and `delete-file` native functions.
///
/// Called during `build()` for standard-env contexts. the VFS module
/// `(tein file)` exports these plus the 4 `open-*-file` re-exports and
/// 4 scheme-level higher-order wrappers.
fn register_file_module(&self) -> Result<()> {
    self.define_fn_variadic("file-exists?", file_exists_trampoline)?;
    self.define_fn_variadic("delete-file", delete_file_trampoline)?;
    Ok(())
}
```

**Step 5: call registration in `build()`**

in `build()` around line 1591, after the toml registration:

```rust
            if self.standard_env {
                context.register_file_module()?;
            }
```

**Step 6: run tests**

Run: `cargo test test_tein_file -- --nocapture`
Expected: all pass

Run: `just test`
Expected: all pass

**Step 7: commit**

```
feat: implement (tein file) trampolines and registration (#87)

file-exists? checks FsPolicy read prefixes (unsandboxed: allow all).
delete-file checks FsPolicy write prefixes. higher-order wrappers
(call-with-*-file, with-*-from/to-file) are pure scheme in the VFS.
```

---

### Task 7: implement `(tein load)` trampoline + registration

**Files:**
- Modify: `tein/src/context.rs` (trampoline, registration, build() call)

**Step 1: write tests**

```rust
#[test]
fn test_tein_load_vfs_path() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein load))").unwrap();
    // load a VFS file that defines something
    // tein/test.scm defines the test framework — loading it shouldn't error
    let r = ctx.evaluate("(load \"/vfs/lib/tein/test.scm\")");
    assert!(r.is_ok(), "VFS load failed: {:?}", r);
}

#[test]
fn test_tein_load_non_vfs_rejected() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein load))").unwrap();
    let r = ctx.evaluate("(load \"/etc/passwd\")");
    assert!(r.is_err(), "expected non-VFS path to be rejected: {:?}", r);
}

#[test]
fn test_tein_load_nonexistent_vfs() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein load))").unwrap();
    let r = ctx.evaluate("(load \"/vfs/lib/nonexistent.scm\")");
    assert!(r.is_err(), "expected missing VFS path to error: {:?}", r);
}
```

**Step 2: run tests to verify they fail**

Run: `cargo test test_tein_load -- --nocapture`
Expected: FAIL

**Step 3: implement trampoline**

```rust
/// load trampoline: VFS-only file loading.
///
/// resolves the path through `tein_vfs_lookup`, opens a string input port
/// from the embedded content, and loops read+eval. rejects non-VFS paths
/// with a sandbox violation error.
unsafe extern "C" fn load_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let first_arg = ffi::sexp_car(args);
        if ffi::sexp_stringp(first_arg) == 0 {
            let msg = "load: expected string argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let c_str = ffi::sexp_string_data(first_arg);
        let len = ffi::sexp_string_size(first_arg) as usize;
        let path =
            std::str::from_utf8(std::slice::from_raw_parts(c_str as *const u8, len)).unwrap_or("");

        // VFS-only gate
        if !path.starts_with("/vfs/") {
            let msg = format!("[sandbox:load] {} (only VFS paths permitted)", path);
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        // look up VFS content
        let c_path = match CString::new(path) {
            Ok(s) => s,
            Err(_) => {
                let msg = "load: path contains null bytes";
                let c_msg = CString::new(msg).unwrap_or_default();
                return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };
        let content = match ffi::vfs_lookup(&c_path) {
            Some(bytes) => bytes,
            None => {
                let msg = format!("load: VFS path not found: {}", path);
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };

        // open input string port from VFS content
        let scheme_str = ffi::sexp_c_str(
            ctx,
            content.as_ptr() as *const c_char,
            content.len() as ffi::sexp_sint_t,
        );
        if ffi::sexp_exceptionp(scheme_str) != 0 {
            return scheme_str;
        }
        let _str_root = ffi::GcRoot::new(ctx, scheme_str);

        let port = ffi::sexp_open_input_string(ctx, scheme_str);
        if ffi::sexp_exceptionp(port) != 0 {
            return port;
        }
        let _port_root = ffi::GcRoot::new(ctx, port);

        let env = ffi::sexp_context_env(ctx);
        let mut result = ffi::get_void();

        loop {
            let expr = ffi::sexp_read(ctx, port);
            if ffi::sexp_eofp(expr) != 0 {
                break;
            }
            if ffi::sexp_exceptionp(expr) != 0 {
                return expr;
            }
            let _expr_root = ffi::GcRoot::new(ctx, expr);
            result = ffi::sexp_evaluate(ctx, expr, env);
            if ffi::sexp_exceptionp(result) != 0 {
                return result;
            }
        }

        result
    }
}
```

**Step 4: add registration**

```rust
/// Register the `load` native function (VFS-only).
///
/// Called during `build()` for standard-env contexts.
fn register_load_module(&self) -> Result<()> {
    self.define_fn_variadic("load", load_trampoline)?;
    Ok(())
}
```

**Step 5: call in `build()`**

```rust
            if self.standard_env {
                context.register_load_module()?;
            }
```

**Step 6: run tests**

Run: `cargo test test_tein_load -- --nocapture`
Expected: pass

Run: `just test`
Expected: all pass

**Step 7: commit**

```
feat: implement (tein load) VFS-only load trampoline (#87)

resolves path through tein_vfs_lookup, reads embedded content via string
port, loops read+eval. rejects all non-VFS paths.
```

---

### Task 8: implement `(tein process)` trampolines + registration

**Files:**
- Modify: `tein/src/context.rs` (4 trampolines, registration, build() call)

**Step 1: write tests**

```rust
#[test]
fn test_tein_process_get_env_var() {
    std::env::set_var("TEIN_TEST_VAR", "hello");
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein process))").unwrap();
    let r = ctx.evaluate("(get-environment-variable \"TEIN_TEST_VAR\")").unwrap();
    assert_eq!(r, Value::String("hello".to_string()));
    // unset var returns #f
    let r = ctx.evaluate("(get-environment-variable \"TEIN_NONEXISTENT_VAR_XYZ\")").unwrap();
    assert_eq!(r, Value::Boolean(false));
    std::env::remove_var("TEIN_TEST_VAR");
}

#[test]
fn test_tein_process_get_env_vars() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein process))").unwrap();
    let r = ctx.evaluate("(pair? (get-environment-variables))").unwrap();
    assert_eq!(r, Value::Boolean(true), "should return non-empty alist");
}

#[test]
fn test_tein_process_command_line() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein process))").unwrap();
    let r = ctx.evaluate("(list? (command-line))").unwrap();
    assert_eq!(r, Value::Boolean(true), "should return a list");
}

#[test]
fn test_tein_process_exit_no_args() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein process))").unwrap();
    let r = ctx.evaluate("(begin (exit) (+ 1 2))").unwrap();
    assert_eq!(r, Value::Integer(0), "(exit) should return 0");
}

#[test]
fn test_tein_process_exit_with_value() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein process))").unwrap();
    let r = ctx.evaluate("(begin (exit 42) (+ 1 2))").unwrap();
    assert_eq!(r, Value::Integer(42), "(exit 42) should return 42");
}

#[test]
fn test_tein_process_exit_true() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein process))").unwrap();
    let r = ctx.evaluate("(begin (exit #t) 999)").unwrap();
    assert_eq!(r, Value::Integer(0), "(exit #t) should return 0");
}

#[test]
fn test_tein_process_exit_false() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein process))").unwrap();
    let r = ctx.evaluate("(begin (exit #f) 999)").unwrap();
    assert_eq!(r, Value::Integer(1), "(exit #f) should return 1");
}

#[test]
fn test_tein_process_exit_string() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein process))").unwrap();
    let r = ctx.evaluate("(begin (exit \"done\") 999)").unwrap();
    assert_eq!(r, Value::String("done".to_string()));
}
```

**Step 2: run tests to verify they fail**

Run: `cargo test test_tein_process -- --nocapture`
Expected: FAIL

**Step 3: implement trampolines**

```rust
/// get-environment-variable trampoline: returns env var value or #f.
unsafe extern "C" fn get_env_var_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let first_arg = ffi::sexp_car(args);
        if ffi::sexp_stringp(first_arg) == 0 {
            let msg = "get-environment-variable: expected string argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let c_str = ffi::sexp_string_data(first_arg);
        let len = ffi::sexp_string_size(first_arg) as usize;
        let name =
            std::str::from_utf8(std::slice::from_raw_parts(c_str as *const u8, len)).unwrap_or("");

        match std::env::var(name) {
            Ok(val) => {
                let c_val = CString::new(val.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_val.as_ptr(), val.len() as ffi::sexp_sint_t)
            }
            Err(_) => ffi::get_false(),
        }
    }
}

/// get-environment-variables trampoline: returns alist of all env vars.
unsafe extern "C" fn get_env_vars_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let mut result = ffi::get_null();
        let _result_root = ffi::GcRoot::new(ctx, result);
        for (key, val) in std::env::vars() {
            let c_key = CString::new(key.as_str()).unwrap_or_default();
            let c_val = CString::new(val.as_str()).unwrap_or_default();
            let s_key = ffi::sexp_c_str(ctx, c_key.as_ptr(), key.len() as ffi::sexp_sint_t);
            if ffi::sexp_exceptionp(s_key) != 0 { return s_key; }
            let _key_root = ffi::GcRoot::new(ctx, s_key);
            let s_val = ffi::sexp_c_str(ctx, c_val.as_ptr(), val.len() as ffi::sexp_sint_t);
            if ffi::sexp_exceptionp(s_val) != 0 { return s_val; }
            let _val_root = ffi::GcRoot::new(ctx, s_val);
            let pair = ffi::sexp_cons(ctx, s_key, s_val);
            if ffi::sexp_exceptionp(pair) != 0 { return pair; }
            let _pair_root = ffi::GcRoot::new(ctx, pair);
            result = ffi::sexp_cons(ctx, pair, result);
            if ffi::sexp_exceptionp(result) != 0 { return result; }
        }
        result
    }
}

/// command-line trampoline: returns list of command-line args.
unsafe extern "C" fn command_line_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let mut result = ffi::get_null();
        // build list in reverse, then reverse (or build backwards)
        let args: Vec<String> = std::env::args().collect();
        for arg in args.iter().rev() {
            let c_arg = CString::new(arg.as_str()).unwrap_or_default();
            let s_arg = ffi::sexp_c_str(ctx, c_arg.as_ptr(), arg.len() as ffi::sexp_sint_t);
            if ffi::sexp_exceptionp(s_arg) != 0 { return s_arg; }
            let _arg_root = ffi::GcRoot::new(ctx, s_arg);
            let _tail_root = ffi::GcRoot::new(ctx, result);
            result = ffi::sexp_cons(ctx, s_arg, result);
            if ffi::sexp_exceptionp(result) != 0 { return result; }
        }
        result
    }
}

/// exit trampoline: eval escape hatch.
///
/// sets EXIT_REQUESTED + EXIT_VALUE thread-locals and returns a scheme
/// exception to immediately stop the VM. the eval loop intercepts this
/// via check_exit() and returns Ok(value) to the rust caller.
///
/// semantics: (exit) → 0, (exit #t) → 0, (exit #f) → 1, (exit obj) → obj
unsafe extern "C" fn exit_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        // determine exit value based on arg count and value
        let exit_val = if n == 0 || ffi::sexp_nullp(args) != 0 {
            // (exit) — no args, return fixnum 0
            ffi::sexp_fixnum(0)
        } else {
            let arg = ffi::sexp_car(args);
            if ffi::sexp_booleanp(arg) != 0 {
                // (exit #t) → 0, (exit #f) → 1
                if ffi::sexp_truep(arg) != 0 {
                    ffi::sexp_fixnum(0)
                } else {
                    ffi::sexp_fixnum(1)
                }
            } else {
                arg
            }
        };

        // GC-root the exit value (fixnums are immediates and don't need
        // rooting, but heap objects like strings do — preserve unconditionally
        // for safety; sexp_preserve_object is a no-op for immediates in
        // chibi's implementation)
        ffi::sexp_preserve_object(ctx, exit_val);
        EXIT_REQUESTED.with(|c| c.set(true));
        EXIT_VALUE.with(|c| c.set(exit_val));

        // return exception to stop VM immediately
        let msg = "exit";
        let c_msg = CString::new(msg).unwrap_or_default();
        ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
    }
}
```

**Step 4: add registration**

```rust
/// Register `get-environment-variable`, `get-environment-variables`,
/// `command-line`, and `exit` native functions.
///
/// Called during `build()` for standard-env contexts.
fn register_process_module(&self) -> Result<()> {
    self.define_fn_variadic("get-environment-variable", get_env_var_trampoline)?;
    self.define_fn_variadic("get-environment-variables", get_env_vars_trampoline)?;
    self.define_fn_variadic("command-line", command_line_trampoline)?;
    self.define_fn_variadic("exit", exit_trampoline)?;
    Ok(())
}
```

**Step 5: call in `build()`**

```rust
            if self.standard_env {
                context.register_process_module()?;
            }
```

**Step 6: verify check_exit + FFI helpers**

verify `sexp_fixnum`, `sexp_booleanp`, `sexp_truep`, `sexp_nullp` exist in ffi.rs. if any are missing, add wrappers. also verify `sexp_preserve_object` and `sexp_release_object` are available as standalone functions (not just through GcRoot).

**Step 7: run tests**

Run: `cargo test test_tein_process -- --nocapture`
Expected: all pass

Run: `just test`
Expected: all pass

**Step 8: commit**

```
feat: implement (tein process) trampolines with exit escape hatch (#87)

get-environment-variable, get-environment-variables, command-line via
std::env. exit sets thread-local flag + returns exception; eval loop
intercepts and returns Ok(value) to rust caller.
```

---

### Task 9: scheme-level integration tests

**Files:**
- Create: `tein/tests/scheme/tein_file.scm`
- Create: `tein/tests/scheme/tein_process.scm`
- Modify: `tein/tests/scheme_tests.rs` (add test entries)

**Step 1: create `tests/scheme/tein_file.scm`**

```scheme
;;; (tein file) integration tests
;;; NOTE: run in unsandboxed standard context — no FsPolicy restriction

(import (tein file))

;; file-exists? on known file
(test-true "file-exists?/cargo" (file-exists? "Cargo.toml"))
(test-false "file-exists?/nonexistent" (file-exists? "/nonexistent/path/xyz.txt"))

;; higher-order wrappers (need a real file — use Cargo.toml as read target)
(test-true "call-with-input-file"
  (string? (call-with-input-file "Cargo.toml"
             (lambda (port) (read-line port)))))
```

**Step 2: create `tests/scheme/tein_process.scm`**

```scheme
;;; (tein process) integration tests

(import (tein process))

;; command-line returns a list
(test-true "command-line/list" (list? (command-line)))

;; get-environment-variables returns an alist
(test-true "get-env-vars/pair" (pair? (get-environment-variables)))

;; get-environment-variable for missing var returns #f
(test-false "get-env-var/missing" (get-environment-variable "TEIN_NONEXISTENT_VAR_XYZ"))
```

**Step 3: add test entries to `scheme_tests.rs`**

```rust
#[test]
fn test_scheme_tein_file() {
    run_scheme_test(include_str!("scheme/tein_file.scm"));
}

#[test]
fn test_scheme_tein_process() {
    run_scheme_test(include_str!("scheme/tein_process.scm"));
}
```

**Step 4: run tests**

Run: `just test`
Expected: all pass

**Step 5: commit**

```
test: add scheme-level integration tests for (tein file/process) (#87)
```

---

### Task 10: update AGENTS.md and docs

**Files:**
- Modify: `AGENTS.md` (architecture section, SAFE_MODULES note)
- Modify: `docs/plans/2026-02-28-safe-module-wrappers-design.md` (mark complete)

**Step 1: update AGENTS.md architecture section**

add to the architecture file listing:
```
  json.rs      — ... (existing)
  toml.rs      — ... (existing)
```

add entries in the architecture for the new modules. add to "critical gotchas" section about exit mechanism.

**Step 2: update SAFE_MODULES documentation**

update any doc comments in sandbox.rs that reference the old `"tein/"` blanket.

**Step 3: run `just lint`**

Expected: passes

**Step 4: commit**

```
docs: update AGENTS.md and sandbox docs for #87 modules

documents (tein file), (tein load), (tein process) in architecture,
exit escape hatch mechanism, and SAFE_MODULES change rationale.
closes #87
```

---

### Task 11: push VFS files to chibi fork

**NOTE:** this task requires pushing to the emesal/chibi-scheme fork (branch emesal-tein). the 6 new `.sld`/`.scm` files created locally in task 4 need to be committed to the fork so that `build.rs` can fetch them.

**Step 1: push files to fork**

navigate to `target/chibi-scheme/`, add the 6 new files, commit, push to emesal-tein branch.

**Step 2: rebuild from clean to verify VFS fetch works**

Run: `just clean && cargo build`
Expected: build succeeds, VFS includes new modules

**Step 3: run full test suite**

Run: `just test`
Expected: all pass

**Step 4: commit any build.rs adjustments if needed**

---

### Task 12: final lint + full test run

**Step 1: run `just lint`**

Expected: clean

**Step 2: run `just test`**

Expected: all pass (existing 386+ tests + new tests)

**Step 3: verify test counts**

new tests expected:
- ~8 rust unit tests (file, load, process, exit, sandbox)
- ~2 scheme integration tests (tein_file.scm, tein_process.scm)
- ~6 assertions in scheme tests
