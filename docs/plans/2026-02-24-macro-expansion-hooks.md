# macro expansion hooks implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** intercept macro expansion via a thread-local hook, enabling observation and transformation of expanded forms.

**Architecture:** thread-local hook slot in `tein_shim.c`, eval.c patch in `analyze_macro_once()`, scheme-first API via `(tein macro)` VFS module, thin rust convenience layer. follows the reader dispatch pattern exactly.

**Tech Stack:** C (tein_shim.c, eval.c patch), rust (ffi.rs, context.rs), scheme (VFS module)

---

### Task 1: C shim — thread-local hook slot

**Files:**
- Modify: `tein/vendor/chibi-scheme/tein_shim.c` (append after reader dispatch section, ~line 400)

**Step 1: Add thread-local hook variables and shim functions**

append after the reader dispatch section (after `tein_reader_dispatch_clear`):

```c
// ─── macro expansion hook ───────────────────────────────────────────
// thread-local hook called after each macro expansion. when set (not
// SEXP_FALSE), the hook receives (name unexpanded expanded env) and
// its return value replaces the expanded form (replace-and-reanalyze).
// the active flag prevents recursion when the hook body uses macros.

TEIN_THREAD_LOCAL sexp tein_macro_expand_hook = SEXP_FALSE;
TEIN_THREAD_LOCAL int tein_macro_expand_hook_active = 0;

void tein_macro_expand_hook_set(sexp ctx, sexp proc) {
    if (tein_macro_expand_hook != SEXP_FALSE)
        sexp_release_object(ctx, tein_macro_expand_hook);
    tein_macro_expand_hook = proc;
    if (proc != SEXP_FALSE)
        sexp_preserve_object(ctx, proc);
}

sexp tein_macro_expand_hook_get(void) {
    return tein_macro_expand_hook;
}

void tein_macro_expand_hook_clear(sexp ctx) {
    if (tein_macro_expand_hook != SEXP_FALSE)
        sexp_release_object(ctx, tein_macro_expand_hook);
    tein_macro_expand_hook = SEXP_FALSE;
    tein_macro_expand_hook_active = 0;
}
```

**Step 2: Verify build**

