# (tein introspect) Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `(tein introspect)`, an environment introspection API that lets LLM agents discover available modules, inspect exports, query procedure arity, and dump structured environment overviews — all from within scheme.

**Architecture:** Layered hybrid — rust trampolines for module-level data (VFS_ALLOWLIST, MODULE_EXPORTS), C shims in tein_shim.c for env-level introspection (env chain walking, lambda formals, opcode arity), scheme for composition and formatting. Module is `VfsSource::Embedded`, `default_safe: true`.

**Tech Stack:** Rust (trampolines + FFI wrappers), C (chibi internals via tein_shim.c), R7RS Scheme (composition layer)

**Spec:** `docs/plans/2026-03-12-tein-introspect-design.md`

**Branch:** `feature/tein-introspect-2603` (already created and active)

**Closes:** #27, #83

---

## Progress

- **Chunk 1 (tasks 1–5): COMPLETE** — all C shims committed to chibi fork, FFI wrappers in ffi.rs
- **Task 6 (available-modules): COMPLETE** — trampoline + VFS entry + scheme skeleton + tests pass
- **Tasks 7–17: PENDING** — next session starts at task 7 (module-exports)

## Implementation Notes for Continuity

### Critical: trampoline registration pattern
The plan originally used `define_fn_variadic` for trampoline registration, but this puts trampolines in the **top-level env** which is invisible to library bodies (chibi creates fresh envs per library). The fix: use `register_native_trampoline(ctx, prim_env, ...)` to register into the **primitive env** BEFORE `load_standard_env`. This puts them into `*chibi-env*`, visible via `(import (chibi))`. The `.sld` already imports `(chibi)`.

- `register_native_trampoline` was made `pub(crate)` for `introspect.rs` to use
- `register_introspect_trampolines(ctx, prim_env)` takes raw sexp pointers, not `&Context`
- call site is in `build()` at line ~2265, alongside other prim_env registrations
- **ALL remaining trampolines (tasks 7–10) must follow this same pattern** — add to `register_introspect_trampolines`, NOT use `define_fn_variadic`

### VFS registry entries
- `tein/introspect` and `tein/introspect/docs` both added to `vfs_registry.rs`
- `tein/introspect` deps: `["scheme/base", "scheme/write", "scheme/eval"]`
- `tein/introspect/docs` deps: `["scheme/base"]`

### `spec_to_path` (task 7)
- exists at `context.rs:1224` as `unsafe fn spec_to_path`
- needs `pub(crate)` visibility for `introspect.rs` to import it
- plan's note about this is correct

### Imports in introspect.rs
- uses `crate::sandbox::{VFS_ALLOWLIST, registry_all_allowlist}` (NOT the `vfs_registry` module directly — it's `include!`'d into `sandbox.rs`)
- `VFS_REGISTRY` and `feature_enabled` are private to sandbox; use `registry_all_allowlist()` instead

### Chibi fork state
- branch: `emesal-tein`
- 2 commits pushed: C shims + scheme skeleton (introspect.sld, introspect.scm, introspect/docs.sld, introspect/docs.scm)
- scheme files are minimal stubs — will be extended as trampolines are added

---

## Chunk 1: C Shims and FFI Wrappers

### Task 1: Add C shim — `tein_procedure_arity` ✓

**Files:**
- Modify: `~/forks/chibi-scheme/tein_shim.c` (append after line ~648)

- [x] **Step 1: Write the C shim**

Add to `~/forks/chibi-scheme/tein_shim.c`:

```c
/* (tein introspect) — procedure arity.
 * returns cons(min, max) where max is SEXP_FALSE if variadic.
 * returns SEXP_FALSE for non-procedures. */
sexp tein_procedure_arity(sexp ctx, sexp proc) {
    sexp_sint_t num_args;
    int variadic;
    if (sexp_procedurep(proc)) {
        num_args = sexp_unbox_fixnum(sexp_procedure_num_args(proc));
        variadic = sexp_procedure_variadic_p(proc);
    } else if (sexp_opcodep(proc)) {
        num_args = sexp_opcode_num_args(proc);
        variadic = sexp_opcode_variadic_p(proc);
    } else {
        return SEXP_FALSE;
    }
    return sexp_cons(ctx,
                     sexp_make_fixnum(num_args),
                     variadic ? SEXP_FALSE : sexp_make_fixnum(num_args));
}
```

- [x] **Step 2: Push the chibi fork changes**

```bash
cd ~/forks/chibi-scheme
git add tein_shim.c
git commit -m "feat: add tein_procedure_arity shim for (tein introspect)"
git push
```

- [x] **Step 3: Rebuild tein to pull the fork change**

```bash
cd ~/projects/tein
cargo build -p tein 2>&1 | tail -5
```

