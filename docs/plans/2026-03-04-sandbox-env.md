# sandbox fake env vars + command-line — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** allow sandboxed contexts to expose fake environment variables and a configurable command-line instead of unconditionally neutering them.

**Architecture:** two new thread-locals (`SANDBOX_ENV`, `SANDBOX_COMMAND_LINE`) following the existing `FS_POLICY` / `VFS_ALLOWLIST` pattern. `ContextBuilder` seeds defaults when sandboxed; explicit builder methods merge (env) or replace (command-line). trampolines consult the thread-locals when `IS_SANDBOXED`.

**Tech Stack:** rust, chibi-scheme FFI trampolines, thread-local state.

**Design doc:** `docs/plans/2026-03-04-sandbox-env-design.md`

**Branch:** create with `just feature sandbox-env-2603`

**Issue:** closes #99

---

### task 1: add thread-locals + Context fields + ContextBuilder fields

**files:**
- modify: `tein/src/context.rs`

**step 1: add HashMap import**

at `tein/src/context.rs:47` (the `use std::cell` line), add:

```rust
use std::collections::HashMap;
```

**step 2: add thread-locals**

after the `STUB_MODULE_MAP` thread-local block (line ~132), add:

```rust
// --- sandbox fake process environment thread-locals ---
//
// populated during sandboxed() build path: fake env vars and command-line
// for sandboxed contexts. cleared on Context::drop().
thread_local! {
    static SANDBOX_ENV: RefCell<Option<HashMap<String, String>>> = RefCell::new(None);
    static SANDBOX_COMMAND_LINE: RefCell<Option<Vec<String>>> = RefCell::new(None);
}
```

**step 3: add prev fields to Context struct**

after `prev_is_sandboxed: bool` (line ~1861), add:

```rust
    /// previous SANDBOX_ENV value, restored on drop
    prev_sandbox_env: Option<HashMap<String, String>>,
    /// previous SANDBOX_COMMAND_LINE value, restored on drop
    prev_sandbox_command_line: Option<Vec<String>>,
```

**step 4: add fields to ContextBuilder struct**

after `with_vfs_shadows: bool` (line ~1409), add:

```rust
    /// fake environment variables for sandboxed contexts.
    sandbox_env: Option<Vec<(String, String)>>,
    /// fake command-line for sandboxed contexts.
    sandbox_command_line: Option<Vec<String>>,
```

**step 5: initialise new ContextBuilder fields in Default / builder()**

find where `ContextBuilder` fields are initialised (inside `Context::builder()`) and add:

```rust
            sandbox_env: None,
            sandbox_command_line: None,
```

**step 6: add builder methods**

after the `allow_module` method, add:

```rust
    /// Inject fake environment variables for sandboxed contexts.
    ///
    /// Merges with the default seed (`TEIN_SANDBOX=true`). User entries
    /// override defaults on key conflict. Ignored for unsandboxed contexts.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, sandbox::Modules};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::builder()
    ///     .standard_env()
    ///     .sandboxed(Modules::Safe)
    ///     .environment_variables(&[("CHIBI_HASH_SALT", "42")])
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn environment_variables(mut self, vars: &[(&str, &str)]) -> Self {
        self.sandbox_env = Some(vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect());
        self
    }

    /// Set the fake command-line for sandboxed contexts.
    ///
    /// Overrides the default `["tein", "--sandbox"]` entirely.
    /// Ignored for unsandboxed contexts.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, sandbox::Modules};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::builder()
    ///     .standard_env()
    ///     .sandboxed(Modules::Safe)
    ///     .command_line(&["my-app", "--verbose"])
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn command_line(mut self, args: &[&str]) -> Self {
        self.sandbox_command_line = Some(args.iter().map(|s| s.to_string()).collect());
        self
    }
```

**step 7: commit**

```
git add tein/src/context.rs
git commit -m "feat: add SANDBOX_ENV + SANDBOX_COMMAND_LINE thread-locals and builder methods (#99)"
```

---

### task 2: wire thread-locals into build() and drop()

**files:**
- modify: `tein/src/context.rs`

**step 1: save + set thread-locals in build()**

in the `build()` method, after the existing `prev_is_sandboxed` save (line ~1629), add:

```rust
            let prev_sandbox_env = SANDBOX_ENV.with(|cell| cell.borrow().clone());
            let prev_sandbox_command_line = SANDBOX_COMMAND_LINE.with(|cell| cell.borrow().clone());
```

inside the `if let Some(ref modules) = self.sandbox_modules.take()` block, after `IS_SANDBOXED.with(|c| c.set(true))` (line ~1641), add:

```rust
                // seed fake process environment for sandboxed contexts
                {
                    let mut env_map = HashMap::new();
                    env_map.insert("TEIN_SANDBOX".to_string(), "true".to_string());
                    if let Some(user_env) = self.sandbox_env.take() {
                        for (k, v) in user_env {
                            env_map.insert(k, v);
                        }
                    }
                    SANDBOX_ENV.with(|cell| {
                        *cell.borrow_mut() = Some(env_map);
                    });

                    let cmd_line = self.sandbox_command_line.take()
                        .unwrap_or_else(|| vec!["tein".to_string(), "--sandbox".to_string()]);
                    SANDBOX_COMMAND_LINE.with(|cell| {
                        *cell.borrow_mut() = Some(cmd_line);
                    });
                }
```

**step 2: add new prev fields to Context construction**

in the `Context { ... }` struct literal (line ~1749), add after `prev_is_sandboxed`:

```rust
                prev_sandbox_env,
                prev_sandbox_command_line,
```

**step 3: restore in drop()**

in `impl Drop for Context`, after `IS_SANDBOXED.with(|c| c.set(self.prev_is_sandboxed))` (line ~2970), add:

```rust
        // restore previous fake process environment
        SANDBOX_ENV.with(|cell| {
            *cell.borrow_mut() = std::mem::take(&mut self.prev_sandbox_env);
        });
        SANDBOX_COMMAND_LINE.with(|cell| {
            *cell.borrow_mut() = std::mem::take(&mut self.prev_sandbox_command_line);
        });
```

**step 4: verify it compiles**

run: `cargo build -p tein 2>&1 | tail -5`
expected: compiles successfully (warnings ok at this stage)

**step 5: commit**

```
git add tein/src/context.rs
git commit -m "feat: wire SANDBOX_ENV + SANDBOX_COMMAND_LINE into build() and drop() (#99)"
```

---

### task 3: update trampolines to consult thread-locals

**files:**
- modify: `tein/src/context.rs`

**step 1: update get_env_var_trampoline**

replace the sandboxed branch (lines ~1208-1211):

```rust
        // sandboxed contexts get neutered env var access
        if IS_SANDBOXED.with(|c| c.get()) {
            return ffi::get_false();
        }
```

with:

```rust
        // sandboxed contexts consult the fake env map
        if IS_SANDBOXED.with(|c| c.get()) {
            return SANDBOX_ENV.with(|cell| {
                let borrow = cell.borrow();
                match borrow.as_ref().and_then(|m| m.get(name)) {
                    Some(val) => {
                        let c_val = CString::new(val.as_str()).unwrap_or_default();
                        ffi::sexp_c_str(ctx, c_val.as_ptr(), val.len() as ffi::sexp_sint_t)
                    }
                    None => ffi::get_false(),
                }
            });
        }
```

**step 2: update get_env_vars_trampoline**

replace the sandboxed branch (lines ~1232-1234):

```rust
        // sandboxed contexts get neutered env var access
        if IS_SANDBOXED.with(|c| c.get()) {
            return ffi::get_null();
        }
```

with:

```rust
        // sandboxed contexts return the fake env as an alist
        if IS_SANDBOXED.with(|c| c.get()) {
            return SANDBOX_ENV.with(|cell| {
                let borrow = cell.borrow();
                let Some(map) = borrow.as_ref() else {
                    return ffi::get_null();
                };
                let mut result = ffi::get_null();
                for (key, val) in map {
                    let _tail_root = ffi::GcRoot::new(ctx, result);
                    let c_key = CString::new(key.as_str()).unwrap_or_default();
                    let c_val = CString::new(val.as_str()).unwrap_or_default();
                    let s_key = ffi::sexp_c_str(ctx, c_key.as_ptr(), key.len() as ffi::sexp_sint_t);
                    if ffi::sexp_exceptionp(s_key) != 0 {
                        return s_key;
                    }
                    let _key_root = ffi::GcRoot::new(ctx, s_key);
                    let s_val = ffi::sexp_c_str(ctx, c_val.as_ptr(), val.len() as ffi::sexp_sint_t);
                    if ffi::sexp_exceptionp(s_val) != 0 {
                        return s_val;
                    }
                    let _val_root = ffi::GcRoot::new(ctx, s_val);
                    let pair = ffi::sexp_cons(ctx, s_key, s_val);
                    if ffi::sexp_exceptionp(pair) != 0 {
                        return pair;
                    }
                    let _pair_root = ffi::GcRoot::new(ctx, pair);
                    result = ffi::sexp_cons(ctx, pair, result);
                    if ffi::sexp_exceptionp(result) != 0 {
                        return result;
                    }
                }
                result
            });
        }
```

**step 3: update command_line_trampoline**

replace the sandboxed branch (lines ~1276-1285):