Run: `cargo build -p tein 2>&1 | tail -5`
Expected: build succeeds (new C code compiles but isn't called yet)

**Step 3: Commit**

```bash
git add tein/vendor/chibi-scheme/tein_shim.c
git commit -m "feat: add macro expansion hook thread-locals to tein_shim.c"
```

---

### Task 2: eval.c patch — call hook after expansion

**Files:**
- Modify: `tein/vendor/chibi-scheme/eval.c:775-793` (patch `analyze_macro_once`)

**Step 1: Patch analyze_macro_once to call the hook**

the function currently ends at line 791-793:

```c
  sexp_gc_release1(ctx);
  return res;
}
```

replace the body of `analyze_macro_once` (lines 775-793) with a version that calls the hook after expansion. the key change is: after `sexp_apply` returns `res`, check `tein_macro_expand_hook` and call it if set.

note: `x` is the unexpanded form. at the call sites, when `x` is a pair, `sexp_car(x)` is the macro name; when `x` is an identifier (bare macro, line 1199), `x` itself is the name. we need to pass the name to the hook, so `analyze_macro_once` needs a `name` parameter.

**modify the function signature** from:
```c
static sexp analyze_macro_once (sexp ctx, sexp x, sexp op, int depth) {
```
to:
```c
static sexp analyze_macro_once (sexp ctx, sexp name, sexp x, sexp op, int depth) {
```

**full replacement body:**
```c
static sexp analyze_macro_once (sexp ctx, sexp name, sexp x, sexp op, int depth) {
  sexp res;
  sexp_gc_var2(tmp, hook_args);
  sexp_gc_preserve2(ctx, tmp, hook_args);
  tmp = sexp_cons(ctx, sexp_macro_env(op), SEXP_NULL);
  tmp = sexp_cons(ctx, sexp_context_env(ctx), tmp);
  tmp = sexp_cons(ctx, x, tmp);
  res = sexp_exceptionp(tmp) ? tmp : sexp_make_child_context(ctx, sexp_context_lambda(ctx));
  if (!sexp_exceptionp(res) && !sexp_exceptionp(sexp_context_exception(ctx)))
    res = sexp_apply(res, sexp_macro_proc(op), tmp);
  if (sexp_pairp(sexp_car(tmp)) && sexp_pair_source(sexp_car(tmp))) {
    if (sexp_pairp(res))
      sexp_pair_source(res) = sexp_pair_source(sexp_car(tmp));
    else if (sexp_exceptionp(res) && sexp_not(sexp_exception_source(x)))
      sexp_exception_source(res) = sexp_pair_source(sexp_car(tmp));
  }
  /* tein: macro expansion hook (D) */
  if (!sexp_exceptionp(res) && tein_macro_expand_hook != SEXP_FALSE
      && !tein_macro_expand_hook_active) {
    tein_macro_expand_hook_active = 1;
    hook_args = sexp_cons(ctx, sexp_context_env(ctx), SEXP_NULL);
    hook_args = sexp_cons(ctx, res, hook_args);
    hook_args = sexp_cons(ctx, x, hook_args);
    hook_args = sexp_cons(ctx, name, hook_args);
    res = sexp_apply(ctx, tein_macro_expand_hook, hook_args);
    tein_macro_expand_hook_active = 0;
  }
  sexp_gc_release2(ctx);
  return res;
}
```

**Step 2: Update the 3 call sites to pass the macro name**

line 826 (`analyze_set`):
```c
        res = analyze_macro_once(ctx, sexp_car(x), x, op, depth);
```

line 1164 (pair-form macro in `analyze`):
```c
          x = analyze_macro_once(ctx, sexp_car(x), x, op, depth);
```

line 1199 (bare identifier macro in `analyze`):
```c
      x = analyze_macro_once(ctx, x, x, op, depth);
```

**Step 3: Add extern declaration for the thread-locals**

at the top of eval.c (near other tein externs), add:

```c
extern TEIN_THREAD_LOCAL sexp tein_macro_expand_hook;
extern TEIN_THREAD_LOCAL int tein_macro_expand_hook_active;
```

note: `TEIN_THREAD_LOCAL` is defined in `tein_shim.c`. since eval.c is compiled separately, we need the macro available. add before the extern declarations:

```c
#ifndef TEIN_THREAD_LOCAL
#ifdef _MSC_VER
#define TEIN_THREAD_LOCAL __declspec(thread)
#else
#define TEIN_THREAD_LOCAL __thread
#endif
#endif
```

**Step 4: Verify build**

Run: `cargo build -p tein 2>&1 | tail -5`
Expected: build succeeds

**Step 5: Commit**

```bash
git add tein/vendor/chibi-scheme/eval.c
git commit -m "feat: patch eval.c to call macro expansion hook after expansion"
```

---

### Task 3: rust FFI bindings

**Files:**
- Modify: `tein/src/ffi.rs` (add extern declarations + safe wrappers)

**Step 1: Add extern declarations**

in the `extern "C"` block (near the reader dispatch declarations, ~line 196-200), add:

```rust
    // macro expansion hook
    pub fn tein_macro_expand_hook_set(ctx: sexp, proc: sexp);
    pub fn tein_macro_expand_hook_get() -> sexp;
    pub fn tein_macro_expand_hook_clear(ctx: sexp);
```

**Step 2: Add safe wrappers**

after the reader dispatch safe wrappers (~line 692), add:

```rust
/// set the macro expansion hook procedure, or SEXP_FALSE to clear.
/// GC-safe: preserves the proc and releases any previous hook.
#[inline]
pub unsafe fn macro_expand_hook_set(ctx: sexp, proc: sexp) {
    unsafe { tein_macro_expand_hook_set(ctx, proc) }
}

/// get the current macro expansion hook, or SEXP_FALSE if none.
#[inline]
pub unsafe fn macro_expand_hook_get() -> sexp {
    unsafe { tein_macro_expand_hook_get() }
}

/// clear the macro expansion hook, releasing the GC reference.
#[inline]
pub unsafe fn macro_expand_hook_clear(ctx: sexp) {
    unsafe { tein_macro_expand_hook_clear(ctx) }
}
```

**Step 3: Verify build**

Run: `cargo build -p tein 2>&1 | tail -5`
Expected: build succeeds

**Step 4: Commit**

```bash
git add tein/src/ffi.rs
git commit -m "feat: add macro expansion hook FFI bindings"
```

---

### Task 4: scheme wrappers in context.rs

**Files:**
- Modify: `tein/src/context.rs` (add wrapper functions, registration, cleanup)

**Step 1: Add extern "C" wrapper functions**

near the reader dispatch wrappers (`reader_set_wrapper`, ~line 326), add:

```rust
/// extern "C" wrapper for (set-macro-expand-hook! proc)
unsafe extern "C" fn macro_expand_hook_set_wrapper(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        if ffi::sexp_nullp(args) != 0 {
            let msg = "set-macro-expand-hook!: expected (set-macro-expand-hook! proc)";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let proc = ffi::sexp_car(args);
        if ffi::sexp_procedurep(proc) == 0 {
            let msg = "set-macro-expand-hook!: argument must be a procedure";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        ffi::macro_expand_hook_set(ctx, proc);
        ffi::get_void()
    }
}

/// extern "C" wrapper for (unset-macro-expand-hook!)
unsafe extern "C" fn macro_expand_hook_unset_wrapper(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        ffi::macro_expand_hook_clear(ctx);
        ffi::get_void()
    }
}

/// extern "C" wrapper for (macro-expand-hook)
unsafe extern "C" fn macro_expand_hook_get_wrapper(
    _ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe { ffi::macro_expand_hook_get() }
}
```

**Step 2: Add registration method on Context**

near `register_reader_protocol` (~line 1377), add:

```rust
    /// register the macro expansion hook protocol functions.
    ///
    /// called automatically by `build()` for standard env contexts. the native
    /// fns are used by the `(tein macro)` VFS module to provide the public API
    /// (`set-macro-expand-hook!`, `unset-macro-expand-hook!`, etc.).
    fn register_macro_expand_protocol(&self) -> Result<()> {
        self.define_fn_variadic("set-macro-expand-hook!", macro_expand_hook_set_wrapper)?;
        self.define_fn_variadic("unset-macro-expand-hook!", macro_expand_hook_unset_wrapper)?;
        self.define_fn_variadic("macro-expand-hook", macro_expand_hook_get_wrapper)?;
        Ok(())
    }
```

**Step 3: Call registration in build()**

in the `build()` method, near the `register_reader_protocol` call (~line 947), add:

```rust
                context.register_macro_expand_protocol()?;
```

(inside the same `if self.standard_env` block)

**Step 4: Add cleanup in drop()**

in `impl Drop for Context`, near the `reader_dispatch_clear` call (~line 1757), add:

```rust
        // clear macro expansion hook
        unsafe { ffi::macro_expand_hook_clear(self.ctx) };
```

**Step 5: Add rust convenience API methods**

near `register_reader` (~line 1403), add:

```rust
    /// set a scheme procedure as the macro expansion hook.
    ///
    /// the hook receives `(name unexpanded expanded env)` after each macro
    /// expansion and returns the form to use (replace-and-reanalyze semantics).
    /// return `expanded` unchanged for observation-only mode.
    ///
    /// # example
    ///
    /// ```
    /// # use tein::Context;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new_standard()?;
    /// let hook = ctx.evaluate(
    ///     "(lambda (name unexpanded expanded env) expanded)"
    /// )?;
    /// ctx.set_macro_expand_hook(&hook)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_macro_expand_hook(&self, proc: &Value) -> Result<()> {
        let raw_proc = proc
            .as_procedure()
            .ok_or_else(|| Error::TypeError("hook must be a procedure".into()))?;
        unsafe { ffi::macro_expand_hook_set(self.ctx, raw_proc) };
        Ok(())
    }

    /// clear the macro expansion hook.
    pub fn unset_macro_expand_hook(&self) {
        unsafe { ffi::macro_expand_hook_clear(self.ctx) };
    }

    /// return the current macro expansion hook, or `None` if not set.
    pub fn macro_expand_hook(&self) -> Option<Value> {
        let raw = unsafe { ffi::macro_expand_hook_get() };
        if unsafe { ffi::sexp_booleanp(raw) != 0 } {
            None
        } else {
            Some(Value::Procedure(raw))
        }
    }
