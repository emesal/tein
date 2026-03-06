# Dynamic Module Registration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** enable runtime module registration from both rust and scheme, so LLMs can create importable tools inside sandboxed environments.

**Architecture:** three layers — (1) internal `allow_module_runtime` for live allowlist mutation, (2) public `Context::register_module` that parses `define-library`, registers into VFS, updates allowlist, (3) `(tein modules)` scheme API gated behind `allow_dynamic_modules()`. a new C function `tein_vfs_lookup_static` enables collision detection against built-in modules only.

**Tech Stack:** rust, chibi-scheme C FFI, tein VFS

**Branch:** `just feature dynamic-module-registration-2603`

---

### Task 1: add `tein_vfs_lookup_static` to chibi fork

the collision check needs to distinguish static (built-in) from dynamic (runtime) VFS entries. add a C function that only searches the compile-time table.

**Files:**
- Modify: `~/forks/chibi-scheme/tein_shim.c` (after `tein_vfs_lookup`, ~line 387)

**Step 1: add the function**

after the existing `tein_vfs_lookup` function (line 387), add:

```c
// look up content in static VFS table only (skips dynamic entries).
// used by rust to detect collisions with built-in modules.
const char* tein_vfs_lookup_static(const char *full_path, unsigned int *out_length) {
    for (int i = 0; tein_vfs_table[i].key != NULL; i++) {
        if (strcmp(tein_vfs_table[i].key, full_path) == 0) {
            if (out_length) *out_length = tein_vfs_table[i].length;
            return tein_vfs_table[i].content;
        }
    }
    return NULL;
}
```

**Step 2: push to chibi fork**

```bash
cd ~/forks/chibi-scheme
git add tein_shim.c
git commit -m "feat: add tein_vfs_lookup_static for collision detection (#132)"
git push
```

**Step 3: rebuild tein to pull the change**

```bash
cd ~/projects/tein
just clean && cargo build -p tein 2>&1 | tail -5
```

Expected: build succeeds (the function is compiled but not yet called from rust).

**Step 4: commit**

nothing to commit in tein yet — the change is in the chibi fork.

---

### Task 2: expose `tein_vfs_lookup_static` in rust FFI

**Files:**
- Modify: `tein/src/ffi.rs:248` (extern block, near `tein_vfs_lookup`)
- Modify: `tein/src/ffi.rs:679` (safe wrapper, near `vfs_lookup`)

**Step 1: add extern declaration**

in the extern block (after `tein_vfs_lookup` declaration at line 253), add:

```rust
    /// look up a VFS path in the static (compile-time) table only.
    ///
    /// skips dynamic entries — used for collision detection in register_module.
    /// returns null if the path is not in the static VFS.
    pub fn tein_vfs_lookup_static(full_path: *const c_char, out_length: *mut c_uint)
        -> *const c_char;
```

**Step 2: add safe wrapper**

after the `vfs_lookup` wrapper (~line 689), add:

```rust
/// Check if a path exists in the static (compile-time) VFS table.
///
/// Returns `true` if the path is a built-in module. Does NOT check
/// dynamic (runtime-registered) entries. Used by `register_module`
/// for collision detection.
#[inline]
pub unsafe fn vfs_static_exists(path: &std::ffi::CStr) -> bool {
    unsafe {
        let ptr = tein_vfs_lookup_static(path.as_ptr(), std::ptr::null_mut());
        !ptr.is_null()
    }
}
```

**Step 3: verify it compiles**

```bash
cargo build -p tein 2>&1 | tail -5
```

Expected: build succeeds.

**Step 4: commit**

```bash
git add tein/src/ffi.rs
git commit -m "feat(ffi): expose tein_vfs_lookup_static for collision detection (#132)"
```

---

### Task 3: add `allow_module_runtime` (layer 1)

internal method on `Context` that appends to the live `VFS_ALLOWLIST` thread-local.

**Files:**
- Modify: `tein/src/context.rs` (after `register_vfs_module`, ~line 3029)
- Test: `tein/src/context.rs` (test module, after existing VFS gate tests ~line 5950)

**Step 1: write failing test**

add in the test module (after `test_vfs_gate_not_set_without_sandboxed`, ~line 5960):

