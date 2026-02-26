# sandbox-aware error messages — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** make sandbox policy violations structurally distinguishable from code bugs via `Error::SandboxViolation`

**Architecture:** add one new error variant; detect violations via sentinel prefixes in exception messages and module policy thread-local; register stub foreign fns for stripped bindings. all detection funnels through `extract_exception_message`.

**Tech Stack:** rust, chibi-scheme ffi, existing tein_shim.c

---

### Task 1: add `Error::SandboxViolation` variant

**Files:**
- Modify: `tein/src/error.rs:10-31` (Error enum)
- Modify: `tein/src/error.rs:33-44` (Display impl)

**Step 1: Write the failing test**

add to `tein/src/context.rs` test module (bottom of file):

```rust
#[test]
fn test_sandbox_violation_error_variant() {
    // SandboxViolation should be a distinct variant with its own Display
    let err = Error::SandboxViolation("test message".to_string());
    assert!(matches!(err, Error::SandboxViolation(_)));
    assert_eq!(format!("{}", err), "sandbox violation: test message");

    // should not match EvalError
    assert!(!matches!(err, Error::EvalError(_)));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p tein test_sandbox_violation_error_variant`
Expected: FAIL — `SandboxViolation` doesn't exist yet

**Step 3: Write minimal implementation**

in `tein/src/error.rs`, add to the `Error` enum after `Timeout`:

```rust
    /// evaluation was blocked by sandbox policy (not a code bug)
    ///
    /// indicates the scheme code attempted something explicitly restricted
    /// by the context's configuration: a blocked module import, denied
    /// file access, or use of a primitive not included in the active presets.
    SandboxViolation(String),
```

add to the Display impl match:

```rust
            Error::SandboxViolation(msg) => write!(f, "sandbox violation: {}", msg),
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p tein test_sandbox_violation_error_variant`
Expected: PASS

**Step 5: Commit**

```bash
git add tein/src/error.rs tein/src/context.rs
git commit -m "feat: add Error::SandboxViolation variant"
```

---

### Task 2: add `ALL_PRESETS` collection + `sexp_opcode_name` shim

**Files:**
- Modify: `tein/src/sandbox.rs` (add `ALL_PRESETS`)
- Modify: `tein/vendor/chibi-scheme/tein_shim.c` (add opcode name accessor)
- Modify: `tein/src/ffi.rs` (add extern decl + safe wrapper)

**Step 1: add `ALL_PRESETS` to sandbox.rs**

after the last preset definition (`FILE_WRITE_SUPPORT`), add:

```rust
/// all presets known to tein, for stub registration during sandbox build.
///
/// used internally to determine which primitives should get sandbox stubs
/// when they aren't included in a context's allowlist.
pub(crate) const ALL_PRESETS: &[&Preset] = &[
    &ARITHMETIC,
    &MATH,
    &LISTS,
    &VECTORS,
    &STRINGS,
    &CHARACTERS,
    &TYPE_PREDICATES,
    &MUTATION,
    &STRING_PORTS,
    &STDOUT_ONLY,
    &EXCEPTIONS,
    &BYTEVECTORS,
    &IO_READ,
    &CONTROL,
    &FILE_READ_SUPPORT,
    &FILE_WRITE_SUPPORT,
];
```

**Step 2: add `tein_sexp_opcode_name` to tein_shim.c**

```c
/* extract the name (scheme string) from an opcode/foreign-fn object */
sexp tein_sexp_opcode_name(sexp op) {
    return sexp_opcode_name(op);
}
```

**Step 3: add ffi wrapper**

in `tein/src/ffi.rs`, add extern declaration inside the `unsafe extern "C"` block:

```rust
    pub fn tein_sexp_opcode_name(op: sexp) -> sexp;
```

add safe wrapper below the existing wrappers:

```rust
/// extract the name (scheme string) from an opcode/foreign-fn object
#[inline]
pub unsafe fn sexp_opcode_name(op: sexp) -> sexp {
    unsafe { tein_sexp_opcode_name(op) }
}
```

**Step 4: Run build to verify compilation**

Run: `cargo build -p tein`
Expected: compiles cleanly

**Step 5: Commit**

```bash
git add tein/src/sandbox.rs tein/vendor/chibi-scheme/tein_shim.c tein/src/ffi.rs
git commit -m "feat: add ALL_PRESETS collection + sexp_opcode_name shim"
```

---

### Task 3: change `extract_exception_message` to return `Error`

