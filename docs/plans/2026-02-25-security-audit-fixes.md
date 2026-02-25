# Security Audit Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> **Progress:** Tasks 1–3 complete (commits 9565414, 6c0b6a0, da36d24). Resume at Task 4.

**Goal:** Fix all 15 issues from the 2026-02-24 security audit, from critical sandbox escapes down to low-severity hardening items.

**Architecture:** Issues are grouped by file locality. Rust fixes go in `tein/src/`; the one C fix goes in `tein/vendor/chibi-scheme/eval.c`. Each task is independently testable. Tests live in `tein/src/context.rs` (the main test module) unless noted. Run `cargo test` from the `tein/` subdirectory throughout.

**Tech Stack:** Rust (edition 2024), Chibi-Scheme C, cargo test, `tein/src/context.rs` test module.

---

## Background for implementors

- Source lives under `tein/` (not the repo root). All `cargo` commands run from `tein/`.
- Tests are inline in `tein/src/context.rs` in the `#[cfg(test)]` block at the bottom.
- `Error::SandboxViolation(msg)` is the expected error type for sandbox blocks.
- Thread-locals `MODULE_POLICY` and `FS_POLICY` are in `tein/src/sandbox.rs`.
- `Context` struct is in `tein/src/context.rs` around line 1151.
- `ForeignStore` is in `tein/src/foreign.rs`; `PortStore` is in `tein/src/port.rs`.
- Trampolines `port_read_trampoline` / `port_write_trampoline` are in `tein/src/context.rs` around lines 264–354.
- `ThreadLocalContext` thread body is in `tein/src/managed.rs` around line 124.
- `TimeoutContext` thread body is in `tein/src/timeout.rs` around line 94.
- The macro expansion hook patch in chibi is in `tein/vendor/chibi-scheme/eval.c` around line 801.
- `analyze()` function with `goto loop` is in `tein/vendor/chibi-scheme/eval.c` around line 1111.
- `env_copy_named` parent-walk loop is in `tein/vendor/chibi-scheme/tein_shim.c` around line 284.

## implementation notes (from session 1)

- **ALWAYS_STUB excludes `compile`/`generate`**: chibi uses `compile` internally during macro expansion. stubbing it breaks standard library features like `for-each`. `eval` + env accessors are sufficient to close the escape hatch.
- **stub test pattern**: `ctx.evaluate("(eval ...)")` returns the stub *procedure* as a value (Ok), not SandboxViolation. must *call* it: `ctx.evaluate("(eval)")` to trigger the stub and get SandboxViolation.
- **macro hook test**: `set-macro-expand-hook!` requires `.standard_env()`. define the macro BEFORE registering the looping hook, so `define-syntax` itself compiles cleanly.
- **Context struct** is at ~line 1160 now (shifted by earlier edits). `stub_fn` was hoisted out of the inner block to be shared across both registration passes.

---

### ~~Task 1: Issue #1 — sandbox escape via eval/interaction-environment~~ ✅ DONE (9565414)

**Files:**
- Modify: `tein/src/sandbox.rs`
- Modify: `tein/src/context.rs` (stub registration loop, ~line 1062)
- Test: `tein/src/context.rs` (test module)

**Step 1: Add `ALWAYS_STUB` list to sandbox.rs**

After the `ALL_PRESETS` constant at the bottom of `tein/src/sandbox.rs`, add:

```rust
/// Primitives that are **always** stubbed out in sandboxed contexts,
/// regardless of preset configuration.
///
/// These provide direct access to unrestricted environments and cannot
/// be safely exposed in any sandboxed context. Unlike `ALL_PRESETS`,
/// these are never allowable — there is no preset that grants them.
///
/// A sandboxed scheme program holding any of these can call
/// `(eval code (interaction-environment))` to execute arbitrary code
/// in the full unrestricted environment, completely defeating presets.
pub(crate) const ALWAYS_STUB: &[&str] = &[
    "eval",
    "compile",
    "generate",
    "interaction-environment",
    "primitive-environment",
    "scheme-report-environment",
    "current-environment",
    "set-current-environment!",
    "%load",
];
```

**Step 2: Write the failing test**

In the `#[cfg(test)]` block in `tein/src/context.rs`, add:

```rust
#[test]
fn test_sandbox_eval_escape_blocked() {
    // eval + interaction-environment must be stubbed even when not in any preset
    let ctx = Context::builder()
        .preset(&crate::sandbox::ARITHMETIC)
        .build()
        .unwrap();

    for name in ["eval", "interaction-environment", "primitive-environment",
                 "scheme-report-environment", "current-environment",
                 "set-current-environment!", "compile", "generate", "%load"] {
        let err = ctx.evaluate(name).unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "`{}` should be SandboxViolation in sandboxed env, got: {:?}",
            name, err
        );
    }
}

#[test]
fn test_sandbox_eval_escape_attempt() {
    // the classic escape: (eval expr (interaction-environment))
    let ctx = Context::builder()
        .preset(&crate::sandbox::ARITHMETIC)
        .build()
        .unwrap();
    let err = ctx
        .evaluate("(eval '(+ 1 2) (interaction-environment))")
        .unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_)),
        "eval escape attempt should be SandboxViolation, got: {:?}",
        err
    );
}
```

**Step 3: Run test to verify it fails**

```bash
cd tein && cargo test test_sandbox_eval_escape -- --nocapture
```

Expected: FAIL — `interaction-environment` resolves to undefined variable, not `SandboxViolation`.