```

**Step 6: Verify build**

Run: `cargo build -p tein 2>&1 | tail -5`
Expected: build succeeds

**Step 7: Commit**

```bash
git add tein/src/context.rs
git commit -m "feat: add macro expansion hook scheme wrappers, registration, cleanup"
```

---

### Task 5: VFS module — `(tein macro)`

**Files:**
- Create: `tein/vendor/chibi-scheme/lib/tein/macro.sld`
- Create: `tein/vendor/chibi-scheme/lib/tein/macro.scm`
- Modify: `tein/build.rs` (add to VFS_FILES)

**Step 1: Create the library definition**

`tein/vendor/chibi-scheme/lib/tein/macro.sld`:
```scheme
(define-library (tein macro)
  (export set-macro-expand-hook! unset-macro-expand-hook! macro-expand-hook)
  (include "macro.scm"))
```

**Step 2: Create the module documentation**

`tein/vendor/chibi-scheme/lib/tein/macro.scm`:
```scheme
;;; (tein macro) — macro expansion hook
;;;
;;; set-macro-expand-hook!, unset-macro-expand-hook!, macro-expand-hook are
;;; registered from rust as native functions in the context env. this module
;;; re-exports them for idiomatic r7rs (import (tein macro)) usage.
;;;
;;; the hook receives (name unexpanded expanded env) after each macro expansion
;;; and returns the form to use. return expanded unchanged for observation.
;;;
;;; note: these bindings are already available in the global env for
;;; standard_env contexts — the import is optional but recommended.
```

**Step 3: Add to VFS_FILES in build.rs**

in the `VFS_FILES` array (~line 72-73), add:

```rust
    // tein macro expansion hook
    "lib/tein/macro.sld",
    "lib/tein/macro.scm",