**Files:**
- Modify: `tein/src/value.rs:272-294` (extract_exception_message)

**Step 1: Write the failing test**

add to `tein/src/context.rs` test module:

```rust
#[test]
fn test_file_io_sandbox_violation_type() {
    use crate::sandbox::*;
    let ctx = Context::builder()
        .standard_env()
        .preset(&ARITHMETIC)
        .file_read(&["/allowed/"])
        .build()
        .expect("builder");

    let err = ctx.evaluate("(open-input-file \"/etc/passwd\")").unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_)),
        "expected SandboxViolation, got: {:?}",
        err
    );
    let msg = format!("{}", err);
    assert!(
        msg.contains("file access denied"),
        "expected 'file access denied', got: {}",
        msg
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p tein test_file_io_sandbox_violation_type`
Expected: FAIL — currently returns `EvalError`

**Step 3: Implement detection in extract_exception_message**

change `extract_exception_message` signature and body in `tein/src/value.rs`:

```rust
    /// extract a structured error from a chibi exception
    ///
    /// detects sandbox sentinel prefixes (`[sandbox:file]`, `[sandbox:binding]`)
    /// and module policy violations, returning `SandboxViolation` for those cases
    /// and `EvalError` for everything else.
    unsafe fn extract_exception_error(ctx: ffi::sexp, exn: ffi::sexp) -> Error {
        unsafe {
            let msg_sexp = ffi::sexp_exception_message(exn);
            let message = if ffi::sexp_stringp(msg_sexp) != 0 {
                let ptr = ffi::sexp_string_data(msg_sexp);
                let len = ffi::sexp_string_size(msg_sexp) as usize;
                let bytes = std::slice::from_raw_parts(ptr as *const u8, len);
                std::string::String::from_utf8_lossy(bytes).into_owned()
            } else {
                "unknown error".to_owned()
            };

            // extract irritants for appending to messages
            let irritant_str = {
                let irritants = ffi::sexp_exception_irritants(exn);
                if ffi::sexp_pairp(irritants) != 0 {
                    Value::from_raw(ctx, irritants).ok().map(|v| format!("{}", v))
                } else {
                    None
                }
            };

            // sentinel: file IO policy denial
            if let Some(path) = message.strip_prefix("[sandbox:file] ") {
                return Error::SandboxViolation(format!("file access denied: {}", path));
            }

            // sentinel: binding stub
            if let Some(rest) = message.strip_prefix("[sandbox:binding] ") {
                return Error::SandboxViolation(rest.to_string());
            }

            // module policy: detect "couldn't find file in module path" when VfsOnly
            if message == "couldn't find file in module path" {
                use crate::sandbox::MODULE_POLICY;
                use crate::sandbox::ModulePolicy;
                let is_vfs_only = MODULE_POLICY.with(|cell| cell.get() == ModulePolicy::VfsOnly);
                if is_vfs_only {
                    let module = irritant_str.as_deref().unwrap_or("unknown");
                    return Error::SandboxViolation(format!(
                        "module import blocked: {} (not available in this sandbox)",
                        module
                    ));
                }
            }

            // default: ordinary eval error with irritants appended
            if let Some(irr) = irritant_str {
                Error::EvalError(format!("{}: {}", message, irr))
            } else {
                Error::EvalError(message)
            }
        }
    }
```

update the call site in `from_raw_depth` (same file, around line 128-129):

from:
```rust
            if ffi::sexp_exceptionp(raw) != 0 {
                return Err(Error::EvalError(Self::extract_exception_message(ctx, raw)));
            }
```

to:
```rust
            if ffi::sexp_exceptionp(raw) != 0 {
                return Err(Self::extract_exception_error(ctx, raw));
            }
```

**Step 4: update file IO sentinel prefix**

in `tein/src/context.rs`, `check_and_delegate` (around line 102-105), change:

from:
```rust
            let msg = format!("access denied: {}", path);
```

to:
```rust
            let op_kind = if op.is_read() { "read" } else { "write" };
            let msg = format!("[sandbox:file] {} ({} not permitted)", path, op_kind);
```

**Step 5: Run test to verify it passes**

Run: `cargo test -p tein test_file_io_sandbox_violation_type`
Expected: PASS

**Step 6: Run full test suite**

Run: `cargo test -p tein`
Expected: existing `test_io_policy_blocks_read` may fail (it asserts `msg.contains("access denied")`) — update it to assert `SandboxViolation` instead. other tests should pass.