```rust
    #[test]
    fn test_allow_module_runtime_appends_to_allowlist() {
        use crate::sandbox::{Modules, VFS_ALLOWLIST};

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .build()
            .expect("sandboxed context");

        // verify "my/tool" is not in the allowlist
        let before = VFS_ALLOWLIST.with(|cell| cell.borrow().contains(&"my/tool".to_string()));
        assert!(!before, "my/tool should not be in allowlist initially");

        ctx.allow_module_runtime("my/tool");

        let after = VFS_ALLOWLIST.with(|cell| cell.borrow().contains(&"my/tool".to_string()));
        assert!(after, "my/tool should be in allowlist after allow_module_runtime");
    }
```

**Step 2: run test — expect FAIL**

```bash
cargo test -p tein --lib test_allow_module_runtime_appends_to_allowlist -- --nocapture 2>&1 | tail -10
```

Expected: compilation error — `allow_module_runtime` not defined.

**Step 3: implement**

add after `register_vfs_module` (~line 3029):

```rust
    /// Append a module path to the live VFS allowlist.
    ///
    /// Only meaningful in sandboxed contexts (where `VFS_GATE` is `GATE_CHECK`).
    /// In unsandboxed contexts this is a no-op — the gate never checks the list.
    ///
    /// Used by `register_module` to make dynamically registered modules importable.
    pub(crate) fn allow_module_runtime(&self, path: &str) {
        use crate::sandbox::VFS_ALLOWLIST;
        VFS_ALLOWLIST.with(|cell| {
            let mut list = cell.borrow_mut();
            if !list.iter().any(|p| p == path) {
                list.push(path.to_string());
            }
        });
    }
```

**Step 4: run test — expect PASS**

```bash
cargo test -p tein --lib test_allow_module_runtime_appends_to_allowlist -- --nocapture 2>&1 | tail -10
```

Expected: PASS.

**Step 5: commit**

```bash
git add tein/src/context.rs
git commit -m "feat(context): add allow_module_runtime for live allowlist mutation (#132)"
```

---

### Task 4: implement `Context::register_module` (layer 2)

the main public API. parses `define-library`, validates, registers VFS entry, updates allowlist.

**Files:**
- Modify: `tein/src/context.rs` (after `allow_module_runtime`)
- Test: `tein/src/context.rs` (test module)

**Step 1: write failing tests**

add in the test module:

```rust
    #[test]
    fn test_register_module_basic() {
        let ctx = Context::new_standard().expect("standard context");
        ctx.register_module(
            "(define-library (my tool) (import (scheme base)) (export greet) (begin (define (greet x) (string-append \"hi \" x))))",
        )
        .expect("register_module");

        let result = ctx.evaluate("(import (my tool)) (greet \"world\")").expect("eval");
        assert_eq!(result, Value::String("hi world".into()));
    }

    #[test]
    fn test_register_module_collision_with_builtin() {
        let ctx = Context::new_standard().expect("standard context");
        let err = ctx
            .register_module(
                "(define-library (scheme base) (import (scheme base)) (export +) (begin))",
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("already exists"),
            "should reject collision with builtin: {msg}"
        );
    }

    #[test]
    fn test_register_module_rejects_include() {
        let ctx = Context::new_standard().expect("standard context");
        let err = ctx
            .register_module(
                "(define-library (my mod) (import (scheme base)) (export x) (include \"foo.scm\"))",
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("include"),
            "should reject (include ...): {msg}"
        );
    }

    #[test]
    fn test_register_module_not_define_library() {
        let ctx = Context::new_standard().expect("standard context");
        let err = ctx.register_module("(+ 1 2)").unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("define-library"),
            "should reject non-define-library: {msg}"
        );
    }

    #[test]
    fn test_register_module_dynamic_update() {
        let ctx = Context::new_standard().expect("standard context");
        ctx.register_module(
            "(define-library (my versioned) (import (scheme base)) (export val) (begin (define val 1)))",
        )
        .expect("first registration");

        let v1 = ctx.evaluate("(import (my versioned)) val").expect("eval v1");
        assert_eq!(v1, Value::Integer(1));

        // re-register (update) — VFS entry is shadowed, but chibi caches the module
        ctx.register_module(
            "(define-library (my versioned) (import (scheme base)) (export val) (begin (define val 2)))",
        )
        .expect("second registration should succeed (dynamic-over-dynamic)");
        // NOTE: chibi's module cache means the import still returns v1
    }

    #[test]
    fn test_register_module_sandboxed_importable() {
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .build()
            .expect("sandboxed context");

        ctx.register_module(
            "(define-library (my sandboxed-tool) (import (scheme base)) (export answer) (begin (define answer 42)))",
        )
        .expect("register in sandboxed context");

        let result = ctx
            .evaluate("(import (my sandboxed-tool)) answer")
            .expect("import dynamically registered module in sandbox");
        assert_eq!(result, Value::Integer(42));
    }
```

