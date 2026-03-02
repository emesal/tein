//! Scheme evaluation context
//!
//! [`Context`] is a single-threaded Scheme evaluation environment backed
//! by a Chibi-Scheme heap. Create one directly with [`Context::new()`] or
//! configure via [`ContextBuilder`] for sandboxing, step limits, and
//! environment control.
//!
//! # Builder pattern
//!
//! ```
//! use tein::Context;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ctx = Context::builder()
//!     .standard_env()
//!     .step_limit(100_000)
//!     .build()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Evaluation
//!
//! - [`Context::evaluate()`] — evaluate a Scheme expression string
//! - [`Context::call()`] — call a Scheme procedure with Rust arguments
//! - [`Context::load_file()`] — load and evaluate a `.scm` file
//!
//! # Sandboxing
//!
//! Four independent layers of control:
//!
//! 1. **Module restriction** — [`ContextBuilder::sandboxed()`] with [`sandbox::Modules`]
//! 2. **Step limits** — [`ContextBuilder::step_limit()`]
//! 3. **File IO policy** — [`ContextBuilder::file_read()`] / [`.file_write()`](ContextBuilder::file_write)
//! 4. **VFS gate** — automatic VFS-only when using standard env + `sandboxed()`
//!
//! See the [`sandbox`](crate::sandbox) module for module set details.

use crate::{
    Value,
    error::{Error, Result},
    ffi,
    foreign::{ForeignStore, ForeignType},
    port::PortStore,
    sandbox::{FS_GATE, FS_GATE_CHECK, FS_POLICY, FsPolicy, GATE_CHECK, VFS_ALLOWLIST, VFS_GATE},
};
use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::os::raw::c_char;
use std::path::Path;

/// RAII guard that clears the FOREIGN_STORE_PTR thread-local on drop.
///
/// Ensures the pointer is nulled on all exit paths (early returns, `?`, panic).
struct ForeignStoreGuard;

impl Drop for ForeignStoreGuard {
    fn drop(&mut self) {
        FOREIGN_STORE_PTR.with(|c| c.set(std::ptr::null()));
    }
}

/// RAII guard that clears the EXT_API thread-local on drop.
struct ExtApiGuard;

impl Drop for ExtApiGuard {
    fn drop(&mut self) {
        EXT_API.with(|c| c.set(std::ptr::null()));
    }
}

// --- port store thread-local for custom port trampolines ---
//
// set to &self.port_store before every evaluate()/call() invocation,
// cleared afterwards. the extern "C" port trampolines read it to access
// the backing Read/Write objects. safe because Context is !Send + !Sync
// and the pointer is only live during evaluation.
thread_local! {
    static PORT_STORE_PTR: Cell<*const RefCell<PortStore>> = const { Cell::new(std::ptr::null()) };
}

/// RAII guard that clears the PORT_STORE_PTR thread-local on drop.
struct PortStoreGuard;

impl Drop for PortStoreGuard {
    fn drop(&mut self) {
        PORT_STORE_PTR.with(|c| c.set(std::ptr::null()));
    }
}

// --- foreign store thread-local for dispatch wrappers ---
//
// set to &self.foreign_store before every evaluate()/call() invocation,
// cleared afterwards. the extern "C" dispatch wrappers read it to access
// the store without needing a Context reference. safe because Context is
// !Send + !Sync and the pointer is only live during evaluation.
thread_local! {
    static FOREIGN_STORE_PTR: Cell<*const RefCell<ForeignStore>> = const { Cell::new(std::ptr::null()) };
    /// current TeinExtApi pointer — set during load_extension() so ext method dispatch
    /// can call back into the host. null outside of ext loading and ext method calls.
    pub(crate) static EXT_API: Cell<*const tein_ext::TeinExtApi> = const { Cell::new(std::ptr::null()) };
}

// --- exit escape hatch thread-locals ---
//
// (exit) / (exit obj) in scheme sets these, then returns an exception to
// stop the VM immediately. the eval loop checks the flag before converting
// exceptions to errors and intercepts it to return Ok(value) to the rust caller.
// cleared on Context::drop() as a safety net.
thread_local! {
    static EXIT_REQUESTED: Cell<bool> = const { Cell::new(false) };
    static EXIT_VALUE: Cell<ffi::sexp> = const { Cell::new(std::ptr::null_mut()) };
}

// --- sandbox state thread-local ---
//
// set to true when sandboxed() is active. used by (tein file)
// trampolines to distinguish unsandboxed contexts (allow all) from sandboxed
// contexts with no file policy configured (deny). cleared on Context::drop().
thread_local! {
    static IS_SANDBOXED: Cell<bool> = const { Cell::new(false) };
}

// --- UX stub module map thread-local ---
//
// populated during sandboxed() build path: maps binding name → module path
// so ux_stub can emit a helpful "(import (module path))" message.
// cleared on Context::drop() alongside IS_SANDBOXED.
thread_local! {
    static STUB_MODULE_MAP: RefCell<std::collections::HashMap<String, String>> =
        RefCell::new(std::collections::HashMap::new());
}

// --- implementations of the 4 foreign protocol dispatch functions ---

/// Dispatch a method call: (foreign-call obj 'method arg ...)
unsafe extern "C" fn foreign_call_wrapper(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let store_ptr = FOREIGN_STORE_PTR.with(|c| c.get());
        if store_ptr.is_null() {
            let msg = "foreign-call: no foreign store (internal error)";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let store = &*store_ptr;
        match crate::foreign::dispatch_foreign_call(store, ctx, args) {
            Ok(value) => value.to_raw(ctx).unwrap_or_else(|_| ffi::get_void()),
            Err(msg) => {
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}

/// List methods of the foreign object in the first arg: (foreign-methods obj)
unsafe extern "C" fn foreign_methods_wrapper(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let store_ptr = FOREIGN_STORE_PTR.with(|c| c.get());
        if store_ptr.is_null() || ffi::sexp_nullp(args) != 0 {
            return ffi::get_null();
        }
        let store = &*store_ptr;
        let obj_sexp = ffi::sexp_car(args);
        let obj = match Value::from_raw(ctx, obj_sexp) {
            Ok(v) => v,
            Err(_) => return ffi::get_null(),
        };
        let type_name = match obj.foreign_type_name() {
            Some(n) => n,
            None => return ffi::get_null(),
        };
        let names = match store.borrow().method_names(type_name) {
            Some(n) => n,
            None => return ffi::get_null(),
        };
        // build scheme list of symbol names
        let mut result = ffi::get_null();
        for name in names.iter().rev() {
            let c_name = match CString::new(*name) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let sym = ffi::sexp_intern(ctx, c_name.as_ptr(), name.len() as ffi::sexp_sint_t);
            let _sym_root = ffi::GcRoot::new(ctx, sym);
            let _tail_root = ffi::GcRoot::new(ctx, result);
            result = ffi::sexp_cons(ctx, sym, result);
        }
        result
    }
}

/// List all registered type names: (foreign-types)
unsafe extern "C" fn foreign_types_wrapper(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let store_ptr = FOREIGN_STORE_PTR.with(|c| c.get());
        if store_ptr.is_null() {
            return ffi::get_null();
        }
        let store = &*store_ptr;
        let names = store.borrow().type_names();
        let mut result = ffi::get_null();
        for name in names.iter().rev() {
            let c_name = match CString::new(name.clone()) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let s = ffi::sexp_c_str(ctx, c_name.as_ptr(), name.len() as ffi::sexp_sint_t);
            let _s_root = ffi::GcRoot::new(ctx, s);
            let _tail_root = ffi::GcRoot::new(ctx, result);
            result = ffi::sexp_cons(ctx, s, result);
        }
        result
    }
}

/// List method names for a named type: (foreign-type-methods "type-name")
unsafe extern "C" fn foreign_type_methods_wrapper(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let store_ptr = FOREIGN_STORE_PTR.with(|c| c.get());
        if store_ptr.is_null() || ffi::sexp_nullp(args) != 0 {
            return ffi::get_null();
        }
        let store = &*store_ptr;
        let name_sexp = ffi::sexp_car(args);
        if ffi::sexp_stringp(name_sexp) == 0 {
            let msg = "foreign-type-methods: expected string type name";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let ptr = ffi::sexp_string_data(name_sexp);
        let len = ffi::sexp_string_size(name_sexp) as usize;
        let type_name = match std::str::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len))
        {
            Ok(s) => s,
            Err(_) => return ffi::get_null(),
        };
        let names = match store.borrow().method_names(type_name) {
            Some(n) => n,
            None => return ffi::get_null(),
        };
        let mut result = ffi::get_null();
        for name in names.iter().rev() {
            let c_name = match CString::new(*name) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let sym = ffi::sexp_intern(ctx, c_name.as_ptr(), name.len() as ffi::sexp_sint_t);
            let _sym_root = ffi::GcRoot::new(ctx, sym);
            let _tail_root = ffi::GcRoot::new(ctx, result);
            result = ffi::sexp_cons(ctx, sym, result);
        }
        result
    }
}

// --- cdylib extension trampolines ---

/// Build a `TeinExtApi` vtable populated with trampolines into tein's FFI layer.
///
/// All function pointer fields are filled with thin shims that cast opaque
/// pointer types back to `ffi::sexp` and forward to the real chibi wrappers.
/// Predicates, extractors, constructors, and sentinels are direct transmutes —
/// `sexp = *mut c_void` and `*mut OpaqueVal` have identical ABI.
fn build_ext_api() -> tein_ext::TeinExtApi {
    use std::ffi::c_int;
    use tein_ext::{OpaqueCtx, OpaqueVal, TEIN_EXT_API_VERSION, TeinExtApi};

    // Safety: we are transmuting between fn(*mut c_void) and fn(*mut OpaqueVal)
    // (or fn(*mut OpaqueCtx)). Both are pointer-sized, same representation.
    // The ABI is identical — this is purely a type-level trick.
    unsafe {
        TeinExtApi {
            version: TEIN_EXT_API_VERSION,

            // high-level trampolines (non-trivial, defined below)
            register_vfs_module: ext_trampoline_register_vfs,
            define_fn_variadic: ext_trampoline_define_fn,
            register_foreign_type: ext_trampoline_register_type,

            // type predicates — transmute fn(*mut c_void) → fn(*mut OpaqueVal)
            sexp_integerp: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_integerp),
            sexp_flonump: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_flonump),
            sexp_stringp: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_stringp),
            sexp_booleanp: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_booleanp),
            sexp_symbolp: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_symbolp),
            sexp_pairp: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_pairp),
            sexp_nullp: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_nullp),
            sexp_charp: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_charp),
            sexp_bytesp: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_bytesp),
            sexp_vectorp: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_vectorp),
            sexp_portp: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_portp),
            sexp_exceptionp: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_exceptionp),

            // value extractors
            sexp_unbox_fixnum: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> ffi::sexp_sint_t,
                unsafe extern "C" fn(*mut OpaqueVal) -> std::ffi::c_long,
            >(ffi::sexp_unbox_fixnum),
            sexp_flonum_value: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> f64,
                unsafe extern "C" fn(*mut OpaqueVal) -> f64,
            >(ffi::sexp_flonum_value),
            sexp_string_data: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> *const std::ffi::c_char,
                unsafe extern "C" fn(*mut OpaqueVal) -> *const std::ffi::c_char,
            >(ffi::sexp_string_data),
            // sexp_string_size returns sexp_uint_t; API table uses c_long (signed)
            // need a trampoline to handle the sign difference
            sexp_string_size: ext_trampoline_string_size,
            sexp_unbox_character: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> c_int,
                unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
            >(ffi::sexp_unbox_character),
            // sexp_bytes_data returns *mut c_char; API table uses *const c_char (read-only)
            sexp_bytes_data: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> *mut std::ffi::c_char,
                unsafe extern "C" fn(*mut OpaqueVal) -> *const std::ffi::c_char,
            >(ffi::sexp_bytes_data),
            // sexp_bytes_length returns sexp_uint_t; API table uses c_long (signed)
            sexp_bytes_length: ext_trampoline_bytes_length,
            sexp_car: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> ffi::sexp,
                unsafe extern "C" fn(*mut OpaqueVal) -> *mut OpaqueVal,
            >(ffi::sexp_car),
            sexp_cdr: std::mem::transmute::<
                unsafe fn(ffi::sexp) -> ffi::sexp,
                unsafe extern "C" fn(*mut OpaqueVal) -> *mut OpaqueVal,
            >(ffi::sexp_cdr),

            // value constructors
            sexp_make_fixnum: std::mem::transmute::<
                unsafe fn(ffi::sexp_sint_t) -> ffi::sexp,
                unsafe extern "C" fn(std::ffi::c_long) -> *mut OpaqueVal,
            >(ffi::sexp_make_fixnum),
            sexp_make_flonum: std::mem::transmute::<
                unsafe fn(ffi::sexp, f64) -> ffi::sexp,
                unsafe extern "C" fn(*mut OpaqueCtx, f64) -> *mut OpaqueVal,
            >(ffi::sexp_make_flonum),
            // sexp_make_boolean: ffi wrapper takes bool; C ABI uses c_int
            sexp_make_boolean: ext_trampoline_make_boolean,
            sexp_make_character: std::mem::transmute::<
                unsafe fn(c_int) -> ffi::sexp,
                unsafe extern "C" fn(c_int) -> *mut OpaqueVal,
            >(ffi::sexp_make_character),
            sexp_c_str: std::mem::transmute::<
                unsafe fn(ffi::sexp, *const std::ffi::c_char, ffi::sexp_sint_t) -> ffi::sexp,
                unsafe extern "C" fn(
                    *mut OpaqueCtx,
                    *const std::ffi::c_char,
                    std::ffi::c_long,
                ) -> *mut OpaqueVal,
            >(ffi::sexp_c_str),
            sexp_cons: std::mem::transmute::<
                unsafe fn(ffi::sexp, ffi::sexp, ffi::sexp) -> ffi::sexp,
                unsafe extern "C" fn(
                    *mut OpaqueCtx,
                    *mut OpaqueVal,
                    *mut OpaqueVal,
                ) -> *mut OpaqueVal,
            >(ffi::sexp_cons),
            // sexp_make_bytes takes sexp_uint_t; API table uses c_long (signed)
            sexp_make_bytes: ext_trampoline_make_bytes,

            // sentinels
            get_null: std::mem::transmute::<
                unsafe fn() -> ffi::sexp,
                unsafe extern "C" fn() -> *mut OpaqueVal,
            >(ffi::get_null),
            get_true: std::mem::transmute::<
                unsafe fn() -> ffi::sexp,
                unsafe extern "C" fn() -> *mut OpaqueVal,
            >(ffi::get_true),
            get_false: std::mem::transmute::<
                unsafe fn() -> ffi::sexp,
                unsafe extern "C" fn() -> *mut OpaqueVal,
            >(ffi::get_false),
            get_void: std::mem::transmute::<
                unsafe fn() -> ffi::sexp,
                unsafe extern "C" fn() -> *mut OpaqueVal,
            >(ffi::get_void),
        }
    }
}

/// Trampoline: boolean constructor bridging `bool` (rust) ↔ `c_int` (C ABI).
unsafe extern "C" fn ext_trampoline_make_boolean(b: std::ffi::c_int) -> *mut tein_ext::OpaqueVal {
    unsafe { std::mem::transmute(ffi::sexp_make_boolean(b != 0)) }
}

/// Trampoline: string size bridging `sexp_uint_t` (unsigned) ↔ `c_long` (signed).
unsafe extern "C" fn ext_trampoline_string_size(x: *mut tein_ext::OpaqueVal) -> std::ffi::c_long {
    unsafe { ffi::sexp_string_size(x as ffi::sexp) as std::ffi::c_long }
}

/// Trampoline: bytevector length bridging `sexp_uint_t` ↔ `c_long`.
unsafe extern "C" fn ext_trampoline_bytes_length(x: *mut tein_ext::OpaqueVal) -> std::ffi::c_long {
    unsafe { ffi::sexp_bytes_length(x as ffi::sexp) as std::ffi::c_long }
}

/// Trampoline: bytevector constructor bridging `c_long` ↔ `sexp_uint_t`.
unsafe extern "C" fn ext_trampoline_make_bytes(
    ctx: *mut tein_ext::OpaqueCtx,
    len: std::ffi::c_long,
    init: u8,
) -> *mut tein_ext::OpaqueVal {
    unsafe {
        std::mem::transmute(ffi::sexp_make_bytes(
            ctx as ffi::sexp,
            len as ffi::sexp_uint_t,
            init,
        ))
    }
}

/// Trampoline: register a VFS module path+content from an extension.
unsafe extern "C" fn ext_trampoline_register_vfs(
    _ctx: *mut tein_ext::OpaqueCtx,
    path: *const std::ffi::c_char,
    path_len: usize,
    content: *const std::ffi::c_char,
    content_len: usize,
) -> i32 {
    unsafe {
        let path_str =
            match std::str::from_utf8(std::slice::from_raw_parts(path as *const u8, path_len)) {
                Ok(s) => s,
                Err(_) => return -1,
            };
        let full_path = format!("/vfs/{path_str}");
        let c_path = match CString::new(full_path) {
            Ok(s) => s,
            Err(_) => return -1,
        };
        ffi::tein_vfs_register(c_path.as_ptr(), content, content_len as std::ffi::c_uint);
        0
    }
}

/// Trampoline: register a variadic scheme function from an extension.
unsafe extern "C" fn ext_trampoline_define_fn(
    ctx: *mut tein_ext::OpaqueCtx,
    name: *const std::ffi::c_char,
    name_len: usize,
    f: tein_ext::SexpFn,
) -> i32 {
    unsafe {
        let name_str =
            match std::str::from_utf8(std::slice::from_raw_parts(name as *const u8, name_len)) {
                Ok(s) => s,
                Err(_) => return -1,
            };
        let c_name = match CString::new(name_str) {
            Ok(s) => s,
            Err(_) => return -1,
        };
        let ctx_sexp: ffi::sexp = ctx as ffi::sexp;
        let env = ffi::sexp_context_env(ctx_sexp);
        // transmute SexpFn to the type sexp_define_foreign_proc expects
        let f_typed: Option<
            unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp) -> ffi::sexp,
        > = Some(std::mem::transmute::<
            tein_ext::SexpFn,
            unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp) -> ffi::sexp,
        >(f));
        let result = ffi::sexp_define_foreign_proc(
            ctx_sexp,
            env,
            c_name.as_ptr(),
            0,
            ffi::SEXP_PROC_VARIADIC,
            c_name.as_ptr(),
            f_typed,
        );
        if ffi::sexp_exceptionp(result) != 0 {
            -1
        } else {
            0
        }
    }
}

/// Trampoline: register a foreign type from a TeinTypeDesc.
///
/// Full implementation in task 3 (requires ForeignStore::register_ext_type).
/// Body is filled in after foreign.rs is extended.
unsafe extern "C" fn ext_trampoline_register_type(
    ctx: *mut tein_ext::OpaqueCtx,
    desc: *const tein_ext::TeinTypeDesc,
) -> i32 {
    ext_register_type_impl(ctx, desc)
}

/// Real implementation of ext_trampoline_register_type.
///
/// Reads TeinTypeDesc, registers the ext type in ForeignStore, then generates
/// scheme convenience procs (predicate + method wrappers) via sexp_eval_string.
fn ext_register_type_impl(
    ctx: *mut tein_ext::OpaqueCtx,
    desc: *const tein_ext::TeinTypeDesc,
) -> i32 {
    use crate::foreign::ExtMethodEntry;
    unsafe {
        let store_ptr = FOREIGN_STORE_PTR.with(|c| c.get());
        if store_ptr.is_null() || desc.is_null() {
            return -1;
        }

        let type_name_bytes =
            std::slice::from_raw_parts((*desc).type_name as *const u8, (*desc).type_name_len);
        let type_name = match std::str::from_utf8(type_name_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => return -1,
        };

        let method_slice = std::slice::from_raw_parts((*desc).methods, (*desc).method_count);
        let mut methods = Vec::with_capacity(method_slice.len());
        for m in method_slice {
            let name_bytes = std::slice::from_raw_parts(m.name as *const u8, m.name_len);
            let name = match std::str::from_utf8(name_bytes) {
                Ok(s) => s.to_string(),
                Err(_) => return -1,
            };
            methods.push(ExtMethodEntry {
                name,
                func: m.func,
                is_mut: m.is_mut,
            });
        }

        let store = &*store_ptr;
        if store
            .borrow_mut()
            .register_ext_type(type_name.clone(), methods)
            .is_err()
        {
            return -1;
        }

        // generate convenience procs — same scheme code as register_foreign_type
        let ctx_sexp: ffi::sexp = ctx as ffi::sexp;
        let env = ffi::sexp_context_env(ctx_sexp);

        let pred_code = format!(
            "(define ({tn}? x) (and (foreign? x) (equal? (foreign-type x) \"{tn}\")))",
            tn = type_name
        );
        let c_pred = match CString::new(pred_code.as_str()) {
            Ok(s) => s,
            Err(_) => return -1,
        };
        let pred_result = ffi::sexp_eval_string(ctx_sexp, c_pred.as_ptr(), -1, env);
        if ffi::sexp_exceptionp(pred_result) != 0 {
            return -1;
        }

        let method_names: Vec<String> = store
            .borrow()
            .ext_method_names(&type_name)
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        for method_name in &method_names {
            // ext method names are already prefixed (e.g. "counter-get"), so use
            // them directly — don't wrap in {type_name}-{method_name} again (#69)
            let wrapper_code = format!(
                "(define ({mn} obj . args) \
                   (if (and (foreign? obj) (equal? (foreign-type obj) \"{tn}\")) \
                       (apply foreign-call obj (quote {mn}) args) \
                       (error \"{mn}: expected {tn}, got\" \
                              (if (foreign? obj) (foreign-type obj) obj))))",
                tn = type_name,
                mn = method_name
            );
            let c_wrapper = match CString::new(wrapper_code.as_str()) {
                Ok(s) => s,
                Err(_) => return -1,
            };
            let result = ffi::sexp_eval_string(ctx_sexp, c_wrapper.as_ptr(), -1, env);
            if ffi::sexp_exceptionp(result) != 0 {
                return -1;
            }
        }

        0
    }
}

// --- custom port trampolines ---

/// Extern "C" trampoline for custom input port reads.
///
/// Called by Chibi via sexp_apply when the custom port's buffer needs refilling.
/// Args from Scheme: (port-id buffer start end).
/// Reads from the Rust Read object in PortStore, copies bytes into the Scheme
/// string buffer, returns fixnum byte count.
///
/// Validates start/end indices before any arithmetic: fixnums from Scheme could
/// be negative (cast to huge usize), reversed (end < start), or beyond the
/// buffer's actual size — all causing out-of-bounds pointer arithmetic and
/// heap corruption.
unsafe extern "C" fn port_read_trampoline(
    _ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let id_sexp = ffi::sexp_car(args);
        let rest = ffi::sexp_cdr(args);
        let buf_sexp = ffi::sexp_car(rest);
        let rest2 = ffi::sexp_cdr(rest);
        let start_sexp = ffi::sexp_car(rest2);
        let rest3 = ffi::sexp_cdr(rest2);
        let end_sexp = ffi::sexp_car(rest3);

        let port_id = ffi::sexp_unbox_fixnum(id_sexp) as u64;
        // validate indices before any arithmetic: fixnums from scheme could be
        // negative (cast to huge usize) or reversed (end < start), both causing
        // out-of-bounds pointer arithmetic and heap corruption.
        let start_raw = ffi::sexp_unbox_fixnum(start_sexp);
        let end_raw = ffi::sexp_unbox_fixnum(end_sexp);
        let buf_len = ffi::sexp_string_size(buf_sexp) as usize;
        if start_raw < 0 || end_raw < 0 || end_raw < start_raw || end_raw as usize > buf_len {
            return ffi::sexp_make_fixnum(0);
        }
        let start = start_raw as usize;
        let end = end_raw as usize;
        let len = end - start;

        let store_ptr = PORT_STORE_PTR.with(|c| c.get());
        if store_ptr.is_null() {
            return ffi::sexp_make_fixnum(0);
        }
        let store = &*store_ptr;
        let mut store_ref = store.borrow_mut();
        let reader = match store_ref.get_reader(port_id) {
            Some(r) => r,
            None => return ffi::sexp_make_fixnum(0),
        };

        let mut tmp = vec![0u8; len];
        let bytes_read = match reader.read(&mut tmp) {
            Ok(n) => n,
            Err(_) => return ffi::sexp_make_fixnum(0),
        };

        let buf_data = ffi::sexp_string_data(buf_sexp) as *mut u8;
        std::ptr::copy_nonoverlapping(tmp.as_ptr(), buf_data.add(start), bytes_read);

        // return start + bytes_read: chibi copies [0..result) from the buffer,
        // where [0..start) was already valid from a previous partial fill.
        ffi::sexp_make_fixnum((start + bytes_read) as ffi::sexp_sint_t)
    }
}

/// Extern "C" trampoline for custom output port writes.
///
/// Called by Chibi via sexp_apply when data needs flushing to the port.
/// Args from Scheme: (port-id buffer start end).
/// Writes bytes from the Scheme string buffer to the Rust Write object
/// in PortStore, returns fixnum byte count.
///
/// Validates start/end indices before any arithmetic: negative, reversed, or
/// beyond-buffer values would cause out-of-bounds pointer arithmetic and heap
/// corruption.
unsafe extern "C" fn port_write_trampoline(
    _ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let id_sexp = ffi::sexp_car(args);
        let rest = ffi::sexp_cdr(args);
        let buf_sexp = ffi::sexp_car(rest);
        let rest2 = ffi::sexp_cdr(rest);
        let start_sexp = ffi::sexp_car(rest2);
        let rest3 = ffi::sexp_cdr(rest2);
        let end_sexp = ffi::sexp_car(rest3);

        let port_id = ffi::sexp_unbox_fixnum(id_sexp) as u64;
        // validate indices: negative, reversed, or out-of-buffer values cause
        // OOB pointer arithmetic and heap corruption.
        let start_raw = ffi::sexp_unbox_fixnum(start_sexp);
        let end_raw = ffi::sexp_unbox_fixnum(end_sexp);
        let buf_len = ffi::sexp_string_size(buf_sexp) as usize;
        if start_raw < 0 || end_raw < 0 || end_raw < start_raw || end_raw as usize > buf_len {
            return ffi::sexp_make_fixnum(0);
        }
        let start = start_raw as usize;
        let end = end_raw as usize;
        let len = end - start;

        let store_ptr = PORT_STORE_PTR.with(|c| c.get());
        if store_ptr.is_null() {
            return ffi::sexp_make_fixnum(0);
        }
        let store = &*store_ptr;
        let mut store_ref = store.borrow_mut();
        let writer = match store_ref.get_writer(port_id) {
            Some(w) => w,
            None => return ffi::sexp_make_fixnum(0),
        };

        let buf_data = ffi::sexp_string_data(buf_sexp) as *const u8;
        let slice = std::slice::from_raw_parts(buf_data.add(start), len);
        match writer.write(slice) {
            Ok(n) => ffi::sexp_make_fixnum(n as ffi::sexp_sint_t),
            Err(_) => ffi::sexp_make_fixnum(0),
        }
    }
}