Expected: build succeeds (the shim compiles but isn't called yet)

- [x] **Step 4: Commit**

Nothing to commit in tein yet — the fork change is upstream.

---

### Task 2: Add C shim — `tein_binding_kind` ✓

**Files:**
- Modify: `~/forks/chibi-scheme/tein_shim.c`

- [x] **Step 1: Write the C shim**

Append to `~/forks/chibi-scheme/tein_shim.c`:

```c
/* (tein introspect) — classify a binding value.
 * returns an interned symbol: procedure, syntax, or variable. */
sexp tein_binding_kind(sexp ctx, sexp value) {
    if (sexp_procedurep(value) || sexp_opcodep(value)) {
        return sexp_intern(ctx, "procedure", -1);
    } else if (sexp_syntacticp(value)) {
        return sexp_intern(ctx, "syntax", -1);
    } else {
        return sexp_intern(ctx, "variable", -1);
    }
}
```

- [x] **Step 2: Push the chibi fork**

```bash
cd ~/forks/chibi-scheme
git add tein_shim.c
git commit -m "feat: add tein_binding_kind shim for (tein introspect)"
git push
```

- [x] **Step 3: Rebuild tein**

```bash
cd ~/projects/tein && cargo build -p tein 2>&1 | tail -5
```

---

### Task 3: Add C shim — `tein_env_bindings_list` ✓

**Files:**
- Modify: `~/forks/chibi-scheme/tein_shim.c`

- [x] **Step 1: Write the C shim**

Append to `~/forks/chibi-scheme/tein_shim.c`:

```c
/* (tein introspect) — collect all bindings from env chain.
 * walks sexp_env_bindings + parent chain. returns alist of (name . kind).
 * if prefix is non-SEXP_FALSE, filters by string prefix on symbol name.
 * deduplicates: innermost binding wins (seen list tracks visited symbols). */
sexp tein_env_bindings_list(sexp ctx, sexp prefix) {
    sexp_gc_var6(result, seen, cell, kind_sym, sym_str, prefix_root);
    sexp_gc_preserve6(ctx, result, seen, cell, kind_sym, sym_str, prefix_root);
    prefix_root = prefix; /* root the prefix arg against GC */

    result = SEXP_NULL;
    seen = SEXP_NULL;

    const char *prefix_str = NULL;
    sexp_uint_t prefix_len = 0;
    if (sexp_stringp(prefix_root)) {
        prefix_str = sexp_string_data(prefix_root);
        prefix_len = sexp_string_size(prefix_root);
    }

    sexp env = sexp_context_env(ctx);
    while (sexp_envp(env)) {
        sexp bindings = sexp_env_bindings(env);
        while (sexp_pairp(bindings)) {
            cell = sexp_car(bindings);
            sexp name = sexp_car(cell);
            sexp value = sexp_cdr(cell);

            /* skip if already seen (innermost wins) */
            if (sexp_memq(ctx, name, seen) != SEXP_FALSE) {
                bindings = sexp_cdr(bindings);
                continue;
            }

            /* prefix filter */
            if (prefix_str) {
                /* sexp_symbol_to_string may allocate — consume result
                 * via sexp_string_data before next allocating call */
                sym_str = sexp_symbol_to_string(ctx, name);
                const char *sym_data = sexp_string_data(sym_str);
                sexp_uint_t sym_len = sexp_string_size(sym_str);
                if (sym_len < prefix_len ||
                    memcmp(sym_data, prefix_str, prefix_len) != 0) {
                    bindings = sexp_cdr(bindings);
                    continue;
                }
            }

            /* classify */
            kind_sym = tein_binding_kind(ctx, value);

            /* prepend (name . kind) to result, name to seen */
            result = sexp_cons(ctx, sexp_cons(ctx, name, kind_sym), result);
            seen = sexp_cons(ctx, name, seen);

            bindings = sexp_cdr(bindings);
        }
        env = sexp_env_parent(env);
    }

    sexp_gc_release6(ctx);
    return result;
}
```

- [x] **Step 2: Push the chibi fork**

```bash
cd ~/forks/chibi-scheme
git add tein_shim.c
git commit -m "feat: add tein_env_bindings_list shim for (tein introspect)"
git push
```

- [x] **Step 3: Rebuild tein**

```bash
cd ~/projects/tein && cargo build -p tein 2>&1 | tail -5
```

---

### Task 4: Add C shim — `tein_imported_modules_list` ✓

**Files:**
- Modify: `~/forks/chibi-scheme/tein_shim.c`

- [x] **Step 1: Write the C shim**

Append to `~/forks/chibi-scheme/tein_shim.c`:

```c
/* (tein introspect) — list loaded modules.
 * walks *modules* alist in meta env, returns names where module-env is non-#f.
 * caller (rust wrapper) handles sandbox filtering. */
sexp tein_imported_modules_list(sexp ctx) {
    sexp_gc_var3(result, modules_sym, modules_alist);
    sexp_gc_preserve3(ctx, result, modules_sym, modules_alist);

    result = SEXP_NULL;

    sexp meta_env = sexp_global(ctx, SEXP_G_META_ENV);
    modules_sym = sexp_intern(ctx, "*modules*", -1);
    modules_alist = sexp_env_ref(ctx, meta_env, modules_sym, SEXP_FALSE);

    if (sexp_pairp(modules_alist)) {
        sexp ls = modules_alist;
        while (sexp_pairp(ls)) {
            sexp entry = sexp_car(ls);
            /* entry is (name . module-vector) */
            if (sexp_pairp(entry)) {
                sexp mod_vec = sexp_cdr(entry);
                /* module-env is vector-ref 1 */
                if (sexp_vectorp(mod_vec) &&
                    sexp_vector_length(mod_vec) > 1 &&
                    sexp_vector_ref(mod_vec, SEXP_ONE) != SEXP_FALSE) {
                    sexp name = sexp_car(entry);
                    result = sexp_cons(ctx, name, result);
                }
            }
            ls = sexp_cdr(ls);
        }
    }

    sexp_gc_release3(ctx);
    return result;
}
```

- [x] **Step 2: Push the chibi fork**

```bash
cd ~/forks/chibi-scheme
git add tein_shim.c
git commit -m "feat: add tein_imported_modules_list shim for (tein introspect)"
git push
```

- [x] **Step 3: Rebuild tein**

```bash
cd ~/projects/tein && cargo build -p tein 2>&1 | tail -5
```

---

### Task 5: Add FFI extern declarations and safe wrappers ✓

**Files:**
- Modify: `tein/src/ffi.rs` (add extern declarations inside `extern "C"` block near line 270, add safe wrappers after existing ones near line 850)

- [x] **Step 1: Add extern declarations**

Add inside the `extern "C"` block in `src/ffi.rs` (before the closing `}`), near the other `tein_*` declarations:

```rust
    // introspection shims (tein_shim.c)
    /// returns cons(min, max) where max is SEXP_FALSE if variadic.
    /// returns SEXP_FALSE for non-procedures.
    pub fn tein_procedure_arity(ctx: sexp, proc: sexp) -> sexp;
    /// returns an interned kind symbol: procedure, syntax, or variable.
    pub fn tein_binding_kind(ctx: sexp, value: sexp) -> sexp;
    /// returns alist of (name-symbol . kind-symbol) for all bindings in env chain.
    /// prefix is a scheme string for filtering, or SEXP_FALSE for no filter.
    pub fn tein_env_bindings_list(ctx: sexp, prefix: sexp) -> sexp;
    /// returns list of module name lists for loaded modules from meta env *modules*.
    pub fn tein_imported_modules_list(ctx: sexp) -> sexp;
```

- [x] **Step 2: Add safe wrappers**

Add after the existing safe wrappers in `src/ffi.rs` (after the `vfs_lookup` wrapper area):

```rust
/// safe wrapper for `tein_procedure_arity`.
pub(crate) fn procedure_arity(ctx: sexp, proc: sexp) -> sexp {
    unsafe { tein_procedure_arity(ctx, proc) }
}

/// safe wrapper for `tein_binding_kind`.
pub(crate) fn binding_kind(ctx: sexp, value: sexp) -> sexp {
    unsafe { tein_binding_kind(ctx, value) }
}

/// safe wrapper for `tein_env_bindings_list`.
pub(crate) fn env_bindings_list(ctx: sexp, prefix: sexp) -> sexp {
    unsafe { tein_env_bindings_list(ctx, prefix) }
}

/// safe wrapper for `tein_imported_modules_list`.
pub(crate) fn imported_modules_list(ctx: sexp) -> sexp {
    unsafe { tein_imported_modules_list(ctx) }
}
```

- [x] **Step 3: Verify it compiles**

```bash
cd ~/projects/tein && cargo build -p tein 2>&1 | tail -5
```

Expected: build succeeds (wrappers aren't called yet but link against the shims)

- [x] **Step 4: Commit**

```bash
git add tein/src/ffi.rs
git commit -m "feat: FFI declarations and safe wrappers for introspection shims"
```

- [x] **Step 5: Lint**

```bash
just lint
```

---

## Chunk 2: Rust Trampolines

### Task 6: Create `src/introspect.rs` with `available_modules` trampoline ✓

**Files:**
- Create: `tein/src/introspect.rs`
- Modify: `tein/src/lib.rs` (add `mod introspect;`)

- [x] **Step 1: Write the test**

Add to `tein/src/context.rs` at the bottom with other tests (inside `#[cfg(test)] mod tests`):

```rust
#[test]
fn test_available_modules_unsandboxed() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    let r = ctx.evaluate("(available-modules)").unwrap();
    // unsandboxed: returns all registry modules; must include scheme/base
    if let Value::List(modules) = &r {
        let has_scheme_base = modules.iter().any(|m| {
            if let Value::List(parts) = m {
                parts.len() == 2
                    && parts[0] == Value::Symbol("scheme".into())
                    && parts[1] == Value::Symbol("base".into())
            } else {
                false
            }
        });
        assert!(has_scheme_base, "should include (scheme base), got: {:?}", r);
    } else {
        panic!("expected list, got: {:?}", r);
    }
}

#[test]
fn test_available_modules_sandboxed() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Only(vec!["scheme/base".into(), "tein/introspect".into()]))
        .build()
        .unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    let r = ctx.evaluate("(length (available-modules))").unwrap();
    // Only scheme/base, tein/introspect, and their resolved deps
    if let Value::Integer(n) = r {
        assert!(n >= 2, "should have at least 2 modules, got {}", n);
    } else {
        panic!("expected integer, got: {:?}", r);
    }
}
```

- [x] **Step 2: Run the test to verify it fails**

```bash
cargo test -p tein test_available_modules -- --nocapture 2>&1 | tail -10
```

Expected: FAIL (module doesn't exist yet)

- [x] **Step 3: Create `src/introspect.rs` with the trampoline**

Create `tein/src/introspect.rs`:

```rust
//! `(tein introspect)` — environment introspection for LLM agents.
//!
//! provides runtime discovery of available modules, module exports,
//! procedure arity, and environment bindings. designed for LLM agents
//! that need to understand their sandbox from within scheme.

use std::ffi::CString;

use crate::ffi;
use crate::sandbox::VFS_ALLOWLIST;
use crate::vfs_registry::{VFS_REGISTRY, feature_enabled};

/// `available-modules` trampoline: returns list of importable module paths.
///
/// sandboxed contexts return modules in VFS_ALLOWLIST.
/// unsandboxed contexts return all VFS_REGISTRY paths.
/// each module path is a proper list: `(scheme base)`, `(tein json)`, etc.
unsafe extern "C" fn available_modules_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let paths: Vec<String> = VFS_ALLOWLIST.with(|cell| {
            let list = cell.borrow();
            if list.is_empty() {
                // unsandboxed: return all registry paths
                VFS_REGISTRY
                    .iter()
                    .filter(|e| feature_enabled(e.feature))
                    .map(|e| e.path.to_string())
                    .collect()
            } else {
                list.clone()
            }
        });

        // filter out /docs sub-libraries — they're implementation details
        let paths: Vec<&str> = paths
            .iter()
            .map(|s| s.as_str())
            .filter(|p| !p.contains("/docs"))
            .collect();

        build_module_path_list(ctx, &paths)
    }
}

/// convert a slash-separated path to a scheme list of symbols.
/// e.g. "scheme/base" → (scheme base)
unsafe fn path_to_module_list(ctx: ffi::sexp, path: &str) -> ffi::sexp {
    unsafe {
        let parts: Vec<&str> = path.split('/').collect();
        let mut result = ffi::sexp_null();
        for part in parts.iter().rev() {
            let c_part = CString::new(*part).unwrap_or_default();
            let sym = ffi::sexp_intern(ctx, c_part.as_ptr(), part.len() as ffi::sexp_sint_t);
            result = ffi::sexp_cons(ctx, sym, result);
        }
        result
    }
}

/// build a scheme list of module path lists from slash-separated path strings.
unsafe fn build_module_path_list(ctx: ffi::sexp, paths: &[&str]) -> ffi::sexp {
    unsafe {
        let mut result = ffi::sexp_null();
        for path in paths.iter().rev() {
            let module_list = path_to_module_list(ctx, path);
            result = ffi::sexp_cons(ctx, module_list, result);
        }
        result
    }
}

/// register all (tein introspect) trampolines.
///
/// called from `Context::build()` after other trampoline registrations.
/// uses `define_fn_variadic` (not primitive env) since these don't need
/// to override chibi builtins — they're new names.
pub(crate) fn register_introspect_trampolines(ctx: &crate::Context) -> crate::Result<()> {
    ctx.define_fn_variadic(
        "tein-available-modules-internal",
        available_modules_trampoline,
    )?;
    Ok(())
}
```

- [x] **Step 4: Add `mod introspect;` to `lib.rs`**

Add `mod introspect;` to `tein/src/lib.rs` alongside the other module declarations.

- [x] **Step 5: Register the trampoline in `build()`**

In `tein/src/context.rs`, inside the `if self.standard_env { ... }` block (near line 2596, after `register_modules_module()`), add:

```rust
crate::introspect::register_introspect_trampolines(&context)?;
```

- [x] **Step 6: Add VFS registry entry**

In `tein/src/vfs_registry.rs`, add a new `VfsEntry` before the closing bracket of `VFS_REGISTRY`:

```rust
VfsEntry {
    path: "tein/introspect",
    // scheme/write for describe-environment/text, scheme/eval for doc alist loading
    deps: &["scheme/base", "scheme/write", "scheme/eval"],
    files: &[
        "lib/tein/introspect.sld",
        "lib/tein/introspect.scm",
        "lib/tein/introspect/docs.sld",
        "lib/tein/introspect/docs.scm",
    ],
    clib: None,
    default_safe: true,
    source: VfsSource::Embedded,
    feature: None,
    shadow_sld: None,
},
```

- [x] **Step 7: Create minimal `.sld` and `.scm` in chibi fork**

Create `~/forks/chibi-scheme/lib/tein/introspect.sld`:

```scheme
(define-library (tein introspect)
  (import (scheme base) (scheme write) (scheme eval) (chibi))
  (export available-modules)
  (include "introspect.scm"))
```

Create `~/forks/chibi-scheme/lib/tein/introspect.scm`:

```scheme
;;; (tein introspect) — environment introspection for LLM agents
;;;
;;; provides runtime discovery of available modules, module exports,
;;; procedure arity, and environment bindings.

(define (available-modules) (tein-available-modules-internal))
```

Create `~/forks/chibi-scheme/lib/tein/introspect/docs.sld`:

```scheme
(define-library (tein introspect docs)
  (import (scheme base))
  (export introspect-docs)
  (include "docs.scm"))
```

Create `~/forks/chibi-scheme/lib/tein/introspect/docs.scm`:

```scheme
;;; (tein introspect docs) — documentation alist for (tein introspect)

(define introspect-docs
  '((__module__ . "tein introspect")
    (available-modules . "list modules importable in current context")
    (imported-modules . "list modules already imported in current context")
    (module-exports . "list exported binding names of a module")
    (env-bindings . "list all bindings in current environment, optional prefix filter")
    (binding-info . "detailed info about a binding: kind, arity, module, docs")
    (procedure-arity . "return (min . max) arity, #f for max if variadic")
    (describe-environment . "structured data dump of all available modules and exports")
    (describe-environment/text . "pretty-printed text overview of the environment")))
```

Push the fork:

```bash
cd ~/forks/chibi-scheme
git add lib/tein/introspect.sld lib/tein/introspect.scm lib/tein/introspect/docs.sld lib/tein/introspect/docs.scm
git commit -m "feat: add (tein introspect) scheme module skeleton"
git push
```

- [x] **Step 8: Run tests**

```bash
cargo test -p tein test_available_modules -- --nocapture 2>&1 | tail -20
```

Expected: PASS

- [x] **Step 9: Commit**

```bash
git add tein/src/introspect.rs tein/src/lib.rs tein/src/context.rs tein/src/vfs_registry.rs
git commit -m "feat(introspect): available-modules trampoline + VFS registration

closes the module discovery part of #27 and #83.
available-modules returns importable module paths as scheme lists,
respecting the VFS allowlist in sandboxed contexts."
```

- [x] **Step 10: Lint**

```bash
just lint
```

---

### Task 7: Add `module_exports` trampoline

**Files:**
- Modify: `tein/src/introspect.rs`
- Modify: `tein/src/context.rs` (test)
- Modify: `~/forks/chibi-scheme/lib/tein/introspect.sld` (add export)
- Modify: `~/forks/chibi-scheme/lib/tein/introspect.scm` (add wrapper)

- [ ] **Step 1: Write the tests**

Add to `context.rs` tests:

```rust
#[test]
fn test_module_exports() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    let r = ctx
        .evaluate("(module-exports '(scheme write))")
        .unwrap();
    // scheme/write exports: write, display, write-shared
    if let Value::List(exports) = &r {
        let has_display = exports.iter().any(|e| *e == Value::Symbol("display".into()));
        assert!(has_display, "should include display, got: {:?}", r);
    } else {
        panic!("expected list, got: {:?}", r);
    }
}

#[test]
fn test_module_exports_sandboxed_blocked() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Only(vec!["scheme/base".into(), "tein/introspect".into()]))
        .build()
        .unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    // scheme/regex is not in the allowlist — should error
    let r = ctx.evaluate("(module-exports '(scheme regex))");
    assert!(r.is_err(), "should error for disallowed module");
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p tein test_module_exports -- --nocapture 2>&1 | tail -10
```

- [ ] **Step 3: Implement the trampoline**

Add to `tein/src/introspect.rs`:

```rust
use crate::sandbox;

/// `module-exports` trampoline: returns list of exported binding symbols.
///
/// reads from build-generated MODULE_EXPORTS table. validates the module
/// is in the current allowlist for sandboxed contexts.
unsafe extern "C" fn module_exports_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        if ffi::sexp_nullp(args) != 0 {
            let msg = "module-exports: expected 1 argument (module path list)";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let spec = ffi::sexp_car(args);
        if ffi::sexp_pairp(spec) == 0 {
            let msg = "module-exports: argument must be a list, e.g. '(scheme base)";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        // convert (scheme base) → "scheme/base"
        let module_path = match spec_to_path(ctx, spec) {
            Ok(p) => p,
            Err(e) => return e,
        };

        // check allowlist in sandboxed contexts
        let allowed = VFS_ALLOWLIST.with(|cell| {
            let list = cell.borrow();
            if list.is_empty() {
                true // unsandboxed
            } else {
                list.iter().any(|p| p == &module_path)
            }
        });
        if !allowed {
            let msg = format!(
                "module-exports: module ({}) not available in current context",
                module_path.replace('/', " ")
            );
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        // look up exports
        match sandbox::module_exports(&module_path) {
            Some(exports) => {
                let mut result = ffi::sexp_null();
                for name in exports.iter().rev() {
                    let c_name = CString::new(*name).unwrap_or_default();
                    let sym =
                        ffi::sexp_intern(ctx, c_name.as_ptr(), name.len() as ffi::sexp_sint_t);
                    result = ffi::sexp_cons(ctx, sym, result);
                }
                result
            }
            None => {
                let msg = format!(
                    "module-exports: unknown module ({})",
                    module_path.replace('/', " ")
                );
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}

**NOTE**: `spec_to_path` already exists in `context.rs:1224`. make it `pub(crate)` and import it in `introspect.rs` as `crate::context::spec_to_path` — do NOT redefine it.

**NOTE**: trampoline registration uses `register_native_trampoline(ctx, prim_env, name, fn)` NOT `define_fn_variadic`. see Progress section.
```

- [ ] **Step 4: Register the trampoline**

In `register_introspect_trampolines(ctx, prim_env)`, add:

```rust
crate::context::register_native_trampoline(ctx, prim_env, "tein-module-exports-internal", module_exports_trampoline)?;
```

- [ ] **Step 5: Update the scheme files**

In `introspect.sld`, add `module-exports` to the export list.

In `introspect.scm`, add:

```scheme
(define (module-exports mod-path) (tein-module-exports-internal mod-path))
```

Push the fork.

- [ ] **Step 6: Run tests**

```bash
cargo test -p tein test_module_exports -- --nocapture 2>&1 | tail -20
```

Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add tein/src/introspect.rs tein/src/context.rs
git commit -m "feat(introspect): module-exports trampoline

returns exported binding symbols for a given module path.
validates against VFS allowlist in sandboxed contexts."
```

- [ ] **Step 8: Lint**

```bash
just lint
```

---

### Task 8: Add `procedure_arity` trampoline

**Files:**
- Modify: `tein/src/introspect.rs`
- Modify: `tein/src/context.rs` (tests)
- Modify: scheme files in fork

- [ ] **Step 1: Write the tests**

```rust
#[test]
fn test_procedure_arity_lambda() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    // fixed arity lambda
    let r = ctx.evaluate("(procedure-arity (lambda (a b) a))").unwrap();
    assert_eq!(r, Value::Pair(Box::new(Value::Integer(2)), Box::new(Value::Integer(2))));
}

#[test]
fn test_procedure_arity_variadic() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    // variadic lambda
    let r = ctx.evaluate("(procedure-arity (lambda (a . rest) a))").unwrap();
    assert_eq!(r, Value::Pair(Box::new(Value::Integer(1)), Box::new(Value::Boolean(false))));
}

#[test]
fn test_procedure_arity_builtin() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    let r = ctx.evaluate("(procedure-arity cons)").unwrap();
    assert_eq!(r, Value::Pair(Box::new(Value::Integer(2)), Box::new(Value::Integer(2))));
}