**Step 2: run tests — expect FAIL**

```bash
cargo test -p tein --lib test_register_module_basic -- --nocapture 2>&1 | tail -10
```

Expected: compilation error — `register_module` not defined.

**Step 3: implement `register_module`**

add after `allow_module_runtime`:

```rust
    /// Register a scheme module from a `define-library` source string.
    ///
    /// Parses the library name, validates the form, registers the source into
    /// the dynamic VFS, and (if sandboxed) appends to the live import allowlist.
    ///
    /// The source must use `(begin ...)` for all definitions — `(include ...)`,
    /// `(include-ci ...)`, and `(include-library-declarations ...)` are not
    /// supported and will return an error.
    ///
    /// # collision detection
    ///
    /// Rejects registration if the module already exists in the static
    /// (compile-time) VFS — prevents shadowing built-in modules like
    /// `scheme/base` or `tein/json`. Dynamic-over-dynamic shadowing is
    /// allowed (update semantics for re-registration).
    ///
    /// # chibi module caching
    ///
    /// Chibi caches module environments after first `(import ...)`. Re-registering
    /// a module's VFS entry does NOT invalidate the cache. A subsequent import in
    /// the same context returns the old version. Use a fresh context (or
    /// `ManagedContext::reset()`) for updated imports.
    ///
    /// # errors
    ///
    /// - `Error::EvalError` if source is not a valid `define-library` form
    /// - `Error::EvalError` if library name is empty
    /// - `Error::EvalError` if module collides with a built-in VFS entry
    /// - `Error::EvalError` if source contains `(include ...)` or similar
    /// - `Error::EvalError` if VFS registration fails (OOM)
    ///
    /// # examples
    ///
    /// ```
    /// # use tein::{Context, Value};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new_standard()?;
    /// ctx.register_module(r#"
    ///     (define-library (my tool)
    ///       (import (scheme base))
    ///       (export greet)
    ///       (begin (define (greet x) (string-append "hi " x))))
    /// "#)?;
    /// let result = ctx.evaluate("(import (my tool)) (greet \"world\")")?;
    /// assert_eq!(result, Value::String("hi world".into()));
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_module(&self, source: &str) -> Result<()> {
        // step 1: sexp_read the source to get the define-library form
        let (lib_name_parts, has_forbidden_include) = unsafe {
            let scheme_str = ffi::sexp_c_str(
                self.ctx,
                source.as_ptr() as *const c_char,
                source.len() as ffi::sexp_sint_t,
            );
            if ffi::sexp_exceptionp(scheme_str) != 0 {
                return Err(Error::EvalError(
                    "register_module: failed to create scheme string".into(),
                ));
            }
            let _str_root = ffi::GcRoot::new(self.ctx, scheme_str);

            let port = ffi::sexp_open_input_string(self.ctx, scheme_str);
            if ffi::sexp_exceptionp(port) != 0 {
                return Err(Error::EvalError(
                    "register_module: failed to open input port".into(),
                ));
            }
            let _port_root = ffi::GcRoot::new(self.ctx, port);

            let form = ffi::sexp_read(self.ctx, port);
            if ffi::sexp_exceptionp(form) != 0 || ffi::sexp_eofp(form) != 0 {
                return Err(Error::EvalError(
                    "register_module: source is not a valid s-expression".into(),
                ));
            }
            let _form_root = ffi::GcRoot::new(self.ctx, form);

            // step 2: validate it's (define-library (name ...) ...)
            if ffi::sexp_pairp(form) == 0 {
                return Err(Error::EvalError(
                    "register_module: expected (define-library ...) form".into(),
                ));
            }

            let head = ffi::sexp_car(form);
            if ffi::sexp_symbolp(head) == 0 {
                return Err(Error::EvalError(
                    "register_module: expected (define-library ...) form".into(),
                ));
            }

            // check the symbol is "define-library"
            let head_str = ffi::sexp_symbol_to_string(self.ctx, head);
            let head_ptr = ffi::sexp_string_data(head_str);
            let head_len = ffi::sexp_string_size(head_str) as usize;
            let head_name =
                std::str::from_utf8(std::slice::from_raw_parts(head_ptr as *const u8, head_len))
                    .unwrap_or("");
            if head_name != "define-library" {
                return Err(Error::EvalError(
                    "register_module: expected (define-library ...) form".into(),
                ));
            }

            // extract library name list: second element of the form
            let rest = ffi::sexp_cdr(form);
            if ffi::sexp_pairp(rest) == 0 {
                return Err(Error::EvalError(
                    "register_module: define-library has no library name".into(),
                ));
            }
            let name_list = ffi::sexp_car(rest);
            if ffi::sexp_pairp(name_list) == 0 {
                return Err(Error::EvalError(
                    "register_module: library name must be a list of symbols".into(),
                ));
            }

            // walk the name list to extract parts
            let mut parts = Vec::new();
            let mut cursor = name_list;
            while ffi::sexp_pairp(cursor) != 0 {
                let elem = ffi::sexp_car(cursor);
                if ffi::sexp_symbolp(elem) != 0 {
                    let s = ffi::sexp_symbol_to_string(self.ctx, elem);
                    let ptr = ffi::sexp_string_data(s);
                    let len = ffi::sexp_string_size(s) as usize;
                    let slice = std::slice::from_raw_parts(ptr as *const u8, len);
                    parts.push(String::from_utf8_lossy(slice).into_owned());
                } else if ffi::sexp_integerp(elem) != 0 {
                    let n = ffi::sexp_unbox_fixnum(elem);
                    parts.push(n.to_string());
                } else {
                    return Err(Error::EvalError(
                        "register_module: library name elements must be symbols or integers".into(),
                    ));
                }
                cursor = ffi::sexp_cdr(cursor);
            }

            // check for forbidden include forms in the library body
            let mut has_include = false;
            let mut body = ffi::sexp_cdr(rest); // skip name list
            while ffi::sexp_pairp(body) != 0 {
                let clause = ffi::sexp_car(body);
                if ffi::sexp_pairp(clause) != 0 {
                    let clause_head = ffi::sexp_car(clause);
                    if ffi::sexp_symbolp(clause_head) != 0 {
                        let s = ffi::sexp_symbol_to_string(self.ctx, clause_head);
                        let ptr = ffi::sexp_string_data(s);
                        let len = ffi::sexp_string_size(s) as usize;
                        let sym =
                            std::str::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len))
                                .unwrap_or("");
                        if sym == "include" || sym == "include-ci"
                            || sym == "include-library-declarations"
                        {
                            has_include = true;
                            break;
                        }
                    }
                }
                body = ffi::sexp_cdr(body);
            }

            (parts, has_include)
        };

        // step 3: validate name is non-empty
        if lib_name_parts.is_empty() {
            return Err(Error::EvalError(
                "register_module: library name is empty".into(),
            ));
        }

        // step 4: reject include
        if has_forbidden_include {
            return Err(Error::EvalError(
                "register_module: (include ...) is not supported in dynamically registered modules; use (begin ...) instead".into(),
            ));
        }

        // step 5: derive VFS path
        let module_path = lib_name_parts.join("/");
        let vfs_sld_path = format!("/vfs/lib/{module_path}.sld");

        // step 6: collision check — reject if in static VFS
        let c_vfs_path = CString::new(vfs_sld_path.as_str())
            .map_err(|_| Error::EvalError("register_module: path contains null bytes".into()))?;
        let collision = unsafe { ffi::vfs_static_exists(&c_vfs_path) };
        if collision {
            return Err(Error::EvalError(format!(
                "register_module: module '{module_path}' already exists as a built-in module"
            )));
        }

        // step 7: register into dynamic VFS
        self.register_vfs_module(
            &format!("lib/{module_path}.sld"),
            source,
        )?;

        // step 8: update live allowlist
        self.allow_module_runtime(&module_path);

        Ok(())
    }