```

**Step 4: Verify build**

Run: `cargo clean -p tein && cargo build -p tein 2>&1 | tail -5`
Expected: build succeeds (clean build needed to regenerate VFS)

**Step 5: Commit**

```bash
git add tein/vendor/chibi-scheme/lib/tein/macro.sld tein/vendor/chibi-scheme/lib/tein/macro.scm tein/build.rs
git commit -m "feat: add (tein macro) VFS module for macro expansion hooks"
```

---

### Task 6: Tests — observation mode

**Files:**
- Modify: `tein/src/context.rs` (add tests at end of test module)

**Step 1: Write test_macro_expand_hook_basic**

```rust
    #[test]
    fn test_macro_expand_hook_basic() {
        let ctx = Context::new_standard().expect("context");
        // define a simple macro
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        // set a hook that records it was called and returns expanded unchanged
        ctx.evaluate(
            "(define hook-called #f)
             (set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (set! hook-called #t)
                 expanded))"
        ).expect("set hook");
        ctx.evaluate("(double 5)").expect("use macro");
        let called = ctx.evaluate("hook-called").expect("check");
        assert_eq!(called, Value::Boolean(true));
    }
```

**Step 2: Run test to verify it passes**

Run: `cargo test -p tein test_macro_expand_hook_basic -- --nocapture 2>&1 | tail -10`
Expected: PASS

**Step 3: Write test_macro_expand_hook_observation**

```rust
    #[test]
    fn test_macro_expand_hook_observation() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        // hook that captures the unexpanded form
        ctx.evaluate(
            "(define captured-unexpanded #f)
             (set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (set! captured-unexpanded unexpanded)
                 expanded))"
        ).expect("set hook");
        ctx.evaluate("(double 5)").expect("use macro");
        let captured = ctx.evaluate("captured-unexpanded").expect("check");
        // the unexpanded form should be (double 5)
        let list = captured.as_list().expect("should be list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[1], Value::Integer(5));
    }
```

**Step 4: Run test**

Run: `cargo test -p tein test_macro_expand_hook_observation -- --nocapture 2>&1 | tail -10`
Expected: PASS

**Step 5: Write test_macro_expand_hook_name_arg**

```rust
    #[test]
    fn test_macro_expand_hook_name_arg() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(define captured-name #f)
             (set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (set! captured-name name)
                 expanded))"
        ).expect("set hook");
        ctx.evaluate("(double 5)").expect("use macro");
        let name = ctx.evaluate("captured-name").expect("check");
        assert_eq!(name, Value::Symbol("double".into()));
    }