#[test]
fn test_procedure_arity_non_procedure() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    let r = ctx.evaluate("(procedure-arity 42)").unwrap();
    assert_eq!(r, Value::Boolean(false));
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p tein test_procedure_arity -- --nocapture 2>&1 | tail -10
```

- [ ] **Step 3: Implement the trampoline**

Add to `tein/src/introspect.rs`:

```rust
/// `procedure-arity` trampoline: returns (min . max) or #f.
unsafe extern "C" fn procedure_arity_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        if ffi::sexp_nullp(args) != 0 {
            let msg = "procedure-arity: expected 1 argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let proc = ffi::sexp_car(args);
        ffi::procedure_arity(ctx, proc)
    }
}
```

- [ ] **Step 4: Register the trampoline**

In `register_introspect_trampolines(ctx, prim_env)`, add:

```rust
crate::context::register_native_trampoline(ctx, prim_env, "tein-procedure-arity-internal", procedure_arity_trampoline)?;
```

- [ ] **Step 5: Update scheme files**

Add `procedure-arity` to `.sld` exports. Add to `.scm`:

```scheme
(define (procedure-arity proc) (tein-procedure-arity-internal proc))
```

Push the fork.

- [ ] **Step 6: Run tests**

```bash
cargo test -p tein test_procedure_arity -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 7: Commit**

