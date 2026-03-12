//! `(tein introspect)` — environment introspection for LLM agents.
//!
//! provides runtime discovery of available modules, module exports,
//! procedure arity, and environment bindings. designed for LLM agents
//! that need to understand their sandbox from within scheme.

use std::ffi::CString;

use crate::ffi;
use crate::sandbox::{VFS_ALLOWLIST, module_exports as vfs_module_exports, registry_all_allowlist};

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
                registry_all_allowlist()
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
/// e.g. "scheme/base" -> (scheme base)
unsafe fn path_to_module_list(ctx: ffi::sexp, path: &str) -> ffi::sexp {
    unsafe {
        let parts: Vec<&str> = path.split('/').collect();
        let mut result = ffi::get_null();
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
        let mut result = ffi::get_null();
        for path in paths.iter().rev() {
            let module_list = path_to_module_list(ctx, path);
            result = ffi::sexp_cons(ctx, module_list, result);
        }
        result
    }
}

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
        let module_path = match crate::context::spec_to_path(ctx, spec) {
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
        match vfs_module_exports(&module_path) {
            Some(exports) => {
                let mut result = ffi::get_null();
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

/// `procedure-arity` trampoline: returns `(min . max)` or `#f`.
///
/// delegates to `tein_procedure_arity` C shim. max is `#f` for variadic.
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

/// `env-bindings` trampoline: returns alist of `(name . kind)` pairs.
///
/// optional string prefix argument for filtering.
/// delegates to `tein_env_bindings_list` C shim.
///
/// **important**: chibi passes a child apply-context as `ctx` during native
/// fn dispatch. `sexp_context_env` on the child ctx returns NULL. we must
/// use the real top-level context from `CONTEXT_PTR` thread-local so that
/// `tein_env_bindings_list` can walk `sexp_context_env(ctx)` correctly.
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

        // chibi's native fn dispatch passes a child apply-context as `ctx`.
        // `sexp_context_env(child_ctx)` is NULL — use the real top-level ctx
        // from the CONTEXT_PTR thread-local, which is set by evaluate()/call().
        let real_ctx = crate::context::CONTEXT_PTR.with(|c| {
            let ptr = c.get();
            if ptr.is_null() {
                ctx
            } else {
                (*ptr).raw_ctx()
            }
        });
        ffi::env_bindings_list(real_ctx, prefix)
    }
}

/// `imported-modules` trampoline: returns list of actually-imported module paths.
///
/// walks chibi's `*modules*` in meta env. in sandboxed contexts, filters
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
            let mut result = ffi::get_null();
            let mut ls = raw_list;
            while ffi::sexp_pairp(ls) != 0 {
                let name = ffi::sexp_car(ls);
                // convert module name list to path string for allowlist check
                if let Ok(path) = crate::context::spec_to_path(ctx, name) {
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

/// register all (tein introspect) trampolines into the primitive env.
///
/// called from `Context::build()` BEFORE `load_standard_env` so that
/// trampolines end up in `*chibi-env*` and are visible to library bodies
/// via `(import (chibi))`. `(tein introspect)` imports `(chibi)` in its `.sld`.
pub(crate) fn register_introspect_trampolines(
    ctx: ffi::sexp,
    prim_env: ffi::sexp,
) -> crate::Result<()> {
    crate::context::register_native_trampoline(
        ctx,
        prim_env,
        "tein-available-modules-internal",
        available_modules_trampoline,
    )?;
    crate::context::register_native_trampoline(
        ctx,
        prim_env,
        "tein-module-exports-internal",
        module_exports_trampoline,
    )?;
    crate::context::register_native_trampoline(
        ctx,
        prim_env,
        "tein-procedure-arity-internal",
        procedure_arity_trampoline,
    )?;
    crate::context::register_native_trampoline(
        ctx,
        prim_env,
        "tein-env-bindings-internal",
        env_bindings_trampoline,
    )?;
    crate::context::register_native_trampoline(
        ctx,
        prim_env,
        "tein-imported-modules-internal",
        imported_modules_trampoline,
    )?;
    Ok(())
}