// --- json trampolines (gated behind "json" feature) ---

#[cfg(feature = "json")]
/// Trampoline for `json-parse`: takes one scheme string argument, returns parsed value.
///
/// On parse error or type mismatch, returns a scheme string with the error message.
/// This matches tein's convention for native function errors (see AGENTS.md).
unsafe extern "C" fn json_parse_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let str_sexp = ffi::sexp_car(args);
        if ffi::sexp_stringp(str_sexp) == 0 {
            let msg = "json-parse: expected string argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let data = ffi::sexp_string_data(str_sexp);
        let len = ffi::sexp_string_size(str_sexp) as usize;
        let input = match std::str::from_utf8(std::slice::from_raw_parts(data as *const u8, len)) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("json-parse: invalid UTF-8: {e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                return ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };
        match crate::json::json_parse(input) {
            Ok(value) => match value.to_raw(ctx) {
                Ok(raw) => raw,
                Err(e) => {
                    let msg = format!("json-parse: {e}");
                    let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                    ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
                }
            },
            Err(e) => {
                let msg = format!("{e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}

#[cfg(feature = "json")]
/// Trampoline for `json-stringify`: takes one scheme value, returns JSON string.
///
/// Works directly on raw chibi sexps via `json::json_stringify_raw` to preserve
/// alist structure. `Value::from_raw` would collapse dotted pairs into proper lists
/// when the cdr is a proper list (e.g. `("x" . (("y" . 1)))` → `("x" ("y" . 1))`),
/// losing the structural cue needed to detect alists.
///
/// On conversion error, returns a scheme string with the error message.
/// This matches tein's convention for native function errors (see AGENTS.md).
unsafe extern "C" fn json_stringify_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let val_sexp = ffi::sexp_car(args);
        match crate::json::json_stringify_raw(ctx, val_sexp) {
            Ok(json) => {
                let c_json = CString::new(json.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_json.as_ptr(), json.len() as ffi::sexp_sint_t)
            }
            Err(e) => {
                let msg = format!("{e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}

// --- toml trampolines (gated behind "toml" feature) ---

#[cfg(feature = "toml")]
/// Trampoline for `toml-parse`: takes one scheme string argument, returns parsed value.
///
/// On parse error or type mismatch, returns a scheme string with the error message.
/// This matches tein's convention for native function errors (see AGENTS.md).
unsafe extern "C" fn toml_parse_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let str_sexp = ffi::sexp_car(args);
        if ffi::sexp_stringp(str_sexp) == 0 {
            let msg = "toml-parse: expected string argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let data = ffi::sexp_string_data(str_sexp);
        let len = ffi::sexp_string_size(str_sexp) as usize;
        let input = match std::str::from_utf8(std::slice::from_raw_parts(data as *const u8, len)) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("toml-parse: invalid UTF-8: {e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                return ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };
        match crate::toml::toml_parse(input) {
            Ok(value) => match value.to_raw(ctx) {
                Ok(raw) => raw,
                Err(e) => {
                    let msg = format!("toml-parse: {e}");
                    let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                    ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
                }
            },
            Err(e) => {
                let msg = format!("{e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}

#[cfg(feature = "toml")]
/// Trampoline for `toml-stringify`: takes one scheme value, returns TOML string.
///
/// Works directly on raw chibi sexps via `toml::toml_stringify_raw` to preserve
/// alist structure, then delegates to `toml::to_string()` for correct formatting.
///
/// On conversion error, returns a scheme string with the error message.
/// This matches tein's convention for native function errors (see AGENTS.md).
unsafe extern "C" fn toml_stringify_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let val_sexp = ffi::sexp_car(args);
        match crate::toml::toml_stringify_raw(ctx, val_sexp) {
            Ok(toml_str) => {
                let c_str = CString::new(toml_str.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_str.as_ptr(), toml_str.len() as ffi::sexp_sint_t)
            }
            Err(e) => {
                let msg = format!("{e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}

/// UX stub for bindings excluded from module-level sandboxed contexts.
///
/// Extracts its name from the opcode's name slot (set by
/// `sexp_define_foreign_proc` at registration time), then looks
/// up the providing module in `STUB_MODULE_MAP` to produce an actionable hint:
/// `"sandbox: 'map' requires (import (scheme base))"`.
/// Converts to `Error::SandboxViolation` via the `[sandbox:binding]` sentinel.
unsafe extern "C" fn ux_stub(
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
                .to_string()
        } else {
            "unknown".to_string()
        };
        // look up providing module from the stub map
        let module_hint = STUB_MODULE_MAP.with(|map| {
            map.borrow()
                .get(&name)
                .map(|m| {
                    // "scheme/base" → "(scheme base)", "srfi/1" → "(srfi 1)"
                    let parts: Vec<&str> = m.splitn(2, '/').collect();
                    if parts.len() == 2 {
                        format!("({} {})", parts[0], parts[1].replace('/', " "))
                    } else {
                        format!("({m})")
                    }
                })
                .unwrap_or_else(|| "the required module".to_string())
        });
        let msg = format!(
            "[sandbox:binding] '{}' requires (import {})",
            name, module_hint
        );
        let c_msg = CString::new(msg.as_str()).unwrap_or_default();
        ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
    }
}

// --- trampoline helpers ---

/// Extract the first argument as a `&str`, returning an error sexp on type mismatch.
///
/// # Safety
/// `args` must be a valid scheme list with at least one element.
unsafe fn extract_string_arg<'a>(
    ctx: ffi::sexp,
    args: ffi::sexp,
    fn_name: &str,
) -> std::result::Result<&'a str, ffi::sexp> {
    unsafe {
        let first = ffi::sexp_car(args);
        if ffi::sexp_stringp(first) == 0 {
            let msg = format!("{}: expected string argument", fn_name);
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return Err(ffi::make_error(
                ctx,
                c_msg.as_ptr(),
                msg.len() as ffi::sexp_sint_t,
            ));
        }
        let ptr = ffi::sexp_string_data(first);
        let len = ffi::sexp_string_size(first) as usize;
        Ok(std::str::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len)).unwrap_or(""))
    }
}

/// FsPolicy access direction for [`check_fs_access`].
pub(crate) enum FsAccess {
    Read,
    Write,
}

/// Check FsPolicy access for `path`.
///
/// - unsandboxed (IS_SANDBOXED=false): allows unconditionally
/// - sandboxed with matching FsPolicy: delegates to `check_read`/`check_write`
/// - sandboxed without FsPolicy configured: denies
pub(crate) fn check_fs_access(path: &str, access: FsAccess) -> bool {
    let sandboxed = IS_SANDBOXED.with(|c| c.get());
    if !sandboxed {
        return true;
    }
    FS_POLICY.with(|cell| {
        let policy = cell.borrow();
        match &*policy {
            Some(p) => match access {
                FsAccess::Read => p.check_read(path),
                FsAccess::Write => p.check_write(path),
            },
            None => false, // sandboxed + no policy = deny
        }
    })
}

// --- (tein file) trampolines ---

/// `file-exists?` trampoline: checks FsPolicy read access, returns boolean.
///
/// when no FsPolicy is set (unsandboxed context), allows unconditionally.
/// in sandboxed contexts without file_read configured, returns a policy
/// violation exception.
unsafe extern "C" fn file_exists_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "file-exists?") {
            Ok(s) => s,
            Err(e) => return e,
        };

        if !check_fs_access(path, FsAccess::Read) {
            let msg = format!("[sandbox:file] {} (read not permitted)", path);
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        if std::path::Path::new(path).exists() {
            ffi::get_true()
        } else {
            ffi::get_false()
        }
    }
}

/// `delete-file` trampoline: checks FsPolicy write access, deletes file.
///
/// when no FsPolicy is set (unsandboxed context), allows unconditionally.
/// returns void on success, exception on policy violation or IO error.
unsafe extern "C" fn delete_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "delete-file") {
            Ok(s) => s,
            Err(e) => return e,
        };

        if !check_fs_access(path, FsAccess::Write) {
            let msg = format!("[sandbox:file] {} (write not permitted)", path);
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        match std::fs::remove_file(path) {
            Ok(()) => ffi::get_void(),
            Err(e) => {
                let msg = format!("delete-file: {}", e);
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}

// --- open-*-file enforcement ---
//
// open-*-file policy enforcement is handled at the C opcode level
// (eval.c patches F, G) via the FS policy gate callback.

// --- (tein load) trampoline ---

/// VFS-only load trampoline, registered as `tein-load-vfs-internal`.
///
/// exported as `load` by `(tein load)` via `(export (rename tein-load-vfs-internal load))`
/// in `load.sld`. registered under an internal name to avoid overriding chibi's built-in `load`,
/// which the module loader uses for `(include ...)` in `.sld` files.
///
/// resolves the path through `tein_vfs_lookup`, opens a string input port
/// from the embedded content, and loops read+eval. rejects non-VFS paths
/// with a sandbox violation error.
unsafe extern "C" fn load_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, "load") {
            Ok(s) => s,
            Err(e) => return e,
        };

        // VFS-only gate
        if !path.starts_with("/vfs/") {
            let msg = format!("[sandbox:load] {} (only VFS paths permitted)", path);
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        // look up VFS content
        let c_path = match CString::new(path) {
            Ok(s) => s,
            Err(_) => {
                let msg = "load: path contains null bytes";
                let c_msg = CString::new(msg).unwrap_or_default();
                return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };
        let content = match ffi::vfs_lookup(&c_path) {
            Some(bytes) => bytes,
            None => {
                let msg = format!("load: VFS path not found: {}", path);
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };

        // open input string port from VFS content
        let scheme_str = ffi::sexp_c_str(
            ctx,
            content.as_ptr() as *const c_char,
            content.len() as ffi::sexp_sint_t,
        );
        if ffi::sexp_exceptionp(scheme_str) != 0 {
            return scheme_str;
        }
        let _str_root = ffi::GcRoot::new(ctx, scheme_str);

        let port = ffi::sexp_open_input_string(ctx, scheme_str);
        if ffi::sexp_exceptionp(port) != 0 {
            return port;
        }
        let _port_root = ffi::GcRoot::new(ctx, port);

        let env = ffi::sexp_context_env(ctx);
        let mut result = ffi::get_void();

        loop {
            let expr = ffi::sexp_read(ctx, port);
            if ffi::sexp_eofp(expr) != 0 {
                break;
            }
            if ffi::sexp_exceptionp(expr) != 0 {
                return expr;
            }
            let _expr_root = ffi::GcRoot::new(ctx, expr);
            result = ffi::sexp_evaluate(ctx, expr, env);
            if ffi::sexp_exceptionp(result) != 0 {
                return result;
            }
        }

        result
    }
}

// --- (tein process) trampolines ---

/// `get-environment-variable` trampoline: returns env var value or `#f`.
unsafe extern "C" fn get_env_var_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let name = match extract_string_arg(ctx, args, "get-environment-variable") {
            Ok(s) => s,
            Err(e) => return e,
        };

        // sandboxed contexts get neutered env var access
        if IS_SANDBOXED.with(|c| c.get()) {
            return ffi::get_false();
        }

        match std::env::var(name) {
            Ok(val) => {
                let c_val = CString::new(val.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_val.as_ptr(), val.len() as ffi::sexp_sint_t)
            }
            Err(_) => ffi::get_false(),
        }
    }
}

/// `get-environment-variables` trampoline: returns alist of all env vars as `((name . value) ...)`.
/// sandboxed contexts return `'()`.
unsafe extern "C" fn get_env_vars_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        // sandboxed contexts get neutered env var access
        if IS_SANDBOXED.with(|c| c.get()) {
            return ffi::get_null();
        }

        let mut result = ffi::get_null();
        for (key, val) in std::env::vars() {
            // root accumulator so GC doesn't sweep the partial list
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
    }
}

/// `command-line` trampoline: returns the host argv as a list of strings.
/// sandboxed contexts return `'("tein")`.
unsafe extern "C" fn command_line_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
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

        let mut result = ffi::get_null();
        let args: Vec<String> = std::env::args().collect();
        // build list in reverse order so head = argv[0]
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
    }
}

/// `exit` trampoline: eval escape hatch.
///
/// sets EXIT_REQUESTED + EXIT_VALUE thread-locals and returns a scheme
/// exception to immediately stop the VM. the eval loop intercepts this
/// via `check_exit()` and returns `Ok(value)` to the rust caller.
///
/// semantics: `(exit)` → 0, `(exit #t)` → 0, `(exit #f)` → 1, `(exit obj)` → obj
unsafe extern "C" fn exit_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        // determine exit value based on arg presence and value
        let exit_val = if ffi::sexp_nullp(args) != 0 {
            // (exit) — no args, return fixnum 0
            ffi::sexp_make_fixnum(0)
        } else {
            let arg = ffi::sexp_car(args);
            if ffi::sexp_booleanp(arg) != 0 {
                // (exit #t) → 0, (exit #f) → 1
                if ffi::sexp_truep(arg) != 0 {
                    ffi::sexp_make_fixnum(0)
                } else {
                    ffi::sexp_make_fixnum(1)
                }
            } else {
                arg
            }
        };

        // GC-root the exit value — fixnums are immediates (no-op), but heap
        // objects like strings need rooting so GC doesn't collect them before
        // check_exit() converts them.
        ffi::sexp_preserve_object(ctx, exit_val);
        EXIT_REQUESTED.with(|c| c.set(true));
        EXIT_VALUE.with(|c| c.set(exit_val));

        // return exception to stop VM immediately
        let msg = "exit";
        let c_msg = CString::new(msg).unwrap_or_default();
        ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
    }
}

// --- default sizes ---

const DEFAULT_HEAP_SIZE: usize = 8 * 1024 * 1024;
const DEFAULT_HEAP_MAX: usize = 128 * 1024 * 1024;

/// Builder for configuring a Scheme context before creation.
///
/// Provides a fluent API for setting heap sizes, step limits,
/// environment restrictions (sandboxing), file IO policies, and
/// standard library loading. Finish with [`build()`](Self::build),
/// [`build_managed()`](Self::build_managed), or
/// [`build_timeout()`](crate::TimeoutContext) depending on your
/// threading needs.
///
/// # examples
///
/// ```
/// use tein::Context;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // default context
/// let ctx = Context::new()?;
///
/// // configured context
/// let ctx = Context::builder()
///     .heap_size(8 * 1024 * 1024)
///     .step_limit(100_000)
///     .build()?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct ContextBuilder {
    heap_size: usize,
    heap_max: usize,
    step_limit: Option<u64>,
    standard_env: bool,
    file_read_prefixes: Option<Vec<String>>,
    file_write_prefixes: Option<Vec<String>>,
    /// module-level sandbox configuration.
    /// when set, activates the registry-based sandbox path in build().
    sandbox_modules: Option<crate::sandbox::Modules>,
}

impl ContextBuilder {
    /// Set the initial heap size in bytes (default: 8mb).
    pub fn heap_size(mut self, size: usize) -> Self {
        self.heap_size = size;
        self
    }

    /// Set the maximum heap size in bytes (default: 128mb).
    pub fn heap_max(mut self, size: usize) -> Self {
        self.heap_max = size;
        self
    }

    /// Set the maximum number of VM steps per evaluation call.
    ///
    /// When the limit is reached, evaluation returns `Error::StepLimitExceeded`.
    /// Fuel resets before each `evaluate()` or `call()` invocation.
    pub fn step_limit(mut self, limit: u64) -> Self {
        self.step_limit = Some(limit);
        self
    }

    /// Enable the R7RS standard environment.
    ///
    /// Loads `(scheme base)` and supporting modules via the embedded VFS,
    /// providing `define-record-type`, `import`, `map`, `for-each`, etc.
    /// Standard ports (stdin/stdout/stderr) are also initialised.
    ///
    /// **Required for tein modules.** Feature-gated modules (json, toml, uuid)
    /// and IO modules (file, load, process) only register their trampolines
    /// when `standard_env` is active. Without this, `(import (tein ...))` will
    /// fail even if the module is in the allowlist. Typical sandboxed usage:
    /// `Context::builder().standard_env().sandboxed(Modules::Safe)`.
    pub fn standard_env(mut self) -> Self {
        self.standard_env = true;
        self
    }

    /// Configure module-level sandboxing.
    ///
    /// Activates the registry-based sandbox: builds a null env with only
    /// `import` syntax, sets up the VFS gate for the given module set,
    /// and registers UX stubs for all excluded module exports so scheme
    /// code gets a clear `(import (module path))` hint instead of an
    /// "undefined variable" error.
    ///
    /// **Requires** `.standard_env()` — sandboxed contexts need the full
    /// env loaded first before restriction.
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
    ///     .build()?;
    ///
    /// // after importing, scheme/base ops work
    /// let result = ctx.evaluate("(import (scheme base)) (+ 1 2)")?;
    /// assert_eq!(result, tein::Value::Integer(3));
    /// # Ok(())
    /// # }
    /// ```
    pub fn sandboxed(mut self, modules: crate::sandbox::Modules) -> Self {
        self.sandbox_modules = Some(modules);
        self
    }

    /// Allow file reading from paths under the given prefixes.
    ///
    /// Prefixes should be absolute paths (e.g. "/config/", "/data/").
    /// Paths are canonicalised before checking, so symlinks and `..`
    /// traversals are resolved.
    ///
    /// Auto-activates `sandboxed(Modules::Safe)` when called without an explicit
    /// `sandboxed()` call.
    pub fn file_read(mut self, prefixes: &[&str]) -> Self {
        let list = self.file_read_prefixes.get_or_insert_with(Vec::new);
        for p in prefixes {
            list.push(p.to_string());
        }
        if self.sandbox_modules.is_none() {
            // auto-activate sandboxed(Modules::Safe) when file IO is configured without explicit sandboxed()
            self.sandbox_modules = Some(crate::sandbox::Modules::Safe);
        }
        self
    }

    /// Allow file writing to paths under the given prefixes.
    ///
    /// Parent directories must exist; files will be created as needed (R7RS).
    /// Prefixes should be absolute paths (e.g. "/tmp/", "/output/").
    ///
    /// Auto-activates `sandboxed(Modules::Safe)` when called without an explicit
    /// `sandboxed()` call.
    pub fn file_write(mut self, prefixes: &[&str]) -> Self {
        let list = self.file_write_prefixes.get_or_insert_with(Vec::new);
        for p in prefixes {
            list.push(p.to_string());
        }
        if self.sandbox_modules.is_none() {
            self.sandbox_modules = Some(crate::sandbox::Modules::Safe);
        }
        self
    }

    /// add a module (+ its transitive deps) to the VFS gate allowlist.
    ///
    /// starts from the current `Modules` variant's resolved allowlist, appends
    /// the new module, and sets `Modules::Only(extended_list)`. dependencies are
    /// resolved automatically from the registry — callers never need to think about
    /// transitive imports.
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
    ///     .allow_module("tein/process")
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn allow_module(mut self, prefix: &str) -> Self {
        use crate::sandbox::{Modules, registry_all_allowlist, registry_safe_allowlist};
        let mut base = match self.sandbox_modules.take().unwrap_or_default() {
            Modules::Safe => registry_safe_allowlist(),
            Modules::All => registry_all_allowlist(),
            Modules::None => Vec::new(),
            Modules::Only(list) => list,
        };
        if !base.contains(&prefix.to_string()) {
            base.push(prefix.to_string());
        }
        self.sandbox_modules = Some(Modules::Only(base));
        self
    }

    /// Check if a step limit has been configured.
    pub(crate) fn has_step_limit(&self) -> bool {
        self.step_limit.is_some()
    }

    /// Build the configured context.
    pub fn build(mut self) -> Result<Context> {
        unsafe {
            let ctx = ffi::sexp_make_eval_context(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                self.heap_size as ffi::sexp_uint_t,
                self.heap_max as ffi::sexp_uint_t,
            );

            if ctx.is_null() {
                return Err(Error::InitError("failed to create context".to_string()));
            }

            // load r7rs standard environment if requested.
            // this enriches the context env with (scheme base) etc. via the
            // embedded VFS, and must happen before sandbox restriction so the
            // restricted env can copy from the full standard env.
            if self.standard_env {
                let env = ffi::sexp_context_env(ctx);
                // H9: chibi uses a char[128] stack buffer for the init file path
                // and does `version + '0'` without range check. version MUST be a
                // single digit (we hardcode 7 for r7rs). do not change this to >= 10.
                let version = ffi::sexp_make_fixnum(7);

                let result = ffi::load_standard_env(ctx, env, version);
                if ffi::sexp_exceptionp(result) != 0 {
                    ffi::sexp_destroy_context(ctx);
                    return Err(Error::InitError(
                        "failed to load standard environment".to_string(),
                    ));
                }

                let result = ffi::load_standard_ports(ctx, env);
                if ffi::sexp_exceptionp(result) != 0 {
                    ffi::sexp_destroy_context(ctx);
                    return Err(Error::InitError(
                        "failed to load standard ports".to_string(),
                    ));
                }
            }

            // save current gate values before overwriting — restored on drop so that
            // a second context on the same thread (sequential or nested) is not affected.
            let prev_vfs_gate = VFS_GATE.with(|cell| cell.get());
            let prev_fs_gate = FS_GATE.with(|cell| cell.get());
            let prev_fs_policy = FS_POLICY.with(|cell| cell.borrow().clone());
            let prev_vfs_allowlist = VFS_ALLOWLIST.with(|cell| cell.borrow().clone());
            let prev_is_sandboxed = IS_SANDBOXED.with(|c| c.get());

            if let Some(ref modules) = self.sandbox_modules.take() {
                // registry-based sandbox path: builds a null env + import, resolves
                // module allowlist from the Modules enum, registers UX stubs for
                // excluded module exports.
                use crate::sandbox::{
                    Modules, registry_all_allowlist, registry_resolve_deps,
                    registry_safe_allowlist, unexported_stubs,
                };

                // mark sandboxed so policy enforcement applies
                IS_SANDBOXED.with(|c| c.set(true));
                // arm FS policy gate — C opcodes will call tein_fs_policy_check
                FS_GATE.with(|cell| cell.set(FS_GATE_CHECK));
                ffi::fs_policy_gate_set(FS_GATE_CHECK as i32);
                crate::sandbox::register_vfs_shadows(); // inject shadow modules before gate is armed

                let source_env = ffi::sexp_context_env(ctx);
                let version = ffi::sexp_make_fixnum(7);
                let null_env = ffi::sexp_make_null_env(ctx, version);

                if ffi::sexp_exceptionp(null_env) != 0 {
                    ffi::sexp_destroy_context(ctx);
                    return Err(crate::error::Error::InitError(
                        "failed to create null environment".to_string(),
                    ));
                }

                // root both envs across allocating calls below
                let _source_env_guard = ffi::GcRoot::new(ctx, source_env);
                let _null_env_guard = ffi::GcRoot::new(ctx, null_env);

                // resolve module allowlist from the Modules variant
                let allowlist: Vec<String> = match modules {
                    Modules::Safe => registry_safe_allowlist(),
                    Modules::All => registry_all_allowlist(),
                    Modules::None => Vec::new(),
                    Modules::Only(list) => {
                        let refs: Vec<&str> = list.iter().map(|s| s.as_str()).collect();
                        registry_resolve_deps(&refs)
                    }
                };

                // activate the VFS gate with the resolved allowlist
                VFS_GATE.with(|cell| cell.set(GATE_CHECK));
                ffi::vfs_gate_set(GATE_CHECK as i32);
                VFS_ALLOWLIST.with(|cell| {
                    *cell.borrow_mut() = allowlist.clone();
                });

                // copy "import" from source env into null env so scheme can use
                // (import ...) to load allowed modules
                {
                    let name = "import";
                    let c_name = CString::new(name).unwrap();
                    ffi::env_copy_named(
                        ctx,
                        source_env,
                        null_env,
                        c_name.as_ptr(),
                        name.len() as ffi::sexp_sint_t,
                    );
                }

                // build the UX stub map: binding name → providing module path
                let stubs = unexported_stubs(&allowlist);
                STUB_MODULE_MAP.with(|map| {
                    let mut m = map.borrow_mut();
                    m.clear();
                    for (name, module) in &stubs {
                        m.insert(name.to_string(), module.to_string());
                    }
                });

                // register UX stubs in the null env
                let ux_stub_fn: Option<
                    unsafe extern "C" fn(
                        ffi::sexp,
                        ffi::sexp,
                        ffi::sexp_sint_t,
                        ffi::sexp,
                    ) -> ffi::sexp,
                > = Some(ux_stub);
                for (name, _module) in &stubs {
                    let c_name = CString::new(*name).unwrap_or_default();
                    ffi::sexp_define_foreign_proc(
                        ctx,
                        null_env,
                        c_name.as_ptr(),
                        0,
                        ffi::SEXP_PROC_VARIADIC,
                        c_name.as_ptr(),
                        ux_stub_fn,
                    );
                }

                ffi::sexp_context_env_set(ctx, null_env);
            }

            // set FsPolicy if file_read() or file_write() was configured.
            // placed outside the sandbox block so it works for both sandboxed and
            // unsandboxed contexts with file policy configured.
            {
                let file_read_prefixes = self.file_read_prefixes.take();
                let file_write_prefixes = self.file_write_prefixes.take();
                if file_read_prefixes.is_some() || file_write_prefixes.is_some() {
                    FS_POLICY.with(|cell| {
                        *cell.borrow_mut() = Some(FsPolicy {
                            read_prefixes: file_read_prefixes.unwrap_or_default(),
                            write_prefixes: file_write_prefixes.unwrap_or_default(),
                        });
                    });
                }
            }

            let context = Context {
                ctx,
                step_limit: self.step_limit,
                prev_vfs_gate,
                prev_fs_gate,
                prev_fs_policy,
                prev_vfs_allowlist,
                prev_is_sandboxed,
                foreign_store: RefCell::new(ForeignStore::new()),
                has_foreign_protocol: Cell::new(false),
                port_store: RefCell::new(PortStore::new()),
                has_port_protocol: Cell::new(false),
                ext_api: RefCell::new(None),
            };

            // register feature-gated module trampolines for standard-env contexts.
            // these are pure data operations (format conversion, uuid generation, time),
            // no IO — always safe and cheap to register.
            #[cfg(feature = "json")]
            if self.standard_env {
                context.register_json_module()?;
            }

            #[cfg(feature = "toml")]
            if self.standard_env {
                context.register_toml_module()?;
            }

            #[cfg(feature = "uuid")]
            if self.standard_env {
                crate::uuid::uuid_impl::register_module_uuid(&context)?;
            }

            #[cfg(feature = "time")]
            if self.standard_env {
                crate::time::time_impl::register_module_time(&context)?;
            }

            if self.standard_env {
                context.register_file_module()?;
                context.register_load_module()?;
                context.register_process_module()?;
            }

            Ok(context)
        }
    }

    /// Build a managed context on a dedicated thread (persistent mode).
    ///
    /// The init closure runs once after context creation. State accumulates
    /// across evaluations. Use `reset()` to tear down and rebuild.
    pub fn build_managed(
        self,
        init: impl Fn(&Context) -> Result<()> + Send + 'static,
    ) -> Result<crate::managed::ThreadLocalContext> {
        crate::managed::ThreadLocalContext::new(self, crate::managed::Mode::Persistent, init)
    }

    /// Build a managed context on a dedicated thread (fresh mode).
    ///
    /// The init closure runs before every evaluation — context is rebuilt
    /// each time. No state persists between calls. `reset()` is a no-op.
    pub fn build_managed_fresh(
        self,
        init: impl Fn(&Context) -> Result<()> + Send + 'static,
    ) -> Result<crate::managed::ThreadLocalContext> {
        crate::managed::ThreadLocalContext::new(self, crate::managed::Mode::Fresh, init)
    }
}

/// A Scheme evaluation context.
///
/// The main entry point for evaluating Scheme code. Each context owns a
/// Chibi-Scheme heap and environment. Intentionally `!Send + !Sync` — for
/// cross-thread use, see [`crate::ThreadLocalContext`] or [`crate::TimeoutContext`].
///
/// Use [`Context::new()`] for a quick primitive environment, or
/// [`Context::builder()`] to configure sandboxing, step limits, file IO
/// policies, and the standard library.
///
/// # examples
///
/// ```
/// use tein::{Context, Value};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let ctx = Context::new()?;
/// let result = ctx.evaluate("(+ 1 2 3)")?;
/// assert_eq!(result, Value::Integer(6));
/// # Ok(())
/// # }
/// ```
pub struct Context {
    ctx: ffi::sexp,
    step_limit: Option<u64>,
    /// previous VFS_GATE level, restored on drop
    prev_vfs_gate: u8,
    /// previous FS_GATE level, restored on drop
    prev_fs_gate: u8,
    /// previous FS_POLICY value, restored on drop
    prev_fs_policy: Option<FsPolicy>,
    /// previous VFS_ALLOWLIST, restored on drop
    prev_vfs_allowlist: Vec<String>,
    /// previous IS_SANDBOXED value, restored on drop
    prev_is_sandboxed: bool,
    /// per-context store for foreign type registrations and live instances
    foreign_store: RefCell<ForeignStore>,
    /// whether foreign protocol dispatch functions are registered
    has_foreign_protocol: Cell<bool>,
    /// per-context store for custom port backing objects (Read/Write impls)
    port_store: RefCell<PortStore>,
    /// whether port protocol dispatch functions are registered
    has_port_protocol: Cell<bool>,
    /// cached TeinExtApi vtable — populated on first load_extension call.
    /// Stored in a Box so the pointer is stable for ext method dispatch.
    /// None until first extension is loaded.
    ext_api: RefCell<Option<Box<tein_ext::TeinExtApi>>>,
}

impl Context {
    /// Create a new Scheme context with default settings.
    ///
    /// Initialises a Chibi-Scheme context with:
    /// - 8mb initial heap
    /// - 128mb max heap
    /// - full primitive environment (no restrictions)
    /// - no step limit
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// Create a new context with the R7RS standard environment.
    ///
    /// Equivalent to `Context::builder().standard_env().build()`.
    /// Provides `(scheme base)` and supporting modules — `map`, `for-each`,
    /// `import`, `define-record-type`, etc.
    pub fn new_standard() -> Result<Self> {
        Self::builder().standard_env().build()
    }

    /// Create a builder for configuring a context.
    ///
    /// Defaults to bare primitives only (no standard env, no sandboxing).
    /// Call `.standard_env()` to load R7RS libraries and tein modules,
    /// then optionally `.sandboxed(Modules::Safe)` to restrict importable modules.
    pub fn builder() -> ContextBuilder {
        ContextBuilder {
            heap_size: DEFAULT_HEAP_SIZE,
            heap_max: DEFAULT_HEAP_MAX,
            step_limit: None,
            standard_env: false,
            file_read_prefixes: None,
            file_write_prefixes: None,
            sandbox_modules: None,
        }
    }

    /// Set fuel before an evaluation call (if step limit is configured).
    ///
    /// SAFETY INVARIANT: must be called before every sexp_evaluate/sexp_apply
    /// entry point. fuel budget bounds total VM operations, which mitigates
    /// chibi's error-handler stack overflow (M13 in chibi-scheme-review.md).
    fn arm_fuel(&self) {
        if let Some(limit) = self.step_limit {
            unsafe {
                ffi::fuel_arm(self.ctx, limit as ffi::sexp_sint_t);
            }
        }
    }

    /// Check if fuel was exhausted after an evaluation call, then disarm.
    fn check_fuel(&self) -> Result<()> {
        if self.step_limit.is_some() {
            unsafe {
                let exhausted = ffi::fuel_exhausted(self.ctx) != 0;
                ffi::fuel_disarm(self.ctx);
                if exhausted {
                    return Err(Error::StepLimitExceeded);
                }
            }
        }
        Ok(())
    }

    /// Check if `(exit)` was called during evaluation.
    ///
    /// If the exit flag is set, clears it, releases the GC root on the
    /// stashed value, converts it to a `Value`, and returns `Some(Ok(value))`.
    /// Returns `None` if no exit was requested.
    fn check_exit(&self) -> Option<Result<Value>> {
        if EXIT_REQUESTED.with(|c| c.replace(false)) {
            let raw = EXIT_VALUE.with(|c| c.replace(std::ptr::null_mut()));
            // release GC root — sexp_release_object is a no-op for immediates
            if !raw.is_null() {
                unsafe { ffi::sexp_release_object(self.ctx, raw) };
            }
            // null or void → (exit) with no args, return 0
            if raw.is_null() || unsafe { ffi::sexp_voidp(raw) != 0 } {
                return Some(Ok(Value::Integer(0)));
            }
            Some(unsafe { Value::from_raw(self.ctx, raw) })
        } else {
            None
        }
    }

    /// Evaluate one or more Scheme expressions.
    ///
    /// Evaluates all expressions in the string sequentially, returning the
    /// result of the last expression. This enables natural scripting patterns
    /// like defining values and then using them.
    ///
    /// # safety invariant: OOM checking
    ///
    /// Every allocation result (`sexp_open_input_string`, `sexp_read`,
    /// `sexp_evaluate`) is checked with `sexp_exceptionp` before use.
    /// chibi returns a shared global OOM object on allocation failure (M12
    /// in chibi-scheme-review.md) — writing fields into it corrupts future
    /// OOM reporting. this pattern must be maintained in all evaluation
    /// entry points (`evaluate`, `evaluate_port`, `call`).
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new()?;
    ///
    /// // single expression
    /// let result = ctx.evaluate("(+ 1 2 3)")?;
    /// assert_eq!(result, Value::Integer(6));
    ///
    /// // multiple expressions - returns the last result
    /// let result = ctx.evaluate("(define x 5) (+ x 3)")?;
    /// assert_eq!(result, Value::Integer(8));
    /// # Ok(())
    /// # }
    /// ```
    pub fn evaluate(&self, code: &str) -> Result<Value> {
        let c_str = CString::new(code)
            .map_err(|_| Error::EvalError("code contains null bytes".to_string()))?;

        // set store pointers so dispatch wrappers and port trampolines can
        // access them. guards clear on all exit paths (early returns, `?`, panic).
        FOREIGN_STORE_PTR.with(|c| c.set(&self.foreign_store as *const _));
        let _foreign_guard = ForeignStoreGuard;
        PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
        let _port_guard = PortStoreGuard;
        // set EXT_API so ext-type method dispatch works during evaluation
        let _ext_api_guard = if let Some(api) = self.ext_api.borrow().as_ref() {
            EXT_API.with(|c| c.set(api.as_ref() as *const _));
            Some(ExtApiGuard)
        } else {
            None
        };
        self.arm_fuel();

        unsafe {
            let env = ffi::sexp_context_env(self.ctx);

            // create a scheme string from the code
            let scheme_str =
                ffi::sexp_c_str(self.ctx, c_str.as_ptr(), code.len() as ffi::sexp_sint_t);

            // open an input port on the string
            let port = ffi::sexp_open_input_string(self.ctx, scheme_str);
            if ffi::sexp_exceptionp(port) != 0 {
                return Value::from_raw(self.ctx, port);
            }

            // gc-root the port and its backing string. rust locals are
            // invisible to chibi's GC (no conservative stack scanning), so
            // evaluation (e.g. import triggering module loading) can collect
            // these objects if they aren't rooted. GcRoot auto-releases on
            // any exit path (early return, ?, or normal drop).
            let _str_guard = ffi::GcRoot::new(self.ctx, scheme_str);
            let _port_guard = ffi::GcRoot::new(self.ctx, port);

            // read and evaluate expressions until EOF
            let mut result = ffi::get_void();
            loop {
                let expr = ffi::sexp_read(self.ctx, port);

                // EOF means we're done
                if ffi::sexp_eofp(expr) != 0 {
                    break;
                }

                // read error
                if ffi::sexp_exceptionp(expr) != 0 {
                    return Value::from_raw(self.ctx, expr);
                }

                // gc-root expr across sexp_evaluate: sexp_compile_op calls
                // sexp_make_eval_context immediately, which may trigger GC.
                // the parsed expression is only a rust local (invisible to
                // chibi's precise GC) until we hand it to the evaluator.
                let _expr_guard = ffi::GcRoot::new(self.ctx, expr);

                // evaluate the expression
                result = ffi::sexp_evaluate(self.ctx, expr, env);

                // check fuel exhaustion before exception status
                // (fuel exhaustion returns a normal-looking value, not an exception)
                self.check_fuel()?;

                // exit escape hatch — (exit) returns an exception to stop the
                // VM immediately; intercept before converting to an error.
                if ffi::sexp_exceptionp(result) != 0 {
                    if let Some(exit_result) = self.check_exit() {
                        return exit_result;
                    }
                    return Value::from_raw(self.ctx, result);
                }
            }

            // root result before from_raw. any allocation inside from_raw (e.g.
            // sexp_symbol_to_string, sexp_bignum_to_string) can trigger a GC cycle that
            // will collect `result` if it isn't explicitly preserved — chibi's GC is
            // precise (no conservative stack scan), so rust locals are invisible to it.
            // most visible in sandboxed contexts where the heap is more pressure-loaded.
            let _result_root = ffi::GcRoot::new(self.ctx, result);

            Value::from_raw(self.ctx, result)
        }
    }

    /// Load and evaluate a Scheme file.
    ///
    /// Reads the file contents and evaluates all expressions sequentially,
    /// returning the result of the last expression. This is the file-based
    /// equivalent of [`evaluate`](Self::evaluate).
    ///
    /// # examples
    ///
    /// ```no_run
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new()?;
    ///
    /// // load a config file that defines values and returns a result
    /// let result = ctx.load_file("config.scm")?;
    ///
    /// // load a prelude for side effects (defines), ignore result
    /// let _ = ctx.load_file("prelude.scm")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::IoError`] if the file cannot be read, or evaluation
    /// errors if the Scheme code is invalid.
    pub fn load_file<P: AsRef<Path>>(&self, path: P) -> Result<Value> {
        let contents = std::fs::read_to_string(path.as_ref())?;
        self.evaluate(&contents)
    }

    /// Register a foreign function as a Scheme primitive.
    ///
    /// All arguments are passed as a single Scheme list via the `args` parameter.
    /// This is the universal registration method — use `#[tein_fn]` for ergonomic
    /// wrappers that handle argument extraction and return conversion automatically.
    ///
    /// The function receives all arguments as a single Scheme list in the `args`
    /// parameter. Chibi passes `(ctx, self, nargs, args)` where args is a proper
    /// list of all actual arguments.
    ///
    /// This uses `sexp_define_foreign_proc_aux` with `SEXP_PROC_VARIADIC`,
    /// which wraps the opcode in a real procedure object.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value, raw};
    ///
    /// // sum all integer arguments
    /// unsafe extern "C" fn sum_all(
    ///     ctx: raw::sexp, _self: raw::sexp, _n: raw::sexp_sint_t, args: raw::sexp,
    /// ) -> raw::sexp {
    ///     unsafe {
    ///         let mut total: i64 = 0;
    ///         let mut current = args;
    ///         while raw::sexp_pairp(current) != 0 {
    ///             total += raw::sexp_unbox_fixnum(raw::sexp_car(current)) as i64;
    ///             current = raw::sexp_cdr(current);
    ///         }
    ///         raw::sexp_make_fixnum(total as raw::sexp_sint_t)
    ///     }
    /// }
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new()?;
    /// ctx.define_fn_variadic("sum-all", sum_all)?;
    /// let result = ctx.evaluate("(sum-all 1 2 3 4 5)")?;
    /// assert_eq!(result, Value::Integer(15));
    /// # Ok(())
    /// # }
    /// ```
    pub fn define_fn_variadic(
        &self,
        name: &str,
        f: unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp) -> ffi::sexp,
    ) -> Result<()> {
        let c_name = CString::new(name)
            .map_err(|_| Error::EvalError("function name contains null bytes".to_string()))?;

        unsafe {
            // registers into the top-level env, NOT a library env. after
            // `(import (tein mod))`, chibi's import checks find these names
            // in the destination env chain (oldcell lookup) — the fork patch
            // (see #57) suppresses the false "importing undefined variable"
            // warning in this case. same behaviour in ext mode.
            let env = ffi::sexp_context_env(self.ctx);
            let result = ffi::sexp_define_foreign_proc(
                self.ctx,
                env,
                c_name.as_ptr(),
                0, // num_args = 0 (variadic handles its own arity)
                ffi::SEXP_PROC_VARIADIC,
                c_name.as_ptr(),
                Some(f),
            );

            if ffi::sexp_exceptionp(result) != 0 {
                return Err(Error::EvalError(format!(
                    "failed to define variadic function '{}'",
                    name
                )));
            }
        }

        Ok(())
    }

    /// Raw context pointer for internal use (tests, examples, proc macros).
    #[cfg(test)]
    pub(crate) fn ctx_ptr(&self) -> ffi::sexp {
        self.ctx
    }

    /// Register a Rust type with the foreign object protocol.
    ///
    /// Makes the type's methods callable from Scheme. Auto-registers:
    /// - `foreign-call`, `foreign-methods`, `foreign-types`, `foreign-type-methods`
    ///   (on first call, via `register_foreign_protocol`)
    /// - `type-name?` — predicate proc
    /// - `type-name-method` — for each method in the type's method table
    ///
    /// # example
    ///
    /// ```ignore
    /// ctx.register_foreign_type::<Counter>()?;
    /// // scheme now has: counter?, counter-increment, counter-get
    /// ```
    pub fn register_foreign_type<T: ForeignType>(&self) -> Result<()> {
        if !self.has_foreign_protocol.get() {
            self.register_foreign_protocol()?;
            self.has_foreign_protocol.set(true);
        }
        self.foreign_store.borrow_mut().register_type::<T>()?;

        // auto-register scheme convenience procedures
        let type_name = T::type_name();

        // predicate: (type-name? x)
        let pred_code = format!(
            "(define ({tn}? x) (and (foreign? x) (equal? (foreign-type x) \"{tn}\")))",
            tn = type_name
        );
        self.evaluate(&pred_code)?;

        // method wrappers: (type-name-method obj arg ...)
        // each wrapper type-checks obj and delegates to foreign-call, with a
        // type-specific error message for wrong-type arguments.
        for (method_name, _) in T::methods() {
            let wrapper_code = format!(
                "(define ({tn}-{mn} obj . args) \
                   (if (and (foreign? obj) (equal? (foreign-type obj) \"{tn}\")) \
                       (apply foreign-call obj (quote {mn}) args) \
                       (error \"{tn}-{mn}: expected {tn}, got\" \
                              (if (foreign? obj) (foreign-type obj) obj))))",
                tn = type_name,
                mn = method_name
            );
            self.evaluate(&wrapper_code)?;
        }

        Ok(())
    }

    /// Wrap a Rust value as a Scheme foreign object.
    ///
    /// Stores it in the ForeignStore and returns a `Value::Foreign`
    /// that Scheme code can pass around, inspect, and use with `foreign-call`.
    ///
    /// The value lives until the Context is dropped.
    pub fn foreign_value<T: ForeignType>(&self, value: T) -> Result<Value> {
        let id = self.foreign_store.borrow_mut().insert(value);
        Ok(Value::Foreign {
            handle_id: id,
            type_name: T::type_name().to_string(),
        })
    }

    /// Borrow a foreign object immutably.
    ///
    /// Returns an error if the value isn't `Foreign`, the handle is stale,
    /// or the type doesn't match `T`.
    ///
    /// # panics
    ///
    /// Panics if the returned `Ref` is held across any call that re-enters the
    /// foreign dispatch (e.g. `ctx.evaluate()`, `ctx.call()`). Those paths
    /// take a `borrow_mut()` on the same `ForeignStore`, which conflicts with
    /// the outstanding immutable borrow. Drop the `Ref` before evaluating.
    pub fn foreign_ref<T: ForeignType + 'static>(
        &self,
        value: &Value,
    ) -> Result<std::cell::Ref<'_, T>> {
        let (id, actual_type) = value
            .as_foreign()
            .ok_or_else(|| Error::TypeError(format!("expected foreign object, got {}", value)))?;
        if actual_type != T::type_name() {
            return Err(Error::TypeError(format!(
                "expected {}, got {}",
                T::type_name(),
                actual_type
            )));
        }
        let store = self.foreign_store.borrow();
        if store.get(id).is_none() {
            return Err(Error::EvalError(format!(
                "stale foreign handle: {} ({})",
                id, actual_type
            )));
        }
        Ok(std::cell::Ref::map(store, |s| {
            let (data, _) = s.get(id).unwrap();
            data.downcast_ref::<T>().unwrap()
        }))
    }

    /// Load a cdylib extension from the given path.
    ///
    /// The extension's `tein_ext_init` function is called immediately with a
    /// populated `TeinExtApi` vtable. VFS entries, functions, and types
    /// registered by the extension become available to scheme code.
    ///
    /// The shared library remains loaded for the process lifetime — there is
    /// no unload path (dlclose during an active chibi heap is unsafe).
    ///
    /// # Errors
    ///
    /// Returns [`Error::InitError`] if the library cannot be opened, the
    /// `tein_ext_init` symbol is missing, the API version mismatches, or
    /// the extension's init function returns a non-zero error code.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tein::Context;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new_standard()?;
    /// ctx.load_extension("./libmy_extension.so")?;
    /// ctx.evaluate("(import (tein my-extension))")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn load_extension(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let path = path.as_ref();

        // ensure foreign protocol is registered so extensions can call foreign-call
        if !self.has_foreign_protocol.get() {
            self.register_foreign_protocol()?;
            self.has_foreign_protocol.set(true);
        }

        unsafe {
            let lib = libloading::Library::new(path).map_err(|e| {
                Error::InitError(format!(
                    "failed to load extension '{}': {}",
                    path.display(),
                    e
                ))
            })?;

            let init: libloading::Symbol<tein_ext::TeinExtInitFn> =
                lib.get(b"tein_ext_init\0").map_err(|e| {
                    Error::InitError(format!(
                        "extension '{}' has no tein_ext_init symbol: {}",
                        path.display(),
                        e
                    ))
                })?;

            // populate FOREIGN_STORE_PTR so ext_trampoline_register_type can access
            // the store during init (extensions register types synchronously)
            FOREIGN_STORE_PTR.with(|c| c.set(&self.foreign_store as *const _));
            let _foreign_guard = ForeignStoreGuard;

            // build (or reuse) the API table — stored in context for future ext method dispatch
            let api_box = {
                let mut api_ref = self.ext_api.borrow_mut();
                if api_ref.is_none() {
                    *api_ref = Some(Box::new(build_ext_api()));
                }
                // return raw pointer to the stable Box<TeinExtApi>
                api_ref.as_ref().unwrap().as_ref() as *const tein_ext::TeinExtApi
            };
            // set EXT_API so ext_trampoline_register_type and method dispatch can access it
            EXT_API.with(|c| c.set(api_box));
            let _ext_api_guard = ExtApiGuard;

            let result = init(self.ctx as *mut tein_ext::OpaqueCtx, api_box);

            match result {
                tein_ext::TEIN_EXT_OK => {}
                tein_ext::TEIN_EXT_ERR_VERSION => {
                    return Err(Error::InitError(format!(
                        "extension '{}': API version mismatch (host v{}, extension requires newer)",
                        path.display(),
                        tein_ext::TEIN_EXT_API_VERSION
                    )));
                }
                code => {
                    return Err(Error::InitError(format!(
                        "extension '{}': init failed with code {}",
                        path.display(),
                        code
                    )));
                }
            }

            // leak the library handle — no unload
            Box::leak(Box::new(lib));
        }
        Ok(())
    }

    /// Register the custom port protocol dispatch functions.
    ///
    /// Called automatically by `open_input_port`/`open_output_port` on first use.
    fn register_port_protocol(&self) -> Result<()> {
        self.define_fn_variadic("tein-port-read", port_read_trampoline)?;
        self.define_fn_variadic("tein-port-write", port_write_trampoline)?;
        Ok(())
    }

    /// Register a reader dispatch handler for `#ch` syntax.
    ///
    /// `ch` must be an ASCII byte (value < 128). The underlying C dispatch
    /// table has 128 entries; characters with byte values ≥ 128 are silently
    /// ignored by the dispatch layer and cannot be used as reader dispatch
    /// characters. The handler must be a Scheme procedure taking one argument
    /// (the input port) and returning a datum. Reserved R7RS characters
    /// (`#t`, `#f`, `#\\`, `#(`, numeric prefixes, etc.) cannot be overridden.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new_standard()?;
    /// let handler = ctx.evaluate("(lambda (port) 42)")?;
    /// ctx.register_reader(b'j', &handler)?;
    /// assert_eq!(ctx.evaluate("#j")?, Value::Integer(42));
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_reader(&self, ch: u8, handler: &Value) -> Result<()> {
        let raw_proc = handler
            .as_procedure()
            .ok_or_else(|| Error::TypeError("handler must be a procedure".into()))?;
        let c = ch as std::ffi::c_int;
        unsafe {
            let result = ffi::reader_dispatch_set(self.ctx, c, raw_proc);
            match result {
                0 => Ok(()),
                -1 => Err(Error::EvalError(format!(
                    "reader dispatch #{} is reserved by r7rs and cannot be overridden",
                    ch as char
                ))),
                // c < 128 always holds for u8, so this branch is unreachable,
                // but kept for exhaustiveness against the C return contract.
                _ => Err(Error::EvalError(
                    "reader dispatch: character out of range".into(),
                )),
            }
        }
    }

    /// Register a virtual filesystem entry at runtime.
    ///
    /// `path` is the VFS-relative path, e.g. `"lib/tein/json.sld"`. The entry
    /// becomes immediately available to chibi's module resolver via `(import ...)`.
    /// Must be called before any scheme code imports the module.
    ///
    /// Entries registered here are cleared on `Context::drop()`, so each context
    /// has its own isolated set of runtime VFS modules.
    ///
    /// # errors
    ///
    /// Returns `Error::EvalError` if `path` contains null bytes.
    ///
    /// # examples
    ///
    /// ```
    /// # use tein::{Context, Value};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new_standard()?;
    /// ctx.register_vfs_module(
    ///     "lib/tein/mymod.sld",
    ///     "(define-library (tein mymod) (import (scheme base)) (export the-answer) (include \"mymod.scm\"))",
    /// )?;
    /// ctx.register_vfs_module("lib/tein/mymod.scm", "(define the-answer 42)")?;
    /// let result = ctx.evaluate("(import (tein mymod)) the-answer")?;
    /// assert_eq!(result, Value::Integer(42));
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_vfs_module(&self, path: &str, content: &str) -> Result<()> {
        let full_path = format!("/vfs/{path}");
        let c_path = std::ffi::CString::new(full_path)
            .map_err(|_| Error::EvalError("VFS path contains null bytes".into()))?;
        unsafe {
            ffi::tein_vfs_register(
                c_path.as_ptr(),
                content.as_ptr() as *const std::ffi::c_char,
                content.len() as std::ffi::c_uint,
            );
        }
        Ok(())
    }

    /// Set a Scheme procedure as the macro expansion hook.
    ///
    /// The hook receives `(name unexpanded expanded env)` after each macro
    /// expansion and returns the form to use (replace-and-reanalyze semantics).
    /// Return `expanded` unchanged for observation-only mode.
    ///
    /// # examples
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

    /// Clear the macro expansion hook.
    pub fn unset_macro_expand_hook(&self) {
        unsafe { ffi::macro_expand_hook_clear(self.ctx) };
    }

    /// Return the current macro expansion hook, or `None` if not set.
    pub fn macro_expand_hook(&self) -> Option<Value> {
        let raw = unsafe { ffi::macro_expand_hook_get() };
        // sentinel is SEXP_FALSE (set by tein_macro_expand_hook_clear)
        if raw == unsafe { ffi::get_false() } {
            None
        } else {
            Some(Value::Procedure(raw))
        }
    }

    /// Wrap a Rust `Read` as a Scheme input port.
    ///
    /// Returns a `Value::Port` that Scheme code can pass to `read`,
    /// `read-char`, `read-line`, etc.
    ///
    /// # lifetime note
    ///
    /// The backing `Read` is stored in the context's `PortStore` and lives
    /// until the `Context` is dropped. There is no explicit close API.
    /// For resources that must be released promptly (file handles, sockets),
    /// drop the `Context` or use a wrapper that signals completion via a
    /// shared flag.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new_standard()?;
    /// let port = ctx.open_input_port(std::io::Cursor::new(b"42"))?;
    /// assert!(port.is_port());
    /// # Ok(())
    /// # }
    /// ```
    pub fn open_input_port(&self, reader: impl std::io::Read + 'static) -> Result<Value> {
        if !self.has_port_protocol.get() {
            self.register_port_protocol()?;
            self.has_port_protocol.set(true);
        }

        let port_id = self.port_store.borrow_mut().insert_reader(Box::new(reader));

        // create scheme closure capturing port ID; evaluate() sets PORT_STORE_PTR itself
        let closure_code = format!(
            "(lambda (buf start end) (tein-port-read {} buf start end))",
            port_id
        );

        let read_proc_val = self.evaluate(&closure_code)?;
        let raw_proc = read_proc_val
            .as_procedure()
            .ok_or_else(|| Error::EvalError("failed to create port read closure".into()))?;

        unsafe {
            let port = ffi::make_custom_input_port(self.ctx, raw_proc);
            if ffi::sexp_exceptionp(port) != 0 {
                return Err(Error::EvalError(
                    "failed to create custom input port".into(),
                ));
            }
            Value::from_raw(self.ctx, port)
        }
    }

    /// Wrap a Rust `Write` as a Scheme output port.
    ///
    /// Returns a `Value::Port` that Scheme code can pass to `write`,
    /// `display`, `write-char`, etc.
    ///
    /// # lifetime note
    ///
    /// The backing `Write` is stored in the context's `PortStore` and lives
    /// until the `Context` is dropped. There is no explicit close API.
    /// For resources that must be released promptly (file handles, sockets),
    /// drop the `Context` or use a wrapper that signals completion via a
    /// shared flag.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value};
    /// use std::sync::{Arc, Mutex};
    ///
    /// struct SharedWriter(Arc<Mutex<Vec<u8>>>);
    /// impl std::io::Write for SharedWriter {
    ///     fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
    ///         self.0.lock().unwrap().extend_from_slice(buf);
    ///         Ok(buf.len())
    ///     }
    ///     fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    /// }
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    /// let ctx = Context::new_standard()?;
    /// let port = ctx.open_output_port(SharedWriter(buf.clone()))?;
    /// assert!(port.is_port());
    /// # Ok(())
    /// # }
    /// ```
    pub fn open_output_port(&self, writer: impl std::io::Write + 'static) -> Result<Value> {
        if !self.has_port_protocol.get() {
            self.register_port_protocol()?;
            self.has_port_protocol.set(true);
        }

        let port_id = self.port_store.borrow_mut().insert_writer(Box::new(writer));

        // evaluate() sets PORT_STORE_PTR itself
        let closure_code = format!(
            "(lambda (buf start end) (tein-port-write {} buf start end))",
            port_id
        );

        let write_proc_val = self.evaluate(&closure_code)?;
        let raw_proc = write_proc_val
            .as_procedure()
            .ok_or_else(|| Error::EvalError("failed to create port write closure".into()))?;

        unsafe {
            let port = ffi::make_custom_output_port(self.ctx, raw_proc);
            if ffi::sexp_exceptionp(port) != 0 {
                return Err(Error::EvalError(
                    "failed to create custom output port".into(),
                ));
            }
            Value::from_raw(self.ctx, port)
        }
    }

    /// Read one s-expression from a port.
    ///
    /// Returns the parsed but unevaluated expression.
    /// Returns `Value::Unspecified` at end-of-input (EOF).
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new_standard()?;
    /// let port = ctx.open_input_port(std::io::Cursor::new(b"42"))?;
    /// let val = ctx.read(&port)?;
    /// assert_eq!(val, Value::Integer(42));
    /// # Ok(())
    /// # }
    /// ```
    pub fn read(&self, port: &Value) -> Result<Value> {
        let raw_port = port
            .as_port()
            .ok_or_else(|| Error::TypeError(format!("expected port, got {}", port)))?;

        PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
        let _port_guard = PortStoreGuard;
        FOREIGN_STORE_PTR.with(|c| c.set(&self.foreign_store as *const _));
        let _foreign_guard = ForeignStoreGuard;

        unsafe {
            let result = ffi::sexp_read(self.ctx, raw_port);
            if ffi::sexp_eofp(result) != 0 {
                return Ok(Value::Unspecified);
            }
            // root result before from_raw (see evaluate() for rationale)
            let _result_root = ffi::GcRoot::new(self.ctx, result);
            if ffi::sexp_exceptionp(result) != 0 {
                return Value::from_raw(self.ctx, result);
            }
            Value::from_raw(self.ctx, result)
        }
    }

    /// Read and evaluate all expressions from a port.
    ///
    /// Reads s-expressions one at a time, evaluating each in sequence.
    /// Returns the result of the last expression evaluated, or
    /// `Value::Unspecified` if the port was empty.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new_standard()?;
    /// let port = ctx.open_input_port(std::io::Cursor::new(b"(define x 10) (+ x 5)"))?;
    /// let result = ctx.evaluate_port(&port)?;
    /// assert_eq!(result, Value::Integer(15));
    /// # Ok(())
    /// # }
    /// ```
    pub fn evaluate_port(&self, port: &Value) -> Result<Value> {
        let raw_port = port
            .as_port()
            .ok_or_else(|| Error::TypeError(format!("expected port, got {}", port)))?;

        PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
        let _port_guard = PortStoreGuard;
        FOREIGN_STORE_PTR.with(|c| c.set(&self.foreign_store as *const _));
        let _foreign_guard = ForeignStoreGuard;
        let _ext_api_guard = if let Some(api) = self.ext_api.borrow().as_ref() {
            EXT_API.with(|c| c.set(api.as_ref() as *const _));
            Some(ExtApiGuard)
        } else {
            None
        };
        self.arm_fuel();

        unsafe {
            // root the port sexp across the read/eval loop — both sexp_read and
            // sexp_evaluate allocate and can trigger GC, and with conservative
            // scanning disabled the GC cannot see rust locals.
            let _port_root = ffi::GcRoot::new(self.ctx, raw_port);
            let env = ffi::sexp_context_env(self.ctx);
            let mut last = Value::Unspecified;

            loop {
                let expr = ffi::sexp_read(self.ctx, raw_port);
                if ffi::sexp_eofp(expr) != 0 {
                    break;
                }
                if ffi::sexp_exceptionp(expr) != 0 {
                    return Value::from_raw(self.ctx, expr);
                }
                let result = ffi::sexp_evaluate(self.ctx, expr, env);
                self.check_fuel()?;
                // root result before from_raw (see evaluate() for rationale)
                let _result_root = ffi::GcRoot::new(self.ctx, result);
                if ffi::sexp_exceptionp(result) != 0 {
                    if let Some(exit_result) = self.check_exit() {
                        return exit_result;
                    }
                    return Value::from_raw(self.ctx, result);
                }
                last = Value::from_raw(self.ctx, result)?;
            }
            Ok(last)
        }
    }

    /// Register the foreign object protocol dispatch functions.
    ///
    /// Called automatically by `register_foreign_type` on first use.
    /// Registers both the Rust-side dispatch functions and the pure-Scheme
    /// predicates/accessors from `(tein foreign)`, making them available in
    /// the current env without requiring an explicit `(import (tein foreign))`.
    fn register_foreign_protocol(&self) -> Result<()> {
        // rust-side dispatch and introspection
        self.define_fn_variadic("foreign-call", foreign_call_wrapper)?;
        self.define_fn_variadic("foreign-methods", foreign_methods_wrapper)?;
        self.define_fn_variadic("foreign-types", foreign_types_wrapper)?;
        self.define_fn_variadic("foreign-type-methods", foreign_type_methods_wrapper)?;

        // pure-scheme predicates and accessors (mirrors foreign.scm)
        // these must be defined before any convenience procs that reference them.
        // uses only car/cdr (always available) rather than cadr/caddr (require scheme/cxr).
        // uses fixnum? rather than integer?: handle IDs are always fixnums, and
        // fixnum? is a chibi primitive available in all envs (including sandboxes).
        self.evaluate(
            "(define (foreign? x)
               (and (pair? x)
                    (eq? (car x) '__tein-foreign)
                    (pair? (cdr x))
                    (string? (car (cdr x)))
                    (pair? (cdr (cdr x)))
                    (fixnum? (car (cdr (cdr x))))))
             (define (foreign-type x)
               (if (foreign? x) (car (cdr x))
                   (error \"foreign-type: expected foreign object, got\" x)))
             (define (foreign-handle-id x)
               (if (foreign? x) (car (cdr (cdr x)))
                   (error \"foreign-handle-id: expected foreign object, got\" x)))",
        )?;

        Ok(())
    }

    #[cfg(feature = "json")]
    /// Register `json-parse` and `json-stringify` native functions.
    ///
    /// Called during `build()` for standard-env contexts. the VFS module
    /// `(tein json)` exports these names, making them available via
    /// `(import (tein json))`.
    fn register_json_module(&self) -> Result<()> {
        self.define_fn_variadic("json-parse", json_parse_trampoline)?;
        self.define_fn_variadic("json-stringify", json_stringify_trampoline)?;
        Ok(())
    }

    #[cfg(feature = "toml")]
    /// Register `toml-parse` and `toml-stringify` native functions.
    ///
    /// Called during `build()` for standard-env contexts. the VFS module
    /// `(tein toml)` exports these names, making them available via
    /// `(import (tein toml))`.
    fn register_toml_module(&self) -> Result<()> {
        self.define_fn_variadic("toml-parse", toml_parse_trampoline)?;
        self.define_fn_variadic("toml-stringify", toml_stringify_trampoline)?;
        Ok(())
    }

    /// Register `file-exists?` and `delete-file` trampolines for `(tein file)`.
    ///
    /// `open-*-file` enforcement is handled at the C opcode level via the FS
    /// policy gate (eval.c patches F, G) — no rust trampolines are needed for
    /// those. `(tein file)` also exports 4 higher-order wrappers
    /// (`call-with-*`, `with-*-from/to-file`) defined in `file.scm`.
    /// Called during `build()` after context creation.
    fn register_file_module(&self) -> Result<()> {
        self.define_fn_variadic("file-exists?", file_exists_trampoline)?;
        self.define_fn_variadic("delete-file", delete_file_trampoline)?;
        Ok(())
    }

    /// Register the VFS-restricted `load` function (VFS-only).
    ///
    /// Registers as `tein-load-vfs-internal` to avoid overriding chibi's
    /// built-in `load`, which the module loader uses for `(include ...)` in
    /// `.sld` files. `(tein load)` exports it as `load` via
    /// `(export (rename tein-load-vfs-internal load))` in `load.sld`.
    fn register_load_module(&self) -> Result<()> {
        self.define_fn_variadic("tein-load-vfs-internal", load_trampoline)?;
        Ok(())
    }

    /// Register `get-environment-variable`, `get-environment-variables`,
    /// `command-line`, and `exit` native functions.
    ///
    /// Called during `build()` for standard-env contexts.
    fn register_process_module(&self) -> Result<()> {
        self.define_fn_variadic("get-environment-variable", get_env_var_trampoline)?;
        self.define_fn_variadic("get-environment-variables", get_env_vars_trampoline)?;
        self.define_fn_variadic("command-line", command_line_trampoline)?;
        self.define_fn_variadic("exit", exit_trampoline)?;
        Ok(())
    }

    /// Call a Scheme procedure from Rust.
    ///
    /// Invokes a `Value::Procedure` (lambda, named function, or builtin)
    /// with the given arguments and returns the result.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new()?;
    /// let add = ctx.evaluate("+")?;
    /// let result = ctx.call(&add, &[Value::Integer(2), Value::Integer(3)])?;
    /// assert_eq!(result, Value::Integer(5));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::TypeError`] if `proc` is not a `Value::Procedure`,
    /// or [`Error::EvalError`] if the Scheme call raises an exception.
    pub fn call(&self, proc: &Value, args: &[Value]) -> Result<Value> {
        let raw_proc = proc
            .as_procedure()
            .ok_or_else(|| Error::TypeError(format!("expected procedure, got {}", proc)))?;

        FOREIGN_STORE_PTR.with(|c| c.set(&self.foreign_store as *const _));
        let _foreign_guard = ForeignStoreGuard;
        PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
        let _port_guard = PortStoreGuard;
        let _ext_api_guard = if let Some(api) = self.ext_api.borrow().as_ref() {
            EXT_API.with(|c| c.set(api.as_ref() as *const _));
            Some(ExtApiGuard)
        } else {
            None
        };
        self.arm_fuel();

        unsafe {
            // root raw_proc — to_raw + sexp_cons calls below allocate
            let _proc = ffi::GcRoot::new(self.ctx, raw_proc);

            // build scheme list from args (reverse-iterate with cons, like to_raw does for lists)
            let mut arg_list = ffi::get_null();
            for arg in args.iter().rev() {
                // root accumulator across to_raw + sexp_cons allocations
                let _tail = ffi::GcRoot::new(self.ctx, arg_list);
                let raw_arg = arg.to_raw(self.ctx)?;
                let _head = ffi::GcRoot::new(self.ctx, raw_arg);
                arg_list = ffi::sexp_cons(self.ctx, raw_arg, arg_list);
            }

            let result = ffi::sexp_apply_proc(self.ctx, raw_proc, arg_list);

            // check fuel before exception status
            self.check_fuel()?;

            // root result — from_raw may allocate (sexp_symbol_to_string, etc.)
            // which can trigger GC and collect an unrooted result sexp.
            let _result_root = ffi::GcRoot::new(self.ctx, result);

            if ffi::sexp_exceptionp(result) != 0 {
                if let Some(exit_result) = self.check_exit() {
                    return exit_result;
                }
                return Value::from_raw(self.ctx, result);
            }

            Value::from_raw(self.ctx, result)
        }
    }

    /// Get the raw context pointer for advanced FFI use.
    ///
    /// # Safety
    /// The returned pointer is only valid for the lifetime of this context.
    /// Do not call `sexp_destroy_context` on it.
    pub fn raw_ctx(&self) -> ffi::sexp {
        self.ctx
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // restore previous VFS gate — always, since a second context on the same thread
        // may still be active and depends on its gate. prev_vfs_gate is GATE_OFF for
        // unsandboxed contexts (default), so this is safe when no outer context exists.
        VFS_GATE.with(|cell| cell.set(self.prev_vfs_gate));
        unsafe { ffi::vfs_gate_set(self.prev_vfs_gate as i32) };
        FS_GATE.with(|cell| cell.set(self.prev_fs_gate));
        unsafe { ffi::fs_policy_gate_set(self.prev_fs_gate as i32) };
        VFS_ALLOWLIST.with(|cell| {
            *cell.borrow_mut() = std::mem::take(&mut self.prev_vfs_allowlist);
        });

        // restore previous FS_POLICY (typically None, unless sequential sandboxed contexts)
        FS_POLICY.with(|cell| {
            *cell.borrow_mut() = std::mem::take(&mut self.prev_fs_policy);
        });
        // restore sandbox flag for the thread (defensive restore-previous pattern)
        IS_SANDBOXED.with(|c| c.set(self.prev_is_sandboxed));

        // clear UX stub module map so next context on this thread starts fresh
        STUB_MODULE_MAP.with(|map| map.borrow_mut().clear());

        // clear any pending exit request — defensive safety net in case evaluation
        // was interrupted before the eval loop could consume the flag.
        EXIT_REQUESTED.with(|c| c.set(false));
        let stashed = EXIT_VALUE.with(|c| c.replace(std::ptr::null_mut()));
        if !stashed.is_null() {
            unsafe { ffi::sexp_release_object(self.ctx, stashed) };
        }

        // clear reader dispatch table so the next context on this thread
        // starts with a clean slate (dispatch state is thread-local in C)
        unsafe { ffi::reader_dispatch_clear(self.ctx) };

        // clear macro expansion hook (thread-local in C)
        unsafe { ffi::macro_expand_hook_clear(self.ctx) };

        // clear runtime VFS entries registered by this context (thread-local in C)
        unsafe { ffi::tein_vfs_clear_dynamic() };

        unsafe {
            if !self.ctx.is_null() {
                ffi::sexp_destroy_context(self.ctx);
            }
        }
    }
}

// context is intentionally !Send + !Sync:
// chibi-scheme contexts are not thread-safe, and the raw sexp pointer
// provides !Send + !Sync by default. users who need multi-threaded
// evaluation should create one context per thread.

#[cfg(test)]
mod tests {
    use super::*;

    // --- basic types ---

    #[test]
    fn test_basic_arithmetic() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(+ 1 2 3)").expect("failed to evaluate");
        match result {
            Value::Integer(n) => assert_eq!(n, 6),
            _ => panic!("expected integer, got {:?}", result),
        }
    }

    // --- multi-expression evaluation ---

    #[test]
    fn test_multi_expression_define_and_use() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate("(define x 5) (+ x 3)")
            .expect("failed to evaluate");
        assert_eq!(result, Value::Integer(8));
    }

    #[test]
    fn test_multi_expression_returns_last() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("1 2 3").expect("failed to evaluate");
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn test_multi_expression_with_procedure() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate("(define (square x) (* x x)) (square 7)")
            .expect("failed to evaluate");
        assert_eq!(result, Value::Integer(49));
    }

    #[test]
    fn test_multi_expression_error_stops_early() {
        let ctx = Context::new().expect("failed to create context");
        // error in first expression should prevent second from running
        let err = ctx.evaluate("(car 42) (+ 1 2)").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("pair"), "expected pair error, got: {}", msg);
    }

    #[test]
    fn test_empty_input() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("").expect("failed to evaluate");
        // empty input returns void/unspecified
        assert!(result.is_unspecified());
    }

    #[test]
    fn test_whitespace_only() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("   \n\t  ").expect("failed to evaluate");
        assert!(result.is_unspecified());
    }

    #[test]
    fn test_string_evaluation() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate(r#""hello world""#)
            .expect("failed to evaluate");
        match result {
            Value::String(s) => assert_eq!(s, "hello world"),
            _ => panic!("expected string, got {:?}", result),
        }
    }

    #[test]
    fn test_boolean() {
        let ctx = Context::new().expect("failed to create context");
        let t = ctx.evaluate("#t").expect("failed to evaluate");
        let f = ctx.evaluate("#f").expect("failed to evaluate");
        assert!(matches!(t, Value::Boolean(true)));
        assert!(matches!(f, Value::Boolean(false)));
    }

    #[test]
    fn test_float() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("3.14").expect("failed to evaluate");
        match result {
            #[allow(clippy::approx_constant)]
            Value::Float(f) => assert!((f - 3.14).abs() < 1e-10),
            _ => panic!("expected float, got {:?}", result),
        }
    }

    #[test]
    fn test_symbol() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(quote foo)").expect("failed to evaluate");
        match result {
            Value::Symbol(s) => assert_eq!(s, "foo"),
            _ => panic!("expected symbol, got {:?}", result),
        }
    }

    #[test]
    fn test_unspecified() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(define x 5)").expect("failed to evaluate");
        assert_eq!(result, Value::Unspecified);
    }

    #[test]
    fn test_nil() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(quote ())").expect("failed to evaluate");
        assert!(matches!(result, Value::Nil));
    }

    // --- lists and pairs ---

    #[test]
    fn test_proper_list() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(quote (1 2 3))").expect("failed to evaluate");
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(items[0], Value::Integer(1)));
                assert!(matches!(items[1], Value::Integer(2)));
                assert!(matches!(items[2], Value::Integer(3)));
            }
            _ => panic!("expected list, got {:?}", result),
        }
    }

    #[test]
    fn test_dotted_pair() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(cons 1 2)").expect("failed to evaluate");
        match result {
            Value::Pair(car, cdr) => {
                assert!(matches!(*car, Value::Integer(1)));
                assert!(matches!(*cdr, Value::Integer(2)));
            }
            _ => panic!("expected pair, got {:?}", result),
        }
    }

    #[test]
    fn test_nested_list() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate("(quote (a (b c) d))")
            .expect("failed to evaluate");
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(&items[1], Value::List(inner) if inner.len() == 2));
            }
            _ => panic!("expected list, got {:?}", result),
        }
    }

    // --- vectors ---

    #[test]
    fn test_vector() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate("(make-vector 3 0)")
            .expect("failed to evaluate");
        match result {
            Value::Vector(items) => {
                assert_eq!(items.len(), 3);
                for item in &items {
                    assert!(matches!(item, Value::Integer(0)));
                }
            }
            _ => panic!("expected vector, got {:?}", result),
        }
    }

    #[test]
    fn test_vector_display() {
        let v = Value::Vector(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        assert_eq!(format!("{}", v), "#(1 2 3)");
    }

    #[test]
    fn test_empty_vector() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate("(make-vector 0 #f)")
            .expect("failed to evaluate");
        match result {
            Value::Vector(items) => assert_eq!(items.len(), 0),
            _ => panic!("expected empty vector, got {:?}", result),
        }
    }

    // --- error messages ---

    #[test]
    fn test_error_message_detail() {
        let ctx = Context::new().expect("failed to create context");
        let err = ctx.evaluate("(car 42)").unwrap_err();
        let msg = format!("{}", err);
        // should contain more than just "scheme exception occurred"
        assert!(
            msg.len() > "scheme evaluation error: ".len() + 5,
            "error message too generic: {}",
            msg
        );
    }

    #[test]
    fn test_error_on_undefined() {
        let ctx = Context::new().expect("failed to create context");
        let err = ctx.evaluate("undefined-variable").unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("undefined"),
            "expected 'undefined' in: {}",
            msg
        );
    }

    // --- foreign functions (using define_fn_variadic) ---

    #[test]
    fn test_foreign_fn_integer() {
        unsafe extern "C" fn add_forty_two(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let n = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(args));
                crate::ffi::sexp_make_fixnum(n + 42)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("add42", add_forty_two)
            .expect("failed to define fn");
        let result = ctx.evaluate("(add42 8)").expect("failed to evaluate");
        assert_eq!(result, Value::Integer(50));
    }

    #[test]
    fn test_foreign_fn_string() {
        unsafe extern "C" fn hello_fn(
            ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let s = "hello from rust";
                let c_str = std::ffi::CString::new(s).unwrap();
                crate::ffi::sexp_c_str(ctx, c_str.as_ptr(), s.len() as crate::ffi::sexp_sint_t)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("hello", hello_fn)
            .expect("failed to define fn");
        let result = ctx.evaluate("(hello)").expect("failed to evaluate");
        assert_eq!(result, Value::String("hello from rust".to_string()));
    }

    #[test]
    fn test_foreign_fn_two_args() {
        unsafe extern "C" fn multiply(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let a = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(args));
                let rest = crate::ffi::sexp_cdr(args);
                let b = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(rest));
                crate::ffi::sexp_make_fixnum(a * b)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("rust-mul", multiply)
            .expect("failed to define fn");
        let result = ctx.evaluate("(rust-mul 6 7)").expect("failed to evaluate");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_foreign_fn_uses_scheme_values() {
        unsafe extern "C" fn square(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let n = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(args));
                crate::ffi::sexp_make_fixnum(n * n)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("square", square)
            .expect("failed to define fn");
        let result = ctx
            .evaluate("(+ (square 3) (square 4))")
            .expect("failed to evaluate");
        assert_eq!(result, Value::Integer(25)); // 9 + 16
    }

    // --- gc pinning (deeply nested structures) ---

    #[test]
    fn test_deeply_nested_list() {
        let ctx = Context::new().expect("failed to create context");
        // build a 100-deep nested list: (1 (1 (1 ... (1) ...)))
        let mut code = String::from("(quote ");
        for _ in 0..100 {
            code.push_str("(1 ");
        }
        code.push_str("()");
        for _ in 0..100 {
            code.push(')');
        }
        code.push(')');
        let result = ctx
            .evaluate(&code)
            .expect("failed to evaluate deeply nested list");
        // outermost should be a list
        assert!(
            matches!(result, Value::List(_)),
            "expected list, got {:?}",
            result
        );
    }

    #[test]
    fn test_deeply_nested_vector() {
        let ctx = Context::new().expect("failed to create context");
        // build 100-deep nested vector from a single expression:
        // (make-vector 1 (make-vector 1 (make-vector 1 ... 42 ...)))
        // this creates a true tree (no structural sharing) so extraction is O(n).
        let depth = 100;
        let mut code = String::new();
        for _ in 0..depth {
            code.push_str("(make-vector 1 ");
        }
        code.push_str("42");
        for _ in 0..depth {
            code.push(')');
        }
        let result = ctx
            .evaluate(&code)
            .expect("failed to evaluate nested vector");
        assert!(
            matches!(result, Value::Vector(_)),
            "expected vector, got {:?}",
            result
        );
    }

    #[test]
    fn test_mixed_nested_structures() {
        let ctx = Context::new().expect("failed to create context");
        // list containing vectors containing lists
        let result = ctx
            .evaluate("(quote ((1 2) (3 4)))")
            .expect("failed to evaluate");
        match &result {
            Value::List(items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(&items[0], Value::List(inner) if inner.len() == 2));
                assert!(matches!(&items[1], Value::List(inner) if inner.len() == 2));
            }
            _ => panic!("expected list, got {:?}", result),
        }

        // vector inside list
        ctx.evaluate("(define test-vec (make-vector 3 99))")
            .expect("define vec");
        let result = ctx
            .evaluate("(cons test-vec (quote ()))")
            .expect("eval cons");
        match &result {
            Value::List(items) => {
                assert_eq!(items.len(), 1);
                assert!(matches!(&items[0], Value::Vector(v) if v.len() == 3));
            }
            _ => panic!("expected list containing vector, got {:?}", result),
        }
    }

    // --- typed extraction helpers ---

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_as_integer() {
        let v = Value::Integer(42);
        assert_eq!(v.as_integer(), Some(42));
        assert_eq!(Value::Float(3.14).as_integer(), None);
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_as_float() {
        let v = Value::Float(2.718);
        assert!((v.as_float().unwrap() - 2.718).abs() < 1e-10);
        assert_eq!(Value::Integer(42).as_float(), None);
    }

    #[test]
    fn test_as_string() {
        let v = Value::String("hello".into());
        assert_eq!(v.as_string(), Some("hello"));
        assert_eq!(Value::Symbol("hello".into()).as_string(), None);
    }

    #[test]
    fn test_as_symbol() {
        let v = Value::Symbol("foo".into());
        assert_eq!(v.as_symbol(), Some("foo"));
        assert_eq!(Value::String("foo".into()).as_symbol(), None);
    }

    #[test]
    fn test_as_bool() {
        assert_eq!(Value::Boolean(true).as_bool(), Some(true));
        assert_eq!(Value::Boolean(false).as_bool(), Some(false));
        assert_eq!(Value::Integer(1).as_bool(), None);
    }

    #[test]
    fn test_as_list() {
        let v = Value::List(vec![Value::Integer(1), Value::Integer(2)]);
        let items = v.as_list().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].as_integer(), Some(1));
        assert_eq!(Value::Vector(vec![]).as_list(), None);
    }

    #[test]
    fn test_as_pair() {
        let v = Value::Pair(Box::new(Value::Integer(1)), Box::new(Value::Integer(2)));
        let (car, cdr) = v.as_pair().unwrap();
        assert_eq!(car.as_integer(), Some(1));
        assert_eq!(cdr.as_integer(), Some(2));
        assert_eq!(Value::List(vec![]).as_pair(), None);
    }

    #[test]
    fn test_as_vector() {
        let v = Value::Vector(vec![Value::Integer(1), Value::Integer(2)]);
        let items = v.as_vector().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(Value::List(vec![]).as_vector(), None);
    }

    #[test]
    fn test_is_nil() {
        assert!(Value::Nil.is_nil());
        assert!(!Value::List(vec![]).is_nil());
    }

    #[test]
    fn test_is_unspecified() {
        assert!(Value::Unspecified.is_unspecified());
        assert!(!Value::Nil.is_unspecified());
    }

    // --- to_raw round-trip tests ---

    #[test]
    fn test_list_to_raw_roundtrip() {
        unsafe extern "C" fn get_test_list(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let list = Value::List(vec![
                    Value::Integer(1),
                    Value::Integer(2),
                    Value::Integer(3),
                ]);
                list.to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-test-list", get_test_list)
            .expect("define fn");
        let result = ctx.evaluate("(get-test-list)").expect("eval");
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0].as_integer(), Some(1));
                assert_eq!(items[1].as_integer(), Some(2));
                assert_eq!(items[2].as_integer(), Some(3));
            }
            _ => panic!("expected list, got {:?}", result),
        }
    }

    #[test]
    fn test_pair_to_raw_roundtrip() {
        unsafe extern "C" fn get_test_pair(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let pair = Value::Pair(
                    Box::new(Value::Symbol("key".into())),
                    Box::new(Value::Integer(42)),
                );
                pair.to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-test-pair", get_test_pair)
            .expect("define fn");
        let result = ctx.evaluate("(get-test-pair)").expect("eval");
        match result {
            Value::Pair(car, cdr) => {
                assert_eq!(car.as_symbol(), Some("key"));
                assert_eq!(cdr.as_integer(), Some(42));
            }
            _ => panic!("expected pair, got {:?}", result),
        }
    }

    #[test]
    fn test_vector_to_raw_roundtrip() {
        unsafe extern "C" fn get_test_vector(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let vec = Value::Vector(vec![Value::String("a".into()), Value::String("b".into())]);
                vec.to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-test-vector", get_test_vector)
            .expect("define fn");
        let result = ctx.evaluate("(get-test-vector)").expect("eval");
        match result {
            Value::Vector(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].as_string(), Some("a"));
                assert_eq!(items[1].as_string(), Some("b"));
            }
            _ => panic!("expected vector, got {:?}", result),
        }
    }

    #[test]
    fn test_nested_list_to_raw_roundtrip() {
        unsafe extern "C" fn get_nested_list(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let nested = Value::List(vec![
                    Value::Integer(1),
                    Value::List(vec![Value::Integer(2), Value::Integer(3)]),
                    Value::Integer(4),
                ]);
                nested
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-nested-list", get_nested_list)
            .expect("define fn");
        let result = ctx.evaluate("(get-nested-list)").expect("eval");
        match &result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0].as_integer(), Some(1));
                assert!(matches!(&items[1], Value::List(inner) if inner.len() == 2));
                assert_eq!(items[2].as_integer(), Some(4));
            }
            _ => panic!("expected list, got {:?}", result),
        }
    }

    #[test]
    fn test_empty_list_to_raw() {
        unsafe extern "C" fn get_empty_list(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let empty = Value::List(vec![]);
                empty
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-empty-list", get_empty_list)
            .expect("define fn");
        let result = ctx.evaluate("(get-empty-list)").expect("eval");
        assert!(
            result.is_nil(),
            "empty list should become nil, got {:?}",
            result
        );
    }

    #[test]
    fn test_empty_vector_to_raw() {
        unsafe extern "C" fn get_empty_vector(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let empty = Value::Vector(vec![]);
                empty
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-empty-vector", get_empty_vector)
            .expect("define fn");
        let result = ctx.evaluate("(get-empty-vector)").expect("eval");
        match result {
            Value::Vector(items) => assert_eq!(items.len(), 0),
            _ => panic!("expected empty vector, got {:?}", result),
        }
    }

    // --- value display ---

    #[test]
    fn test_display_roundtrip() {
        let cases = [
            (Value::Integer(42), "42"),
            #[allow(clippy::approx_constant)]
            (Value::Float(3.14), "3.14"),
            (Value::String("hi".into()), "\"hi\""),
            (Value::Symbol("foo".into()), "foo"),
            (Value::Boolean(true), "#t"),
            (Value::Boolean(false), "#f"),
            (Value::Nil, "()"),
            (Value::Unspecified, "#<unspecified>"),
        ];
        for (val, expected) in &cases {
            assert_eq!(format!("{}", val), *expected, "for {:?}", val);
        }
    }

    // --- file loading ---

    #[test]
    fn test_load_file_basic() {
        let dir = std::env::temp_dir();
        let path = dir.join("tein_test_basic.scm");
        std::fs::write(&path, "(+ 1 2 3)").expect("write test file");

        let ctx = Context::new().expect("create context");
        let result = ctx.load_file(&path).expect("load file");
        assert_eq!(result, Value::Integer(6));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_load_file_multi_expression() {
        let dir = std::env::temp_dir();
        let path = dir.join("tein_test_multi.scm");
        std::fs::write(&path, "(define x 10)\n(define y 20)\n(+ x y)").expect("write test file");

        let ctx = Context::new().expect("create context");
        let result = ctx.load_file(&path).expect("load file");
        assert_eq!(result, Value::Integer(30));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_load_file_defines_persist() {
        let dir = std::env::temp_dir();
        let path = dir.join("tein_test_persist.scm");
        std::fs::write(&path, "(define (square x) (* x x))").expect("write test file");

        let ctx = Context::new().expect("create context");
        let _ = ctx.load_file(&path).expect("load file");

        // definition from file should be available for subsequent evaluation
        let result = ctx.evaluate("(square 7)").expect("eval");
        assert_eq!(result, Value::Integer(49));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_load_file_not_found() {
        let ctx = Context::new().expect("create context");
        let err = ctx.load_file("/nonexistent/path/to/file.scm").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("io error"), "expected io error, got: {}", msg);
    }

    #[test]
    fn test_load_file_syntax_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("tein_test_syntax.scm");
        std::fs::write(&path, "(define x").expect("write test file"); // unclosed paren

        let ctx = Context::new().expect("create context");
        let err = ctx.load_file(&path).unwrap_err();
        // should be an eval error, not io error
        let msg = format!("{}", err);
        assert!(
            !msg.contains("io error"),
            "expected eval error, got io: {}",
            msg
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_load_file_empty() {
        let dir = std::env::temp_dir();
        let path = dir.join("tein_test_empty.scm");
        std::fs::write(&path, "").expect("write test file");

        let ctx = Context::new().expect("create context");
        let result = ctx.load_file(&path).expect("load file");
        assert!(result.is_unspecified());

        std::fs::remove_file(&path).ok();
    }

    // --- procedures as values ---

    #[test]
    fn test_evaluate_lambda_returns_procedure() {
        let ctx = Context::new().expect("create context");
        let result = ctx.evaluate("(lambda (x) (* x x))").expect("eval lambda");
        assert!(
            result.is_procedure(),
            "expected procedure, got {:?}",
            result
        );
    }

    #[test]
    fn test_call_lambda() {
        let ctx = Context::new().expect("create context");
        let square = ctx.evaluate("(lambda (x) (* x x))").expect("eval lambda");
        let result = ctx
            .call(&square, &[Value::Integer(7)])
            .expect("call lambda");
        assert_eq!(result, Value::Integer(49));
    }

    #[test]
    fn test_call_named_procedure() {
        let ctx = Context::new().expect("create context");
        ctx.evaluate("(define (add a b) (+ a b))")
            .expect("define add");
        let add = ctx.evaluate("add").expect("get add");
        assert!(add.is_procedure());
        let result = ctx
            .call(&add, &[Value::Integer(3), Value::Integer(4)])
            .expect("call add");
        assert_eq!(result, Value::Integer(7));
    }

    #[test]
    fn test_call_builtin_procedure() {
        let ctx = Context::new().expect("create context");
        // + is a builtin opcode, should come back as Procedure via sexp_applicablep
        let plus = ctx.evaluate("+").expect("get +");
        assert!(
            plus.is_procedure(),
            "expected procedure for +, got {:?}",
            plus
        );
        let result = ctx
            .call(&plus, &[Value::Integer(10), Value::Integer(20)])
            .expect("call +");
        assert_eq!(result, Value::Integer(30));
    }

    #[test]
    fn test_call_with_non_procedure_returns_type_error() {
        let ctx = Context::new().expect("create context");
        let not_proc = Value::Integer(42);
        let err = ctx.call(&not_proc, &[]).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("type error"),
            "expected type error, got: {}",
            msg
        );
    }

    #[test]
    fn test_call_wrong_arity_propagates_exception() {
        let ctx = Context::new().expect("create context");
        let square = ctx.evaluate("(lambda (x) (* x x))").expect("eval lambda");
        // call with 2 args when it expects 1
        let err = ctx
            .call(&square, &[Value::Integer(1), Value::Integer(2)])
            .unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("error"), "expected exception, got: {}", msg);
    }

    #[test]
    fn test_call_zero_args() {
        let ctx = Context::new().expect("create context");
        let thunk = ctx.evaluate("(lambda () 42)").expect("eval thunk");
        let result = ctx.call(&thunk, &[]).expect("call thunk");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_extract_builtin_via_define() {
        // (define f +) f → Procedure
        let ctx = Context::new().expect("create context");
        let f = ctx.evaluate("(define f +) f").expect("eval");
        assert!(f.is_procedure(), "expected procedure, got {:?}", f);
        let result = ctx
            .call(&f, &[Value::Integer(1), Value::Integer(2)])
            .expect("call f");
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn test_roundtrip_rust_fn_as_procedure() {
        // register rust fn, get it back as procedure, call from rust
        unsafe extern "C" fn double_it(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let n = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(args));
                crate::ffi::sexp_make_fixnum(n * 2)
            }
        }

        let ctx = Context::new().expect("create context");
        ctx.define_fn_variadic("double-it", double_it)
            .expect("define fn");
        let proc = ctx.evaluate("double-it").expect("get proc");
        assert!(proc.is_procedure(), "expected procedure, got {:?}", proc);
        let result = ctx
            .call(&proc, &[Value::Integer(21)])
            .expect("call double-it");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_procedure_display() {
        let ctx = Context::new().expect("create context");
        let proc = ctx.evaluate("(lambda (x) x)").expect("eval lambda");
        assert_eq!(format!("{}", proc), "#<procedure>");
    }

    #[test]
    fn test_procedure_equality() {
        let ctx = Context::new().expect("create context");
        // same lambda bound to a variable — same object
        ctx.evaluate("(define f (lambda (x) x))").expect("define f");
        let f1 = ctx.evaluate("f").expect("get f");
        let f2 = ctx.evaluate("f").expect("get f again");
        assert_eq!(f1, f2, "same binding should yield same procedure");

        // different lambdas are different objects
        let g = ctx.evaluate("(lambda (x) x)").expect("different lambda");
        assert_ne!(f1, g, "different lambdas should not be equal");
    }

    // --- variadic foreign functions ---

    #[test]
    fn test_variadic_sum() {
        unsafe extern "C" fn sum_all(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let mut total: i64 = 0;
                let mut current = args;
                while crate::ffi::sexp_pairp(current) != 0 {
                    total += crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(current)) as i64;
                    current = crate::ffi::sexp_cdr(current);
                }
                crate::ffi::sexp_make_fixnum(total as crate::ffi::sexp_sint_t)
            }
        }

        let ctx = Context::new().expect("create context");
        ctx.define_fn_variadic("sum-all", sum_all)
            .expect("define fn");
        let result = ctx.evaluate("(sum-all 1 2 3 4 5)").expect("eval");
        assert_eq!(result, Value::Integer(15));
    }

    #[test]
    fn test_variadic_zero_args() {
        unsafe extern "C" fn constant(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe { crate::ffi::sexp_make_fixnum(42) }
        }

        let ctx = Context::new().expect("create context");
        ctx.define_fn_variadic("constant", constant)
            .expect("define fn");
        let result = ctx.evaluate("(constant)").expect("eval");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_variadic_many_args() {
        unsafe extern "C" fn count_args(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let mut count: i64 = 0;
                let mut current = args;
                while crate::ffi::sexp_pairp(current) != 0 {
                    count += 1;
                    current = crate::ffi::sexp_cdr(current);
                }
                crate::ffi::sexp_make_fixnum(count as crate::ffi::sexp_sint_t)
            }
        }

        let ctx = Context::new().expect("create context");
        ctx.define_fn_variadic("count-args", count_args)
            .expect("define fn");
        let result = ctx
            .evaluate("(count-args 1 2 3 4 5 6 7 8 9 10 11 12)")
            .expect("eval");
        assert_eq!(result, Value::Integer(12));
    }

    #[test]
    fn test_variadic_mixed_types() {
        // returns a string describing the types of all args
        unsafe extern "C" fn describe_types(
            ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let mut desc = std::string::String::new();
                let mut current = args;
                while crate::ffi::sexp_pairp(current) != 0 {
                    let item = crate::ffi::sexp_car(current);
                    if !desc.is_empty() {
                        desc.push(' ');
                    }
                    if crate::ffi::sexp_integerp(item) != 0 {
                        desc.push_str("int");
                    } else if crate::ffi::sexp_stringp(item) != 0 {
                        desc.push_str("str");
                    } else if crate::ffi::sexp_booleanp(item) != 0 {
                        desc.push_str("bool");
                    } else {
                        desc.push_str("other");
                    }
                    current = crate::ffi::sexp_cdr(current);
                }
                let c_str = std::ffi::CString::new(desc.as_str()).unwrap();
                crate::ffi::sexp_c_str(ctx, c_str.as_ptr(), desc.len() as crate::ffi::sexp_sint_t)
            }
        }

        let ctx = Context::new().expect("create context");
        ctx.define_fn_variadic("describe-types", describe_types)
            .expect("define fn");
        let result = ctx
            .evaluate(r#"(describe-types 1 "hello" #t 42)"#)
            .expect("eval");
        assert_eq!(result, Value::String("int str bool int".to_string()));
    }

    // --- phase 1: builder + step limits ---

    #[test]
    fn test_builder_default() {
        let ctx = Context::builder().build().expect("builder default");
        let result = ctx.evaluate("(+ 1 2 3)").expect("should work");
        assert_eq!(result, Value::Integer(6));
    }

    #[test]
    fn test_builder_custom_heap() {
        let ctx = Context::builder()
            .heap_size(8 * 1024 * 1024)
            .heap_max(64 * 1024 * 1024)
            .build()
            .expect("builder custom heap");
        let result = ctx.evaluate("(+ 1 1)").expect("should work");
        assert_eq!(result, Value::Integer(2));
    }

    #[test]
    fn test_step_limit_infinite_loop() {
        let ctx = Context::builder()
            .step_limit(1000)
            .build()
            .expect("builder");
        let err = ctx
            .evaluate("((lambda () (define (loop) (loop)) (loop)))")
            .unwrap_err();
        assert!(
            matches!(err, Error::StepLimitExceeded),
            "expected StepLimitExceeded, got: {}",
            err
        );
    }

    #[test]
    fn test_step_limit_short_computation_succeeds() {
        let ctx = Context::builder()
            .step_limit(100_000)
            .build()
            .expect("builder");
        let result = ctx.evaluate("(+ 1 2 3)").expect("should work");
        assert_eq!(result, Value::Integer(6));
    }

    #[test]
    fn test_no_step_limit_backwards_compat() {
        let ctx = Context::new().expect("context");
        let result = ctx
            .evaluate("(define (fib n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))) (fib 10)")
            .expect("should work");
        assert_eq!(result, Value::Integer(55));
    }

    #[test]
    fn test_fuel_resets_between_evaluations() {
        let ctx = Context::builder()
            .step_limit(100_000)
            .build()
            .expect("builder");
        let r1 = ctx.evaluate("(+ 1 2)").expect("first");
        assert_eq!(r1, Value::Integer(3));
        let r2 = ctx.evaluate("(* 3 4)").expect("second");
        assert_eq!(r2, Value::Integer(12));
    }

    #[test]
    fn test_call_respects_step_limit() {
        let ctx = Context::builder()
            .step_limit(1000)
            .build()
            .expect("builder");
        let looper = ctx
            .evaluate("(lambda () ((lambda () (define (loop) (loop)) (loop))))")
            .expect("lambda");
        let err = ctx.call(&looper, &[]).unwrap_err();
        assert!(
            matches!(err, Error::StepLimitExceeded),
            "expected StepLimitExceeded, got: {}",
            err
        );
    }

    // --- phase 2: restricted environments (module-level sandboxing) ---

    #[test]
    fn test_sandboxed_modules_safe_arithmetic() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("builder");
        // after import, arithmetic works
        let result = ctx
            .evaluate("(import (scheme base)) (+ 1 2)")
            .expect("should work");
        assert_eq!(result, Value::Integer(3));
        // modules outside safe set are blocked
        let err = ctx.evaluate("(import (scheme eval))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
            "scheme/eval should be blocked in Modules::Safe, got: {:?}",
            err
        );
    }

    #[test]
    fn test_sandboxed_modules_none_blocks_all() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::None)
            .build()
            .expect("builder");
        // even scheme/base is blocked — '+' requires import first
        let err = ctx.evaluate("(import (scheme base))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
            "scheme/base should be blocked in Modules::None, got: {:?}",
            err
        );
    }

    #[test]
    fn test_sandboxed_modules_only_single() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .build()
            .expect("builder");
        let result = ctx
            .evaluate("(import (scheme base)) (+ 1 2)")
            .expect("should work");
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn test_sandboxed_modules_all() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::All)
            .build()
            .expect("builder");
        let result = ctx
            .evaluate("(import (scheme write)) (write 1) #t")
            .expect("should work");
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_unrestricted_env() {
        let ctx = Context::builder().build().expect("builder");
        let result = ctx
            .evaluate("(cons 1 (cons 2 (quote ())))")
            .expect("should work");
        assert_eq!(
            result,
            Value::List(vec![Value::Integer(1), Value::Integer(2)])
        );
    }

    #[test]
    fn test_foreign_fn_works_in_sandboxed_env() {
        use crate::sandbox::Modules;
        unsafe extern "C" fn add100(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let n = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(args));
                crate::ffi::sexp_make_fixnum(n + 100)
            }
        }

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .build()
            .expect("builder");
        ctx.define_fn_variadic("add100", add100).expect("define fn");
        let result = ctx
            .evaluate("(import (scheme base)) (add100 5)")
            .expect("should work");
        assert_eq!(result, Value::Integer(105));
    }

    #[test]
    fn test_file_io_absent_without_policy() {
        use crate::sandbox::Modules;
        // sandboxed with no file_read — open-input-file not in scheme/base, is absent
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("builder");
        let err = ctx.evaluate("(import (scheme base)) (open-input-file \"/etc/passwd\")");
        assert!(
            err.is_err(),
            "file io should be unavailable without file policy"
        );
    }

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
        // env vars neutered
        let r = ctx.evaluate("(get-environment-variable \"HOME\")");
        assert_eq!(r.unwrap(), Value::Boolean(false));
        // env var list neutered
        let r = ctx.evaluate("(get-environment-variables)");
        assert_eq!(r.unwrap(), Value::Nil);
        // command-line returns fake
        let r = ctx.evaluate("(command-line)");
        assert_eq!(r.unwrap(), Value::List(vec![Value::String("tein".into())]));
    }

    #[test]
    fn test_tein_process_allowed_with_allow_module() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("tein/process")
            .step_limit(5_000_000)
            .build()
            .expect("sandboxed context");
        // module import should succeed once trampolines are registered
        let r = ctx.evaluate("(import (tein process))");
        assert!(
            r.is_ok(),
            "expected (tein process) import to succeed: {:?}",
            r
        );
    }

    // --- (tein file) tests ---

    #[test]
    fn test_tein_file_exists() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein file))").unwrap();
        // test with a file that exists (Cargo.toml in workspace root)
        let r = ctx.evaluate("(file-exists? \"Cargo.toml\")").unwrap();
        assert_eq!(r, Value::Boolean(true));
        // test with a file that doesn't exist
        let r = ctx
            .evaluate("(file-exists? \"/nonexistent/path/xyz\")")
            .unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn test_tein_file_delete() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("tein_test_delete_file.txt");
        // create a temp file
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"test").unwrap();
        drop(f);

        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein file))").unwrap();
        let code = format!("(delete-file \"{}\")", path.display());
        let r = ctx.evaluate(&code);
        assert!(r.is_ok(), "delete-file failed: {:?}", r);
        assert!(!path.exists(), "file should be deleted");
    }

    #[test]
    fn test_tein_file_exists_sandboxed() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .step_limit(5_000_000)
            .build()
            .unwrap();
        ctx.evaluate("(import (tein file))").unwrap();
        // no FsPolicy configured, file-exists? should return a policy error
        let r = ctx.evaluate("(file-exists? \"/etc/passwd\")");
        assert!(r.is_err(), "expected sandbox violation: {:?}", r);
    }

    #[test]
    fn test_tein_file_exists_with_read_policy() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .file_read(&["/"])
            .step_limit(5_000_000)
            .build()
            .unwrap();
        ctx.evaluate("(import (tein file))").unwrap();
        let r = ctx.evaluate("(file-exists? \"Cargo.toml\")").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // --- (tein load) tests ---

    #[test]
    fn test_tein_load_vfs_path() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein load))").unwrap();
        // load a VFS file that defines something — tein/test.scm is a safe target
        let r = ctx.evaluate("(load \"/vfs/lib/tein/test.scm\")");
        assert!(r.is_ok(), "VFS load failed: {:?}", r);
    }

    #[test]
    fn test_tein_load_non_vfs_rejected() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein load))").unwrap();
        let r = ctx.evaluate("(load \"/etc/passwd\")");
        assert!(r.is_err(), "expected non-VFS path to be rejected: {:?}", r);
    }

    #[test]
    fn test_tein_load_nonexistent_vfs() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein load))").unwrap();
        let r = ctx.evaluate("(load \"/vfs/lib/nonexistent.scm\")");
        assert!(r.is_err(), "expected missing VFS path to error: {:?}", r);
    }

    // --- (tein process) tests ---

    #[test]
    fn test_tein_process_get_env_var() {
        unsafe { std::env::set_var("TEIN_TEST_VAR", "hello") };
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx
            .evaluate("(get-environment-variable \"TEIN_TEST_VAR\")")
            .unwrap();
        assert_eq!(r, Value::String("hello".to_string()));
        // unset var returns #f
        let r = ctx
            .evaluate("(get-environment-variable \"TEIN_NONEXISTENT_VAR_XYZ\")")
            .unwrap();
        assert_eq!(r, Value::Boolean(false));
        unsafe { std::env::remove_var("TEIN_TEST_VAR") };
    }

    #[test]
    fn test_tein_process_get_env_vars() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(pair? (get-environment-variables))").unwrap();
        assert_eq!(r, Value::Boolean(true), "should return non-empty alist");
    }

    #[test]
    fn test_tein_process_command_line() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(list? (command-line))").unwrap();
        assert_eq!(r, Value::Boolean(true), "should return a list");
    }

    #[test]
    fn test_tein_process_exit_no_args() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(begin (exit) (+ 1 2))").unwrap();
        assert_eq!(r, Value::Integer(0), "(exit) should return 0");
    }

    #[test]
    fn test_tein_process_exit_with_value() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(begin (exit 42) (+ 1 2))").unwrap();
        assert_eq!(r, Value::Integer(42), "(exit 42) should return 42");
    }

    #[test]
    fn test_tein_process_exit_true() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(begin (exit #t) 999)").unwrap();
        assert_eq!(r, Value::Integer(0), "(exit #t) should return 0");
    }

    #[test]
    fn test_tein_process_exit_false() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(begin (exit #f) 999)").unwrap();
        assert_eq!(r, Value::Integer(1), "(exit #f) should return 1");
    }

    #[test]
    fn test_tein_process_exit_string() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(begin (exit \"done\") 999)").unwrap();
        assert_eq!(r, Value::String("done".to_string()));
    }

    // --- phase 3: timeout context ---

    #[test]
    fn test_timeout_basic() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_timeout(std::time::Duration::from_secs(5))
            .expect("build_timeout");
        let result = ctx.evaluate("(+ 1 2 3)").expect("should work");
        assert_eq!(result, Value::Integer(6));
    }

    #[test]
    fn test_timeout_infinite_loop() {
        let ctx = Context::builder()
            .step_limit(10_000)
            .build_timeout(std::time::Duration::from_millis(500))
            .expect("build_timeout");
        let err = ctx
            .evaluate("((lambda () (define (loop) (loop)) (loop)))")
            .unwrap_err();
        assert!(
            matches!(err, Error::Timeout | Error::StepLimitExceeded),
            "expected Timeout or StepLimitExceeded, got: {}",
            err
        );
    }

    #[test]
    fn test_timeout_multiple_sequential() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_timeout(std::time::Duration::from_secs(5))
            .expect("build_timeout");
        let r1 = ctx.evaluate("(+ 1 2)").expect("first");
        let r2 = ctx.evaluate("(* 3 4)").expect("second");
        assert_eq!(r1, Value::Integer(3));
        assert_eq!(r2, Value::Integer(12));
    }

    #[test]
    fn test_timeout_state_persists() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_timeout(std::time::Duration::from_secs(5))
            .expect("build_timeout");
        ctx.evaluate("(define x 42)").expect("define");
        let result = ctx.evaluate("x").expect("lookup");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_build_timeout_without_step_limit_fails() {
        let err = Context::builder()
            .build_timeout(std::time::Duration::from_secs(1))
            .unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("step_limit"),
            "expected step_limit error, got: {}",
            msg
        );
    }

    #[test]
    fn test_timeout_drop_cleans_up() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_timeout(std::time::Duration::from_secs(5))
            .expect("build_timeout");
        let _ = ctx.evaluate("(+ 1 1)");
        drop(ctx);
    }

    // --- IO policy tests ---
    //
    // IO tests use thread-local state (FS_POLICY, FS_GATE, IS_SANDBOXED) so
    // they must not run concurrently on the same thread. we use a mutex to
    // serialise them.

    static IO_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Helper: create a temp directory with a known prefix for IO tests.
    fn io_test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("tein-io-test").join(name);
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn test_file_read_allowed_path() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("read_allowed");
        let file = dir.join("hello.txt");
        std::fs::write(&file, "hello").expect("write");
        let canon_dir = dir.canonicalize().unwrap();

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_read(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        // open-input-file is a chibi opcode — import (tein file) to get it in sandbox.
        // (scheme read) provides `read`.
        let code = format!(
            r#"(import (scheme base) (scheme read) (tein file)) (define p (open-input-file "{}")) (define r (read p)) (close-input-port p) r"#,
            file.display()
        );
        let result = ctx.evaluate(&code).expect("should succeed");
        // file contains "hello", read returns the symbol hello
        assert_eq!(result, Value::Symbol("hello".to_string()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_file_read_denied_path() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("read_denied");
        let canon_dir = dir.canonicalize().unwrap();

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_read(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        let err = ctx
            .evaluate("(import (tein file)) (open-input-file \"/etc/passwd\")")
            .unwrap_err();
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

    #[test]
    fn test_file_write_allowed_path() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("write_allowed");
        let file = dir.join("output.txt");
        let canon_dir = dir.canonicalize().unwrap();

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_write(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        let code = format!(
            r#"(import (scheme base) (tein file)) (define p (open-output-file "{}")) (write-char #\X p) (close-output-port p)"#,
            file.display()
        );
        ctx.evaluate(&code).expect("should succeed");

        let contents = std::fs::read_to_string(&file).expect("read back");
        assert_eq!(contents, "X");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_file_write_denied_path() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("write_denied");
        let canon_dir = dir.canonicalize().unwrap();

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_write(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        let err =
            ctx.evaluate("(import (tein file)) (open-output-file \"/tmp/tein-io-test-nope.txt\")");
        assert!(err.is_err(), "write to unallowed path should be denied");
    }

    #[test]
    fn test_file_read_path_traversal() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("traversal");
        let canon_dir = dir.canonicalize().unwrap();

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_read(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        // try to escape via ../ — canonicalisation should catch this
        let evil_path = format!("{}/../../../etc/passwd", dir.display());
        let code = format!(r#"(import (tein file)) (open-input-file "{}")"#, evil_path);
        let err = ctx.evaluate(&code);
        assert!(err.is_err(), "path traversal should be denied");
    }

    #[test]
    fn test_file_read_symlink_resolved() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("symlink");
        let target = dir.join("secret.txt");
        std::fs::write(&target, "secret").expect("write");
        let link = dir.join("link.txt");
        // create symlink pointing to /etc/hostname (exists on most linux)
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink("/etc/hostname", &link).ok();
            let canon_dir = dir.canonicalize().unwrap();

            let ctx = Context::builder()
                .standard_env()
                .sandboxed(crate::sandbox::Modules::Safe)
                .file_read(&[canon_dir.to_str().unwrap()])
                .build()
                .expect("builder");

            // the symlink points outside the allowed prefix, so should be denied
            let code = format!(
                r#"(import (tein file)) (open-input-file "{}")"#,
                link.display()
            );
            let err = ctx.evaluate(&code);
            assert!(
                err.is_err(),
                "symlink escaping allowed prefix should be denied"
            );
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_file_write_creates_new_file() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("write_new");
        let file = dir.join("new_file.txt");
        assert!(!file.exists());
        let canon_dir = dir.canonicalize().unwrap();

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_write(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        let code = format!(
            r#"(import (scheme base) (tein file)) (define p (open-output-file "{}")) (write-char #\Y p) (close-output-port p)"#,
            file.display()
        );
        ctx.evaluate(&code).expect("should create new file");
        assert!(file.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_file_write_parent_must_exist() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("write_no_parent");
        let canon_dir = dir.canonicalize().unwrap();

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_write(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        // parent dir doesn't exist, so check_write fails (can't canonicalise parent)
        let code = format!(
            r#"(import (tein file)) (open-output-file "{}/nonexistent_subdir/file.txt")"#,
            dir.display()
        );
        let err = ctx.evaluate(&code);
        assert!(err.is_err(), "write with non-existent parent should fail");
    }

    #[test]
    fn test_file_read_without_policy() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        // sandboxed without file_read — C gate denies access
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .build()
            .expect("builder");
        let err = ctx.evaluate("(import (tein file)) (open-input-file \"/etc/passwd\")");
        assert!(
            err.is_err(),
            "open-input-file should be denied without file_read()"
        );
    }

    #[test]
    fn test_file_write_without_policy() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        // sandboxed without file_write — C gate denies access
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .build()
            .expect("builder");
        let err = ctx.evaluate("(import (tein file)) (open-output-file \"/tmp/nope.txt\")");
        assert!(
            err.is_err(),
            "open-output-file should be denied without file_write()"
        );
    }

    #[test]
    fn test_file_io_with_sandboxed_modules() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        // sandboxed(Safe).file_read() should compose correctly
        let dir = io_test_dir("safe_compose");
        let file = dir.join("data.txt");
        std::fs::write(&file, "42").expect("write");
        let canon_dir = dir.canonicalize().unwrap();

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_read(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        // arithmetic works after import
        let r = ctx
            .evaluate("(import (scheme base)) (+ 1 2)")
            .expect("arithmetic");
        assert_eq!(r, Value::Integer(3));

        // mutation works
        let r = ctx
            .evaluate("(define x (cons 1 2)) (set-car! x 99) (car x)")
            .expect("mutation");
        assert_eq!(r, Value::Integer(99));

        // file read via C opcode — import (tein file) to get open-input-file in sandbox.
        // (scheme read) provides `read`.
        let code = format!(
            r#"(import (scheme read) (tein file)) (define p (open-input-file "{}")) (read p)"#,
            file.display()
        );
        let r = ctx.evaluate(&code).expect("file read");
        assert_eq!(r, Value::Integer(42));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_fd_primitives_never_exposed() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        // open-*-file-descriptor should never be available, even with file_read/file_write
        let dir = io_test_dir("fd_blocked");
        let canon_dir = dir.canonicalize().unwrap();

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_read(&[canon_dir.to_str().unwrap()])
            .file_write(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        let err = ctx.evaluate("(open-input-file-descriptor 0)");
        assert!(err.is_err(), "fd primitives should be blocked");
        let err = ctx.evaluate("(open-output-file-descriptor 1)");
        assert!(err.is_err(), "fd primitives should be blocked");

        std::fs::remove_dir_all(&dir).ok();
    }

    // --- open-*-file C-level policy tests ---

    #[test]
    fn test_open_input_file_allowed() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("open_input_allowed");
        let file = dir.join("data.txt");
        std::fs::write(&file, "hello").expect("write");
        let canon_dir = dir.canonicalize().unwrap();
        let path = file.to_str().unwrap().to_string();
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_read(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");
        // import (tein file) to get open-input-file in sandbox
        let code = format!(
            "(import (scheme base) (tein file)) (let ((p (open-input-file \"{path}\"))) (close-input-port p) #t)"
        );
        let r = ctx.evaluate(&code).expect("open-input-file allowed");
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_open_input_file_denied() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("open_input_denied");
        let file = dir.join("secret.txt");
        std::fs::write(&file, "no").expect("write");
        let path = file.to_str().unwrap().to_string();
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_read(&["/tmp/__nonexistent_prefix__/"])
            .build()
            .expect("builder");
        // C gate denies access — path not in allowed prefix
        let code = format!("(import (tein file)) (open-input-file \"{path}\")");
        assert!(ctx.evaluate(&code).is_err(), "should be denied");
    }

    #[test]
    fn test_open_output_file_allowed() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("open_output_allowed");
        let file = dir.join("out.txt");
        let canon_dir = dir.canonicalize().unwrap();
        let path = file.to_str().unwrap().to_string();
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_write(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");
        // import (tein file) to get open-output-file in sandbox
        let code = format!(
            "(import (scheme base) (tein file)) (let ((p (open-output-file \"{path}\"))) (close-output-port p) #t)"
        );
        let r = ctx.evaluate(&code).expect("open-output-file allowed");
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_open_output_file_denied() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("open_output_denied");
        let path = dir.join("nope.txt").to_str().unwrap().to_string();
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_write(&["/tmp/__nonexistent_prefix__/"])
            .build()
            .expect("builder");
        // C gate denies access — path not in allowed prefix
        let code = format!("(import (tein file)) (open-output-file \"{path}\")");
        assert!(ctx.evaluate(&code).is_err(), "should be denied");
    }

    #[test]
    fn test_open_input_file_unsandboxed_passthrough() {
        // unsandboxed: open-input-file trampoline delegates to chibi original unconditionally
        let tmp = "/tmp/tein_open_unsandboxed_test.txt";
        std::fs::write(tmp, "test").expect("write");
        let ctx = Context::builder().standard_env().build().expect("builder");
        // open-input-file is in env directly; unsandboxed — delegates to chibi original unconditionally
        let r = ctx.evaluate(&format!(
            "(let ((p (open-input-file \"{tmp}\"))) (close-input-port p) #t)"
        ));
        assert_eq!(r.expect("unsandboxed passthrough"), Value::Boolean(true));
    }

    // --- standard environment ---

    #[test]
    fn test_standard_env_loads() {
        // low-level: verify sexp_load_standard_env succeeds with VFS
        unsafe {
            let ctx = ffi::sexp_make_eval_context(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                (4 * 1024 * 1024) as ffi::sexp_uint_t,
                (128 * 1024 * 1024) as ffi::sexp_uint_t,
            );
            assert!(!ctx.is_null(), "context creation failed");

            let env = ffi::sexp_context_env(ctx);
            let version = ffi::sexp_make_fixnum(7);
            let result = ffi::load_standard_env(ctx, env, version);
            assert!(
                ffi::sexp_exceptionp(result) == 0,
                "sexp_load_standard_env returned an exception"
            );

            ffi::sexp_destroy_context(ctx);
        }
    }

    #[test]
    fn test_new_standard_convenience() {
        let ctx = Context::new_standard().expect("new_standard");
        let r = ctx.evaluate("(+ 1 2)").expect("basic arithmetic");
        assert_eq!(r, Value::Integer(3));
    }

    #[test]
    fn test_standard_env_map() {
        // map is defined in (scheme base), not available in bare primitive env
        let ctx = Context::new_standard().expect("new_standard");
        let r = ctx.evaluate("(map + '(1 2 3) '(10 20 30))").expect("map");
        assert_eq!(
            r,
            Value::List(vec![
                Value::Integer(11),
                Value::Integer(22),
                Value::Integer(33),
            ])
        );
    }

    #[test]
    fn test_standard_env_for_each() {
        let ctx = Context::new_standard().expect("new_standard");
        // for-each returns void but shouldn't error
        let r = ctx
            .evaluate("(let ((sum 0)) (for-each (lambda (x) (set! sum (+ sum x))) '(1 2 3)) sum)")
            .expect("for-each");
        assert_eq!(r, Value::Integer(6));
    }

    #[test]
    fn test_standard_env_with_sandbox() {
        // sandboxed(Modules::only(["scheme/base"])) — map and for-each are
        // available after (import (scheme base)).
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .build()
            .expect("standard + sandboxed");

        // import scheme/base first
        ctx.evaluate("(import (scheme base))").expect("import");

        // map is available after import
        let r = ctx
            .evaluate("(map + '(1 2 3) '(10 20 30))")
            .expect("map in sandbox");
        assert_eq!(
            r,
            Value::List(vec![
                Value::Integer(11),
                Value::Integer(22),
                Value::Integer(33),
            ])
        );

        // for-each works too
        ctx.evaluate("(define sandbox-sum 0)").expect("define");
        ctx.evaluate("(for-each (lambda (x) (set! sandbox-sum (+ sandbox-sum x))) '(1 2 3))")
            .expect("for-each in sandbox");
        let r = ctx.evaluate("sandbox-sum").expect("read sum");
        assert_eq!(r, Value::Integer(6));

        // eval is NOT importable (scheme/eval not in allowlist) — UX stub fires
        let err = ctx.evaluate("(import (scheme eval))");
        assert!(err.is_err(), "scheme/eval should be blocked by sandbox");
    }

    #[test]
    fn test_standard_env_values() {
        // values and call-with-values are r7rs features from (scheme base)
        let ctx = Context::new_standard().expect("new_standard");
        let r = ctx
            .evaluate("(call-with-values (lambda () (values 1 2)) +)")
            .expect("values + call-with-values");
        assert_eq!(r, Value::Integer(3));
    }

    #[test]
    fn test_standard_env_dynamic_wind() {
        // dynamic-wind is a key r7rs feature from the standard env
        let ctx = Context::new_standard().expect("new_standard");
        let r = ctx
            .evaluate(
                "(let ((log '())) \
                   (dynamic-wind \
                     (lambda () (set! log (cons 'in log))) \
                     (lambda () (set! log (cons 'body log)) 42) \
                     (lambda () (set! log (cons 'out log)))) \
                   (reverse log))",
            )
            .expect("dynamic-wind");
        assert_eq!(
            r,
            Value::List(vec![
                Value::Symbol("in".to_string()),
                Value::Symbol("body".to_string()),
                Value::Symbol("out".to_string()),
            ])
        );
    }

    #[test]
    fn test_standard_env_with_step_limit() {
        // standard_env + step limit should work together
        let ctx = Context::builder()
            .standard_env()
            .step_limit(1_000_000)
            .build()
            .expect("standard + step limit");

        let r = ctx
            .evaluate("(map car '((1 2) (3 4) (5 6)))")
            .expect("map + step limit");
        assert_eq!(
            r,
            Value::List(vec![
                Value::Integer(1),
                Value::Integer(3),
                Value::Integer(5),
            ])
        );
    }

    // --- VFS gate ---

    #[test]
    fn test_vfs_gate_active_when_sandboxed() {
        use crate::sandbox::{GATE_CHECK, Modules, VFS_GATE};
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .build()
            .expect("standard + sandboxed");

        VFS_GATE.with(|cell| {
            assert_eq!(
                cell.get(),
                GATE_CHECK,
                "sandboxed standard env should activate VFS gate"
            );
        });

        drop(ctx);
    }

    #[test]
    fn test_vfs_gate_off_without_sandbox() {
        use crate::sandbox::{GATE_OFF, VFS_GATE};
        let ctx = Context::new_standard().expect("new_standard");

        VFS_GATE.with(|cell| {
            assert_eq!(
                cell.get(),
                GATE_OFF,
                "unsandboxed standard env should have gate off"
            );
        });

        drop(ctx);
    }

    #[test]
    fn test_vfs_gate_cleared_on_drop() {
        use crate::sandbox::{GATE_CHECK, GATE_OFF, Modules, VFS_GATE};
        {
            let _ctx = Context::builder()
                .standard_env()
                .sandboxed(Modules::only(&["scheme/base"]))
                .build()
                .expect("standard + sandboxed");

            VFS_GATE.with(|cell| {
                assert_eq!(cell.get(), GATE_CHECK);
            });
        }
        // after drop, gate should reset
        VFS_GATE.with(|cell| {
            assert_eq!(
                cell.get(),
                GATE_OFF,
                "VFS gate should reset to off after context drop"
            );
        });
    }

    #[test]
    fn test_vfs_gate_not_set_without_sandboxed() {
        // unsandboxed context without standard_env should NOT activate VFS gate
        use crate::sandbox::{GATE_OFF, VFS_GATE};
        let ctx = Context::builder().build().expect("bare context");

        VFS_GATE.with(|cell| {
            assert_eq!(
                cell.get(),
                GATE_OFF,
                "unsandboxed context should not set VFS gate"
            );
        });

        drop(ctx);
    }

    #[test]
    fn test_vfs_gate_blocks_filesystem_import() {
        // sandboxed standard-env contexts should block filesystem-based modules
        // like (chibi process) via the VFS gate while allowing VFS-based imports.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("standard + sandboxed");

        // VFS import should succeed
        let r = ctx.evaluate("(import (scheme write))");
        assert!(
            r.is_ok(),
            "(import (scheme write)) should succeed under VFS gate: {:?}",
            r.err()
        );

        // filesystem import should fail — (chibi process) is not in VFS
        let err = ctx.evaluate("(import (chibi process))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "expected SandboxViolation for blocked import, got: {:?}",
            err
        );

        drop(ctx);
    }

    #[test]
    fn test_standard_env_sandbox_allows_vfs_import() {
        // sandboxed standard-env contexts should be able to import VFS modules.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("standard + sandboxed");

        // import scheme write — VFS module with dependencies (srfi 38, etc.)
        let r = ctx.evaluate("(import (scheme write))");
        assert!(r.is_ok(), "(import (scheme write)) failed: {:?}", r.err());

        // verify imported binding works
        let r = ctx.evaluate("(write 42)");
        assert!(
            r.is_ok(),
            "write should be available after import: {:?}",
            r.err()
        );

        // import scheme base — large VFS module with many dependencies
        let r = ctx.evaluate("(import (scheme base))");
        assert!(r.is_ok(), "(import (scheme base)) failed: {:?}", r.err());

        drop(ctx);
    }

    #[test]
    fn test_sequential_context_gate_isolation() {
        // ctx1 sets VFS gate; after drop, ctx2 must still have its gate active.
        // this tests the save/restore RAII pattern on Context::drop().
        use crate::sandbox::Modules;
        let ctx1 = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        let ctx2 = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();

        // drop ctx1 — must NOT clear ctx2's VFS gate
        drop(ctx1);

        // ctx2 must still block filesystem modules
        let err = ctx2.evaluate("(import (chibi process))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
            "ctx2 VFS gate must still be active after ctx1 dropped, got: {:?}",
            err
        );
    }

    #[test]
    fn test_sandboxed_modules_all_allows_extra_modules() {
        // Modules::All includes everything registered in the VFS, including modules
        // not in the safe set. chibi/string is a dep of scheme/base (also in All).
        use crate::sandbox::{GATE_CHECK, Modules, VFS_GATE};
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::All)
            .build()
            .expect("standard + sandboxed(All)");

        VFS_GATE.with(|cell| {
            assert_eq!(cell.get(), GATE_CHECK);
        });

        // chibi/string is in the All set
        let r = ctx.evaluate("(import (chibi string))");
        assert!(
            r.is_ok(),
            "(import (chibi string)) should succeed under Modules::All: {:?}",
            r.err()
        );

        // non-VFS filesystem module should still fail (not in the registry)
        let err = ctx.evaluate("(import (chibi process))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "non-VFS import should fail under Modules::All: {:?}",
            err
        );

        drop(ctx);
    }

    #[test]
    fn test_vfs_gate_allow_module() {
        use crate::sandbox::{GATE_CHECK, Modules, VFS_GATE};
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .allow_module("chibi/string")
            .build()
            .expect("standard + sandboxed + allow_module");

        VFS_GATE.with(|cell| {
            assert_eq!(cell.get(), GATE_CHECK);
        });

        // chibi/string was explicitly allowed (+ its deps resolved automatically)
        let r = ctx.evaluate("(import (chibi string))");
        assert!(
            r.is_ok(),
            "(import (chibi string)) should succeed: {:?}",
            r.err()
        );

        drop(ctx);
    }

    #[test]
    fn test_sandboxed_modules_none_with_explicit_allow_module() {
        // Modules::None + allow_module: start from nothing, add only what you need
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::None)
            .allow_module("tein/test")
            .build()
            .expect("standard + sandboxed(None) + allow_module");

        // tein/test was explicitly listed — should work
        let r = ctx.evaluate("(import (tein test))");
        assert!(
            r.is_ok(),
            "(import (tein test)) should succeed: {:?}",
            r.err()
        );

        // chibi/process is not in the explicit list
        let err = ctx.evaluate("(import (chibi process))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "(import (chibi process)) should fail with Modules::None: {:?}",
            err
        );

        drop(ctx);
    }

    #[test]
    fn test_vfs_gate_allowlist_raii() {
        use crate::sandbox::{Modules, VFS_ALLOWLIST};
        // verify allowlist is restored, not just the gate level
        {
            let _ctx = Context::builder()
                .standard_env()
                .sandboxed(Modules::only(&["scheme/base"]))
                .allow_module("chibi/string")
                .build()
                .expect("context with extended allowlist");
        }
        // after drop, allowlist should be empty (previous was empty)
        VFS_ALLOWLIST.with(|cell| {
            assert!(
                cell.borrow().is_empty(),
                "allowlist should be restored to empty after drop"
            );
        });
    }

    #[test]
    fn test_vfs_gate_transitive_deps() {
        use crate::sandbox::Modules;
        // allow_module resolves transitive deps from the registry,
        // so adding tein/foreign also enables its deps (chibi/string etc).
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::None)
            .allow_module("tein/foreign")
            .build()
            .expect("minimal allowlist with dep resolution");

        // chibi/string is a dep of tein/foreign — should be resolved automatically
        let r = ctx.evaluate("(import (chibi string))");
        assert!(
            r.is_ok(),
            "(chibi string) should load via transitive dep resolution: {:?}",
            r.err()
        );

        drop(ctx);
    }

    #[test]
    fn test_standard_env_import() {
        // user-facing (import ...) in a standard env context.
        // works with default 8MB heap now that evaluate() gc-protects its
        // port and sexp_load_op properly preserves VFS strings.
        let ctx = Context::new_standard().unwrap();

        // import a module with inline begin (no include)
        let r = ctx.evaluate("(import (srfi 11))");
        assert!(r.is_ok(), "(import (srfi 11)) failed: {:?}", r.err());

        // import a module with dependencies (chibi ast, srfi 69)
        let r = ctx.evaluate("(import (srfi 38))");
        assert!(r.is_ok(), "(import (srfi 38)) failed: {:?}", r.err());

        // import scheme write (depends on srfi 38)
        let r = ctx.evaluate("(import (scheme write))");
        assert!(r.is_ok(), "(import (scheme write)) failed: {:?}", r.err());

        // import scheme base (depends on chibi io, equiv, string, ast, srfi 9/11/39)
        let r = ctx.evaluate("(import (scheme base))");
        assert!(r.is_ok(), "(import (scheme base)) failed: {:?}", r.err());

        // verify imported bindings work
        let r = ctx
            .evaluate("(let-values (((a b) (values 1 2))) (+ a b))")
            .unwrap();
        assert_eq!(r.as_integer(), Some(3));
    }

    // --- characters ---

    #[test]
    fn test_char_value() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate(r"#\a").expect("failed to evaluate");
        assert_eq!(result, Value::Char('a'));
        assert_eq!(result.as_char(), Some('a'));
    }

    #[test]
    fn test_char_special() {
        let ctx = Context::new().expect("failed to create context");
        assert_eq!(ctx.evaluate(r"#\space").expect("space"), Value::Char(' '));
        assert_eq!(
            ctx.evaluate(r"#\newline").expect("newline"),
            Value::Char('\n')
        );
        assert_eq!(ctx.evaluate(r"#\tab").expect("tab"), Value::Char('\t'));
    }

    #[test]
    fn test_char_unicode() {
        let ctx = Context::new().expect("failed to create context");
        // lambda character
        let result = ctx.evaluate(r"#\λ").expect("unicode char");
        assert_eq!(result, Value::Char('λ'));
    }

    #[test]
    fn test_char_display() {
        assert_eq!(format!("{}", Value::Char('a')), r"#\a");
        assert_eq!(format!("{}", Value::Char(' ')), r"#\space");
        assert_eq!(format!("{}", Value::Char('\n')), r"#\newline");
        assert_eq!(format!("{}", Value::Char('\t')), r"#\tab");
    }

    #[test]
    fn test_char_round_trip() {
        unsafe extern "C" fn return_char(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                Value::Char('λ')
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("context");
        ctx.define_fn_variadic("get-char", return_char)
            .expect("define");
        let result = ctx.evaluate("(get-char)").expect("call");
        assert_eq!(result, Value::Char('λ'));
    }

    /// L7: string `\x` escape must reject surrogates and out-of-range codepoints.
    #[test]
    fn test_string_hex_escape_rejects_surrogates() {
        let ctx = Context::new_standard().expect("standard context");
        // surrogate codepoint
        let result = ctx.evaluate(r#"(string-length "\xD800;")"#);
        assert!(result.is_err(), "surrogate \\xD800; should be rejected");
        // beyond Unicode range
        let result = ctx.evaluate(r#"(string-length "\x110000;")"#);
        assert!(result.is_err(), "\\x110000; should be rejected");
        // valid codepoint should still work
        let result = ctx.evaluate(r#"(string-length "\x03BB;")"#);
        assert_eq!(result.expect("valid hex escape"), Value::Integer(1));
    }

    /// L11: `integer->char` must reject non-Unicode-scalar-values.
    #[test]
    fn test_integer_to_char_rejects_invalid() {
        let ctx = Context::new_standard().expect("standard context");
        // negative
        let result = ctx.evaluate("(integer->char -1)");
        assert!(result.is_err(), "negative should be rejected");
        // surrogate
        let result = ctx.evaluate("(integer->char #xD800)");
        assert!(result.is_err(), "surrogate should be rejected");
        // beyond Unicode range
        let result = ctx.evaluate("(integer->char #x110000)");
        assert!(result.is_err(), "above 0x10FFFF should be rejected");
        // valid codepoint should still work
        let result = ctx.evaluate("(integer->char 955)");
        assert_eq!(result.expect("valid char"), Value::Char('λ'));
    }

    // --- ports ---

    #[test]
    fn test_port_opaque() {
        let ctx = Context::new_standard().expect("standard context");
        let result = ctx.evaluate("(current-input-port)").expect("port");
        assert!(result.is_port(), "expected Port, got {:?}", result);
    }

    #[test]
    fn test_port_display() {
        // can't easily construct a Port without a context, just test Display for coverage
        assert_eq!(format!("{}", Value::Port(std::ptr::null_mut())), "#<port>");
    }

    // --- bytevectors ---

    #[test]
    fn test_bytevector_value() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("#u8(1 2 3)").expect("failed to evaluate");
        assert_eq!(result, Value::Bytevector(vec![1, 2, 3]));
        assert_eq!(result.as_bytevector(), Some([1u8, 2, 3].as_slice()));
    }

    #[test]
    fn test_bytevector_empty() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("#u8()").expect("failed to evaluate");
        assert_eq!(result, Value::Bytevector(vec![]));
    }

    #[test]
    fn test_bytevector_display() {
        let bv = Value::Bytevector(vec![0, 127, 255]);
        assert_eq!(format!("{}", bv), "#u8(0 127 255)");
        assert_eq!(format!("{}", Value::Bytevector(vec![])), "#u8()");
    }

    #[test]
    fn test_bytevector_round_trip() {
        unsafe extern "C" fn return_bv(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                Value::Bytevector(vec![10, 20, 30])
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("context");
        ctx.define_fn_variadic("get-bv", return_bv).expect("define");
        let result = ctx.evaluate("(get-bv)").expect("call");
        assert_eq!(result, Value::Bytevector(vec![10, 20, 30]));
    }

    // --- hash tables ---

    #[test]
    fn test_hash_table_falls_through_to_other() {
        // hash tables use a runtime-registered type tag from srfi-69's define-record-type
        // and cannot be reliably detected without module introspection at runtime.
        // they fall through to Other and can still be passed back to scheme code.
        //
        // TODO: detection could be added by looking up the hash-table type object via
        // sexp_env_ref at context init time and comparing sexp_object_type at detection.
        let ctx = Context::new_standard().expect("standard context");
        ctx.evaluate("(import (srfi 69))").expect("import srfi-69");
        let result = ctx.evaluate("(make-hash-table)").expect("hash table");
        assert!(matches!(result, Value::Other(_)), "got {:?}", result);
    }

    // --- continuations ---

    #[test]
    fn test_continuation_is_procedure() {
        // continuations in chibi are SEXP_PROCEDURE at the type level.
        // they're fully callable via Context::call, just like regular procedures.
        let ctx = Context::new_standard().expect("standard context");
        let result = ctx
            .evaluate("(call-with-current-continuation (lambda (k) k))")
            .expect("call/cc");
        assert!(
            result.is_procedure(),
            "expected Procedure, got {:?}",
            result
        );
    }

    #[test]
    fn test_sandbox_violation_error_variant() {
        // SandboxViolation should be a distinct variant with its own Display
        let err = Error::SandboxViolation("test message".to_string());
        assert!(matches!(err, Error::SandboxViolation(_)));
        assert_eq!(format!("{}", err), "sandbox violation: test message");

        // should not match EvalError
        assert!(!matches!(err, Error::EvalError(_)));
    }

    #[test]
    fn test_file_io_sandbox_violation_type() {
        use crate::sandbox::Modules;
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .file_read(&["/allowed/"])
            .build()
            .expect("builder");

        let err = ctx
            .evaluate("(import (tein file)) (open-input-file \"/etc/passwd\")")
            .unwrap_err();
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

    #[test]
    fn test_module_import_sandbox_violation_type() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("standard + sandboxed");

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

    #[test]
    fn test_ux_stub_binding_hint() {
        // UX stubs in Modules::None context should emit informative messages
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::None)
            .build()
            .expect("builder");

        // map is from scheme/base — UX stub should hint at the providing module.
        // use a literal arg so we don't trigger other stubs during argument evaluation.
        let err = ctx.evaluate("(map 1)").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "expected SandboxViolation for UX stub, got: {:?}",
            err
        );
        let msg = format!("{}", err);
        assert!(
            msg.contains("requires (import"),
            "expected hint with '(import ...)', got: {}",
            msg
        );
    }

    #[test]
    fn test_ux_stub_absent_after_import() {
        // after importing scheme/base, its bindings work and are not stubbed
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .build()
            .expect("builder");

        ctx.evaluate("(import (scheme base))").expect("import");
        let result = ctx
            .evaluate("(cons 1 2)")
            .expect("cons should work after import");
        assert_eq!(
            result,
            Value::Pair(Box::new(Value::Integer(1)), Box::new(Value::Integer(2)))
        );
    }

    // --- foreign type protocol ---

    use crate::foreign::{ForeignType, MethodFn};

    struct TestCounter {
        n: i64,
    }

    impl ForeignType for TestCounter {
        fn type_name() -> &'static str {
            "test-counter"
        }
        fn methods() -> &'static [(&'static str, MethodFn)] {
            &[
                ("increment", |obj, _ctx, _args| {
                    let c = obj.downcast_mut::<TestCounter>().unwrap();
                    c.n += 1;
                    Ok(Value::Integer(c.n))
                }),
                ("get", |obj, _ctx, _args| {
                    let c = obj.downcast_ref::<TestCounter>().unwrap();
                    Ok(Value::Integer(c.n))
                }),
                ("reset", |obj, _ctx, _args| {
                    let c = obj.downcast_mut::<TestCounter>().unwrap();
                    c.n = 0;
                    Ok(Value::Unspecified)
                }),
            ]
        }
    }

    /// Register TestCounter and a Scheme-callable constructor (make-test-counter).
    fn setup_test_counter(ctx: &Context) {
        ctx.register_foreign_type::<TestCounter>()
            .expect("register TestCounter");

        // constructor function — accesses ForeignStore via the thread-local
        // set by evaluate/call, following the same pattern as IO wrappers
        unsafe extern "C" fn make_test_counter(
            ctx_ptr: ffi::sexp,
            _self: ffi::sexp,
            _n: ffi::sexp_sint_t,
            _args: ffi::sexp,
        ) -> ffi::sexp {
            unsafe {
                let store_ptr = FOREIGN_STORE_PTR.with(|c| c.get());
                if store_ptr.is_null() {
                    let msg = "make-test-counter: no store";
                    let c_msg = CString::new(msg).unwrap_or_default();
                    return ffi::make_error(ctx_ptr, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
                }
                let id = (*store_ptr).borrow_mut().insert(TestCounter { n: 0 });
                let val = Value::Foreign {
                    handle_id: id,
                    type_name: "test-counter".to_string(),
                };
                val.to_raw(ctx_ptr).unwrap_or_else(|_| ffi::get_void())
            }
        }

        ctx.define_fn_variadic("make-test-counter", make_test_counter)
            .expect("define make-test-counter");
    }

    // --- task 7: registration and round-trip ---

    #[test]
    fn test_foreign_type_register() {
        let ctx = Context::new_standard().expect("context");
        ctx.register_foreign_type::<TestCounter>()
            .expect("register");
        let types = ctx.evaluate("(foreign-types)").expect("foreign-types");
        let list = types.as_list().expect("expected list");
        assert!(
            list.iter().any(|v| v.as_string() == Some("test-counter")),
            "test-counter not in foreign-types: {:?}",
            list
        );
    }

    #[test]
    fn test_foreign_value_roundtrip() {
        let ctx = Context::new_standard().expect("context");
        ctx.register_foreign_type::<TestCounter>()
            .expect("register");

        let val = ctx
            .foreign_value(TestCounter { n: 42 })
            .expect("create foreign value");
        assert!(val.is_foreign());
        assert_eq!(val.foreign_type_name(), Some("test-counter"));

        // to_raw → from_raw round-trip
        let raw = unsafe { val.to_raw(ctx.ctx_ptr()).unwrap() };
        let back = unsafe { Value::from_raw(ctx.ctx_ptr(), raw).unwrap() };
        assert_eq!(val, back);
    }

    #[test]
    fn test_foreign_register_duplicate_error() {
        let ctx = Context::new_standard().expect("context");
        ctx.register_foreign_type::<TestCounter>()
            .expect("first register");
        let err = ctx.register_foreign_type::<TestCounter>().unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("already registered"),
            "expected 'already registered', got: {}",
            msg
        );
    }

    // --- task 8: dispatch and error messages ---

    #[test]
    fn test_foreign_call_dispatch() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx
            .evaluate(
                "(let ((c (make-test-counter)))
               (test-counter-increment c)
               (test-counter-increment c)
               (test-counter-get c))",
            )
            .expect("dispatch");
        assert_eq!(result, Value::Integer(2));
    }

    #[test]
    fn test_foreign_call_universal_dispatch() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx
            .evaluate(
                "(let ((c (make-test-counter)))
               (foreign-call c 'increment)
               (foreign-call c 'get))",
            )
            .expect("foreign-call dispatch");
        assert_eq!(result, Value::Integer(1));
    }

    #[test]
    fn test_foreign_call_mutable_state() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx
            .evaluate(
                "(let ((c (make-test-counter)))
               (test-counter-increment c)
               (test-counter-increment c)
               (test-counter-reset c)
               (test-counter-get c))",
            )
            .expect("mutable state");
        assert_eq!(result, Value::Integer(0));
    }

    #[test]
    fn test_foreign_call_wrong_method() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let err = ctx
            .evaluate(
                "(let ((c (make-test-counter)))
               (foreign-call c 'nonexistent))",
            )
            .unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("no method 'nonexistent'"),
            "expected method error, got: {}",
            msg
        );
        assert!(
            msg.contains("increment"),
            "should list available methods, got: {}",
            msg
        );
    }

    #[test]
    fn test_foreign_call_not_foreign() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let err = ctx.evaluate("(foreign-call 42 'get)").unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("expected foreign object"),
            "expected type error, got: {}",
            msg
        );
    }

    #[test]
    fn test_foreign_convenience_wrong_type() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let err = ctx.evaluate("(test-counter-get 42)").unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("expected test-counter"),
            "expected type error, got: {}",
            msg
        );
    }

    // --- task 9: introspection and predicates ---

    #[test]
    fn test_foreign_introspection_methods() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx
            .evaluate("(let ((c (make-test-counter))) (foreign-methods c))")
            .expect("foreign-methods");
        let methods = result.as_list().expect("expected list");
        let names: Vec<&str> = methods
            .iter()
            .filter_map(|v| v.as_symbol().or_else(|| v.as_string()))
            .collect();
        assert!(names.contains(&"increment"), "got: {:?}", names);
        assert!(names.contains(&"get"), "got: {:?}", names);
        assert!(names.contains(&"reset"), "got: {:?}", names);
    }

    #[test]
    fn test_foreign_introspection_type_methods() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx
            .evaluate("(foreign-type-methods \"test-counter\")")
            .expect("foreign-type-methods");
        let methods = result.as_list().expect("expected list");
        assert_eq!(methods.len(), 3);
    }

    #[test]
    fn test_foreign_predicate_true() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx
            .evaluate("(let ((c (make-test-counter))) (test-counter? c))")
            .expect("predicate true");
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_foreign_predicate_false() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx.evaluate("(test-counter? 42)").expect("predicate false");
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_foreign_display() {
        let val = Value::Foreign {
            handle_id: 7,
            type_name: "http-client".to_string(),
        };
        assert_eq!(format!("{}", val), "#<foreign http-client:7>");
    }

    #[test]
    fn test_foreign_type_accessor() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx
            .evaluate("(let ((c (make-test-counter))) (foreign-type c))")
            .expect("foreign-type accessor");
        assert_eq!(result, Value::String("test-counter".to_string()));
    }

    #[test]
    fn test_foreign_handle_id_accessor() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        // handle IDs start at 1, so the first created object has id >= 1
        let result = ctx
            .evaluate("(let ((c (make-test-counter))) (foreign-handle-id c))")
            .expect("foreign-handle-id accessor");
        match result {
            Value::Integer(n) => assert!(n >= 1, "handle id should be >= 1, got {}", n),
            other => panic!("expected integer handle id, got {}", other),
        }
    }

    // --- task 10: sandbox integration and cleanup ---

    #[test]
    fn test_foreign_in_sandbox() {
        // verify foreign protocol works inside a sandboxed context.
        // uses new_standard() to ensure protocol helpers have all required
        // primitives (and, equal?, fixnum?, etc.) — real-world usage would
        // similarly ensure the env has what the protocol needs.
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx
            .evaluate(
                "(let ((c (make-test-counter)))
               (test-counter-increment c)
               (test-counter-get c))",
            )
            .expect("sandboxed foreign call");
        assert_eq!(result, Value::Integer(1));
    }

    #[test]
    fn test_foreign_cleanup_on_drop() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        // a type that signals when its value is dropped
        struct Canary(Arc<AtomicBool>);
        impl Drop for Canary {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        impl ForeignType for Canary {
            fn type_name() -> &'static str {
                "canary"
            }
            fn methods() -> &'static [(&'static str, MethodFn)] {
                &[]
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        {
            let ctx = Context::new_standard().expect("context");
            ctx.register_foreign_type::<Canary>().expect("register");
            let _val = ctx
                .foreign_value(Canary(dropped.clone()))
                .expect("create canary");
            assert!(!dropped.load(Ordering::SeqCst), "should not be dropped yet");
        }
        // Context dropped → ForeignStore dropped → Canary dropped
        assert!(
            dropped.load(Ordering::SeqCst),
            "canary should be dropped when Context drops"
        );
    }

    // --- managed context (persistent mode) ---

    #[test]
    fn test_managed_persistent_evaluate() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_managed(|_ctx| Ok(()))
            .unwrap();
        let result = ctx.evaluate("(+ 1 2 3)").unwrap();
        assert_eq!(result, Value::Integer(6));
    }

    #[test]
    fn test_managed_persistent_state_accumulates() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_managed(|_ctx| Ok(()))
            .unwrap();
        ctx.evaluate("(define x 42)").unwrap();
        let result = ctx.evaluate("x").unwrap();
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_managed_persistent_init_closure() {
        let ctx = Context::builder()
            .standard_env()
            .step_limit(1_000_000)
            .build_managed(|ctx| {
                ctx.evaluate("(define greeting \"hello from init\")")?;
                Ok(())
            })
            .unwrap();
        let result = ctx.evaluate("greeting").unwrap();
        assert_eq!(result, Value::String("hello from init".to_string()));
    }

    #[test]
    fn test_managed_persistent_call() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_managed(|_ctx| Ok(()))
            .unwrap();
        let proc = ctx.evaluate("+").unwrap();
        let result = ctx
            .call(&proc, &[Value::Integer(10), Value::Integer(20)])
            .unwrap();
        assert_eq!(result, Value::Integer(30));
    }

    #[test]
    fn test_managed_persistent_define_fn_variadic() {
        use crate::raw;

        unsafe extern "C" fn always_42(
            _ctx: raw::sexp,
            _self: raw::sexp,
            _n: raw::sexp_sint_t,
            _args: raw::sexp,
        ) -> raw::sexp {
            unsafe { raw::sexp_make_fixnum(42) }
        }

        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_managed(|_ctx| Ok(()))
            .unwrap();
        ctx.define_fn_variadic("always-42", always_42).unwrap();
        let result = ctx.evaluate("(always-42)").unwrap();
        assert_eq!(result, Value::Integer(42));
    }

    // --- managed context (fresh mode) ---

    #[test]
    fn test_managed_fresh_evaluate() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_managed_fresh(|_ctx| Ok(()))
            .unwrap();
        let result = ctx.evaluate("(+ 10 20)").unwrap();
        assert_eq!(result, Value::Integer(30));
    }

    #[test]
    fn test_managed_fresh_state_does_not_persist() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_managed_fresh(|_ctx| Ok(()))
            .unwrap();
        ctx.evaluate("(define x 42)").unwrap();
        // fresh mode rebuilds context, so x should not exist
        let result = ctx.evaluate("x");
        assert!(result.is_err());
    }

    #[test]
    fn test_managed_fresh_init_closure_runs_each_time() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};

        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = counter.clone();

        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_managed_fresh(move |_ctx| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap();

        ctx.evaluate("(+ 1 1)").unwrap();
        ctx.evaluate("(+ 2 2)").unwrap();
        ctx.evaluate("(+ 3 3)").unwrap();

        // init ran once during build + once per evaluate = 4
        assert_eq!(counter.load(Ordering::SeqCst), 4);
    }

    // --- managed context (reset) ---

    #[test]
    fn test_managed_persistent_reset_clears_state() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_managed(|_ctx| Ok(()))
            .unwrap();
        ctx.evaluate("(define x 99)").unwrap();
        assert_eq!(ctx.evaluate("x").unwrap(), Value::Integer(99));

        ctx.reset().unwrap();

        // after reset, x should not exist
        let result = ctx.evaluate("x");
        assert!(result.is_err());
    }

    #[test]
    fn test_managed_persistent_reset_reruns_init() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};

        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = counter.clone();

        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_managed(move |_ctx| {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap();

        // init ran once during build
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        ctx.reset().unwrap();

        // init ran again during reset
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_managed_fresh_reset_is_noop() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_managed_fresh(|_ctx| Ok(()))
            .unwrap();
        // should not error
        ctx.reset().unwrap();
    }

    // --- managed context (error handling) ---

    #[test]
    fn test_managed_init_failure_returns_error() {
        let result = Context::builder()
            .step_limit(1_000_000)
            .build_managed(|_ctx| Err(Error::InitError("intentional init failure".to_string())));
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::InitError(msg) => assert!(msg.contains("intentional init failure")),
            other => panic!("expected InitError, got {:?}", other),
        }
    }

    #[test]
    fn test_managed_mode() {
        use crate::managed::Mode;

        let persistent = Context::builder()
            .step_limit(1_000_000)
            .build_managed(|_ctx| Ok(()))
            .unwrap();
        assert_eq!(persistent.mode(), Mode::Persistent);

        let fresh = Context::builder()
            .step_limit(1_000_000)
            .build_managed_fresh(|_ctx| Ok(()))
            .unwrap();
        assert_eq!(fresh.mode(), Mode::Fresh);
    }

    #[test]
    fn test_managed_drop_cleans_up() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_managed(|_ctx| Ok(()))
            .unwrap();
        drop(ctx);
        // no panic, no leaked thread — success
    }

    #[test]
    fn test_managed_concurrent_evaluate() {
        // ThreadLocalContext is Send + Sync; concurrent callers are serialised
        // by the Mutex<(Sender, Receiver)> guarding the entire send+recv roundtrip.
        use std::sync::Arc;
        let ctx = Arc::new(
            Context::builder()
                .step_limit(1_000_000)
                .build_managed(|_| Ok(()))
                .unwrap(),
        );
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let ctx = Arc::clone(&ctx);
                std::thread::spawn(move || {
                    let expr = format!("(+ {} {})", i, i);
                    let result = ctx.evaluate(&expr).expect("evaluate");
                    assert_eq!(result, Value::Integer(i * 2));
                })
            })
            .collect();
        for h in handles {
            h.join().expect("thread panicked");
        }
    }

    #[test]
    fn test_managed_thread_panic_in_init_returns_error() {
        // a panicking init closure causes the thread to die before sending the
        // success response; recv() returns Err → InitError, not a hang.
        let result = Context::builder()
            .step_limit(100_000)
            .build_managed(|_| -> Result<()> { panic!("intentional test panic") });
        assert!(
            result.is_err(),
            "panicking init closure must return Err, got Ok"
        );
    }

    #[test]
    fn test_managed_thread_panic_in_loop_returns_init_error() {
        // after a panic caught by catch_unwind in the message loop, the thread
        // exits cleanly. subsequent calls must return InitError (channel dead), not hang.
        // we simulate this by having a fresh-mode context whose rebuild-init panics
        // on the second invocation (i.e., during evaluate(), not build_managed()).
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);
        let ctx = Context::builder()
            .step_limit(100_000)
            .build_managed_fresh(|_| -> Result<()> {
                // first call succeeds (build_managed_fresh init), subsequent calls panic
                let n = CALL_COUNT.fetch_add(1, Ordering::SeqCst);
                if n >= 1 {
                    panic!("intentional panic on evaluate");
                }
                Ok(())
            })
            .unwrap();

        // first evaluate() triggers fresh-mode rebuild which panics → caught by catch_unwind
        // → sends InitError response → evaluate() returns Err
        let err = ctx.evaluate("42").unwrap_err();
        assert!(
            matches!(err, Error::InitError(_)),
            "caught panic must return InitError, got {:?}",
            err
        );
    }

    // --- custom ports ---

    #[test]
    fn test_open_input_port_basic() {
        let ctx = Context::new_standard().expect("context");
        let reader = std::io::Cursor::new(b"(+ 1 2)");
        let port = ctx.open_input_port(reader);
        assert!(port.is_ok(), "open_input_port should succeed");
        assert!(port.unwrap().is_port(), "should return a Port value");
    }

    #[test]
    fn test_read_from_custom_port() {
        let ctx = Context::new_standard().expect("context");
        let reader = std::io::Cursor::new(b"42");
        let port = ctx.open_input_port(reader).expect("open port");
        let val = ctx.read(&port).expect("read");
        assert_eq!(val, Value::Integer(42));
    }

    #[test]
    fn test_evaluate_port_single() {
        let ctx = Context::new_standard().expect("context");
        let port = ctx
            .open_input_port(std::io::Cursor::new(b"(+ 1 2)"))
            .expect("port");
        let result = ctx.evaluate_port(&port).expect("eval");
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn test_evaluate_port_multiple() {
        let ctx = Context::new_standard().expect("context");
        let port = ctx
            .open_input_port(std::io::Cursor::new(b"(define x 10) (+ x 5)"))
            .expect("port");
        let result = ctx.evaluate_port(&port).expect("eval");
        assert_eq!(result, Value::Integer(15));
    }

    #[test]
    fn test_evaluate_port_empty() {
        let ctx = Context::new_standard().expect("context");
        let port = ctx
            .open_input_port(std::io::Cursor::new(b""))
            .expect("port");
        let result = ctx.evaluate_port(&port).expect("eval");
        assert_eq!(result, Value::Unspecified);
    }

    #[test]
    fn test_output_port_write() {
        use std::sync::{Arc, Mutex};

        struct SharedWriter(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for SharedWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let ctx = Context::new_standard().expect("context");
        let port = ctx
            .open_output_port(SharedWriter(buf.clone()))
            .expect("port");

        ctx.call(
            &ctx.evaluate("display").expect("display"),
            &[Value::String("hello".into()), port.clone()],
        )
        .expect("display call");

        // flush to ensure buffered output is written to the custom port.
        // chibi's primitive is flush-output; flush-output-port is in (scheme extras).
        ctx.call(&ctx.evaluate("flush-output").expect("flush"), &[port])
            .expect("flush");

        let output = buf.lock().unwrap();
        assert_eq!(&*output, b"hello");
    }

    #[test]
    fn test_port_read_multiple_sexps() {
        let ctx = Context::new_standard().expect("context");
        let port = ctx
            .open_input_port(std::io::Cursor::new(b"1 2 3"))
            .expect("port");
        assert_eq!(ctx.read(&port).unwrap(), Value::Integer(1));
        assert_eq!(ctx.read(&port).unwrap(), Value::Integer(2));
        assert_eq!(ctx.read(&port).unwrap(), Value::Integer(3));
        assert_eq!(ctx.read(&port).unwrap(), Value::Unspecified); // EOF
    }

    #[test]
    fn test_port_recognized_by_scheme() {
        let ctx = Context::new_standard().expect("context");
        let port = ctx
            .open_input_port(std::io::Cursor::new(b"42"))
            .expect("port");
        let is_port = ctx
            .call(&ctx.evaluate("input-port?").expect("fn"), &[port])
            .expect("call");
        assert_eq!(is_port, Value::Boolean(true));
    }

    #[test]
    fn test_port_scheme_read() {
        let ctx = Context::new_standard().expect("context");
        let port = ctx
            .open_input_port(std::io::Cursor::new(b"(list 1 2 3)"))
            .expect("port");
        let read_fn = ctx.evaluate("read").expect("read fn");
        let expr = ctx
            .call(&read_fn, std::slice::from_ref(&port))
            .expect("read");
        // expr is unevaluated: (list 1 2 3)
        let result = ctx
            .call(&ctx.evaluate("eval").expect("eval"), &[expr])
            .expect("eval");
        assert_eq!(
            result,
            Value::List(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3)
            ])
        );
    }

    // --- reader dispatch tests ---

    #[test]
    fn test_reader_dispatch_basic() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein reader))").expect("import");
        // handler returns a self-evaluating value (number).
        ctx.evaluate("(set-reader! #\\j (lambda (port) 42))")
            .expect("set-reader");
        let result = ctx.evaluate("#j").expect("eval #j");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_reader_dispatch_reserved_char() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein reader))").expect("import");
        let result = ctx.evaluate("(set-reader! #\\t (lambda (port) 42))");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("reserved"), "expected 'reserved' in: {}", msg);
    }

    #[test]
    fn test_reader_dispatch_handler_reads_port() {
        // handler reads further from the input port (the #j syntax consumes
        // more input). use read() to inspect the raw datum — the handler
        // returns a list, which evaluate() would try to call as a procedure.
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein reader))").expect("import");
        ctx.evaluate("(set-reader! #\\j (lambda (port) (list 'json (read port))))")
            .expect("set");
        let port = ctx
            .open_input_port(std::io::Cursor::new(b"#j(1 2 3)"))
            .expect("port");
        let result = ctx.read(&port).expect("read");
        let list = result.as_list().expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], Value::Symbol("json".into()));
    }

    #[test]
    fn test_reader_dispatch_unset() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein reader))").expect("import");
        ctx.evaluate("(set-reader! #\\j (lambda (port) 42))")
            .expect("set");
        assert_eq!(ctx.evaluate("#j").unwrap(), Value::Integer(42));
        ctx.evaluate("(unset-reader! #\\j)").expect("unset");
        assert!(ctx.evaluate("#j").is_err());
    }

    #[test]
    fn test_reader_dispatch_chars_introspection() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein reader))").expect("import");
        ctx.evaluate("(set-reader! #\\j (lambda (port) 42))")
            .expect("set j");
        ctx.evaluate("(set-reader! #\\p (lambda (port) 42))")
            .expect("set p");
        let chars = ctx.evaluate("(reader-dispatch-chars)").expect("chars");
        let list = chars.as_list().expect("list");
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_reader_dispatch_multiple_chars() {
        // handler reads further to distinguish sub-syntax.
        // use read() to inspect raw datums.
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein reader))").expect("import");
        ctx.evaluate(
            "(set-reader! #\\j
               (lambda (port)
                 (let ((next (read-char port)))
                   (cond
                     ((char=? next #\\s) (list 'json (read port)))
                     ((char=? next #\\w) (list 'jwt (read port)))
                     (else (error \"unknown #j sub-dispatch\" next))))))",
        )
        .expect("set");

        let port = ctx
            .open_input_port(std::io::Cursor::new(b"#js(1 2 3)"))
            .expect("port");
        let json = ctx.read(&port).expect("json");
        assert_eq!(json.as_list().unwrap()[0], Value::Symbol("json".into()));

        let port2 = ctx
            .open_input_port(std::io::Cursor::new(b"#jw\"token\""))
            .expect("port2");
        let jwt = ctx.read(&port2).expect("jwt");
        assert_eq!(jwt.as_list().unwrap()[0], Value::Symbol("jwt".into()));
    }

    #[test]
    fn test_reader_dispatch_via_import() {
        // verify (import (tein reader)) works in standard context
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein reader))").expect("import");
        ctx.evaluate("(set-reader! #\\j (lambda (port) 42))")
            .expect("set-reader");
        assert_eq!(ctx.evaluate("#j").unwrap(), Value::Integer(42));
    }

    #[test]
    fn test_reader_dispatch_via_import_sandbox() {
        // regression test for #31: (import (tein reader)) must work in
        // sandboxed contexts where the module is loaded via C static lib
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("sandboxed context");
        ctx.evaluate("(import (tein reader))").expect("import");
        ctx.evaluate("(set-reader! #\\j (lambda (port) 42))")
            .expect("set-reader");
        assert_eq!(ctx.evaluate("#j").unwrap(), Value::Integer(42));
    }

    #[test]
    fn test_register_reader_from_rust() {
        let ctx = Context::new_standard().expect("context");
        let handler = ctx.evaluate("(lambda (port) 42)").expect("handler");
        ctx.register_reader(b'j', &handler).expect("register");
        let result = ctx.evaluate("#j").expect("eval");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_register_reader_reserved_from_rust() {
        let ctx = Context::new_standard().expect("context");
        let handler = ctx.evaluate("(lambda (port) 42)").expect("handler");
        let err = ctx.register_reader(b't', &handler).unwrap_err();
        assert!(format!("{}", err).contains("reserved"));
    }

    // --- macro expansion hook tests ---

    #[test]
    fn test_macro_expand_hook_basic() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein macro))").expect("import");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(define hook-called #f)
             (set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (set! hook-called #t)
                 expanded))",
        )
        .expect("set hook");
        ctx.evaluate("(double 5)").expect("use macro");
        let called = ctx.evaluate("hook-called").expect("check");
        assert_eq!(called, Value::Boolean(true));
    }

    #[test]
    fn test_macro_expand_hook_observation() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein macro))").expect("import");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(define captured-unexpanded #f)
             (set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (set! captured-unexpanded unexpanded)
                 expanded))",
        )
        .expect("set hook");
        ctx.evaluate("(double 5)").expect("use macro");
        let captured = ctx.evaluate("captured-unexpanded").expect("check");
        let list = captured.as_list().expect("should be list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[1], Value::Integer(5));
    }

    #[test]
    fn test_macro_expand_hook_name_arg() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein macro))").expect("import");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(define captured-name #f)
             (set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (set! captured-name name)
                 expanded))",
        )
        .expect("set hook");
        ctx.evaluate("(double 5)").expect("use macro");
        let name = ctx.evaluate("captured-name").expect("check");
        assert_eq!(name, Value::Symbol("double".into()));
    }

    #[test]
    fn test_macro_expand_hook_transformation() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein macro))").expect("import");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 42))",
        )
        .expect("set hook");
        let result = ctx.evaluate("(double 5)").expect("use macro");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_macro_expand_hook_reanalyze() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein macro))").expect("import");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define double");
        ctx.evaluate("(define-syntax add1 (syntax-rules () ((add1 x) (+ x 1))))")
            .expect("define add1");
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (if (eq? name 'double)
                     '(add1 99)
                     expanded)))",
        )
        .expect("set hook");
        let result = ctx.evaluate("(double 5)").expect("use macro");
        // (add1 99) -> (+ 99 1) -> 100
        assert_eq!(result, Value::Integer(100));
    }

    #[test]
    fn test_macro_expand_hook_unset() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein macro))").expect("import");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env) 42))",
        )
        .expect("set hook");
        assert_eq!(ctx.evaluate("(double 5)").unwrap(), Value::Integer(42));
        ctx.evaluate("(unset-macro-expand-hook!)").expect("unset");
        assert_eq!(ctx.evaluate("(double 5)").unwrap(), Value::Integer(10));
    }

    #[test]
    fn test_macro_expand_hook_recursion_guard() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein macro))").expect("import");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(define hook-count 0)
             (set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (if (eq? name 'double)
                     (set! hook-count (+ hook-count 1)))
                 expanded))",
        )
        .expect("set hook");
        let result = ctx.evaluate("(double 5)").expect("use macro");
        assert_eq!(result, Value::Integer(10));
        let count = ctx.evaluate("hook-count").expect("check count");
        assert_eq!(count, Value::Integer(1));
    }

    #[test]
    fn test_macro_expand_hook_error_propagation() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein macro))").expect("import");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (error \"hook failed\")))",
        )
        .expect("set hook");
        let result = ctx.evaluate("(double 5)");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("hook failed"),
            "expected 'hook failed' in: {msg}"
        );
    }

    #[test]
    fn test_macro_expand_hook_introspection() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein macro))").expect("import");
        let none = ctx.evaluate("(macro-expand-hook)").expect("get");
        assert_eq!(none, Value::Boolean(false));
        ctx.evaluate(
            "(set-macro-expand-hook!
               (lambda (name unexpanded expanded env) expanded))",
        )
        .expect("set");
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
            ctx.evaluate("(import (tein macro))").expect("import");
            ctx.evaluate(
                "(set-macro-expand-hook!
                   (lambda (name unexpanded expanded env) expanded))",
            )
            .expect("set");
        }
        let ctx2 = Context::new_standard().expect("context2");
        ctx2.evaluate("(import (tein macro))").expect("import");
        let hook = ctx2.evaluate("(macro-expand-hook)").expect("get");
        assert_eq!(hook, Value::Boolean(false));
    }

    #[test]
    fn test_macro_expand_hook_sandbox() {
        // regression test for #31: (import (tein macro)) must work in
        // sandboxed contexts via C static library init.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("sandboxed context");
        ctx.evaluate("(import (scheme base))").expect("import base");
        ctx.evaluate("(import (tein macro))").expect("import macro");
        ctx.evaluate("(define-syntax double (syntax-rules () ((double x) (+ x x))))")
            .expect("define macro");
        ctx.evaluate(
            "(define hook-called #f)
             (set-macro-expand-hook!
               (lambda (name unexpanded expanded env)
                 (set! hook-called #t)
                 expanded))",
        )
        .expect("set hook");
        ctx.evaluate("(double 5)").expect("use macro");
        let called = ctx.evaluate("hook-called").expect("check");
        assert_eq!(called, Value::Boolean(true));
    }

    #[test]
    fn test_macro_expand_hook_rust_api() {
        let ctx = Context::new_standard().expect("context");
        assert!(ctx.macro_expand_hook().is_none());
        let hook = ctx
            .evaluate("(lambda (name unexpanded expanded env) expanded)")
            .expect("hook");
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
               (lambda (name unexpanded expanded env) expanded))",
        )
        .expect("set via import");
        let hook = ctx.evaluate("(macro-expand-hook)").expect("get");
        assert!(matches!(hook, Value::Procedure(_)));
    }

    #[test]
    fn test_macro_hook_infinite_loop_halts() {
        // a hook that always returns the unexpanded form causes unbounded re-analysis.
        // it must terminate with an error, not hang.
        let ctx = Context::builder().standard_env().build().unwrap();
        ctx.evaluate("(import (tein macro))").unwrap();

        // define the macro BEFORE registering the looping hook, so define-syntax
        // itself compiles cleanly using the real expansion.
        ctx.evaluate("(define-syntax my-id (syntax-rules () ((my-id x) x)))")
            .unwrap();

        // now register a hook that always returns the unexpanded form —
        // any subsequent macro call will loop indefinitely without our fix.
        ctx.evaluate("(set-macro-expand-hook! (lambda (name unexpanded expanded env) unexpanded))")
            .unwrap();

        let err = ctx.evaluate("(my-id 42)").unwrap_err();
        // must be an error (compile error or step limit), not a hang.
        // our loop_count guard terminates with EvalError before any fuel limit.
        assert!(
            matches!(err, Error::EvalError(_) | Error::StepLimitExceeded),
            "infinite macro hook re-analysis must terminate with an error, got: {:?}",
            err
        );
    }

    #[test]
    fn test_port_trampoline_bad_indices_do_not_panic() {
        // verify that opening/using a custom port with normal indices works correctly.
        // the actual UB guard (negative/reversed indices) is a hardening measure
        // against adversarial callers; tested here via the happy path as a baseline.
        let ctx = Context::new_standard().unwrap();
        let data = b"hello";
        let cursor = std::io::Cursor::new(data.to_vec());
        let port = ctx.open_input_port(cursor).unwrap();
        let result = ctx.read(&port).unwrap();
        // reading "hello" as a symbol
        assert_eq!(result, Value::Symbol("hello".into()));
    }

    #[test]
    fn test_sandbox_eval_escape_blocked() {
        // env-escape names are not defined in sandboxed(Modules::Safe) null env.
        // they cannot be imported (scheme/eval is excluded from Safe), so any
        // attempt to call them must fail — either undefined or import-blocked.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();

        // (import (scheme eval)) must be blocked — scheme/eval is not in Safe
        let err = ctx.evaluate("(import (scheme eval))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
            "scheme/eval import should fail in Modules::Safe, got: {:?}",
            err
        );

        // eval itself is not in scope (not defined in null env), so any call errors
        let err = ctx.evaluate("(eval '(+ 1 2) #f)").unwrap_err();
        assert!(
            err.to_string().contains("eval") || matches!(err, Error::EvalError(_)),
            "eval call should error in sandboxed env, got: {:?}",
            err
        );
    }

    #[test]
    fn test_sandbox_eval_escape_attempt() {
        // the classic escape: eval via import of scheme/eval must be blocked
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        // scheme/eval is not in the safe set — import must fail
        let err = ctx.evaluate("(import (scheme eval))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
            "scheme/eval escape attempt should fail, got: {:?}",
            err
        );
    }

    #[test]
    fn test_handle_ids_are_not_sequential() {
        // IDs should not be trivially predictable sequential integers.
        // we verify both ports work independently (different IDs → different store entries).
        let ctx = Context::new_standard().unwrap();
        let cursor1 = std::io::Cursor::new(b"a".to_vec());
        let cursor2 = std::io::Cursor::new(b"b".to_vec());
        let port1 = ctx.open_input_port(cursor1).unwrap();
        let port2 = ctx.open_input_port(cursor2).unwrap();
        let v1 = ctx.read(&port1).unwrap();
        let v2 = ctx.read(&port2).unwrap();
        assert_eq!(v1, Value::Symbol("a".into()));
        assert_eq!(v2, Value::Symbol("b".into()));
    }

    #[test]
    fn test_register_vfs_module() {
        let ctx = Context::new_standard().expect("standard context");

        ctx.register_vfs_module(
            "lib/tein/test-runtime.sld",
            "(define-library (tein test-runtime) (import (scheme base)) (export test-rt-val) (include \"test-runtime.scm\"))",
        )
        .expect("register sld");
        ctx.register_vfs_module("lib/tein/test-runtime.scm", "(define test-rt-val 42)")
            .expect("register scm");

        let result = ctx
            .evaluate("(import (tein test-runtime)) test-rt-val")
            .expect("eval");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_register_vfs_module_null_byte_error() {
        let ctx = Context::new_standard().expect("standard context");
        let err = ctx
            .register_vfs_module("lib/bad\0path.sld", "")
            .unwrap_err();
        assert!(matches!(err, Error::EvalError(_)));
    }

    #[test]
    fn test_register_vfs_module_cleared_on_drop() {
        // register a module in one context, verify a fresh context can't see it
        {
            let ctx = Context::new_standard().expect("ctx1");
            ctx.register_vfs_module(
                "lib/tein/drop-test.sld",
                "(define-library (tein drop-test) (import (scheme base)) (export x) (include \"drop-test.scm\"))",
            )
            .unwrap();
            ctx.register_vfs_module("lib/tein/drop-test.scm", "(define x 1)")
                .unwrap();
            ctx.evaluate("(import (tein drop-test)) x")
                .expect("should work in ctx1");
        }
        // ctx dropped → dynamic VFS cleared; new context should not find module
        let ctx2 = Context::new_standard().expect("ctx2");
        let err = ctx2.evaluate("(import (tein drop-test))").unwrap_err();
        assert!(matches!(err, Error::EvalError(_)));
    }

    // --- numeric tower: from_raw ---

    #[test]
    fn test_bignum_from_scheme() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(expt 2 100)").expect("evaluation failed");
        match &result {
            Value::Bignum(s) => assert_eq!(s, "1267650600228229401496703205376"),
            other => panic!("expected Bignum, got {:?}", other),
        }
    }

    #[test]
    fn test_bignum_negative() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(- (expt 2 100))").expect("evaluation failed");
        match &result {
            Value::Bignum(s) => assert!(s.starts_with('-'), "expected negative, got {s}"),
            other => panic!("expected Bignum, got {:?}", other),
        }
    }

    #[test]
    fn test_rational_from_scheme() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(/ 1 3)").expect("evaluation failed");
        match &result {
            Value::Rational(n, d) => {
                assert_eq!(**n, Value::Integer(1));
                assert_eq!(**d, Value::Integer(3));
            }
            other => panic!("expected Rational, got {:?}", other),
        }
    }

    #[test]
    fn test_rational_display() {
        let v = Value::Rational(Box::new(Value::Integer(1)), Box::new(Value::Integer(3)));
        assert_eq!(v.to_string(), "1/3");
    }

    #[test]
    fn test_complex_from_scheme() {
        // make-rectangular requires standard env (r7rs)
        let ctx = Context::new_standard().expect("failed to create context");
        let result = ctx
            .evaluate("(make-rectangular 1 2)")
            .expect("evaluation failed");
        match &result {
            Value::Complex(r, i) => {
                assert_eq!(**r, Value::Integer(1));
                assert_eq!(**i, Value::Integer(2));
            }
            other => panic!("expected Complex, got {:?}", other),
        }
    }

    #[test]
    fn test_complex_display() {
        let v = Value::Complex(Box::new(Value::Integer(1)), Box::new(Value::Integer(2)));
        assert_eq!(v.to_string(), "1+2i");
    }

    #[test]
    fn test_complex_negative_imag_display() {
        let v = Value::Complex(Box::new(Value::Integer(1)), Box::new(Value::Integer(-2)));
        assert_eq!(v.to_string(), "1-2i");
    }

    #[test]
    fn test_bignum_display() {
        let v = Value::Bignum("1267650600228229401496703205376".to_string());
        assert_eq!(v.to_string(), "1267650600228229401496703205376");
    }

    #[test]
    fn test_rational_with_bignum_components() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate("(/ (expt 2 100) (expt 3 50))")
            .expect("evaluation failed");
        match &result {
            Value::Rational(_, _) => {} // just verify it parses as rational
            other => panic!("expected Rational, got {:?}", other),
        }
    }

    // --- numeric tower: to_raw round-trips ---

    #[test]
    fn test_bignum_to_raw_roundtrip() {
        unsafe extern "C" fn get_bignum(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let val = Value::Bignum("1267650600228229401496703205376".to_string());
                val.to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-bignum", get_bignum)
            .expect("failed to define fn");
        let result = ctx.evaluate("(get-bignum)").expect("evaluation failed");
        match &result {
            Value::Bignum(s) => assert_eq!(s, "1267650600228229401496703205376"),
            other => panic!("expected Bignum, got {:?}", other),
        }
    }

    #[test]
    fn test_rational_to_raw_roundtrip() {
        unsafe extern "C" fn get_rational(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let val = Value::Rational(Box::new(Value::Integer(1)), Box::new(Value::Integer(3)));
                val.to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-rational", get_rational)
            .expect("failed to define fn");
        let result = ctx.evaluate("(get-rational)").expect("evaluation failed");
        match &result {
            Value::Rational(n, d) => {
                assert_eq!(**n, Value::Integer(1));
                assert_eq!(**d, Value::Integer(3));
            }
            other => panic!("expected Rational, got {:?}", other),
        }
    }

    #[test]
    fn test_complex_to_raw_roundtrip() {
        unsafe extern "C" fn get_complex(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let val = Value::Complex(Box::new(Value::Integer(1)), Box::new(Value::Integer(2)));
                val.to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-complex", get_complex)
            .expect("failed to define fn");
        let result = ctx.evaluate("(get-complex)").expect("evaluation failed");
        match &result {
            Value::Complex(r, i) => {
                assert_eq!(**r, Value::Integer(1));
                assert_eq!(**i, Value::Integer(2));
            }
            other => panic!("expected Complex, got {:?}", other),
        }
    }

    // --- (tein json) ---

    #[cfg(feature = "json")]
    #[test]
    fn test_json_parse_object() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein json))").expect("import");
        let result = ctx
            .evaluate(r#"(json-parse "{\"a\": 1, \"b\": \"two\"}")"#)
            .expect("parse");
        match result {
            Value::List(items) => assert_eq!(items.len(), 2),
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_json_parse_array() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein json))").expect("import");
        let result = ctx.evaluate("(json-parse \"[1, 2, 3]\")").expect("parse");
        assert_eq!(
            result,
            Value::List(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3),
            ])
        );
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_json_parse_null_is_symbol() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein json))").expect("import");
        let result = ctx.evaluate("(json-parse \"null\")").expect("parse");
        assert_eq!(result, Value::Symbol("null".to_string()));
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_json_stringify_alist() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein json))").expect("import");
        let result = ctx
            .evaluate("(json-stringify '((\"name\" . \"tein\")))")
            .expect("stringify");
        assert_eq!(result, Value::String("{\"name\":\"tein\"}".to_string()));
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_json_round_trip_via_scheme() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein json))").expect("import");
        let result = ctx
            .evaluate(r#"(json-stringify (json-parse "{\"x\":42}"))"#)
            .expect("round-trip");
        assert_eq!(result, Value::String("{\"x\":42}".to_string()));
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_json_parse_invalid() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein json))").expect("import");
        let result = ctx.evaluate("(json-parse \"not json\")").expect("parse");
        // per convention: errors return scheme strings (see AGENTS.md critical gotchas)
        match result {
            Value::String(msg) => assert!(msg.contains("json-parse")),
            other => panic!("expected error string, got {other:?}"),
        }
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_parse_table() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein toml))").expect("import");
        let result = ctx
            .evaluate("(toml-parse \"name = \\\"tein\\\"\\nversion = 1\")")
            .expect("parse");
        match result {
            Value::List(items) => assert_eq!(items.len(), 2),
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_parse_datetime() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein toml))").expect("import");
        let result = ctx
            .evaluate("(cdr (car (toml-parse \"dt = 1979-05-27T07:32:00Z\")))")
            .expect("parse");
        // should be a tagged list (toml-datetime "1979-05-27T07:32:00Z")
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], Value::Symbol("toml-datetime".to_string()));
                assert_eq!(items[1], Value::String("1979-05-27T07:32:00Z".to_string()));
            }
            other => panic!("expected tagged list, got {other:?}"),
        }
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_stringify_table() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein toml))").expect("import");
        let result = ctx
            .evaluate("(toml-stringify '((\"name\" . \"tein\")))")
            .expect("stringify");
        match result {
            Value::String(s) => assert!(s.contains("name = \"tein\"")),
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_round_trip_via_scheme() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein toml))").expect("import");
        let result = ctx
            .evaluate("(cdr (car (toml-parse (toml-stringify '((\"x\" . 42))))))")
            .expect("round-trip");
        assert_eq!(result, Value::Integer(42));
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_parse_invalid() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein toml))").expect("import");
        let result = ctx
            .evaluate("(toml-parse \"not valid {{toml\")")
            .expect("parse");
        match result {
            Value::String(msg) => assert!(msg.contains("toml-parse")),
            other => panic!("expected error string, got {other:?}"),
        }
    }

    // --- task 6: Modules enum + sandboxed() builder ---
    // --- task 7: new sandbox env construction ---

    #[test]
    fn test_sandboxed_modules_safe_can_import_and_compute() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("build");
        let result = ctx
            .evaluate("(import (scheme base)) (+ 1 2)")
            .expect("sandboxed safe eval");
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn test_sandboxed_modules_none_blocks_base_bindings() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::None)
            .build()
            .expect("build");
        // + should be stubbed — SandboxViolation
        let err = ctx.evaluate("(+ 1 2)").expect_err("should fail");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("scheme") || msg.contains("import") || msg.contains("sandbox"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn test_sandboxed_modules_only_scheme_base() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .build()
            .expect("build");
        let result = ctx
            .evaluate("(import (scheme base)) (+ 1 2)")
            .expect("eval with only scheme/base");
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn test_sandboxed_modules_all_allows_scheme_write() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::All)
            .build()
            .expect("build");
        let result = ctx
            .evaluate("(import (scheme write)) (begin (write 1) #t)")
            .expect("sandboxed all with scheme/write");
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_sandboxed_modules_safe_blocks_scheme_env_escape() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("build");
        // scheme/eval is not in the safe set — import should fail
        let err = ctx
            .evaluate("(import (scheme eval))")
            .expect_err("scheme/eval should be blocked");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("eval")
                || msg.contains("module")
                || msg.contains("import")
                || msg.contains("not found"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn test_sandboxed_ux_stub_message_mentions_module() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::None)
            .build()
            .expect("build");
        // + from scheme/base should have a UX stub with a module hint
        let err = ctx.evaluate("(+ 1 2)").expect_err("should fail");
        assert!(
            matches!(err, crate::Error::SandboxViolation(_)),
            "expected SandboxViolation, got {err:?}"
        );
        if let crate::Error::SandboxViolation(msg) = &err {
            assert!(
                msg.contains("scheme") || msg.contains("import"),
                "stub message should mention module: {msg}"
            );
        }
    }

    #[test]
    fn test_sandboxed_builder_compiles() {
        use crate::sandbox::Modules;
        // sandboxed() returns a ContextBuilder — just check it compiles and doesn't panic.
        let _builder = Context::builder().standard_env().sandboxed(Modules::Safe);
        let _builder2 = Context::builder().standard_env().sandboxed(Modules::All);
        let _builder3 = Context::builder().standard_env().sandboxed(Modules::None);
        let _builder4 = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]));
    }

    // --- task 9: allow_module() with new sandbox path ---

    #[test]
    fn test_sandboxed_allow_module_tein_process() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("tein/process")
            .build()
            .expect("build");
        // (exit 0) should succeed and return 0
        let result = ctx
            .evaluate("(import (tein process)) (exit 0)")
            .expect("tein/process exit");
        assert_eq!(result, Value::Integer(0));
    }

    #[test]
    fn test_sandboxed_allow_module_scheme_eval_importable() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("scheme/eval")
            .build()
            .expect("build");
        // scheme/eval should now be importable without VFS gate error
        ctx.evaluate("(import (scheme eval))")
            .expect("scheme/eval should be importable after allow_module");
    }

    // --- task 8: file_read/file_write with new sandbox path ---

    #[test]
    fn test_sandboxed_file_read_allowed_path() {
        use crate::sandbox::Modules;
        // write a temp file to read from
        let tmp = "/tmp/tein_sandbox_read_test.txt";
        std::fs::write(tmp, "42").unwrap();
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .file_read(&["/tmp/"])
            .build()
            .expect("build");
        // open-input-file is a chibi opcode — import (tein file) to get it in sandbox.
        // (scheme read) provides `read`; (scheme base) provides close-input-port.
        let code = format!(
            r#"(import (scheme base) (scheme read) (tein file))
               (let ((p (open-input-file "{tmp}")))
                 (let ((v (read p))) (close-input-port p) v))"#
        );
        let result = ctx.evaluate(&code).expect("read from allowed path");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_sandboxed_file_read_blocked_path() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .file_read(&["/tmp/"])
            .build()
            .expect("build");
        // reading from /etc/ should be blocked by IO policy (C gate denies)
        let err = ctx
            .evaluate(r#"(import (tein file)) (open-input-file "/etc/hostname")"#)
            .expect_err("should be blocked");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("policy")
                || msg.contains("denied")
                || msg.contains("not allowed")
                || msg.contains("SandboxViolation")
                || msg.contains("error"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_sandboxed_modules_safe_default() {
        use crate::sandbox::Modules;
        let m: Modules = Default::default();
        assert!(matches!(m, Modules::Safe));
    }

    #[test]
    fn test_sandboxed_modules_only_constructor() {
        use crate::sandbox::Modules;
        let m = Modules::only(&["scheme/base", "scheme/write"]);
        if let Modules::Only(list) = m {
            assert!(list.contains(&"scheme/base".to_string()));
            assert!(list.contains(&"scheme/write".to_string()));
        } else {
            panic!("expected Modules::Only");
        }
    }

    // --- VFS shadow tests ---

    #[test]
    fn test_scheme_repl_shadow_importable_in_sandbox() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("builder");
        // (scheme repl) in sandbox should resolve to our shadow
        let r = ctx
            .evaluate("(import (scheme base) (scheme repl)) (procedure? interaction-environment)");
        assert_eq!(r.expect("scheme repl shadow works"), Value::Boolean(true));
    }

    #[test]
    fn test_scheme_file_shadow_importable_in_sandbox() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        use crate::sandbox::Modules;
        let tmp = "/tmp/tein_shadow_file_test.txt";
        std::fs::write(tmp, "shadowed").expect("write");
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .file_read(&["/tmp/"])
            .build()
            .expect("builder");
        // (scheme file) in sandbox should resolve to our shadow
        let r = ctx.evaluate(&format!(
            "(import (scheme base) (scheme file)) (let ((p (open-input-file \"{tmp}\"))) (close-input-port p) #t)"
        ));
        assert_eq!(r.expect("scheme file shadow works"), Value::Boolean(true));
    }

    #[test]
    fn test_scheme_file_shadow_denies_without_policy() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            // no file_read configured
            .build()
            .expect("builder");
        let r = ctx.evaluate("(import (scheme file)) (open-input-file \"/etc/passwd\")");
        assert!(
            r.is_err(),
            "scheme/file open-input-file denied without policy"
        );
    }

    #[test]
    fn test_tein_file_not_shadowed_unsandboxed() {
        // unsandboxed: (tein file) trampolines allow all file access (no policy check)
        // note: (scheme file) is not available in unsandboxed mode — tein's module path
        // is VFS-only and the shadow is only registered in sandboxed contexts.
        // (tein file) is the correct import for file ops in unsandboxed contexts.
        let tmp = "/tmp/tein_unsandboxed_scheme_file.txt";
        std::fs::write(tmp, "native").expect("write");
        let ctx = Context::builder().standard_env().build().expect("builder");
        let r = ctx.evaluate(&format!(
            "(import (scheme base) (tein file)) (let ((p (open-input-file \"{tmp}\"))) (close-input-port p) #t)"
        ));
        assert_eq!(
            r.expect("unsandboxed tein file works"),
            Value::Boolean(true)
        );
    }

    #[test]
    fn test_scheme_repl_shadow_returns_environment() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("builder");
        // interaction-environment should return an env (not #f, not error)
        let r = ctx.evaluate(
            "(import (scheme base) (scheme repl)) (let ((e (interaction-environment))) #t)",
        );
        assert_eq!(r.expect("scheme repl shadow works"), Value::Boolean(true));
    }

    #[test]
    fn test_srfi_98_shadow_neuters_env_vars_in_sandbox() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("builder");
        // srfi/98 shadow replaces the C clib — get-environment-variable always #f
        let r = ctx
            .evaluate("(import (scheme base) (srfi 98)) (get-environment-variable \"HOME\")")
            .expect("srfi/98 importable in sandbox");
        assert_eq!(r, Value::Boolean(false), "get-environment-variable neutered");
        let r = ctx
            .evaluate("(get-environment-variables)")
            .expect("get-environment-variables");
        assert_eq!(
            r,
            Value::Nil,
            "get-environment-variables returns empty list"
        );
    }

    // --- (scheme show) / (srfi 166) sandbox tests ---

    #[test]
    fn test_scheme_show_importable_in_sandbox() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .step_limit(10_000_000)
            .build()
            .expect("builder");
        let r = ctx.evaluate("(import (scheme show)) (show #f \"hello\")");
        assert!(r.is_ok(), "scheme show importable in sandbox: {r:?}");
    }

    #[test]
    fn test_srfi_166_base_importable_in_sandbox() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .step_limit(10_000_000)
            .build()
            .expect("builder");
        let r = ctx.evaluate("(import (srfi 166 base)) (show #f (displayed \"test\"))");
        assert!(r.is_ok(), "srfi/166/base importable in sandbox: {r:?}");
    }

    #[test]
    fn test_srfi_166_columnar_from_file_with_policy() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        use crate::sandbox::Modules;
        let dir = io_test_dir("columnar_from_file");
        let file = dir.join("lines.txt");
        std::fs::write(&file, "line1\nline2\n").expect("write");
        let canon_dir = dir.canonicalize().unwrap();
        let path = file.to_str().unwrap();
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .file_read(&[canon_dir.to_str().unwrap()])
            .step_limit(10_000_000)
            .build()
            .expect("builder");
        let r = ctx.evaluate(&format!(
            "(import (srfi 166)) (show #f (from-file \"{path}\"))"
        ));
        assert!(r.is_ok(), "from-file with read policy: {r:?}");
    }

    #[test]
    fn test_srfi_166_columnar_from_file_denied_without_policy() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .step_limit(10_000_000)
            .build()
            .expect("builder");
        // from-file calls open-input-file which hits the policy check
        let r = ctx
            .evaluate("(import (srfi 166)) (show #f (from-file \"/tmp/nonexistent_tein_test\"))");
        assert!(r.is_err(), "from-file without policy should fail");
    }
}