```bash
git add tein/src/introspect.rs tein/src/context.rs
git commit -m "feat(introspect): procedure-arity trampoline

returns (min . max) arity pair. max is #f for variadic procedures.
works on compiled lambdas, opcodes, and builtins."
```

- [ ] **Step 8: Lint**

```bash
just lint
```

---

### Task 9: Add `env_bindings` trampoline

**Files:**
- Modify: `tein/src/introspect.rs`
- Modify: `tein/src/context.rs` (tests)
- Modify: scheme files in fork

- [ ] **Step 1: Write the tests**

```rust
#[test]
fn test_env_bindings() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    ctx.evaluate("(define my-test-var 42)").unwrap();
    let r = ctx.evaluate("(env-bindings \"my-test\")").unwrap();
    // should find our defined variable
    if let Value::List(bindings) = &r {
        let has_var = bindings.iter().any(|b| {
            if let Value::Pair(name, kind) = b {
                **name == Value::Symbol("my-test-var".into())
                    && **kind == Value::Symbol("variable".into())
            } else {
                false
            }
        });
        assert!(has_var, "should find my-test-var, got: {:?}", r);
    } else {
        panic!("expected list, got: {:?}", r);
    }
}

#[test]
fn test_env_bindings_no_prefix() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    let r = ctx.evaluate("(length (env-bindings))").unwrap();
    if let Value::Integer(n) = r {
        assert!(n > 10, "standard env should have many bindings, got {}", n);
    } else {
        panic!("expected integer, got: {:?}", r);
    }
}

#[test]
fn test_env_bindings_kind_procedure() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    let r = ctx.evaluate("(assq 'map (env-bindings \"map\"))").unwrap();
    if let Value::Pair(name, kind) = &r {
        assert_eq!(**name, Value::Symbol("map".into()));
        assert_eq!(**kind, Value::Symbol("procedure".into()));
    } else {
        panic!("expected (map . procedure), got: {:?}", r);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p tein test_env_bindings -- --nocapture 2>&1 | tail -10
```