```

**Step 6: Run test**

Run: `cargo test -p tein test_macro_expand_hook_name_arg -- --nocapture 2>&1 | tail -10`
Expected: PASS

**Step 7: Commit**

```bash
git add tein/src/context.rs
git commit -m "test: macro expansion hook observation mode"
```

---

### Task 7: Tests — transformation + edge cases

**Files:**
- Modify: `tein/src/context.rs` (add more tests)

**Step 1: Write test_macro_expand_hook_transformation**

```rust
    #[test]
    fn test_macro_expand_hook_transformation() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        // hook that replaces expansion with a constant
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 42))"
        ).expect("set hook");
        let result = ctx.evaluate("(double 5)").expect("use macro");
        assert_eq!(result, Value::Integer(42));
    }
```

**Step 2: Write test_macro_expand_hook_reanalyze**

```rust
    #[test]
    fn test_macro_expand_hook_reanalyze() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define double");
        ctx.evaluate("(define-syntax add1 (syntax-rules () ((add1 x) (+ x 1))))")
            .expect("define add1");
        // hook returns a form containing another macro — should get expanded
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (if (eq? name 'double)
                     '(add1 99)
                     expanded)))"
        ).expect("set hook");
        let result = ctx.evaluate("(double 5)").expect("use macro");
        // (add1 99) -> (+ 99 1) -> 100
        assert_eq!(result, Value::Integer(100));
    }
```

**Step 3: Write test_macro_expand_hook_unset**

```rust
    #[test]
    fn test_macro_expand_hook_unset() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env) 42))"
        ).expect("set hook");
        assert_eq!(ctx.evaluate("(double 5)").unwrap(), Value::Integer(42));
        ctx.evaluate("(unset-macro-expand-hook!)").expect("unset");
        // after unsetting, macro works normally again
        assert_eq!(ctx.evaluate("(double 5)").unwrap(), Value::Integer(10));
    }
```

**Step 4: Write test_macro_expand_hook_recursion_guard**

```rust
    #[test]
    fn test_macro_expand_hook_recursion_guard() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        // hook body uses `when` (a macro from scheme base) — should not
        // cause infinite recursion thanks to the active flag.
        ctx.evaluate(
            "(define hook-count 0)
             (set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (when (eq? name 'double)
                   (set! hook-count (+ hook-count 1)))
                 expanded))"
        ).expect("set hook");
        let result = ctx.evaluate("(double 5)").expect("use macro");
        assert_eq!(result, Value::Integer(10));
        let count = ctx.evaluate("hook-count").expect("check count");
        assert_eq!(count, Value::Integer(1));
    }
```

**Step 5: Write test_macro_expand_hook_error_propagation**

```rust
    #[test]
    fn test_macro_expand_hook_error_propagation() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (error \"hook failed\")))"
        ).expect("set hook");
        let result = ctx.evaluate("(double 5)");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("hook failed"), "expected 'hook failed' in: {msg}");
    }
```

**Step 6: Write test_macro_expand_hook_introspection**

```rust
    #[test]
    fn test_macro_expand_hook_introspection() {
        let ctx = Context::new_standard().expect("context");
        // no hook set
        let none = ctx.evaluate("(macro-expand-hook)").expect("get");
        assert_eq!(none, Value::Boolean(false));
        // set hook
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env) expanded))"
        ).expect("set");
        let hook = ctx.evaluate("(macro-expand-hook)").expect("get");
        assert!(matches!(hook, Value::Procedure(_)));
        // unset
        ctx.evaluate("(unset-macro-expand-hook!)").expect("unset");
        let none_again = ctx.evaluate("(macro-expand-hook)").expect("get");
        assert_eq!(none_again, Value::Boolean(false));
    }
```

**Step 7: Write test_macro_expand_hook_cleanup_on_drop**

```rust
    #[test]
    fn test_macro_expand_hook_cleanup_on_drop() {
        {
            let ctx = Context::new_standard().expect("context");
            ctx.evaluate(
                "(set-macro-expand-hook!
                   (lambda (name unexpanded expanded env) expanded))"
            ).expect("set");
        } // ctx dropped here — should clear hook
        // create new context on same thread — hook should be clear
        let ctx2 = Context::new_standard().expect("context2");
        let hook = ctx2.evaluate("(macro-expand-hook)").expect("get");
        assert_eq!(hook, Value::Boolean(false));
    }