```rust
        // sandboxed contexts get a fake command line
        if IS_SANDBOXED.with(|c| c.get()) {
            let name = CString::new("tein").unwrap();
            let s = ffi::sexp_c_str(ctx, name.as_ptr(), 4);
            if ffi::sexp_exceptionp(s) != 0 {
                return s;
            }
            let _s_root = ffi::GcRoot::new(ctx, s);
            return ffi::sexp_cons(ctx, s, ffi::get_null());
        }
```

with:

```rust
        // sandboxed contexts consult the fake command-line
        if IS_SANDBOXED.with(|c| c.get()) {
            return SANDBOX_COMMAND_LINE.with(|cell| {
                let borrow = cell.borrow();
                let args = match borrow.as_ref() {
                    Some(a) => a.clone(),
                    None => vec!["tein".to_string(), "--sandbox".to_string()],
                };
                let mut result = ffi::get_null();
                for arg in args.iter().rev() {
                    let c_arg = CString::new(arg.as_str()).unwrap_or_default();
                    let s_arg = ffi::sexp_c_str(ctx, c_arg.as_ptr(), arg.len() as ffi::sexp_sint_t);
                    if ffi::sexp_exceptionp(s_arg) != 0 {
                        return s_arg;
                    }
                    let _arg_root = ffi::GcRoot::new(ctx, s_arg);
                    let _tail_root = ffi::GcRoot::new(ctx, result);
                    result = ffi::sexp_cons(ctx, s_arg, result);
                    if ffi::sexp_exceptionp(result) != 0 {
                        return result;
                    }
                }
                result
            });
        }
```

**step 4: update docstrings** on all three trampolines to reflect the new behaviour.

**step 5: verify it compiles**

run: `cargo build -p tein 2>&1 | tail -5`

**step 6: commit**

```
git add tein/src/context.rs
git commit -m "feat: trampolines consult SANDBOX_ENV + SANDBOX_COMMAND_LINE (#99)"
```

---

### task 4: update existing tests + add new tests

**files:**
- modify: `tein/src/context.rs` (test section)

**step 1: update test_tein_process_safe_in_sandbox**

the existing test (line ~4248) asserts:
- `get-environment-variable "HOME"` → `#f`
- `get-environment-variables` → `Nil`
- `command-line` → `["tein"]`

update to:
- `get-environment-variable "TEIN_SANDBOX"` → `"true"`
- `get-environment-variable "HOME"` → `#f` (not in fake env)
- `get-environment-variables` → non-empty (contains at least `TEIN_SANDBOX`)
- `command-line` → `["tein", "--sandbox"]`

```rust
    #[test]
    fn test_tein_process_safe_in_sandbox() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .step_limit(5_000_000)
            .build()
            .expect("sandboxed context");
        // (tein process) is in the safe set — trampolines neuter env/argv in sandbox
        let r = ctx.evaluate("(import (tein process))");
        assert!(
            r.is_ok(),
            "(tein process) should be importable in sandbox: {r:?}"
        );
        // default fake env seed
        let r = ctx.evaluate("(get-environment-variable \"TEIN_SANDBOX\")");
        assert_eq!(r.unwrap(), Value::String("true".to_string()));
        // vars not in fake env still return #f
        let r = ctx.evaluate("(get-environment-variable \"HOME\")");
        assert_eq!(r.unwrap(), Value::Boolean(false));
        // env var list contains the seed
        let r = ctx.evaluate("(pair? (get-environment-variables))");
        assert_eq!(r.unwrap(), Value::Boolean(true), "should have fake env vars");
        // command-line returns default fake
        let r = ctx.evaluate("(command-line)");
        assert_eq!(
            r.unwrap(),
            Value::List(vec![
                Value::String("tein".into()),
                Value::String("--sandbox".into()),
            ])
        );
    }
```

**step 2: update test_srfi_98_shadow_neuters_env_vars_in_sandbox**

the existing test (line ~7776) asserts env vars neutered to `#f` / `Nil`.

update to check fake env works through srfi/98 shadow:

```rust
    #[test]
    fn test_srfi_98_shadow_uses_fake_env_in_sandbox() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("builder");
        // srfi/98 re-exports (tein process) — fake env should work
        let r = ctx
            .evaluate("(import (scheme base) (srfi 98)) (get-environment-variable \"TEIN_SANDBOX\")")
            .expect("srfi/98 importable in sandbox");
        assert_eq!(
            r,
            Value::String("true".to_string()),
            "get-environment-variable returns fake env value"
        );
        // unknown var still #f
        let r = ctx
            .evaluate("(get-environment-variable \"HOME\")")
            .expect("get-environment-variable");
        assert_eq!(
            r,
            Value::Boolean(false),
            "unknown var returns #f"
        );
        // alist non-empty
        let r = ctx
            .evaluate("(pair? (get-environment-variables))")
            .expect("get-environment-variables");
        assert_eq!(
            r,
            Value::Boolean(true),
            "get-environment-variables returns non-empty alist"
        );
    }
```

