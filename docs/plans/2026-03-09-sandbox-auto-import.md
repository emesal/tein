# sandbox auto-import implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** sandboxed contexts (except `Modules::None`) auto-import `(scheme base)` and `(scheme write)` during `build()`, so basic scheme forms are available without manual imports.

**Architecture:** after the null env is constructed and the VFS gate is armed in `Context::build()`, evaluate `(import (scheme base) (scheme write))` at the raw FFI level. skip for `Modules::None` (which is the "build your own allowlist" entry point). update tests and docs to reflect the new baseline.

**Tech Stack:** rust, chibi-scheme FFI (`sexp_evaluate`, `sexp_read`, `sexp_open_input_string`)

---

### Task 1: auto-import in sandbox build path

**Files:**
- Modify: `tein/src/context.rs:2483` (after `sexp_context_env_set(ctx, null_env)`)

**Step 1: add the auto-import block**

after line 2483 (`ffi::sexp_context_env_set(ctx, null_env);`), before the closing `}` of the sandbox block (line 2484), insert:

```rust
                // auto-import scheme/base + scheme/write so sandboxed contexts
                // start with a usable baseline. skipped for Modules::None (the
                // "build your own allowlist" entry point — users combine it with
                // allow_module() for precise control).
                if !matches!(modules, Modules::None) {
                    let import_code = "(import (scheme base) (scheme write))";
                    let c_import = CString::new(import_code).unwrap();
                    let import_str = ffi::sexp_c_str(
                        ctx,
                        c_import.as_ptr(),
                        import_code.len() as ffi::sexp_sint_t,
                    );
                    let import_port = ffi::sexp_open_input_string(ctx, import_str);
                    let _import_str_guard = ffi::GcRoot::new(ctx, import_str);
                    let _import_port_guard = ffi::GcRoot::new(ctx, import_port);
                    let expr = ffi::sexp_read(ctx, import_port);
                    let _expr_guard = ffi::GcRoot::new(ctx, expr);
                    let result = ffi::sexp_evaluate(ctx, expr, null_env);
                    if ffi::sexp_exceptionp(result) != 0 {
                        let msg = Value::from_raw(ctx, result)
                            .unwrap_or_else(|e| Value::String(format!("{e}")));
                        ffi::sexp_destroy_context(ctx);
                        return Err(crate::error::Error::InitError(
                            format!(
                                "sandbox auto-import of scheme/base + scheme/write failed: {msg}"
                            ),
                        ));
                    }
                }
```

note: `modules` is already available as the `&Modules` ref from the `if let Some(ref modules)` on line 2367. `null_env` is GC-rooted by `_null_env_guard`. `CString` is already imported in this scope.

**Step 2: run the full test suite**

Run: `just test`
Expected: all existing tests pass. many sandbox tests do redundant `(import (scheme base))` — this is harmless (chibi caches modules).

**Step 3: commit**

```
feat(sandbox): auto-import scheme/base + scheme/write in sandboxed contexts

sandboxed contexts (except Modules::None) now start with scheme/base and
scheme/write pre-imported, so basic forms like let, display, +, map etc.
are available immediately. previously, the null env only had core syntax
+ import, making sandboxed contexts unusable without an explicit import.

Modules::None is unchanged — it remains the "build your own allowlist"
entry point for use with allow_module().
```

---

### Task 2: add tests for the new behaviour

**Files:**
- Modify: `tein/src/context.rs` (test module, after existing sandbox tests ~line 9740)

**Step 1: write test — Safe has base+write without explicit import**

```rust
    #[test]
    fn test_sandbox_auto_import_safe_has_base_and_write() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("build");
        // scheme/base: let, +, map are available without explicit import
        let result = ctx
            .evaluate("(let ((x (+ 1 2))) x)")
            .expect("let + should work");
        assert_eq!(result, Value::Integer(3));
        // scheme/write: display is available without explicit import
        let result = ctx
            .evaluate("(begin (display \"\") #t)")
            .expect("display should work");
        assert_eq!(result, Value::Boolean(true));
    }
```

**Step 2: write test — All has base+write without explicit import**

```rust
    #[test]
    fn test_sandbox_auto_import_all_has_base_and_write() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::All)
            .build()
            .expect("build");
        let result = ctx
            .evaluate("(+ 40 2)")
            .expect("+ should work in All");
        assert_eq!(result, Value::Integer(42));
        let result = ctx
            .evaluate("(begin (display \"\") #t)")
            .expect("display should work in All");
        assert_eq!(result, Value::Boolean(true));
    }
```

**Step 3: write test — Only with base+write has them auto-imported**

```rust
    #[test]
    fn test_sandbox_auto_import_only_with_base_and_write() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base", "scheme/write"]))
            .build()
            .expect("build");
        let result = ctx
            .evaluate("(let ((x 42)) x)")
            .expect("let should work");
        assert_eq!(result, Value::Integer(42));
    }
```