```

**Step 8: Write test_macro_expand_hook_sandbox**

```rust
    #[test]
    fn test_macro_expand_hook_sandbox() {
        use crate::sandbox::*;
        let ctx = Context::builder()
            .standard_env()
            .preset(&PURE)
            .allow(&["import", "define-syntax", "syntax-rules", "set!", "define"])
            .build()
            .expect("sandboxed context");
        ctx.evaluate("(import (tein macro))").expect("import");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(define hook-called #f)
             (set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (set! hook-called #t)
                 expanded))"
        ).expect("set hook");
        ctx.evaluate("(double 5)").expect("use macro");
        let called = ctx.evaluate("hook-called").expect("check");
        assert_eq!(called, Value::Boolean(true));
    }
```

**Step 9: Write test_macro_expand_hook_rust_api**

```rust
    #[test]
    fn test_macro_expand_hook_rust_api() {
        let ctx = Context::new_standard().expect("context");
        assert!(ctx.macro_expand_hook().is_none());
        let hook = ctx.evaluate(
            "(lambda (name unexpanded expanded env) expanded)"
        ).expect("hook");
        ctx.set_macro_expand_hook(&hook).expect("set");
        assert!(ctx.macro_expand_hook().is_some());
        ctx.unset_macro_expand_hook();
        assert!(ctx.macro_expand_hook().is_none());
    }
```

**Step 10: Write test_macro_expand_hook_via_import**

```rust
    #[test]
    fn test_macro_expand_hook_via_import() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein macro))").expect("import");
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env) expanded))"
        ).expect("set via import");
        let hook = ctx.evaluate("(macro-expand-hook)").expect("get");
        assert!(matches!(hook, Value::Procedure(_)));
    }
```

**Step 11: Run all macro expansion hook tests**

Run: `cargo test -p tein test_macro_expand_hook -- --nocapture 2>&1 | tail -20`
Expected: all 10 tests pass

**Step 12: Commit**

```bash
git add tein/src/context.rs
git commit -m "test: macro expansion hook transformation, edge cases, sandbox"
```

---

### Task 8: Update documentation

**Files:**
- Modify: `tein/src/lib.rs` (no new re-exports needed, just verify)
- Modify: `AGENTS.md` (update architecture section)
- Modify: `TODO.md` (check off macro expansion hooks)
- Modify: `DEVELOPMENT.md` (add macro expansion hook section if it exists)

**Step 1: Update AGENTS.md**

add to the architecture section after `reader dispatch flow`:

```
**macro expansion hook flow**: `ctx.set_macro_expand_hook(&proc)` or scheme `(set-macro-expand-hook! proc)` → `ffi::macro_expand_hook_set(ctx, proc)` → stores proc in thread-local `tein_macro_expand_hook` with GC preservation. when chibi's `analyze_macro_once()` expands a macro (patched eval.c D), checks hook → if set and not already active, sets `tein_macro_expand_hook_active` recursion guard → calls `sexp_apply(ctx, hook, (name unexpanded expanded env))` → hook return value replaces expanded form → `goto loop` reanalyzes (replace-and-reanalyze semantics). hook cleared on `Context::drop()`.
```

also add to the `vendor/chibi-scheme/` section in the architecture file listing:

```
  eval.c       — 4 patches: VFS module lookup (A + module policy gate), VFS load (B), VFS open-input-file (C), macro expansion hook (D)
```

(update the count from 3 to 4 patches)

**Step 2: Update TODO.md**

change:
```
- [ ] **macro expansion hooks**
```
to:
```
- [x] **macro expansion hooks**
  - thread-local hook in `tein_shim.c` + eval.c patch in `analyze_macro_once()`
  - `(tein macro)` VFS module: `set-macro-expand-hook!`, `unset-macro-expand-hook!`, `macro-expand-hook`
  - `Context::set_macro_expand_hook`, `unset_macro_expand_hook`, `macro_expand_hook` rust API
  - replace-and-reanalyze semantics, recursion guard, GC-safe
  - 10 tests: observation, transformation, reanalyze, unset, recursion, errors, introspection, cleanup, sandbox, rust API
```

**Step 3: Run full test suite**

Run: `cargo test -p tein 2>&1 | tail -5`
Expected: all tests pass (existing + new)

**Step 4: Commit**

```bash
git add AGENTS.md TODO.md
git commit -m "docs: update architecture and roadmap for macro expansion hooks"
```