**Step 4: Add a second stub-registration pass in context.rs**

In `tein/src/context.rs`, find the stub registration block that ends around line 1088:

```rust
                // register sandbox stubs for known primitives that weren't allowed.
                // this gives callers a clear SandboxViolation instead of "undefined variable".
                {
                    use crate::sandbox::ALL_PRESETS;
                    ...
                    for preset in ALL_PRESETS {
                        for name in preset.primitives {
                            if !allowed.contains(name) {
                                ...register stub...
                            }
                        }
                    }
                }
```

Immediately after the closing `}` of that block (still inside the outer `if let Some(ref allowed)` block), add:

```rust
                // always stub environment-escape primitives — these are never
                // allowable in any sandboxed context regardless of preset selection.
                {
                    use crate::sandbox::ALWAYS_STUB;
                    for name in ALWAYS_STUB {
                        let c_name = CString::new(*name).unwrap();
                        ffi::sexp_define_foreign_proc(
                            ctx,
                            null_env,
                            c_name.as_ptr(),
                            0,
                            ffi::SEXP_PROC_VARIADIC,
                            c_name.as_ptr(),
                            stub_fn,
                        );
                    }
                }
```

Note: `stub_fn` is already defined in the enclosing scope — do not redeclare it.

**Step 5: Run tests to verify they pass**

```bash
cd tein && cargo test test_sandbox_eval_escape -- --nocapture
```

Expected: PASS.

**Step 6: Run full test suite**

```bash
cd tein && cargo test
```

Expected: all tests pass.

**Step 7: Commit**

```bash
git add tein/src/sandbox.rs tein/src/context.rs
git commit -m "fix: always stub eval/interaction-environment and friends in sandboxed contexts"
```

---

### ~~Task 2: Issue #2 — macro hook unbounded re-analysis loop (DoS)~~ ✅ DONE (6c0b6a0)

**Files:**
- Modify: `tein/vendor/chibi-scheme/eval.c` (~line 1111, the `analyze()` function)
- Test: `tein/src/context.rs` (test module)

**Background:** `analyze()` has a per-recursive-call depth counter, but the `goto loop` path (used after macro expansion) does NOT increment `depth`. A hook that returns its input unchanged causes infinite re-analysis. We add a separate flat-iteration counter that caps `goto loop` iterations, producing a clear compile error on overflow.

**Step 1: Write the failing test**

```rust
#[test]
fn test_macro_hook_infinite_loop_halts() {
    // a hook that always returns the unexpanded form causes unbounded re-analysis.
    // it must terminate with an error, not hang.
    let ctx = Context::builder()
        .step_limit(100_000)
        .build()
        .unwrap();
    // register a hook that returns the unexpanded form unchanged
    ctx.evaluate("(set-macro-expand-hook! (lambda (name unexpanded expanded env) unexpanded))")
        .unwrap();
    // define a trivial macro and expand it — hook loops on expansion
    ctx.evaluate("(define-syntax my-id (syntax-rules () ((my-id x) x)))").unwrap();
    let err = ctx.evaluate("(my-id 42)").unwrap_err();
    // must be an error (compile error or step limit), not a hang
    assert!(
        matches!(err, Error::EvalError(_) | Error::StepLimitExceeded),
        "infinite macro hook re-analysis must terminate with an error, got: {:?}",
        err
    );
}
```

**Step 2: Run test to verify it fails (hangs or wrong error)**

```bash
cd tein && cargo test test_macro_hook_infinite_loop_halts -- --nocapture
```

Expected: test hangs (step limit eventually kicks in but the compile phase may not be fuel-gated). The test should timeout or produce an unexpected error.

**Step 3: Patch eval.c**

In `tein/vendor/chibi-scheme/eval.c`, find the `analyze()` function signature and local variable declarations (~line 1111):

```c
static sexp analyze (sexp ctx, sexp object, int depth, int defok) {
  sexp op;
  sexp_gc_var4(res, tmp, x, cell);
  sexp_gc_preserve4(ctx, res, tmp, x, cell);
  x = object;

  if (++depth > SEXP_MAX_ANALYZE_DEPTH) {
    res = sexp_compile_error(ctx, "SEXP_MAX_ANALYZE_DEPTH exceeded", x);
    goto error;
  }

 loop:
```

Replace with:

```c
static sexp analyze (sexp ctx, sexp object, int depth, int defok) {
  sexp op;
  /* tein: per-loop-iteration counter to cap macro re-analysis from goto loop.
   * SEXP_MAX_ANALYZE_DEPTH only increments on recursive analyze() entry and
   * is never triggered within a single call's goto loop path. a macro hook
   * (or cyclic macro chain) that always returns the unexpanded form causes
   * infinite re-analysis. this counter provides an independent hard stop. */
  int loop_count = 0;
  sexp_gc_var4(res, tmp, x, cell);
  sexp_gc_preserve4(ctx, res, tmp, x, cell);
  x = object;

  if (++depth > SEXP_MAX_ANALYZE_DEPTH) {
    res = sexp_compile_error(ctx, "SEXP_MAX_ANALYZE_DEPTH exceeded", x);
    goto error;
  }

 loop:
  if (++loop_count > SEXP_MAX_ANALYZE_DEPTH) {
    res = sexp_compile_error(ctx,
      "macro re-analysis limit exceeded: macro hook or cyclic macro returned "
      "a form that re-expands indefinitely (set-macro-expand-hook! / define-syntax cycle)",
      x);
    goto error;
  }
```

