//! `(tein introspect)` — environment introspection for LLM agents.
//!
//! provides runtime discovery of available modules, module exports,
//! procedure arity, and environment bindings. designed for LLM agents
//! that need to understand their sandbox from within scheme.

use std::ffi::CString;

use crate::ffi;
use crate::sandbox::{registry_all_allowlist, VFS_ALLOWLIST};

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
    Ok(())
}
