# macro expansion hooks implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** intercept macro expansion via a thread-local hook, enabling observation and transformation of expanded forms.

**Architecture:** thread-local hook slot in `tein_shim.c`, eval.c patch in `analyze_macro_once()`, scheme-first API via `(tein macro)` VFS module, thin rust convenience layer.

**Tech Stack:** C (tein_shim.c, eval.c patch), rust (ffi.rs, context.rs), scheme (VFS module)

**Worktree:** `.worktrees/macro-expansion-hooks` on branch `feature/macro-expansion-hooks`

---

## progress

- [x] Task 1: C shim — thread-local hook slot
- [x] Task 2: eval.c patch — call hook after expansion
- [x] Task 3: rust FFI bindings
- [x] Task 4: scheme wrappers in context.rs (REVISED — see deviation notes)
- [x] Task 5: VFS module — `(tein macro)` (REVISED — see deviation notes)
- [ ] Task 6: tests — observation mode
- [ ] Task 7: tests — transformation + edge cases
- [ ] Task 8: update documentation

**current state:** 183 existing tests pass. all infrastructure is in place. tests + docs remain.

**to resume:** `cd /home/fey/projects/tein/tein-dev/.worktrees/macro-expansion-hooks` and continue from task 6.

---

## deviation from original plan: dispatch pattern

tasks 4 and 5 were revised during implementation due to a **chibi env binding capacity issue**. defining 6+ foreign procs into a standard env causes sandbox import tests to fail (the env hash table overflows or collides, breaking `(import (scheme write))` in sandboxed contexts).

**original approach (broken):** 3 separate native fns (`set-macro-expand-hook!`, `unset-macro-expand-hook!`, `macro-expand-hook`) registered via `define_fn_variadic`, totalling 6 env bindings with the 3 existing reader protocol fns.

**actual approach (working):** single dispatch native fn `tein-macro-expand-hook-dispatch` that accepts `'set`, `'unset`, or `'get` as first arg, totalling 4 env bindings. scheme-level wrappers (`set-macro-expand-hook!` etc.) are:
- for non-sandboxed standard env: eagerly evaluated as `(define ...)` forms via `register_macro_expand_wrappers()` after sandbox setup
- for sandboxed contexts: provided by `(import (tein macro))` VFS module

**also changed:** both reader and macro protocol native fns are now registered **pre-sandbox** via `register_protocol_fns()` (free function), replacing the old `register_reader_protocol()` / `register_macro_expand_protocol()` methods that ran post-sandbox.

---

## completed tasks (reference only, do not re-execute)

### Task 1: C shim — thread-local hook slot ✓

commit `8597514` — added `tein_macro_expand_hook`, `tein_macro_expand_hook_active`, `tein_macro_expand_hook_set()`, `tein_macro_expand_hook_get()`, `tein_macro_expand_hook_clear()` to `tein/vendor/chibi-scheme/tein_shim.c`.

### Task 2: eval.c patch — call hook after expansion ✓

commit `2fdca6d` — patched `analyze_macro_once()` in `tein/vendor/chibi-scheme/eval.c`:
- added `name` parameter to function signature
- added `TEIN_THREAD_LOCAL` macro + extern declarations at top of file
- added hook call after expansion with `tein_macro_expand_hook_active` recursion guard
- changed `sexp_gc_var1`→`sexp_gc_var2` and `sexp_gc_release1`→`sexp_gc_release2`
- updated 3 call sites to pass macro name

### Task 3: rust FFI bindings ✓

commit `85b3300` — added extern declarations + safe wrappers for `tein_macro_expand_hook_set`, `tein_macro_expand_hook_get`, `tein_macro_expand_hook_clear` in `tein/src/ffi.rs`.

### Task 4: scheme wrappers in context.rs ✓ (REVISED)

commit `6c36737` — `tein/src/context.rs` changes:
- **dispatch wrapper:** `macro_expand_hook_dispatch_wrapper` (single `extern "C"` fn dispatching on symbol arg `'set`/`'unset`/`'get`)
- **pre-sandbox registration:** `register_protocol_fns()` free function registers reader + macro protocol native fns into source env before sandbox restriction (replaces old `register_reader_protocol()` + `register_macro_expand_protocol()` methods)
- **scheme wrappers:** `register_macro_expand_wrappers()` evaluates 3 `(define ...)` forms for non-sandboxed standard env contexts
- **cleanup in drop:** `ffi::macro_expand_hook_clear(self.ctx)` in `impl Drop for Context`
- **rust convenience API:** `set_macro_expand_hook()`, `unset_macro_expand_hook()`, `macro_expand_hook()` on `Context` (these use FFI directly, not the scheme dispatch)

### Task 5: VFS module — `(tein macro)` ✓ (REVISED)

commit `02ca927`:
- `tein/vendor/chibi-scheme/lib/tein/macro.sld` — library definition exporting 3 symbols
- `tein/vendor/chibi-scheme/lib/tein/macro.scm` — scheme wrappers that call `tein-macro-expand-hook-dispatch`
- `tein/build.rs` — added `macro.sld` and `macro.scm` to `VFS_FILES`