```

**Step 4: run tests — expect PASS**

```bash
cargo test -p tein --lib test_register_module -- --nocapture 2>&1 | tail -20
```

Expected: all 6 tests pass.

**Step 5: run full test suite**

```bash
just test 2>&1 | tail -10
```

Expected: no regressions.

**Step 6: commit**

```bash
git add tein/src/context.rs
git commit -m "feat(context): add register_module for dynamic module registration (#132)"
```

---

### Task 5: add `allow_dynamic_modules()` to `ContextBuilder`

sugar method that adds `"tein/modules"` to the sandbox allowlist.

**Files:**
- Modify: `tein/src/context.rs` (ContextBuilder methods, after `allow_module` ~line 1937)
- Test: `tein/src/context.rs` (test module)

**Step 1: write failing test**

```rust
    #[test]
    fn test_allow_dynamic_modules_builder() {
        // verify the builder method doesn't panic and produces a valid context
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_dynamic_modules()
            .build()
            .expect("sandboxed + allow_dynamic_modules");

        // (tein modules) should be importable
        let result = ctx.evaluate("(import (tein modules)) #t");
        assert!(
            result.is_ok(),
            "(tein modules) should be importable with allow_dynamic_modules: {:?}",
            result.unwrap_err()
        );
    }
```

**Step 2: run test — expect FAIL**

```bash
cargo test -p tein --lib test_allow_dynamic_modules_builder -- --nocapture 2>&1 | tail -10
```

Expected: compilation error — method not defined.

**Step 3: implement**

add after `allow_module` (~line 1937):

```rust
    /// Enable dynamic module registration from Scheme code.
    ///
    /// Makes `(tein modules)` importable in sandboxed contexts, providing
    /// `register-module` and `module-registered?` to Scheme code.
    ///
    /// Without this, `(tein modules)` is blocked by the VFS gate in sandboxed
    /// contexts. Unsandboxed contexts can always import it.
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
    ///     .allow_dynamic_modules()
    ///     .build()?;
    /// ctx.evaluate("(import (tein modules)) #t")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn allow_dynamic_modules(self) -> Self {
        self.allow_module("tein/modules")
    }