**Step 7: Fix any broken existing tests**

update `test_io_policy_blocks_read` (around line 2250-2258):

from:
```rust
        let err = ctx.evaluate("(open-input-file \"/etc/passwd\")");
        assert!(err.is_err(), "read from /etc/passwd should be denied");
        let msg = format!("{}", err.unwrap_err());
        assert!(
            msg.contains("access denied"),
            "expected 'access denied', got: {}",
            msg
        );
```

to:
```rust
        let err = ctx.evaluate("(open-input-file \"/etc/passwd\")").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "expected SandboxViolation, got: {:?}",
            err
        );
        let msg = format!("{}", err);
        assert!(
            msg.contains("file access denied"),
            "expected 'file access denied', got: {}",
            msg
        );
```

**Step 8: Run full test suite again**

Run: `cargo test -p tein`
Expected: all tests pass

**Step 9: Commit**

```bash
git add tein/src/value.rs tein/src/context.rs
git commit -m "feat: detect file IO sandbox violations as SandboxViolation"
```

---

### Task 4: module import sandbox detection

**Files:**
- Modify: `tein/src/context.rs` (test)

**Step 1: Write the failing test**

add to `tein/src/context.rs` test module:

```rust
#[test]
fn test_module_import_sandbox_violation_type() {
    use crate::sandbox::*;
    let ctx = Context::builder()
        .standard_env()
        .preset(&ARITHMETIC)
        .allow(&["import"])
        .build()
        .expect("standard + sandbox");

    // VFS import should still succeed
    let r = ctx.evaluate("(import (scheme write))");
    assert!(r.is_ok(), "(scheme write) should work: {:?}", r.err());

    // filesystem import should fail as SandboxViolation
    let err = ctx.evaluate("(import (chibi process))").unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_)),
        "expected SandboxViolation, got: {:?}",
        err
    );
    let msg = format!("{}", err);
    assert!(
        msg.contains("module import blocked"),
        "expected 'module import blocked', got: {}",
        msg
    );
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test -p tein test_module_import_sandbox_violation_type`
Expected: PASS — the detection logic from task 3 already handles this case.

if it fails, debug and fix. the module policy thread-local check in `extract_exception_error` should catch this.

**Step 3: Update existing test**

update `test_module_policy_blocks_filesystem_import` to also assert `SandboxViolation`:

from:
```rust
        let r = ctx.evaluate("(import (chibi process))");
        assert!(
            r.is_err(),
            "(import (chibi process)) should be blocked by VfsOnly policy"
        );
```

to:
```rust
        let err = ctx.evaluate("(import (chibi process))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "expected SandboxViolation for blocked import, got: {:?}",
            err
        );
```

**Step 4: Run full test suite**

Run: `cargo test -p tein`
Expected: all tests pass

**Step 5: Commit**

```bash
git add tein/src/context.rs
git commit -m "feat: detect module import sandbox violations as SandboxViolation"
```

---

### Task 5: sandbox stub binding registration

**Files:**
- Modify: `tein/src/context.rs` (sandbox_stub fn + registration in sandbox_build)

**Step 1: Write the failing test**

add to `tein/src/context.rs` test module:

```rust
#[test]
fn test_sandbox_stub_binding_violation() {
    // arithmetic-only context should have stubs for known non-allowed primitives
    use crate::sandbox::*;
    let ctx = Context::builder()
        .preset(&ARITHMETIC)
        .build()
        .expect("builder");

    // cons is in LISTS preset, not allowed — should produce SandboxViolation
    let err = ctx.evaluate("(cons 1 2)").unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_)),
        "expected SandboxViolation for stubbed binding, got: {:?}",
        err
    );
    let msg = format!("{}", err);
    assert!(
        msg.contains("not available in this sandbox"),
        "expected 'not available in this sandbox', got: {}",
        msg
    );
    assert!(
        msg.contains("cons"),
        "expected stub message to name 'cons', got: {}",
        msg
    );
}

#[test]
fn test_sandbox_stub_does_not_shadow_allowed() {
    // allowed primitives should work normally, not be replaced by stubs
    use crate::sandbox::*;
    let ctx = Context::builder()
        .preset(&ARITHMETIC)
        .preset(&LISTS)
        .build()
        .expect("builder");

    let result = ctx.evaluate("(cons 1 2)").expect("cons should work");
    assert_eq!(result, Value::Pair(Box::new(Value::Integer(1)), Box::new(Value::Integer(2))));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p tein test_sandbox_stub`