**Step 4: write test — None does NOT auto-import**

```rust
    #[test]
    fn test_sandbox_auto_import_none_skips() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::None)
            .build()
            .expect("build");
        // + should still be stubbed in Modules::None
        let err = ctx.evaluate("(+ 1 2)").expect_err("should fail");
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "expected SandboxViolation, got: {err:?}"
        );
    }
```

**Step 5: write test — Only without scheme/write still gets base**

```rust
    #[test]
    fn test_sandbox_auto_import_only_base_without_write() {
        // scheme/write not in allowlist — auto-import fails for write,
        // but scheme/base should still be attempted. the combined import
        // form fails, so this should return an InitError.
        use crate::sandbox::Modules;
        let result = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .build();
        // the combined import (scheme base) (scheme write) will fail
        // because scheme/write is not in the allowlist
        assert!(
            result.is_err(),
            "build should fail when scheme/write is not in allowlist"
        );
    }
```

wait — this test reveals a design question. the auto-import is `(import (scheme base) (scheme write))` as one expression. if scheme/write is not in the allowlist but scheme/base is (e.g. `Modules::only(&["scheme/base"])`), the import fails entirely. we need to decide: should we do two separate imports, or require that both are in the allowlist?

**resolution**: do two separate imports. `scheme/base` is the fundamental one; `scheme/write` is "nice to have". if the allowlist includes base but not write, auto-import base and silently skip write.

**Step 5 (revised): update the implementation from Task 1**

replace the single import with two separate ones:

```rust
                if !matches!(modules, Modules::None) {
                    // auto-import scheme/base — fundamental for all real scheme work.
                    // auto-import scheme/write — display/write/newline baseline.
                    // each import is separate so scheme/write can be skipped if not
                    // in the allowlist (e.g. Modules::only(&["scheme/base"])).
                    for import in &[
                        "(import (scheme base))",
                        "(import (scheme write))",
                    ] {
                        let c_import = CString::new(*import).unwrap();
                        let import_str = ffi::sexp_c_str(
                            ctx,
                            c_import.as_ptr(),
                            import.len() as ffi::sexp_sint_t,
                        );
                        let import_port = ffi::sexp_open_input_string(ctx, import_str);
                        let _import_str_guard = ffi::GcRoot::new(ctx, import_str);
                        let _import_port_guard = ffi::GcRoot::new(ctx, import_port);
                        let expr = ffi::sexp_read(ctx, import_port);
                        let _expr_guard = ffi::GcRoot::new(ctx, expr);
                        let result = ffi::sexp_evaluate(ctx, expr, null_env);
                        if ffi::sexp_exceptionp(result) != 0 {
                            // scheme/write failure is non-fatal (allowlist might exclude it)
                            if *import == "(import (scheme base))" {
                                let msg = Value::from_raw(ctx, result)
                                    .unwrap_or_else(|e| Value::String(format!("{e}")));
                                ffi::sexp_destroy_context(ctx);
                                return Err(crate::error::Error::InitError(
                                    format!("sandbox auto-import failed: {msg}"),
                                ));
                            }
                            // scheme/write: silently skip if not in allowlist
                        }
                    }
                }
```

**Step 6 (revised): rewrite the Only-without-write test**

```rust
    #[test]
    fn test_sandbox_auto_import_only_base_without_write() {
        // scheme/write not in allowlist — auto-import skips it silently,
        // but scheme/base still works
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .build()
            .expect("build should succeed even without scheme/write");
        let result = ctx
            .evaluate("(+ 1 2)")
            .expect("base should work");
        assert_eq!(result, Value::Integer(3));
        // display should fail — scheme/write was not imported
        let err = ctx
            .evaluate("(display 42)")
            .expect_err("display should fail without scheme/write");
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "expected SandboxViolation, got: {err:?}"
        );
    }
```

**Step 7: run tests**

Run: `just test`
Expected: all tests pass.

**Step 8: commit**

```
test(sandbox): add tests for auto-import behaviour
```

---

### Task 3: update sandbox example

**Files:**
- Modify: `tein/examples/sandbox.rs`

**Step 1: remove redundant `(import (scheme base))` calls**

the `Modules::Safe` and `Modules::Only` sections no longer need explicit `(import (scheme base))`. update the example to demonstrate that bindings are available immediately, and add a comment explaining the auto-import.

in the `Modules::Safe` section (lines 31-34), change:
```rust
    println!("==> (import (scheme base)) (+ 1 2) in Modules::Safe");
    ctx.evaluate("(import (scheme base))")?;
    let result = ctx.evaluate("(+ 1 2)")?;
```
to:
```rust
    // scheme/base and scheme/write are auto-imported in sandboxed contexts
    println!("==> (+ 1 2) in Modules::Safe — scheme/base auto-imported");
    let result = ctx.evaluate("(+ 1 2)")?;
```