- [ ] **Step 3: Implement the trampoline**

Add to `tein/src/introspect.rs`:

```rust
/// `env-bindings` trampoline: returns alist of (name . kind) pairs.
///
/// optional string prefix argument for filtering.
unsafe extern "C" fn env_bindings_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let prefix = if ffi::sexp_nullp(args) != 0 {
            ffi::get_false()
        } else {
            let arg = ffi::sexp_car(args);
            if ffi::sexp_stringp(arg) != 0 {
                arg
            } else {
                let msg = "env-bindings: optional argument must be a string prefix";
                let c_msg = CString::new(msg).unwrap_or_default();
                return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };
        ffi::env_bindings_list(ctx, prefix)
    }
}
```

- [ ] **Step 4: Register the trampoline**

In `register_introspect_trampolines(ctx, prim_env)`, add:

```rust
crate::context::register_native_trampoline(ctx, prim_env, "tein-env-bindings-internal", env_bindings_trampoline)?;
```

- [ ] **Step 5: Update scheme files**

Add `env-bindings` to `.sld` exports. Add to `.scm`:

```scheme
(define (env-bindings . args)
  (if (null? args)
      (tein-env-bindings-internal)
      (tein-env-bindings-internal (car args))))
```

Push the fork.

- [ ] **Step 6: Run tests**

```bash
cargo test -p tein test_env_bindings -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 7: Commit**

```bash
git add tein/src/introspect.rs tein/src/context.rs
git commit -m "feat(introspect): env-bindings trampoline

walks env chain returning (name . kind) alist.
supports optional string prefix filtering.
deduplicates shadowed bindings (innermost wins)."
```

- [ ] **Step 8: Lint**

```bash
just lint
```

---

### Task 10: Add `imported_modules` trampoline

**Files:**
- Modify: `tein/src/introspect.rs`
- Modify: `tein/src/context.rs` (tests)
- Modify: scheme files in fork

- [ ] **Step 1: Write the tests**

```rust
#[test]
fn test_imported_modules() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    ctx.evaluate("(import (scheme write))").unwrap();
    let r = ctx.evaluate("(imported-modules)").unwrap();
    if let Value::List(modules) = &r {
        // should at minimum include scheme/base (auto-imported) and tein/introspect
        let has_introspect = modules.iter().any(|m| {
            if let Value::List(parts) = m {
                parts.len() == 2
                    && parts[0] == Value::Symbol("tein".into())
                    && parts[1] == Value::Symbol("introspect".into())
            } else {
                false
            }
        });
        assert!(has_introspect, "should include (tein introspect), got: {:?}", r);
    } else {
        panic!("expected list, got: {:?}", r);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p tein test_imported_modules -- --nocapture 2>&1 | tail -10
```

- [ ] **Step 3: Implement the trampoline**

Add to `tein/src/introspect.rs`:

```rust
/// `imported-modules` trampoline: returns list of actually-imported module paths.
///
/// walks chibi's *modules* in meta env. in sandboxed contexts, filters
/// results to VFS_ALLOWLIST to prevent information leakage.
unsafe extern "C" fn imported_modules_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let raw_list = ffi::imported_modules_list(ctx);

        // in sandboxed contexts, filter to allowlist
        VFS_ALLOWLIST.with(|cell| {
            let list = cell.borrow();
            if list.is_empty() {
                return raw_list; // unsandboxed: return all
            }

            // filter: keep only modules whose path is in the allowlist
            let mut result = ffi::sexp_null();
            let mut ls = raw_list;
            while ffi::sexp_pairp(ls) != 0 {
                let name = ffi::sexp_car(ls);
                // convert module name list to path string for allowlist check
                if let Ok(path) = spec_to_path(ctx, name) {
                    if list.iter().any(|p| p == &path) {
                        result = ffi::sexp_cons(ctx, name, result);
                    }
                }
                ls = ffi::sexp_cdr(ls);
            }
            result
        })
    }
}
```

- [ ] **Step 4: Register the trampoline**

In `register_introspect_trampolines(ctx, prim_env)`, add:

```rust
crate::context::register_native_trampoline(ctx, prim_env, "tein-imported-modules-internal", imported_modules_trampoline)?;
```

- [ ] **Step 5: Update scheme files**

Add `imported-modules` to `.sld` exports. Add to `.scm`:

```scheme
(define (imported-modules) (tein-imported-modules-internal))
```

Push the fork.

- [ ] **Step 6: Run tests**

```bash
cargo test -p tein test_imported_modules -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 7: Commit**

```bash
git add tein/src/introspect.rs tein/src/context.rs
git commit -m "feat(introspect): imported-modules trampoline

walks chibi *modules* alist in meta env. filters to VFS allowlist
in sandboxed contexts to prevent information leakage."
```

- [ ] **Step 8: Lint**

```bash
just lint
```

---

## Chunk 3: Scheme Composition Layer

### Task 11: Implement `binding-info` in scheme