```

**Step 4: run test — will still fail** because `(tein modules)` doesn't exist yet. that's expected — this test will pass after task 6. verify compilation succeeds:

```bash
cargo build -p tein 2>&1 | tail -5
```

Expected: compiles.

**Step 5: commit**

```bash
git add tein/src/context.rs
git commit -m "feat(builder): add allow_dynamic_modules() convenience method (#132)"
```

---

### Task 6: implement `(tein modules)` scheme module (layer 3)

register `register-module` and `module-registered?` trampolines + VFS registry entry + build.rs exports.

**Files:**
- Modify: `tein/src/vfs_registry.rs` (add VfsEntry after other tein/ Dynamic entries, ~line 173)
- Modify: `tein/build.rs:315` (add to DYNAMIC_MODULE_EXPORTS)
- Modify: `tein/src/context.rs` (add trampolines + registration fn)

**Step 1: add VFS registry entry**

in `vfs_registry.rs`, after the `tein/safe-regexp` entry (~line 173), add:

```rust
    VfsEntry {
        path: "tein/modules",
        deps: &["scheme/base"],
        files: &[],
        clib: None,
        default_safe: false,
        source: VfsSource::Dynamic,
        feature: None,
        shadow_sld: None,
    },
```

**Step 2: add to DYNAMIC_MODULE_EXPORTS in build.rs**

in `build.rs`, add to the `DYNAMIC_MODULE_EXPORTS` array (after `tein/crypto`, ~line 359):

```rust
    // src/context.rs — register_modules_module() trampolines
    ("tein/modules", &["register-module", "module-registered?"]),
```

**Step 3: add trampolines and registration function**

in `context.rs`, add the trampolines (before the `// --- (tein process) trampolines ---` section, ~line 1492):