Expected: `test_sandbox_stub_binding_violation` FAILS (currently returns `EvalError("unbound variable: cons")`)
`test_sandbox_stub_does_not_shadow_allowed` should already PASS

**Step 3: Implement sandbox stub function**

add above `check_and_delegate` in `tein/src/context.rs` (around line 68):

```rust
/// sandbox stub for disallowed bindings
///
/// registered under the name of each known preset primitive that wasn't
/// included in the context's allowlist. when called, raises a scheme exception
/// with a `[sandbox:binding]` sentinel that `extract_exception_error` converts
/// to `Error::SandboxViolation`.
///
/// the stub extracts its own name from the opcode's name slot (set by
/// `sexp_define_foreign` at registration time), so one function serves
/// all stubbed bindings.
unsafe extern "C" fn sandbox_stub(
    ctx: ffi::sexp,
    self_: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let name_sexp = ffi::sexp_opcode_name(self_);
        let name = if ffi::sexp_stringp(name_sexp) != 0 {
            let ptr = ffi::sexp_string_data(name_sexp);
            let len = ffi::sexp_string_size(name_sexp) as usize;
            std::str::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len))
                .unwrap_or("unknown")
        } else {
            "unknown"
        };
        let msg = format!("[sandbox:binding] '{}' is not available in this sandbox", name);
        let c_msg = CString::new(msg.as_str()).unwrap_or_default();
        ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
    }
}
```

**Step 4: Register stubs in sandbox_build**

in `tein/src/context.rs`, inside the `if let Some(ref allowed) = self.allowed_primitives` block, after the IO wrapper registration (around line 493, before `Ok(Context { ... })`), add:

```rust
                // register sandbox stubs for known primitives that weren't allowed.
                // this gives callers a clear SandboxViolation instead of "unbound variable".
                {
                    use crate::sandbox::ALL_PRESETS;
                    let stub_fn: Option<
                        unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t) -> ffi::sexp,
                    > = unsafe {
                        std::mem::transmute::<*const std::ffi::c_void, _>(
                            sandbox_stub as *const std::ffi::c_void,
                        )
                    };

                    for preset in ALL_PRESETS {
                        for name in preset.primitives {
                            if !allowed.contains(name) {
                                let c_name = CString::new(*name).unwrap();
                                ffi::sexp_define_foreign(
                                    ctx,
                                    null_env,
                                    c_name.as_ptr(),
                                    0, // num_args (variadic check happens in stub)
                                    c_name.as_ptr(),
                                    stub_fn,
                                );
                            }
                        }
                    }
                }
```

**Step 5: Run tests to verify they pass**

Run: `cargo test -p tein test_sandbox_stub`
Expected: both PASS

**Step 6: Update existing test that asserts `EvalError` for unbound sandbox procs**

update `test_arithmetic_only_env` (around line 2005-2009):

from:
```rust
        let err = ctx.evaluate("(cons 1 2)");
        assert!(
            err.is_err(),
            "cons should be undefined in arithmetic-only env"
        );
```

to:
```rust
        let err = ctx.evaluate("(cons 1 2)").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "cons should produce SandboxViolation in arithmetic-only env, got: {:?}",
            err
        );
```

**Step 7: Run full test suite**

Run: `cargo test -p tein`
Expected: all tests pass. if any test relied on "unbound variable" for a known preset primitive, update it.

**Step 8: Commit**

```bash
git add tein/src/context.rs tein/src/sandbox.rs
git commit -m "feat: register sandbox stubs for disallowed preset bindings"
```

---

### Task 6: clippy, docs, AGENTS.md updates

**Files:**
- Modify: `AGENTS.md` (update error enum docs)
- Check: all modified files

**Step 1: Run clippy**

Run: `cargo clippy -p tein`
Expected: no warnings

**Step 2: Run fmt check**

Run: `cargo fmt -p tein --check`
Expected: no issues

**Step 3: Update AGENTS.md**

in the `error.rs` description line, add `SandboxViolation` to the list:

from:
```
                 StepLimitExceeded, Timeout)
```

to:
```
                 StepLimitExceeded, Timeout, SandboxViolation)
```

**Step 4: Run full test suite one last time**

Run: `cargo test -p tein`
Expected: all tests pass

**Step 5: Commit**

```bash
git add AGENTS.md
git commit -m "docs: add SandboxViolation to AGENTS.md error list"
```