**Files:**
- Modify: `~/forks/chibi-scheme/lib/tein/introspect.sld`
- Modify: `~/forks/chibi-scheme/lib/tein/introspect.scm`
- Modify: `tein/src/context.rs` (tests)

The `binding-info` function composes the lower-level primitives into a structured alist. It also needs the reverse index (binding → module) and doc alist lookup.

- [ ] **Step 1: Write the test**

Add to `context.rs` tests:

```rust
#[test]
fn test_binding_info_procedure() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    let r = ctx.evaluate("(binding-info 'map)").unwrap();
    // should be an alist with at least name, kind, arity
    if let Value::List(entries) = &r {
        let has_name = entries.iter().any(|e| {
            if let Value::Pair(k, v) = e {
                **k == Value::Symbol("name".into()) && **v == Value::Symbol("map".into())
            } else {
                false
            }
        });
        let has_kind = entries.iter().any(|e| {
            if let Value::Pair(k, v) = e {
                **k == Value::Symbol("kind".into()) && **v == Value::Symbol("procedure".into())
            } else {
                false
            }
        });
        assert!(has_name, "should have name entry, got: {:?}", r);
        assert!(has_kind, "should have kind entry, got: {:?}", r);
    } else {
        panic!("expected list (alist), got: {:?}", r);
    }
}

#[test]
fn test_binding_info_undefined() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    let r = ctx.evaluate("(binding-info 'nonexistent-xyz-42)").unwrap();
    assert_eq!(r, Value::Boolean(false));
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p tein test_binding_info -- --nocapture 2>&1 | tail -10
```

- [ ] **Step 3: Add a `tein-binding-kind-internal` trampoline in rust**

Add to `tein/src/introspect.rs`:

```rust
/// `binding-kind-internal` trampoline: looks up a symbol in the current env
/// and returns its kind symbol, or #f if not bound.
unsafe extern "C" fn binding_kind_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        if ffi::sexp_nullp(args) != 0 {
            let msg = "binding-kind: expected 1 argument (symbol)";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let sym = ffi::sexp_car(args);
        if ffi::sexp_symbolp(sym) == 0 {
            let msg = "binding-kind: argument must be a symbol";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let env = ffi::sexp_context_env(ctx);
        let value = ffi::sexp_env_ref(ctx, env, sym, ffi::get_void());
        // sexp_voidp = not found
        if ffi::sexp_voidp(value) != 0 {
            return ffi::get_false();
        }
        ffi::binding_kind(ctx, value)
    }
}
```

Register it in `register_introspect_trampolines(ctx, prim_env)`:

```rust
crate::context::register_native_trampoline(ctx, prim_env, "tein-binding-kind-internal", binding_kind_trampoline)?;
```

- [ ] **Step 4: Implement `binding-info` and the reverse index in scheme**

Update `~/forks/chibi-scheme/lib/tein/introspect.scm` to add the reverse index and `binding-info`:

```scheme
;; --- reverse index: symbol → providing module ---
;; built once at import time by inverting module-exports.

(define *binding-module-index*
  (let loop ((mods (available-modules)) (index '()))
    (if (null? mods)
        index
        (let* ((mod-path (car mods))
               (exports
                (guard (exn (#t '()))
                  (module-exports mod-path))))
          (loop (cdr mods)
                (let inner ((names exports) (idx index))
                  (if (null? names)
                      idx
                      (inner (cdr names)
                             (if (assq (car names) idx)
                                 idx  ; first match wins
                                 (cons (cons (car names) mod-path) idx))))))))))

;; --- doc alist cache ---
;; eagerly load doc sub-libraries for tein modules.

(define *doc-alist-cache*
  (let loop ((mods (available-modules)) (cache '()))
    (if (null? mods)
        cache
        (let ((mod-path (car mods)))
          (if (and (pair? mod-path)
                   (eq? (car mod-path) 'tein)
                   (pair? (cdr mod-path))
                   (null? (cddr mod-path)))
              ;; single-level tein module: try to load (tein X docs)
              (let ((docs-sym
                     (string->symbol
                      (string-append
                       (symbol->string (cadr mod-path)) "-docs"))))
                (guard (exn (#t (loop (cdr mods) cache)))
                  (let ((docs-mod
                         (eval `(begin
                                  (import (tein ,(cadr mod-path) docs))
                                  ,docs-sym)
                               (environment '(scheme base) '(scheme eval)))))
                    (loop (cdr mods)
                          (cons (cons mod-path docs-mod) cache)))))
              (loop (cdr mods) cache))))))