```rust
// --- (tein modules) trampolines ---

/// `register-module` trampoline: registers a define-library source string
/// as a new importable module.
///
/// calls `Context::register_module` through the thread-local `REGISTER_MODULE_CTX`.
/// returns `#t` on success, raises a scheme error on failure.
unsafe extern "C" fn register_module_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let source = match extract_string_arg(ctx, args, "register-module") {
            Ok(s) => s,
            Err(e) => return e,
        };

        // we need to call register_module which requires &Context, but we only
        // have the raw ctx pointer. reconstruct the operation inline.
        // step 1: sexp_read + parse (reuse the same parsing logic)
        let scheme_str = ffi::sexp_c_str(
            ctx,
            source.as_ptr() as *const c_char,
            source.len() as ffi::sexp_sint_t,
        );
        if ffi::sexp_exceptionp(scheme_str) != 0 {
            let msg = "register-module: failed to create scheme string";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let _str_root = ffi::GcRoot::new(ctx, scheme_str);

        let port = ffi::sexp_open_input_string(ctx, scheme_str);
        if ffi::sexp_exceptionp(port) != 0 {
            let msg = "register-module: failed to open input port";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let _port_root = ffi::GcRoot::new(ctx, port);

        let form = ffi::sexp_read(ctx, port);
        if ffi::sexp_exceptionp(form) != 0 || ffi::sexp_eofp(form) != 0 {
            let msg = "register-module: source is not a valid s-expression";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let _form_root = ffi::GcRoot::new(ctx, form);

        // validate define-library
        if ffi::sexp_pairp(form) == 0 {
            let msg = "register-module: expected (define-library ...) form";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        let head = ffi::sexp_car(form);
        if ffi::sexp_symbolp(head) == 0 {
            let msg = "register-module: expected (define-library ...) form";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        let head_s = ffi::sexp_symbol_to_string(ctx, head);
        let head_ptr = ffi::sexp_string_data(head_s);
        let head_len = ffi::sexp_string_size(head_s) as usize;
        let head_name =
            std::str::from_utf8(std::slice::from_raw_parts(head_ptr as *const u8, head_len))
                .unwrap_or("");
        if head_name != "define-library" {
            let msg = "register-module: expected (define-library ...) form";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        let rest = ffi::sexp_cdr(form);
        if ffi::sexp_pairp(rest) == 0 {
            let msg = "register-module: define-library has no library name";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        let name_list = ffi::sexp_car(rest);
        let path_result = spec_to_path(ctx, name_list);
        let module_path = match path_result {
            Ok(p) => p,
            Err(e) => return e,
        };

        if module_path.is_empty() {
            let msg = "register-module: library name is empty";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        // check for include forms
        let mut body = ffi::sexp_cdr(rest);
        while ffi::sexp_pairp(body) != 0 {
            let clause = ffi::sexp_car(body);
            if ffi::sexp_pairp(clause) != 0 {
                let clause_head = ffi::sexp_car(clause);
                if ffi::sexp_symbolp(clause_head) != 0 {
                    let s = ffi::sexp_symbol_to_string(ctx, clause_head);
                    let ptr = ffi::sexp_string_data(s);
                    let len = ffi::sexp_string_size(s) as usize;
                    let sym =
                        std::str::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len))
                            .unwrap_or("");
                    if sym == "include" || sym == "include-ci"
                        || sym == "include-library-declarations"
                    {
                        let msg = "register-module: (include ...) is not supported in dynamically registered modules; use (begin ...) instead";
                        let c_msg = CString::new(msg).unwrap_or_default();
                        return ffi::make_error(
                            ctx,
                            c_msg.as_ptr(),
                            msg.len() as ffi::sexp_sint_t,
                        );
                    }
                }
            }
            body = ffi::sexp_cdr(body);
        }

        // collision check
        let vfs_sld_path = format!("/vfs/lib/{module_path}.sld");
        let c_vfs_path = match CString::new(vfs_sld_path.as_str()) {
            Ok(p) => p,
            Err(_) => {
                let msg = "register-module: path contains null bytes";
                let c_msg = CString::new(msg).unwrap_or_default();
                return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };
        if ffi::vfs_static_exists(&c_vfs_path) {
            let msg = format!(
                "register-module: module '{}' already exists as a built-in module",
                module_path
            );
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        // register VFS entry
        let vfs_rel_path = format!("lib/{module_path}.sld");
        let full_vfs_path = format!("/vfs/{vfs_rel_path}");
        let c_full_path = match CString::new(full_vfs_path.as_str()) {
            Ok(p) => p,
            Err(_) => {
                let msg = "register-module: path contains null bytes";
                let c_msg = CString::new(msg).unwrap_or_default();
                return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };
        let rc = ffi::tein_vfs_register(
            c_full_path.as_ptr(),
            source.as_ptr() as *const c_char,
            source.len() as std::ffi::c_uint,
        );
        if rc != 0 {
            let msg = "register-module: VFS registration failed (out of memory)";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        // update allowlist
        crate::sandbox::VFS_ALLOWLIST.with(|cell| {
            let mut list = cell.borrow_mut();
            if !list.iter().any(|p| p == &module_path) {
                list.push(module_path);
            }
        });

        ffi::get_true()
    }
}

/// `module-registered?` trampoline: checks if a module exists in the VFS.
///
/// takes a quoted list like `'(my tool)`, converts to path, checks VFS lookup.
unsafe extern "C" fn module_registered_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        if ffi::sexp_nullp(args) != 0 {
            let msg = "module-registered?: expected 1 argument, got 0";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        let spec = ffi::sexp_car(args);
        if ffi::sexp_pairp(spec) == 0 {
            let msg = "module-registered?: argument must be a list, e.g. '(my module)";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        let module_path = match spec_to_path(ctx, spec) {
            Ok(p) => p,
            Err(e) => return e,
        };

        let vfs_sld_path = format!("/vfs/lib/{module_path}.sld");
        let c_path = match CString::new(vfs_sld_path.as_str()) {
            Ok(p) => p,
            Err(_) => return ffi::get_false(),
        };

        if ffi::vfs_lookup(&c_path).is_some() {
            ffi::get_true()
        } else {
            ffi::get_false()
        }
    }
}
```

**Step 4: add registration function**

add with the other `register_*_module` functions (~after `register_process_module` at line 3525):

```rust
    /// Register `register-module` and `module-registered?` native functions.
    ///
    /// These form the `(tein modules)` scheme API for dynamic module registration.
    /// Called during `build()` for standard-env contexts.
    fn register_modules_module(&self) -> Result<()> {
        self.define_fn_variadic("register-module", register_module_trampoline)?;
        self.define_fn_variadic("module-registered?", module_registered_trampoline)?;
        Ok(())
    }
```

**Step 5: register `.sld` and call registration in `build()`**

in `build()`, after `context.register_process_module()?;` (~line 2263), add:

```rust
                context.register_modules_module()?;
                context.register_vfs_module(
                    "lib/tein/modules.sld",
                    "(define-library (tein modules) (import (chibi)) (export register-module module-registered?))",
                )?;
```

**Step 6: run tests**

```bash
cargo test -p tein --lib test_register_module -- --nocapture 2>&1 | tail -20
cargo test -p tein --lib test_allow_dynamic_modules_builder -- --nocapture 2>&1 | tail -10
```

Expected: all pass.

**Step 7: commit**

```bash
git add tein/src/context.rs tein/src/vfs_registry.rs tein/build.rs
git commit -m "feat: add (tein modules) scheme API for dynamic module registration (#132)"
```

---

### Task 7: scheme-level integration tests

test the full flow from scheme code: `(register-module ...)` then `(import ...)`.

**Files:**
- Create: `tein/tests/scheme/register_module.scm`
- Modify: `tein/tests/scheme_tests.rs` (add test runner)

**Step 1: check existing scheme test pattern**

look at `tein/tests/scheme_tests.rs` and an existing `.scm` test file to follow the pattern.

**Step 2: create scheme test file**

```scheme
;; tests for (tein modules) dynamic module registration

(import (tein modules))
(import (scheme base))
(import (scheme write))

;; register a module
(register-module
  "(define-library (test greeter)
     (import (scheme base))
     (export greet)
     (begin (define (greet x) (string-append \"hello \" x))))")

;; verify registered
(display (if (module-registered? '(test greeter)) "PASS" "FAIL"))
(display " module-registered? after register\n")

;; import and use
(import (test greeter))
(display (if (equal? (greet "world") "hello world") "PASS" "FAIL"))
(display " greet returned correct value\n")

;; module-registered? for non-existent module
(display (if (not (module-registered? '(nonexistent module))) "PASS" "FAIL"))
(display " module-registered? for nonexistent\n")
```

**Step 3: add integration test runner**

follow the existing pattern in `scheme_tests.rs`. add a test that builds a sandboxed context with `allow_dynamic_modules()`, evaluates the scheme test file, and checks output contains "PASS" and no "FAIL".

**Step 4: run test**

```bash
cargo test -p tein --test scheme_tests register_module -- --nocapture 2>&1 | tail -15
```

Expected: PASS.

**Step 5: commit**

```bash
git add tein/tests/scheme/register_module.scm tein/tests/scheme_tests.rs
git commit -m "test(scheme): integration tests for dynamic module registration (#132)"
```

---

### Task 8: lint, docs, and AGENTS.md update

**Files:**
- Modify: `AGENTS.md` (add notes about `register_module` and `(tein modules)`)
- Modify: `docs/plans/2026-03-06-dynamic-module-registration-design.md` (update issue number)

**Step 1: lint**

```bash
just lint
```

fix any issues.

**Step 2: run full test suite**

```bash
just test
```

Expected: all pass, no regressions.

**Step 3: update AGENTS.md**

add to the architecture section (under the relevant flow):

```
**dynamic module registration flow**: `ctx.register_module(source)` → sexp_read to parse define-library → extract library name → collision check via `tein_vfs_lookup_static` (rejects built-in modules) → reject `(include ...)` → register source as `/vfs/lib/<path>.sld` via `tein_vfs_register` → append to live `VFS_ALLOWLIST`. scheme-side: `(tein modules)` exports `register-module` (trampoline) and `module-registered?`. gated in sandbox via `.allow_dynamic_modules()` (= `.allow_module("tein/modules")`). chibi caches modules after first import — re-registration does not invalidate.
```

add to critical gotchas:

```
**`(tein modules)` is `default_safe: false`**: must use `.allow_dynamic_modules()` to make it available in sandboxed contexts. without it, the VFS gate blocks `(import (tein modules))`.

**chibi module cache vs dynamic re-registration**: `register_module` updates the VFS entry but chibi caches module environments after first `(import ...)`. a second `(import (my tool))` in the same context returns the cached (old) version. fresh context or `ManagedContext::reset()` required for updated imports.

**`register_module` collision check**: rejects if module `.sld` exists in the *static* VFS table (built-in modules). dynamic-over-dynamic is allowed (update semantics). collision check uses `tein_vfs_lookup_static` which skips the dynamic linked list.

**`register_module` requires `(begin ...)`**: `(include ...)`, `(include-ci ...)`, and `(include-library-declarations ...)` are rejected. dynamically registered modules must be self-contained.
```

**Step 4: update design doc**

update the issue reference in the design doc from "(to be filed)" to "#132".

**Step 5: commit**

```bash
git add -A
git commit -m "docs: update AGENTS.md and design doc for dynamic module registration (#132)"
```

---

### Task 9: final verification and lint

**Step 1: full test suite**

```bash
just test
```

Expected: all tests pass.

**Step 2: lint**

```bash
just lint
```

Expected: clean.

**Step 3: halt for review**

stop here. the branch is ready for code review before PR.

---

## notes for AGENTS.md collection (final step)

- `tein_vfs_lookup_static` added to chibi fork `tein_shim.c` — must be documented in safety invariants
- `(tein modules)` is `VfsSource::Dynamic`, `default_safe: false` — add to VFS registry docs
- `register_module` only supports `(begin ...)`, not `(include ...)`
- chibi module cache is not invalidated by re-registration
- `allow_dynamic_modules()` = `allow_module("tein/modules")`

## DRY concern: parsing duplication

the `define-library` parsing logic is duplicated between `Context::register_module` (rust) and `register_module_trampoline` (scheme trampoline). this is intentional — the rust method returns `Result<()>` while the trampoline returns `ffi::sexp` error objects. extracting a shared core would require a helper that returns both forms, adding complexity for two callsites. if a third callsite appears, extract then.

alternatively, the scheme trampoline could call `register_module` through a thread-local `&Context` pointer (similar to `FOREIGN_STORE_PTR`). this would DRY the logic but add another thread-local and lifetime concern. revisit if the duplication becomes a maintenance burden.