---

## remaining tasks

### Task 6: Tests — observation mode

**Files:**
- Modify: `tein/src/context.rs` (add tests at end of test module)

**Step 1: Write and verify 3 observation-mode tests**

add at end of `mod tests` in context.rs:

```rust
    #[test]
    fn test_macro_expand_hook_basic() {
        let ctx = Context::new_standard().expect("context");
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

    #[test]
    fn test_macro_expand_hook_observation() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(define captured-unexpanded #f)
             (set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (set! captured-unexpanded unexpanded)
                 expanded))"
        ).expect("set hook");
        ctx.evaluate("(double 5)").expect("use macro");
        let captured = ctx.evaluate("captured-unexpanded").expect("check");
        let list = captured.as_list().expect("should be list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[1], Value::Integer(5));
    }

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

**Step 2: Run tests**

Run: `cargo test -p tein test_macro_expand_hook -- --nocapture 2>&1 | tail -10`
Expected: 3 tests pass

**Step 3: Commit**

```bash
git add tein/src/context.rs
git commit -m "test: macro expansion hook observation mode"
```

---

### Task 7: Tests — transformation + edge cases

**Files:**
- Modify: `tein/src/context.rs` (add more tests)

**Step 1: Write all remaining tests**

```rust
    #[test]
    fn test_macro_expand_hook_transformation() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 42))"
        ).expect("set hook");
        let result = ctx.evaluate("(double 5)").expect("use macro");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_macro_expand_hook_reanalyze() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define double");
        ctx.evaluate("(define-syntax add1 (syntax-rules () ((add1 x) (+ x 1))))")
            .expect("define add1");
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
        assert_eq!(ctx.evaluate("(double 5)").unwrap(), Value::Integer(10));
    }

    #[test]
    fn test_macro_expand_hook_recursion_guard() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
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

    #[test]
    fn test_macro_expand_hook_introspection() {
        let ctx = Context::new_standard().expect("context");
        let none = ctx.evaluate("(macro-expand-hook)").expect("get");
        assert_eq!(none, Value::Boolean(false));
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env) expanded))"
        ).expect("set");
        let hook = ctx.evaluate("(macro-expand-hook)").expect("get");
        assert!(matches!(hook, Value::Procedure(_)));
        ctx.evaluate("(unset-macro-expand-hook!)").expect("unset");
        let none_again = ctx.evaluate("(macro-expand-hook)").expect("get");
        assert_eq!(none_again, Value::Boolean(false));
    }

    #[test]
    fn test_macro_expand_hook_cleanup_on_drop() {
        {
            let ctx = Context::new_standard().expect("context");
            ctx.evaluate(
                "(set-macro-expand-hook!
                   (lambda (name unexpanded expanded env) expanded))"
            ).expect("set");
        }
        let ctx2 = Context::new_standard().expect("context2");
        let hook = ctx2.evaluate("(macro-expand-hook)").expect("get");
        assert_eq!(hook, Value::Boolean(false));
    }

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

**Step 2: Run all macro expansion hook tests**

Run: `cargo test -p tein test_macro_expand_hook -- --nocapture 2>&1 | tail -20`
Expected: all 10 tests pass

**Step 3: Run full suite**

Run: `cargo test -p tein --lib 2>&1 | tail -3`
Expected: 193 pass (183 existing + 10 new)

**Step 4: Commit**

```bash
git add tein/src/context.rs
git commit -m "test: macro expansion hook transformation, edge cases, sandbox"
```

---

### Task 8: Update documentation

**Files:**
- Modify: `AGENTS.md` (update architecture section)
- Modify: `TODO.md` (check off macro expansion hooks)

**Step 1: Update AGENTS.md**

add to the architecture section after `reader dispatch flow`:

```
**macro expansion hook flow**: `ctx.set_macro_expand_hook(&proc)` or scheme `(set-macro-expand-hook! proc)` → `ffi::macro_expand_hook_set(ctx, proc)` → stores proc in thread-local `tein_macro_expand_hook` with GC preservation. when chibi's `analyze_macro_once()` expands a macro (patched eval.c D), checks hook → if set and not already active, sets `tein_macro_expand_hook_active` recursion guard → calls `sexp_apply(ctx, hook, (name unexpanded expanded env))` → hook return value replaces expanded form → `goto loop` reanalyzes (replace-and-reanalyze semantics). scheme-level API uses single-dispatch native fn `tein-macro-expand-hook-dispatch` with scheme wrappers to stay within chibi's env binding capacity. hook cleared on `Context::drop()`.
```

also update in the `vendor/chibi-scheme/` section:

```
  eval.c       — 4 patches: VFS module lookup (A + module policy gate), VFS load (B), VFS open-input-file (C), macro expansion hook (D)
```

and add:

```
  lib/tein/macro.sld — (tein macro) library definition
  lib/tein/macro.scm — scheme wrappers for macro expansion hook dispatch
```

(update the eval.c count from 3 to 4 patches)

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
  - single-dispatch pattern for env capacity, replace-and-reanalyze semantics, recursion guard, GC-safe
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