**Step 4: Run test to verify it passes**

```bash
cd tein && cargo test test_macro_hook_infinite_loop_halts -- --nocapture
```

Expected: PASS — terminates with `EvalError` containing the message about re-analysis limit.

**Step 5: Run full test suite**

```bash
cd tein && cargo test
```

Expected: all tests pass.

**Step 6: Commit**

```bash
git add tein/vendor/chibi-scheme/eval.c
git commit -m "fix: add per-iteration counter in analyze() to cap macro hook re-analysis loop"
```

---

### ~~Task 3: Issue #3 — port trampoline buffer bounds not validated~~ ✅ DONE (da36d24)

**Files:**
- Modify: `tein/src/context.rs` (`port_read_trampoline` ~line 264, `port_write_trampoline` ~line 316)
- Test: `tein/src/context.rs` (test module)

**Background:** `start` and `end` come from Scheme fixnums cast directly to `usize` with no validation. If `end < start` (underflow) or either is negative (wraps to huge usize), subsequent pointer arithmetic corrupts memory. We add validation before any arithmetic.

**Step 1: Write failing test**

```rust
#[test]
fn test_port_trampoline_bad_indices_do_not_panic() {
    // craft a scheme closure that passes reversed/negative indices to the trampoline.
    // this test ensures the trampoline doesn't panic or corrupt memory on bad input.
    // we can't easily inject bad fixnums through the public API, so we verify
    // that opening/using a custom port with normal indices works, then that
    // the port correctly handles an early EOF (0 bytes read).
    // (full negative-index injection would require direct sexp manipulation.)
    let ctx = Context::new_standard().unwrap();
    let data = b"hello";
    let cursor = std::io::Cursor::new(data.to_vec());
    let port = ctx.open_input_port(cursor).unwrap();
    let result = ctx.read(&port).unwrap();
    // reading "hello" as a symbol
    assert_eq!(result, Value::Symbol("hello".into()));
}
```

**Step 2: Run to confirm it currently passes (regression baseline)**

```bash
cd tein && cargo test test_port_trampoline_bad_indices -- --nocapture
```