**step 3: add test for custom env vars**

```rust
    #[test]
    fn test_sandbox_custom_environment_variables() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .environment_variables(&[("CHIBI_HASH_SALT", "42"), ("MY_VAR", "hello")])
            .step_limit(5_000_000)
            .build()
            .expect("sandboxed context with custom env");
        ctx.evaluate("(import (tein process))").unwrap();
        // custom var present
        let r = ctx.evaluate("(get-environment-variable \"CHIBI_HASH_SALT\")");
        assert_eq!(r.unwrap(), Value::String("42".to_string()));
        let r = ctx.evaluate("(get-environment-variable \"MY_VAR\")");
        assert_eq!(r.unwrap(), Value::String("hello".to_string()));
        // default seed still present (merge, not replace)
        let r = ctx.evaluate("(get-environment-variable \"TEIN_SANDBOX\")");
        assert_eq!(r.unwrap(), Value::String("true".to_string()));
    }
```

**step 4: add test for custom command-line**

```rust
    #[test]
    fn test_sandbox_custom_command_line() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .command_line(&["my-app", "--verbose"])
            .step_limit(5_000_000)
            .build()
            .expect("sandboxed context with custom command-line");
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(command-line)");
        assert_eq!(
            r.unwrap(),
            Value::List(vec![
                Value::String("my-app".into()),
                Value::String("--verbose".into()),
            ])
        );
    }
```

**step 5: add test that unsandboxed ignores fake env**

```rust
    #[test]
    fn test_unsandboxed_ignores_environment_variables() {
        unsafe { std::env::set_var("TEIN_TEST_UNSANDBOXED", "real") };
        let ctx = Context::builder()
            .standard_env()
            .environment_variables(&[("TEIN_TEST_UNSANDBOXED", "fake")])
            .build()
            .expect("unsandboxed context");
        ctx.evaluate("(import (tein process))").unwrap();
        // unsandboxed: reads real env, not fake
        let r = ctx.evaluate("(get-environment-variable \"TEIN_TEST_UNSANDBOXED\")");
        assert_eq!(r.unwrap(), Value::String("real".to_string()));
        unsafe { std::env::remove_var("TEIN_TEST_UNSANDBOXED") };
    }
```

**step 6: add test for env var override of default seed**

```rust
    #[test]
    fn test_sandbox_env_override_default_seed() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .environment_variables(&[("TEIN_SANDBOX", "custom")])
            .step_limit(5_000_000)
            .build()
            .expect("sandboxed context");
        ctx.evaluate("(import (tein process))").unwrap();
        // user override wins
        let r = ctx.evaluate("(get-environment-variable \"TEIN_SANDBOX\")");
        assert_eq!(r.unwrap(), Value::String("custom".to_string()));
    }
```

**step 7: run tests**

run: `cargo test -p tein -- tein_process sandbox_custom sandbox_env unsandboxed_ignores srfi_98_shadow --nocapture 2>&1 | tail -20`

expected: all pass

**step 8: commit**

```
git add tein/src/context.rs
git commit -m "test: sandbox fake env vars + command-line (#99)"
```

---

### task 5: lint + update docs

**files:**
- modify: `tein/src/context.rs` (docstrings only — already done inline in prior tasks)
- modify: `docs/reference.md` (if it documents sandbox behaviour)
- modify: `docs/sandboxing.md` (if it exists)

**step 1: run lint**

run: `just lint`
expected: passes

**step 2: check if sandboxing docs need updates**

grep for `neutered` or `command-line` or `environment-variable` in `docs/`. update any mention of "always returns #f" / "always returns '()" to describe the new fake env behaviour and the builder methods.

**step 3: commit**

```
git add -A
git commit -m "docs: update sandbox docs for fake env vars + command-line (#99)"
```

---

### task 6: update AGENTS.md + collect notes

**files:**
- modify: `AGENTS.md`

**step 1: update sandboxing flow** in AGENTS.md to mention `SANDBOX_ENV` / `SANDBOX_COMMAND_LINE` thread-locals and the default seeds. add to the `sandboxing flow` section something like:

> `SANDBOX_ENV` seeded with `{"TEIN_SANDBOX": "true"}`, `SANDBOX_COMMAND_LINE` seeded with `["tein", "--sandbox"]`. builder `.environment_variables()` merges with seed, `.command_line()` replaces it. both restored on drop. unsandboxed contexts ignore both.

**step 2: run full test suite**

run: `just test`
expected: all tests pass

**step 3: commit**

```
git add AGENTS.md
git commit -m "docs: update AGENTS.md with sandbox env/command-line notes (#99)

closes #99"
```

---

### task 7: final review + PR

**step 1: run lint one final time**

run: `just lint`

**step 2: use superpowers:finishing-a-development-branch**