(define (binding-info sym)
  (let ((kind (tein-binding-kind-internal sym)))
    (if (not kind)
        #f
        (let* ((arity (if (eq? kind 'procedure)
                          (procedure-arity (eval sym (interaction-environment)))
                          #f))
               (mod-entry (assq sym *binding-module-index*))
               (mod-path (and mod-entry (cdr mod-entry)))
               (doc-entry (and mod-path
                               (let ((cache-hit (assoc mod-path *doc-alist-cache*)))
                                 (and cache-hit
                                      (let ((doc-alist (cdr cache-hit)))
                                        (let ((d (assq sym doc-alist)))
                                          (and d (not (string=? (cdr d) "")) (cdr d)))))))))
          (let ((result (list (cons 'name sym)
                              (cons 'kind kind))))
            (let ((result (if arity
                              (append result (list (cons 'arity arity)))
                              result)))
              (let ((result (if mod-path
                                (append result (list (cons 'module mod-path)))
                                result)))
                (if doc-entry
                    (append result (list (cons 'doc doc-entry)))
                    result))))))))
```

- [ ] **Step 5: Update `.sld` exports**

Add `binding-info` to the export list in `introspect.sld`. Also add `(scheme eval)` to the import list (needed for doc alist loading and `binding-info`'s eval of the symbol).

- [ ] **Step 6: Push the fork**

```bash
cd ~/forks/chibi-scheme
git add lib/tein/introspect.sld lib/tein/introspect.scm
git commit -m "feat: add binding-info, reverse index, and doc cache to (tein introspect)"
git push
```

- [ ] **Step 7: Run tests**

```bash
cargo test -p tein test_binding_info -- --nocapture 2>&1 | tail -20
```

Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add tein/src/introspect.rs tein/src/context.rs
git commit -m "feat(introspect): binding-info with reverse index and doc lookup

composes kind, arity, module provenance, and docstrings.
reverse index built once at import time by inverting module-exports.
doc alists eagerly loaded from (tein X docs) sub-libraries."
```

- [ ] **Step 9: Lint**

```bash
just lint
```

---

### Task 12: Implement `describe-environment` and `describe-environment/text`

**Files:**
- Modify: `~/forks/chibi-scheme/lib/tein/introspect.sld`
- Modify: `~/forks/chibi-scheme/lib/tein/introspect.scm`
- Modify: `tein/src/context.rs` (tests)

- [ ] **Step 1: Write the tests**

```rust
#[test]
fn test_describe_environment() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    let r = ctx.evaluate("(describe-environment)").unwrap();
    // should be an alist with a 'modules key
    if let Value::List(entries) = &r {
        let has_modules = entries.iter().any(|e| {
            if let Value::Pair(k, _) = e {
                **k == Value::Symbol("modules".into())
            } else if let Value::List(items) = e {
                items.first() == Some(&Value::Symbol("modules".into()))
            } else {
                false
            }
        });
        assert!(has_modules, "should have modules key, got: {:?}", r);
    } else {
        panic!("expected list, got: {:?}", r);
    }
}

#[test]
fn test_describe_environment_text() {
    let ctx = Context::new_standard().unwrap();
    ctx.evaluate("(import (tein introspect))").unwrap();
    let r = ctx.evaluate("(describe-environment/text)").unwrap();
    if let Value::String(text) = &r {
        assert!(text.contains("scheme base"), "should mention scheme base, got: {}", text);
        assert!(text.contains("modules available"), "should have header, got: {}", text);
    } else {
        panic!("expected string, got: {:?}", r);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p tein test_describe_environment -- --nocapture 2>&1 | tail -10
```

- [ ] **Step 3: Implement in scheme**

Add to `~/forks/chibi-scheme/lib/tein/introspect.scm`:

```scheme
(define (describe-environment)
  (let ((mods
         (map (lambda (mod-path)
                (let* ((exports
                        (guard (exn (#t '()))
                          (module-exports mod-path)))
                       (cache-hit (assoc mod-path *doc-alist-cache*))
                       (base (list (cons 'name mod-path)
                                   (cons 'exports exports))))
                  (if cache-hit
                      (let ((docs (cdr cache-hit)))
                        (append base
                                (list (cons 'docs
                                            (let keep ((rest docs) (acc '()))
                                              (cond
                                               ((null? rest) (reverse acc))
                                               ((eq? (caar rest) '__module__)
                                                (keep (cdr rest) acc))
                                               ((string=? (cdar rest) "")
                                                (keep (cdr rest) acc))
                                               (else
                                                (keep (cdr rest) (cons (car rest) acc)))))))))
                      base)))
              (available-modules))))
    (list (cons 'modules mods))))

(define (describe-environment/text)
  (let* ((env-data (describe-environment))
         (modules (cdr (assq 'modules env-data))))
    (string-append
     "(tein introspect) — environment overview\n\n"
     (number->string (length modules)) " modules available:\n\n"
     (apply string-append
            (map (lambda (mod-info)
                   (let* ((name (cdr (assq 'name mod-info)))
                          (exports (cdr (assq 'exports mod-info)))
                          (docs-entry (assq 'docs mod-info))
                          (name-str (module-path->string name))
                          (n (length exports)))
                     (string-append
                      "(" name-str ") — " (number->string n) " exports\n"
                      (if (and docs-entry (pair? (cdr docs-entry)))
                          ;; tein module with docs: show each with docstring
                          (apply string-append
                                 (map (lambda (exp)
                                        (let ((doc (assq exp (cdr docs-entry))))
                                          (if (and doc (string? (cdr doc))
                                                   (not (string=? (cdr doc) "")))
                                              (string-append "  " (symbol->string exp)
                                                             " — " (cdr doc) "\n")
                                              (string-append "  " (symbol->string exp) "\n"))))
                                      exports))
                          ;; no docs: comma-separated summary
                          (if (> n 0)
                              (string-append
                               "  "
                               (let loop ((rest exports) (acc ""))
                                 (cond
                                  ((null? rest) acc)
                                  ((null? (cdr rest))
                                   (string-append acc (symbol->string (car rest))))
                                  (else
                                   (loop (cdr rest)
                                         (string-append acc (symbol->string (car rest)) ", ")))))
                               "\n")
                              ""))
                      "\n")))
                 modules)))))

(define (module-path->string path)
  (let loop ((rest path) (acc ""))
    (cond
     ((null? rest) acc)
     ((null? (cdr rest))
      (string-append acc (if (symbol? (car rest))
                             (symbol->string (car rest))
                             (number->string (car rest)))))
     (else
      (loop (cdr rest)
            (string-append acc
                           (if (symbol? (car rest))
                               (symbol->string (car rest))
                               (number->string (car rest)))
                           " "))))))
```

- [ ] **Step 4: Update `.sld` exports**

Add `describe-environment` and `describe-environment/text` to the export list.

- [ ] **Step 5: Push the fork**

```bash
cd ~/forks/chibi-scheme
git add lib/tein/introspect.sld lib/tein/introspect.scm
git commit -m "feat: add describe-environment and describe-environment/text"
git push
```

- [ ] **Step 6: Run tests**

```bash
cargo test -p tein test_describe_environment -- --nocapture 2>&1 | tail -20
```

- [ ] **Step 7: Commit**

```bash
git add tein/src/context.rs
git commit -m "feat(introspect): describe-environment and describe-environment/text

structured data dump of all available modules, exports, and docs.
text version produces a pretty-printed overview for prompt injection."
```

- [ ] **Step 8: Lint**

```bash
just lint
```

---

### Task 13: Export `introspect-docs` and finalise `.sld`

**Files:**
- Modify: `~/forks/chibi-scheme/lib/tein/introspect.sld`

- [ ] **Step 1: Finalise the `.sld` with all exports**

The final `introspect.sld` should look like:

```scheme
(define-library (tein introspect)
  (import (scheme base) (scheme write) (scheme eval) (chibi))
  (export available-modules
          imported-modules
          module-exports
          env-bindings
          binding-info
          procedure-arity
          describe-environment
          describe-environment/text
          introspect-docs)
  (include "introspect.scm"))
```

Note: `introspect-docs` is defined directly in `introspect.scm` (imported from the docs sub-library, or defined inline). Since the docs sub-library is a separate module, the simplest approach is to define `introspect-docs` inline in `introspect.scm` and also have the sub-library export it for convention.

Add to `introspect.scm` (near the top, after the doc alist cache):

```scheme
(define introspect-docs
  '((__module__ . "tein introspect")
    (available-modules . "list modules importable in current context")
    (imported-modules . "list modules already imported in current context")
    (module-exports . "list exported binding names of a module")
    (env-bindings . "list all bindings in current environment, optional prefix filter")
    (binding-info . "detailed info about a binding: kind, arity, module, docs")
    (procedure-arity . "return (min . max) arity, #f for max if variadic")
    (describe-environment . "structured data dump of all available modules and exports")
    (describe-environment/text . "pretty-printed text overview of the environment")))
```

- [ ] **Step 2: Push the fork**

```bash
cd ~/forks/chibi-scheme
git add lib/tein/introspect.sld lib/tein/introspect.scm
git commit -m "feat: finalise (tein introspect) exports and introspect-docs"
git push
```

- [ ] **Step 3: Rebuild and test everything**

```bash
cd ~/projects/tein && cargo build -p tein && cargo test -p tein -- introspect --nocapture 2>&1 | tail -30
```

- [ ] **Step 4: Commit (if any rust changes needed)**

```bash
git add -A && git commit -m "feat(introspect): finalise all exports"
```

- [ ] **Step 5: Lint**

```bash
just lint
```

---

## Chunk 4: Integration Tests, Docs, and Cleanup

### Task 14: Add scheme integration tests

**Files:**
- Create: `tein/tests/scheme/introspect.scm`

- [ ] **Step 1: Write the integration test file**

Create `tein/tests/scheme/introspect.scm`:

```scheme
;;; integration tests for (tein introspect)

(import (scheme base) (scheme write) (tein test) (tein introspect))

;; available-modules returns a non-empty list
(test-true "available-modules non-empty"
  (pair? (available-modules)))

;; available-modules entries are lists
(test-true "available-modules entries are lists"
  (list? (car (available-modules))))

;; module-exports returns symbols
(test-true "module-exports scheme/write"
  (memq 'display (module-exports '(scheme write))))

;; module-exports for introspect itself
(test-true "module-exports tein/introspect includes available-modules"
  (memq 'available-modules (module-exports '(tein introspect))))

;; procedure-arity on a lambda
(test-equal "arity of (lambda (a b) a)"
  '(2 . 2)
  (procedure-arity (lambda (a b) a)))

;; procedure-arity on a variadic lambda
(test-equal "arity of (lambda (a . rest) a)"
  '(1 . #f)
  (procedure-arity (lambda (a . rest) a)))

;; procedure-arity on non-procedure
(test-false "arity of 42"
  (procedure-arity 42))

;; env-bindings returns alist
(define my-test-var 99)
(test-true "env-bindings finds my-test-var"
  (let ((entry (assq 'my-test-var (env-bindings "my-test"))))
    (and entry (eq? (cdr entry) 'variable))))

;; binding-info on procedure
(let ((info (binding-info 'map)))
  (test-true "binding-info map has name"
    (and info (assq 'name info)))
  (test-equal "binding-info map kind"
    'procedure
    (cdr (assq 'kind info))))

;; binding-info on undefined symbol
(test-false "binding-info undefined"
  (binding-info 'this-symbol-definitely-does-not-exist-xyz))

;; describe-environment returns structured data
(let ((env-data (describe-environment)))
  (test-true "describe-environment has modules"
    (assq 'modules env-data)))

;; describe-environment/text returns a string
(test-true "describe-environment/text is a string"
  (string? (describe-environment/text)))

;; introspect-docs is an alist
(test-true "introspect-docs has __module__"
  (assq '__module__ introspect-docs))

;; imported-modules includes at least tein/introspect
(test-true "imported-modules includes tein/introspect"
  (member '(tein introspect) (imported-modules)))
```

- [ ] **Step 2: Add the test to `scheme_tests.rs`**

Check how existing scheme tests are registered in `tein/tests/scheme_tests.rs` and add:

```rust
scheme_test!(test_introspect, "scheme/introspect.scm");
```

- [ ] **Step 3: Run the integration tests**

```bash
cargo test -p tein --test scheme_tests test_introspect -- --nocapture 2>&1 | tail -20
```

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add tein/tests/scheme/introspect.scm tein/tests/scheme_tests.rs
git commit -m "test(introspect): scheme integration tests

exercises available-modules, module-exports, procedure-arity,
env-bindings, binding-info, describe-environment,
describe-environment/text, introspect-docs, and imported-modules."
```

---

### Task 15: Run the full test suite

- [ ] **Step 1: Run all tests**

```bash
just test 2>&1 | tail -20
```

Expected: all tests pass (549+ lib + 40+ scheme + vfs_module_tests + etc.)

- [ ] **Step 2: If any failures, fix and re-test**

Check for regressions. the new VFS entry and trampoline registrations could affect sandbox tests if allowlists changed.

- [ ] **Step 3: Commit any fixes**

---

### Task 16: Update documentation

**Files:**
- Modify: `docs/reference.md`
- Modify: `docs/tein-for-agents.md`
- Modify: `docs/modules.md`
- Modify: `AGENTS.md`

- [ ] **Step 1: Update `docs/reference.md`**

Add `(tein introspect)` to the VFS module list with its exports and description.

- [ ] **Step 2: Update `docs/tein-for-agents.md`**

Move the `(tein introspect)` mention from the "what's coming" section to a "what's here" section with usage examples:

```markdown
## environment introspection: (tein introspect)

`(tein introspect)` lets scheme code discover its own environment at runtime:

\```scheme
(import (tein introspect))

(available-modules)          ; what can I import?
(module-exports '(tein json)) ; what does this module provide?
(procedure-arity map)        ; how many args does this take?
(env-bindings "json-")       ; what json-* bindings are in scope?
(binding-info 'json-parse)   ; everything about this binding
(describe-environment/text)  ; full text dump for prompt injection
\```
```

- [ ] **Step 3: Update `docs/modules.md`**

Add a section for `(tein introspect)` documenting all exports.

- [ ] **Step 4: Update `AGENTS.md`**

Add `(tein introspect)` to the architecture section under `lib/tein/`. Add gotchas:
- `*doc-alist-cache*` and `*binding-module-index*` are built at import time — O(modules × exports) cost, paid once
- `procedure-arity` reports `(0 . #f)` for `define_fn_variadic` trampolines
- `imported-modules` relies on chibi's `*modules*` internal
- doc alist loading uses `(scheme eval)` — must be in allowlist for full doc output

- [ ] **Step 5: Commit**

```bash
git add docs/reference.md docs/tein-for-agents.md docs/modules.md AGENTS.md
git commit -m "docs: add (tein introspect) to reference, modules, tein-for-agents, AGENTS.md

closes #27 (VFS-embedded documentation for LLM schemers) and
#83 ((tein introspect) environment introspection API)."
```

---

### Task 17: Final lint and test

- [ ] **Step 1: Lint**

```bash
just lint
```

- [ ] **Step 2: Full test suite**

```bash
just test
```

- [ ] **Step 3: Commit any lint fixes**

- [ ] **Step 4: Collect AGENTS.md notes**

Review all tasks for any gotchas or caveats discovered during implementation that should be added to AGENTS.md. update if needed.

---

## Notes for Implementer

- **chibi fork workflow**: all changes to `.sld`/`.scm` files and `tein_shim.c` happen in `~/forks/chibi-scheme` on branch `emesal-tein`. push after each change. `cargo build` pulls automatically.
- **GC rooting**: the C shims use `sexp_gc_var`/`sexp_gc_preserve`/`sexp_gc_release`. the rust trampolines don't need manual rooting since they call safe wrappers that delegate to C.
- **spec_to_path**: this helper already exists at `context.rs:1224`. make it `pub(crate)` and import in `introspect.rs`. it's used by `module-exports` and `imported-modules`. do NOT redefine it.
- **`describe-environment` uses `(scheme eval)`**: this is needed for the doc alist loading (`eval` to import doc sub-libraries). `scheme/eval` is `default_safe: true` so this works for `Modules::Safe`. for `Modules::Only` without `scheme/eval`, the `guard` clause silently skips doc loading.
- **`binding-info` uses `interaction-environment`**: to get the procedure value for arity lookup when given a symbol. this is available via `(scheme eval)` which `(tein introspect)` imports.
- **test ordering**: tasks 1-5 (C shims + FFI) must come first. tasks 6-10 (rust trampolines) can be done in any order but are presented in dependency order. tasks 11-13 (scheme layer) depend on all trampolines being registered. tasks 14-17 (integration + docs) come last.