in the `Modules::Only` section (lines 48-51), change:
```rust
    println!("\n==> (import (scheme base)) (define (sq x) (* x x)) (sq 7) in Modules::only");
    ctx.evaluate("(import (scheme base))")?;
    let result = ctx.evaluate("(define (sq x) (* x x)) (sq 7)")?;
```
to:
```rust
    println!("\n==> (define (sq x) (* x x)) (sq 7) in Modules::only — base auto-imported");
    let result = ctx.evaluate("(define (sq x) (* x x)) (sq 7)")?;
```

in the step limit + Safe section (lines 79), change:
```rust
    ctx.evaluate("(import (scheme base))")?;
```
to (remove the line — base is already imported).

in the file IO section (lines 140-141), change:
```rust
    ctx.evaluate("(import (scheme base))")?;
    ctx.evaluate("(import (scheme read))")?;
```
to:
```rust
    ctx.evaluate("(import (scheme read))")?;
```

same for the file_write section (line 163): remove `ctx.evaluate("(import (scheme base))")?;`.

**Step 2: run the example**

Run: `cargo run --example sandbox`
Expected: all outputs are correct, no errors.

**Step 3: commit**

```
docs(examples): update sandbox example for auto-import
```

---

### Task 4: update documentation

**Files:**
- Modify: `docs/sandboxing.md`
- Modify: `tein/AGENTS.md`

**Step 1: update sandboxing.md**

at line 24, replace:
```
`.sandboxed(modules)` activates the module sandbox. It builds a null environment containing only `import` syntax, arms the VFS gate to enforce an allowlist, and registers UX stubs for all excluded module exports.
```
with:
```
`.sandboxed(modules)` activates the module sandbox. It builds a null environment, arms the VFS gate to enforce an allowlist, registers UX stubs for all excluded module exports, and auto-imports `(scheme base)` and `(scheme write)` so the context starts with a usable baseline. `Modules::None` skips the auto-import — it is the "build your own allowlist" entry point for use with `allow_module()`.
```

at line 56 (after `### Modules::Safe`), add a note to the description:

after "Included in Safe:" (line 60), add before the list:
```

Sandboxed contexts (except `Modules::None`) auto-import `(scheme base)` and `(scheme write)` during `build()`. These modules are available immediately without an explicit `(import ...)`.

```

at line 83-85 (`### Modules::None`), update:
```
Syntax only. The `import` form is available (so Scheme code can attempt imports), but the VFS gate rejects every module. UX stubs are registered for all known module exports.
```
to:
```
Syntax only — the "build your own allowlist" entry point. The `import` form is available, but the VFS gate rejects every module. UX stubs are registered for all known module exports. Unlike other `Modules` variants, `Modules::None` does **not** auto-import `scheme/base` or `scheme/write`. Combine with `allow_module()` for precise control — transitive deps are resolved automatically.
```

**Step 2: update AGENTS.md**

add a new entry to the `## critical gotchas` section:

```
**sandbox auto-import**: sandboxed contexts (except `Modules::None`) auto-import `(scheme base)` and `(scheme write)` during `build()`. `scheme/base` failure is fatal (`InitError`); `scheme/write` failure is silently skipped (the allowlist might exclude it). `Modules::None` skips both — it's the "build your own allowlist" entry point. the auto-import happens after the VFS gate is armed, so it goes through normal module resolution.
```

**Step 3: commit**

```
docs: document sandbox auto-import of scheme/base and scheme/write
```

---

### Task 5: verify tein-bin sandbox works

**Files:** (no changes — verification only)

**Step 1: test REPL mode**

Run: `echo '(+ 1 2)' | cargo run -p tein-bin -- --sandbox`
Expected: outputs `3` (no `undefined variable` error, no `can't set non-parameter` warning)

Run: `echo '(display "hello") (newline)' | cargo run -p tein-bin -- --sandbox`
Expected: outputs `hello`

Run: `echo '(let ((x 42)) x)' | cargo run -p tein-bin -- --sandbox`
Expected: outputs `42`

**Step 2: test script mode**

Run: `echo '(display (+ 1 2)) (newline)' > /tmp/test_sandbox.scm && cargo run -p tein-bin -- --sandbox /tmp/test_sandbox.scm`
Expected: outputs `3`

**Step 3: run the full test suite one more time**

Run: `just test`
Expected: all tests pass.

**Step 4: commit the design doc (if not already committed)**

```
docs(plans): add sandbox auto-import design and implementation plan
```

---

### Task 6: collect AGENTS.md notes

review the implementation for any new gotchas or patterns that should be documented. the Task 4 AGENTS.md update should already cover the key point. verify no other notes needed.