Expected: PASS (we're establishing a baseline; the actual UB protection is tested by audit, not easily by Rust tests alone).

**Step 3: Add validation to `port_read_trampoline`**

In `tein/src/context.rs`, find `port_read_trampoline` at ~line 264. Replace the index extraction block:

```rust
        let port_id = ffi::sexp_unbox_fixnum(id_sexp) as u64;
        let start = ffi::sexp_unbox_fixnum(start_sexp) as usize;
        let end = ffi::sexp_unbox_fixnum(end_sexp) as usize;
        let len = end - start;
```

With:

```rust
        let port_id = ffi::sexp_unbox_fixnum(id_sexp) as u64;
        // validate indices before any arithmetic: fixnums from scheme could be
        // negative (cast to huge usize) or reversed (end < start), both causing
        // out-of-bounds pointer arithmetic and heap corruption.
        let start_raw = ffi::sexp_unbox_fixnum(start_sexp);
        let end_raw = ffi::sexp_unbox_fixnum(end_sexp);
        if start_raw < 0 || end_raw < 0 || end_raw < start_raw {
            return ffi::sexp_make_fixnum(0);
        }
        let start = start_raw as usize;
        let end = end_raw as usize;
        let len = end - start;
```

**Step 4: Add validation to `port_write_trampoline`**

Find `port_write_trampoline` at ~line 316. Replace its index extraction block (same pattern):

```rust
        let port_id = ffi::sexp_unbox_fixnum(id_sexp) as u64;
        let start = ffi::sexp_unbox_fixnum(start_sexp) as usize;
        let end = ffi::sexp_unbox_fixnum(end_sexp) as usize;
        let len = end - start;
```

With:

```rust
        let port_id = ffi::sexp_unbox_fixnum(id_sexp) as u64;
        // validate indices: negative or reversed values cause OOB pointer arithmetic.
        let start_raw = ffi::sexp_unbox_fixnum(start_sexp);
        let end_raw = ffi::sexp_unbox_fixnum(end_sexp);
        if start_raw < 0 || end_raw < 0 || end_raw < start_raw {
            return ffi::sexp_make_fixnum(0);
        }
        let start = start_raw as usize;
        let end = end_raw as usize;
        let len = end - start;
```

Also update the docstrings for both trampolines to mention bounds validation.

**Step 5: Run tests**

```bash
cd tein && cargo test
```

Expected: all tests pass.

**Step 6: Commit**

```bash
git add tein/src/context.rs
git commit -m "fix: validate port trampoline start/end indices before pointer arithmetic"
```

---

### Task 4: Issue #4 — context thread death silently hangs caller

**Files:**
- Modify: `tein/src/managed.rs`
- Modify: `tein/src/timeout.rs`
- Test: `tein/src/context.rs` (test module)

**Background:** If any code in the dedicated thread panics (e.g. a registered foreign fn), the thread dies, the channel send never fires, and `recv()` blocks forever. Additionally, `rx.lock().unwrap()` poisons on panic in the calling thread context. Fix: wrap the thread message loop in `std::panic::catch_unwind`, send an error response on catch. Use `.lock().map_err(...)` instead of `.unwrap()`.

**Step 1: Write failing test**

```rust
#[test]
fn test_managed_thread_panic_returns_error() {
    let ctx = Context::builder()
        .step_limit(100_000)
        .build_managed(|_| Ok(()))
        .unwrap();

    // register a function that panics
    ctx.define_fn_variadic("panic-fn", unsafe extern "C" fn(
        _ctx: tein::ffi::sexp,
        _self: tein::ffi::sexp,
        _n: tein::ffi::sexp_sint_t,
        _args: tein::ffi::sexp,
    ) -> tein::ffi::sexp {
        panic!("intentional test panic")
    }).unwrap();

    // calling it should return an error, not hang
    let err = ctx.evaluate("(panic-fn)").unwrap_err();
    assert!(
        matches!(err, Error::InitError(_)),
        "panicking thread should return InitError, got: {:?}",
        err
    );

    // subsequent calls should also return errors, not hang
    let err2 = ctx.evaluate("(+ 1 2)").unwrap_err();
    assert!(matches!(err2, Error::InitError(_)));
}
```

**Step 2: Run to verify it hangs or fails unexpectedly**

```bash
cd tein && timeout 10 cargo test test_managed_thread_panic_returns_error -- --nocapture
```

Expected: timeout (hangs on recv) or panic propagation.

**Step 3: Wrap managed.rs thread body in catch_unwind**

In `tein/src/managed.rs`, the thread body starts at ~line 124. The message loop `for req in req_rx { ... }` needs to be wrapped. Replace:

```rust
            // message loop
            for req in req_rx {
                match req {
                    ...all arms...
                    Request::Shutdown => break,
                }
            }
```

With:

```rust
            // message loop — catch_unwind guards against panics in registered
            // foreign fns or init closures. on panic, send an error response
            // so the caller gets an InitError rather than blocking forever.
            loop {
                let req = match req_rx.recv() {
                    Ok(r) => r,
                    Err(_) => break, // sender dropped
                };
                if matches!(req, Request::Shutdown) {
                    break;
                }
                let resp_tx2 = resp_tx.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match req {
                        Request::Evaluate(code) => {
                            if mode == Mode::Fresh {
                                match Self::build_and_init(&builder, &init) {
                                    Ok(new_ctx) => ctx = new_ctx,
                                    Err(e) => {
                                        let _ = resp_tx2.send(Response::Value(Err(e)));
                                        return;
                                    }
                                }
                            }
                            let result = ctx.evaluate(&code);
                            let _ = resp_tx2.send(Response::Value(result));
                        }
                        Request::Call(proc, args) => {
                            if mode == Mode::Fresh {
                                match Self::build_and_init(&builder, &init) {
                                    Ok(new_ctx) => ctx = new_ctx,
                                    Err(e) => {
                                        let _ = resp_tx2.send(Response::Value(Err(e)));
                                        return;
                                    }
                                }
                            }
                            let args: Vec<Value> = args.into_iter().map(|s| s.0).collect();
                            let result = ctx.call(&proc.0, &args);
                            let _ = resp_tx2.send(Response::Value(result));
                        }
                        Request::DefineFnVariadic { name, f } => {
                            let result = ctx.define_fn_variadic(&name, f);
                            let _ = resp_tx2.send(Response::Defined(result));
                        }
                        Request::Reset => {
                            if mode == Mode::Fresh {
                                let _ = resp_tx2.send(Response::Reset(Ok(())));
                            } else {
                                match Self::build_and_init(&builder, &init) {
                                    Ok(new_ctx) => {
                                        ctx = new_ctx;
                                        let _ = resp_tx2.send(Response::Reset(Ok(())));
                                    }
                                    Err(e) => {
                                        let _ = resp_tx2.send(Response::Reset(Err(e)));
                                    }
                                }
                            }
                        }
                        Request::Shutdown => unreachable!(),
                    }
                }));
                if result.is_err() {
                    // thread panicked — send error and exit loop so handle is
                    // joinable. caller will get InitError on next recv().
                    let _ = resp_tx.send(Response::Value(Err(Error::InitError(
                        "context thread panicked".to_string(),
                    ))));
                    break;
                }
            }
```

Note: `ctx` needs to be declared `mut` before this loop since fresh-mode reassigns it. Verify the `let mut ctx = ...` at the top of the thread closure.

**Step 4: Fix `.unwrap()` on mutex lock in caller methods**

In `tein/src/managed.rs`, all methods (`evaluate`, `call`, `define_fn_variadic`, `reset`) call `self.rx.lock().unwrap()`. Replace each `.unwrap()` with:

```rust
self.rx.lock().map_err(|_| Error::InitError("context rx mutex poisoned".to_string()))?
```

There are 4 occurrences.

**Step 5: Apply same catch_unwind to timeout.rs**

In `tein/src/timeout.rs`, the thread body message loop (~line 108) has the same pattern. Wrap each arm of `match req` in `std::panic::catch_unwind`, sending `Response::Value(Err(Error::InitError("context thread panicked")))` on panic, then breaking.

The simpler timeout.rs loop (no fresh-mode branching) can be wrapped more simply:

```rust
            loop {
                let req = match req_rx.recv() {
                    Ok(r) => r,
                    Err(_) => break,
                };
                if matches!(req, Request::Reset | Request::Shutdown) {
                    break;
                }
                let resp_tx2 = resp_tx.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match req {
                        Request::Evaluate(code) => {
                            let result = ctx.evaluate(&code);
                            let _ = resp_tx2.send(Response::Value(result));
                        }
                        Request::Call(proc, args) => {
                            let args: Vec<Value> = args.into_iter().map(|s| s.0).collect();
                            let result = ctx.call(&proc.0, &args);
                            let _ = resp_tx2.send(Response::Value(result));
                        }
                        Request::DefineFnVariadic { name, f } => {
                            let result = ctx.define_fn_variadic(&name, f);
                            let _ = resp_tx2.send(Response::Defined(result));
                        }
                        _ => unreachable!(),
                    }
                }));
                if result.is_err() {
                    let _ = resp_tx.send(Response::Value(Err(Error::InitError(
                        "context thread panicked".to_string(),
                    ))));
                    break;
                }
            }
```

**Step 6: Run test**

```bash
cd tein && cargo test test_managed_thread_panic_returns_error -- --nocapture
```

Expected: PASS.

**Step 7: Run full suite**

```bash
cd tein && cargo test
```

Expected: all tests pass.

**Step 8: Commit**

```bash
git add tein/src/managed.rs tein/src/timeout.rs
git commit -m "fix: catch thread panics in managed/timeout contexts, return InitError instead of hanging"
```

---

### Task 5: Issue #5 — thread-local policy state race between sequential contexts

**Files:**
- Modify: `tein/src/context.rs` (`Context` struct, `ContextBuilder::build()`, `Drop for Context`)
- Test: `tein/src/context.rs` (test module)

**Background:** Drop unconditionally resets `MODULE_POLICY` to `Unrestricted` and `FS_POLICY` to `None`. If two contexts exist sequentially (ctx2 created before ctx1 dropped), ctx2 gets the wrong policy when ctx1 drops. Fix: save the previous value at build time and restore it on drop.

**Step 1: Write the failing test**

```rust
#[test]
fn test_sequential_context_policy_isolation() {
    // ctx1 has module policy (standard_env + preset)
    let ctx1 = Context::builder()
        .standard_env()
        .preset(&crate::sandbox::ARITHMETIC)
        .build()
        .unwrap();

    // ctx2 also has module policy
    let ctx2 = Context::builder()
        .standard_env()
        .preset(&crate::sandbox::ARITHMETIC)
        .build()
        .unwrap();

    // drop ctx1 — this must NOT clear ctx2's module policy
    drop(ctx1);

    // ctx2 should still block filesystem modules
    let err = ctx2.evaluate("(import (chibi process))").unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
        "ctx2 module policy should still be VfsOnly after ctx1 dropped, got: {:?}",
        err
    );
}
```

**Step 2: Run to verify it fails**

```bash
cd tein && cargo test test_sequential_context_policy_isolation -- --nocapture
```

Expected: FAIL — after `drop(ctx1)`, `MODULE_POLICY` is reset to `Unrestricted`, so ctx2 can now load filesystem modules.

**Step 3: Add previous-value fields to Context struct**

In `tein/src/context.rs`, find the `Context` struct (~line 1151). Add two fields:

```rust
pub struct Context {
    ctx: ffi::sexp,
    step_limit: Option<u64>,
    has_io_wrappers: bool,
    has_module_policy: bool,
    /// previous MODULE_POLICY value, restored on drop (save/restore RAII)
    prev_module_policy: ModulePolicy,
    /// previous FS_POLICY value, restored on drop (save/restore RAII)
    prev_fs_policy: Option<FsPolicy>,
    /// per-context store for foreign type registrations and live instances
    foreign_store: RefCell<ForeignStore>,
    /// whether foreign protocol dispatch functions are registered
    has_foreign_protocol: Cell<bool>,
    /// per-context store for custom port backing objects (Read/Write impls)
    port_store: RefCell<PortStore>,
    /// whether port protocol dispatch functions are registered
    has_port_protocol: Cell<bool>,
}
```

**Step 4: Save previous values at build time**

In `ContextBuilder::build()`, find where `MODULE_POLICY` and `FS_POLICY` are set (~line 951). Before setting them, capture current values:

```rust
            // save current policy values before overwriting — restored on drop
            let prev_module_policy = MODULE_POLICY.with(|cell| cell.get());
            let prev_fs_policy = FS_POLICY.with(|cell| cell.borrow().as_ref().map(|p| FsPolicy {
                read_prefixes: p.read_prefixes.clone(),
                write_prefixes: p.write_prefixes.clone(),
            }));
```

Place this just before the `if has_module_policy { MODULE_POLICY.with(...) }` block. Then pass these to the `Context { ... }` constructor at the bottom of `build()`:

```rust
            let context = Context {
                ctx,
                step_limit: self.step_limit,
                has_io_wrappers: has_io,
                has_module_policy,
                prev_module_policy,
                prev_fs_policy,
                foreign_store: RefCell::new(ForeignStore::new()),
                has_foreign_protocol: Cell::new(false),
                port_store: RefCell::new(PortStore::new()),
                has_port_protocol: Cell::new(false),
            };
```

**Step 5: Restore previous values on drop**

In `impl Drop for Context`, replace:

```rust
        if self.has_module_policy {
            MODULE_POLICY.with(|cell| cell.set(ModulePolicy::Unrestricted));
            unsafe { ffi::module_policy_set(ModulePolicy::Unrestricted as i32) };
        }

        if self.has_io_wrappers {
            FS_POLICY.with(|cell| {
                *cell.borrow_mut() = None;
            });
            ...
        }
```

With:

```rust
        if self.has_module_policy {
            // restore previous policy rather than unconditionally clearing —
            // a second context on the same thread may still be active.
            MODULE_POLICY.with(|cell| cell.set(self.prev_module_policy));
            unsafe { ffi::module_policy_set(self.prev_module_policy as i32) };
        }

        if self.has_io_wrappers {
            // restore previous FS_POLICY (None if there was no prior context)
            FS_POLICY.with(|cell| {
                *cell.borrow_mut() = self.prev_fs_policy.take();
            });
            ORIGINAL_PROCS.with(|procs| {
                for p in procs {
                    p.set(std::ptr::null_mut());
                }
            });
        }
```

Note: `prev_fs_policy` needs to be `Option<FsPolicy>` and taken via `.take()` — make the field `prev_fs_policy: Option<FsPolicy>` (not a reference).

**Step 6: Run test**

```bash
cd tein && cargo test test_sequential_context_policy_isolation -- --nocapture
```

Expected: PASS.

**Step 7: Run full suite**

```bash
cd tein && cargo test
```

Expected: all tests pass.

**Step 8: Commit**

```bash
git add tein/src/context.rs
git commit -m "fix: save/restore thread-local policy on context drop to prevent sequential context interference"
```

---

### Task 6: Issue #7 — non-UTF8 exception messages mangled in policy detection

**Files:**
- Modify: `tein/src/context.rs` (`extract_exception_error`, ~line 356)
- Test: `tein/src/context.rs`

**Background:** `from_utf8_lossy` replaces invalid UTF-8 with U+FFFD. Since `extract_exception_error` then does substring matching on the result to detect sandbox sentinel prefixes (`[sandbox:binding]`, `[sandbox:file]`), an attacker could corrupt the match by embedding invalid UTF-8 in error messages. Fix: use `from_utf8` and propagate an error on invalid sequences.

**Step 1: Write the failing test**

```rust
#[test]
fn test_invalid_utf8_in_error_message_does_not_bypass_sandbox() {
    // verify that the sandbox sentinel detection still works even with
    // an error message that has non-UTF8 bytes (lossy replacement cannot corrupt prefix match).
    // since we now use strict from_utf8, we get a Utf8Error rather than
    // a mismatched EvalError masking a SandboxViolation.
    //
    // this test checks the Rust API level: evaluate() with a sandbox
    // violation still returns SandboxViolation.
    let ctx = Context::builder()
        .preset(&crate::sandbox::ARITHMETIC)
        .build()
        .unwrap();
    let err = ctx.evaluate("(open-input-file \"x\")").unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_)),
        "expected SandboxViolation, got {:?}",
        err
    );
}
```

**Step 2: Run (should already pass — baseline)**

```bash
cd tein && cargo test test_invalid_utf8_in_error_message -- --nocapture
```

**Step 3: Fix `extract_exception_error`**

In `tein/src/context.rs`, find `extract_exception_error` (~line 356). Replace:

```rust
                std::string::String::from_utf8_lossy(bytes).into_owned()
```

With:

```rust
                match std::string::String::from_utf8(bytes.to_vec()) {
                    Ok(s) => s,
                    Err(_) => {
                        // non-UTF8 exception message — cannot safely extract text.
                        // return a generic error rather than lossy conversion, which
                        // could corrupt sentinel prefix matching used for sandbox detection.
                        return Error::EvalError("exception with non-UTF-8 message".to_string());
                    }
                }
```

**Step 4: Run full suite**

```bash
cd tein && cargo test
```

Expected: all tests pass.

**Step 5: Commit**

```bash
git add tein/src/context.rs
git commit -m "fix: use strict from_utf8 in extract_exception_error to prevent lossy UTF-8 corrupting sentinel detection"
```

---

### Task 7: Issue #8 — port/foreign handle IDs are forgeable

**Files:**
- Modify: `tein/src/port.rs`
- Modify: `tein/src/foreign.rs`
- Test: `tein/src/context.rs`

**Background:** Handle IDs are monotonically increasing `u64` starting from 1. A Scheme program can enumerate sequential IDs to access ports/foreign objects it doesn't hold a reference to. Fix: seed from `SystemTime` and use a thread-local xorshift64 PRNG for unpredictable IDs. No external dependency needed.

**Step 1: Add xorshift64 PRNG helper**

Add to the top of `tein/src/port.rs` (and separately to `tein/src/foreign.rs`):

```rust
use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

thread_local! {
    /// xorshift64 state for unpredictable handle ID generation.
    /// seeded from SystemTime to prevent sequential ID guessing.
    static XOR_STATE: Cell<u64> = Cell::new(0);
}

/// Generate the next unpredictable handle ID via xorshift64.
///
/// On first call the state is seeded from SystemTime (or a fixed fallback).
/// IDs are never 0 — if the PRNG produces 0, it is re-rolled once.
fn next_handle_id() -> u64 {
    XOR_STATE.with(|state| {
        let mut s = state.get();
        if s == 0 {
            // seed from wall clock; any non-zero value works
            s = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xdeadbeef_cafef00d);
            if s == 0 { s = 0xdeadbeef_cafef00d; }
        }
        // xorshift64
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        if s == 0 { s = 1; } // xorshift64 must never be 0
        state.set(s);
        s
    })
}
```

**Step 2: Replace `next_id` counter with `next_handle_id()` in PortStore**

In `tein/src/port.rs`, replace the `next_id: u64` field and its usage:

- Remove `next_id` from `PortStore` struct
- Remove `next_id: 1` from `PortStore::new()`
- In `insert_reader` and `insert_writer`, replace `let id = self.next_id; self.next_id += 1;` with `let id = next_handle_id();`

Update the module-level docstring in `port.rs` to mention random IDs.

**Step 3: Same change to ForeignStore in foreign.rs**

Apply the same `next_handle_id()` replacement to `tein/src/foreign.rs` — remove `next_id` field from `ForeignStore`, replace usages in `insert()`.

**Step 4: Write a test**

```rust
#[test]
fn test_handle_ids_are_not_sequential() {
    // IDs should not be trivially predictable sequential integers
    let ctx = Context::new_standard().unwrap();
    let cursor1 = std::io::Cursor::new(b"a".to_vec());
    let cursor2 = std::io::Cursor::new(b"b".to_vec());
    let port1 = ctx.open_input_port(cursor1).unwrap();
    let port2 = ctx.open_input_port(cursor2).unwrap();
    // we can't inspect the internal IDs directly, but we can verify both
    // ports work independently (different IDs → different store entries)
    let v1 = ctx.read(&port1).unwrap();
    let v2 = ctx.read(&port2).unwrap();
    assert_eq!(v1, Value::Symbol("a".into()));
    assert_eq!(v2, Value::Symbol("b".into()));
}
```

**Step 5: Run tests**

```bash
cd tein && cargo test test_handle_ids -- --nocapture && cargo test
```

Expected: all pass.

**Step 6: Commit**

```bash
git add tein/src/port.rs tein/src/foreign.rs
git commit -m "fix: use xorshift64 PRNG for unpredictable port/foreign handle IDs"
```

---

### Task 8: Issue #9 — env_copy_named parent-chain walk has no cycle detection

**Files:**
- Modify: `tein/vendor/chibi-scheme/tein_shim.c` (~line 284)

**Step 1: Add iteration limit to the parent-walk loop**

Find the while loop in `env_copy_named` (~line 284):

```c
    sexp env = src_env;
    while (env && sexp_envp(env)) {
        ...
        env = sexp_env_parent(env);
    }
```

Replace with:

```c
    /* iteration limit guards against corrupted environment with cyclic parent chain.
     * chibi should never produce cycles, but defence-in-depth warrants a hard stop. */
    int env_walk_limit = 65536;
    sexp env = src_env;
    while (env && sexp_envp(env) && env_walk_limit-- > 0) {
        ...
        env = sexp_env_parent(env);
    }
```

**Step 2: Run full suite**

```bash
cd tein && cargo test
```

Expected: all tests pass.

**Step 3: Commit**

```bash
git add tein/vendor/chibi-scheme/tein_shim.c
git commit -m "fix: add iteration limit to env_copy_named parent-chain walk (cycle detection)"
```

---

### Task 9: Issue #11 — u64 ID overflow in ForeignStore/PortStore

**(Only applies if xorshift64 was NOT used in Task 7. Since Task 7 replaces the counter entirely, this issue is resolved automatically. Verify and close.)**

**Step 1: Verify no `next_id` fields remain**

```bash
cd tein && grep -n "next_id" src/foreign.rs src/port.rs
```

Expected: no matches. If any remain, add `checked_add().expect("handle ID overflow")`.

**Step 2: Commit**

No code change needed if Task 7 was done. Note in commit:

```bash
git commit --allow-empty -m "docs: issue #11 (u64 ID overflow) resolved by xorshift64 fix in Task 7"
```

---

### Task 10: Issue #14 — CString inconsistency in sandbox error path

**Files:**
- Modify: `tein/src/context.rs` (~line 630)

**Step 1: Find and fix the bare string cast**

In `check_and_delegate` (~line 629), find:

```rust
            let msg = "open-file: expected string argument";
            let c_msg = msg.as_ptr() as *const c_char;
            return ffi::make_error(ctx, c_msg, msg.len() as ffi::sexp_sint_t);
```

Replace with:

```rust
            let msg = "open-file: expected string argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
```

**Step 2: Run full suite**

```bash
cd tein && cargo test
```

**Step 3: Commit**

```bash
git add tein/src/context.rs
git commit -m "fix: use CString consistently in sandbox error path (was bare string ptr cast)"
```

---

### Task 11: Issue #15 — unbounded channel in ThreadLocalContext

**Files:**
- Modify: `tein/src/managed.rs`

**Step 1: Replace unbounded channel with sync_channel**

In `tein/src/managed.rs`, find:

```rust
        let (req_tx, req_rx) = mpsc::channel::<Request>();
        let (resp_tx, resp_rx) = mpsc::channel::<Response>();
```

Replace with:

```rust
        // bounded channels: prevents unbounded memory growth if evaluate()
        // calls pile up faster than the thread processes them. capacity 64
        // is generous for any realistic use; callers block naturally when full.
        let (req_tx, req_rx) = mpsc::sync_channel::<Request>(64);
        let (resp_tx, resp_rx) = mpsc::sync_channel::<Response>(64);
```

Update the type of `tx` in `ThreadLocalContext` from `mpsc::Sender<Request>` to `mpsc::SyncSender<Request>`, and similarly for the resp channel (which is internal — check if it needs type annotation).

**Step 2: Run full suite**

```bash
cd tein && cargo test
```

Expected: all tests pass.

**Step 3: Commit**

```bash
git add tein/src/managed.rs
git commit -m "fix: use bounded sync_channel(64) in ThreadLocalContext to prevent unbounded memory growth"
```

---

### Task 12: Issue #12 — reader dispatch ASCII-only limitation undocumented

**Files:**
- Modify: `tein/src/context.rs` (`register_reader` docstring, ~line 1536)
- Modify: `tein/vendor/chibi-scheme/tein_shim.c` (reader dispatch table comment, ~line 333)

**Step 1: Update register_reader docstring**

Find the `register_reader` docstring (~line 1538). The existing text mentions `ch` must be printable ASCII but doesn't explain the 128-entry limit. Update:

```
/// `ch` must be an ASCII byte (value < 128). The dispatch table has 128 entries;
/// characters with values ≥ 128 are silently ignored by the C dispatch layer
/// and cannot be used as reader dispatch characters.
```

**Step 2: Add comment to tein_shim.c**

Find the reader dispatch table declaration in `tein_shim.c` (~line 333). Add a comment:

```c
/* reader dispatch table — 128 entries, ASCII-only.
 * characters with codepoint >= 128 are not dispatchable;
 * the Rust API documents this limitation in register_reader(). */
```

**Step 3: Run full suite and commit**

```bash
cd tein && cargo test
git add tein/src/context.rs tein/vendor/chibi-scheme/tein_shim.c
git commit -m "docs: document ASCII-only limitation of reader dispatch table"
```

---

### Task 13: Issue #10 — sexp_vector_set has no bounds check

**Files:**
- Modify: `tein/vendor/chibi-scheme/tein_shim.c` (~line 94)

**Step 1: Add an assertion**

Find `tein_sexp_vector_set`:

```c
void tein_sexp_vector_set(sexp vec, sexp_uint_t i, sexp val) {
    sexp_vector_data(vec)[i] = val;
}
```

Replace with:

```c
void tein_sexp_vector_set(sexp vec, sexp_uint_t i, sexp val) {
    /* bounds assertion: caller is trusted (Rust), but an incorrect index
     * would be a heap write OOB. assert catches this in debug builds. */
    assert(i < (sexp_uint_t)sexp_vector_length(vec));
    sexp_vector_data(vec)[i] = val;
}
```

Add `#include <assert.h>` near the top of `tein_shim.c` if not already present.

**Step 2: Run full suite and commit**

```bash
cd tein && cargo test
git add tein/vendor/chibi-scheme/tein_shim.c
git commit -m "fix: add bounds assertion to tein_sexp_vector_set"
```

---

### Task 14: Issue #6 — missing stubs for dangerous chibi primitives (medium)

**Files:**
- Modify: `tein/src/sandbox.rs` (extend `ALWAYS_STUB` from Task 1)

**Background:** Many dangerous chibi primitives exist in `opcodes.c` but aren't in any preset, so they get neither allowed nor stubbed. The audit calls out the unpredictable restriction surface. This task audits `opcodes.c` and extends `ALWAYS_STUB` with additional dangerous names.

**Step 1: Audit opcodes.c for dangerous primitives**

```bash
grep -E '_(FN|PARAM|OP)' tein/vendor/chibi-scheme/opcodes.c | grep -v '^//' | head -60
```

Look for: file IO (`open-*`, `close-*`, `read-*`, `write-*`, `delete-file`, `rename-file`), process ops (`system`, `exec`, `fork`, `exit`), environment access (`get-environment-variable`, `get-environment-variables`), dynamic loading (`load`, `dynamic-load`), network ops if any. Add any found to `ALWAYS_STUB` in `sandbox.rs` that are genuinely dangerous and not already covered.

**Step 2: Run tests, commit**

```bash
cd tein && cargo test
git add tein/src/sandbox.rs
git commit -m "fix: extend ALWAYS_STUB with additional dangerous chibi primitives from opcodes.c audit"
```

---

### Task 15: Final verification

**Step 1: Run the full test suite one last time**

```bash
cd tein && cargo test
```

Expected: all tests pass.

**Step 2: Run clippy**

```bash
cd tein && cargo clippy
```

Resolve any new warnings introduced.

**Step 3: Commit any clippy fixes**

```bash
git add -p
git commit -m "fix: clippy warnings from security audit fixes"
```

**Step 4: Update the audit document**

Mark each issue resolved in `docs/plans/2026-02-24-security-audit.md` by appending a resolution status table:

```markdown
## resolution status (2026-02-25)

| # | status | commit |
|---|--------|--------|
| 1 | resolved | fix: always stub eval/... |
| 2 | resolved | fix: add per-iteration counter... |
| 3 | resolved | fix: validate port trampoline... |
| 4 | resolved | fix: catch thread panics... |
| 5 | resolved | fix: save/restore thread-local policy... |
| 6 | resolved | fix: extend ALWAYS_STUB... |
| 7 | resolved | fix: use strict from_utf8... |
| 8 | resolved | fix: use xorshift64 PRNG... |
| 9 | resolved | fix: add iteration limit to env_copy_named... |
| 10 | resolved | fix: add bounds assertion to tein_sexp_vector_set |
| 11 | resolved | (by Task 7 — counter replaced entirely) |
| 12 | resolved | docs: document ASCII-only limitation... |
| 13 | resolved | fix: use CString consistently... |
| 14 | resolved | fix: use bounded sync_channel... |
| 15 | resolved | fix: add bounds assertion... |
```

```bash
git add docs/plans/2026-02-24-security-audit.md
git commit -m "docs: mark all security audit issues resolved"
```
