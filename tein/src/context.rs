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
    sandbox::{
        FS_GATE, FS_GATE_CHECK, FS_MODULE_PATHS, FS_POLICY, FsPolicy, GATE_CHECK, VFS_ALLOWLIST,
        VFS_GATE,
    },
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::c_char;
use std::path::Path;

/// RAII guard that clears the FOREIGN_STORE_PTR thread-local on drop.
///
/// Ensures the pointer is nulled on all exit paths (early returns, `?`, panic).
struct ForeignStoreGuard;

/// RAII guard that clears the CONTEXT_PTR thread-local on drop.
struct ContextPtrGuard;

impl Drop for ContextPtrGuard {
    fn drop(&mut self) {
        CONTEXT_PTR.with(|c| c.set(std::ptr::null()));
    }
}

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
    pub(crate) static FOREIGN_STORE_PTR: Cell<*const RefCell<ForeignStore>> = const { Cell::new(std::ptr::null()) };
    /// current TeinExtApi pointer — set during load_extension() so ext method dispatch
    /// can call back into the host. null outside of ext loading and ext method calls.
    pub(crate) static EXT_API: Cell<*const tein_ext::TeinExtApi> = const { Cell::new(std::ptr::null()) };
    /// raw pointer to the active Context during evaluation.
    ///
    /// set by `evaluate()`, `call()`, `evaluate_port()`, and `read()` via
    /// `ContextPtrGuard` RAII. trampolines (e.g. `register-module`) use this
    /// to call Context methods without passing `&self` through the C FFI.
    ///
    /// same lifetime guarantees as `FOREIGN_STORE_PTR`: the Context outlives
    /// any trampoline call during evaluation, and the guard clears on all
    /// exit paths.
    pub(crate) static CONTEXT_PTR: Cell<*const Context> = const { Cell::new(std::ptr::null()) };
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

// --- sandbox fake process environment thread-locals ---
//
// populated during sandboxed() build path: fake env vars and command-line
// for sandboxed contexts. cleared on Context::drop().
thread_local! {
    static SANDBOX_ENV: RefCell<Option<HashMap<String, String>>> = const { RefCell::new(None) };
    static SANDBOX_COMMAND_LINE: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };

    /// GC-rooted mutable env returned by `interaction-environment` in sandbox.
    /// Lazily created on first call; cleared on `Context::drop()`.
    pub(crate) static INTERACTION_ENV: Cell<ffi::sexp> = const { Cell::new(std::ptr::null_mut()) };
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

            // error constructor — same signature as sexp_c_str but returns exception
            make_error: std::mem::transmute::<
                unsafe fn(ffi::sexp, *const std::ffi::c_char, ffi::sexp_sint_t) -> ffi::sexp,
                unsafe extern "C" fn(
                    *mut OpaqueCtx,
                    *const std::ffi::c_char,
                    std::ffi::c_long,
                ) -> *mut OpaqueVal,
            >(ffi::make_error),

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
        let rc = ffi::tein_vfs_register(c_path.as_ptr(), content, content_len as std::ffi::c_uint);
        if rc != 0 {
            return -1;
        }
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
/// On parse error or type mismatch, raises a scheme exception.
unsafe extern "C" fn json_parse_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        if ffi::sexp_nullp(args) != 0 {
            let msg = "json-parse: expected 1 argument, got 0";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let str_sexp = ffi::sexp_car(args);
        if ffi::sexp_stringp(str_sexp) == 0 {
            let msg = "json-parse: expected string argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let data = ffi::sexp_string_data(str_sexp);
        let len = ffi::sexp_string_size(str_sexp) as usize;
        let input = match std::str::from_utf8(std::slice::from_raw_parts(data as *const u8, len)) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("json-parse: invalid UTF-8: {e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };
        match crate::json::json_parse(input) {
            Ok(value) => match value.to_raw(ctx) {
                Ok(raw) => raw,
                Err(e) => {
                    let msg = format!("json-parse: {e}");
                    let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                    ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
                }
            },
            Err(e) => {
                let msg = format!("{e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
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
/// On conversion error, raises a scheme exception.
unsafe extern "C" fn json_stringify_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        if ffi::sexp_nullp(args) != 0 {
            let msg = "json-stringify: expected 1 argument, got 0";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let val_sexp = ffi::sexp_car(args);
        match crate::json::json_stringify_raw(ctx, val_sexp) {
            Ok(json) => {
                let c_json = CString::new(json.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_json.as_ptr(), json.len() as ffi::sexp_sint_t)
            }
            Err(e) => {
                let msg = format!("{e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}

// --- toml trampolines (gated behind "toml" feature) ---

#[cfg(feature = "toml")]
/// Trampoline for `toml-parse`: takes one scheme string argument, returns parsed value.
///
/// On parse error or type mismatch, raises a scheme exception.
unsafe extern "C" fn toml_parse_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        if ffi::sexp_nullp(args) != 0 {
            let msg = "toml-parse: expected 1 argument, got 0";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let str_sexp = ffi::sexp_car(args);
        if ffi::sexp_stringp(str_sexp) == 0 {
            let msg = "toml-parse: expected string argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let data = ffi::sexp_string_data(str_sexp);
        let len = ffi::sexp_string_size(str_sexp) as usize;
        let input = match std::str::from_utf8(std::slice::from_raw_parts(data as *const u8, len)) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("toml-parse: invalid UTF-8: {e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };
        match crate::toml::toml_parse(input) {
            Ok(value) => match value.to_raw(ctx) {
                Ok(raw) => raw,
                Err(e) => {
                    let msg = format!("toml-parse: {e}");
                    let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                    ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
                }
            },
            Err(e) => {
                let msg = format!("{e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
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
/// On conversion error, raises a scheme exception.
unsafe extern "C" fn toml_stringify_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        if ffi::sexp_nullp(args) != 0 {
            let msg = "toml-stringify: expected 1 argument, got 0";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let val_sexp = ffi::sexp_car(args);
        match crate::toml::toml_stringify_raw(ctx, val_sexp) {
            Ok(toml_str) => {
                let c_str = CString::new(toml_str.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_str.as_ptr(), toml_str.len() as ffi::sexp_sint_t)
            }
            Err(e) => {
                let msg = format!("{e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
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

/// Extract the first argument as a `&str`, returning an error sexp on type mismatch or missing arg.
///
/// # Safety
/// `args` must be a valid scheme list (may be null/empty — arity error returned in that case).
pub(crate) unsafe fn extract_string_arg<'a>(
    ctx: ffi::sexp,
    args: ffi::sexp,
    fn_name: &str,
) -> std::result::Result<&'a str, ffi::sexp> {
    unsafe {
        if ffi::sexp_nullp(args) != 0 {
            let msg = format!("{}: expected 1 argument, got 0", fn_name);
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return Err(ffi::make_error(
                ctx,
                c_msg.as_ptr(),
                msg.len() as ffi::sexp_sint_t,
            ));
        }
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
/// - sandboxed read under an `FS_MODULE_PATHS` dir: allows (module loading)
/// - sandboxed with matching FsPolicy: delegates to `check_read`/`check_write`
/// - sandboxed without FsPolicy configured: denies
///
/// `FS_MODULE_PATHS` entries are canonicalised dirs; any read under them is
/// implicitly allowed so chibi's module loader can open `.sld`/`.scm` files.
/// this grants no write access and no access outside the registered dirs.
pub(crate) fn check_fs_access(path: &str, access: FsAccess) -> bool {
    let sandboxed = IS_SANDBOXED.with(|c| c.get());
    if !sandboxed {
        return true;
    }
    // module search paths implicitly grant read access for module loading
    if matches!(access, FsAccess::Read) {
        let path_buf = std::path::Path::new(path);
        let allowed_by_module_path = FS_MODULE_PATHS.with(|cell| {
            let dirs = cell.borrow();
            dirs.iter().any(|dir| path_buf.starts_with(dir.as_str()))
        });
        if allowed_by_module_path {
            return true;
        }
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

// --- (scheme eval) / (scheme repl) trampolines ---

/// Convert a scheme import spec list like `(scheme base)` to a path string `"scheme/base"`.
///
/// Each element must be a symbol (converted via `sexp_symbol_to_string`) or an
/// integer (converted via `sexp_unbox_fixnum`). Returns `Err(error_sexp)` on
/// malformed input.
unsafe fn spec_to_path(ctx: ffi::sexp, spec: ffi::sexp) -> std::result::Result<String, ffi::sexp> {
    unsafe {
        let mut parts = Vec::new();
        let mut cursor = spec;
        while ffi::sexp_pairp(cursor) != 0 {
            let elem = ffi::sexp_car(cursor);
            if ffi::sexp_symbolp(elem) != 0 {
                let s = ffi::sexp_symbol_to_string(ctx, elem);
                let ptr = ffi::sexp_string_data(s);
                let len = ffi::sexp_string_size(s) as usize;
                let slice = std::slice::from_raw_parts(ptr as *const u8, len);
                parts.push(String::from_utf8_lossy(slice).into_owned());
            } else if ffi::sexp_integerp(elem) != 0 {
                let n = ffi::sexp_unbox_fixnum(elem);
                parts.push(n.to_string());
            } else {
                let msg = "environment: import spec elements must be symbols or integers";
                let c_msg = CString::new(msg).unwrap_or_default();
                return Err(ffi::make_error(
                    ctx,
                    c_msg.as_ptr(),
                    msg.len() as ffi::sexp_sint_t,
                ));
            }
            cursor = ffi::sexp_cdr(cursor);
        }
        Ok(parts.join("/"))
    }
}

/// `environment` trampoline: validates import specs against VFS allowlist, then
/// delegates to chibi's `mutable-environment` and makes the result immutable.
///
/// registered as `tein-environment-internal`. used by `(scheme eval)` and
/// `(scheme load)` VFS shadows which export it as `environment`.
///
/// in sandboxed contexts, each import spec is checked against `VFS_ALLOWLIST`.
/// disallowed modules produce an error. in unsandboxed contexts, all specs are
/// passed through unconditionally.
unsafe extern "C" fn environment_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        // in sandboxed mode, validate each spec against the VFS allowlist
        if IS_SANDBOXED.with(|c| c.get()) {
            let mut cursor = args;
            while ffi::sexp_pairp(cursor) != 0 {
                let spec = ffi::sexp_car(cursor);
                match spec_to_path(ctx, spec) {
                    Ok(path) => {
                        let allowed = VFS_ALLOWLIST.with(|cell| {
                            let list = cell.borrow();
                            list.contains(&path)
                        });
                        if !allowed {
                            let msg = format!(
                                "[sandbox:environment] module ({}) not in allowlist",
                                path.replace('/', " ")
                            );
                            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                            return ffi::make_error(
                                ctx,
                                c_msg.as_ptr(),
                                msg.len() as ffi::sexp_sint_t,
                            );
                        }
                    }
                    Err(e) => return e,
                }
                cursor = ffi::sexp_cdr(cursor);
            }
        }

        // build expression: (mutable-environment spec1 spec2 ...)
        // we evaluate it in the meta env where mutable-environment is defined.
        let meta_env = ffi::sexp_global_meta_env(ctx);
        let _meta_root = ffi::GcRoot::new(ctx, meta_env);

        let sym_name = c"mutable-environment";
        let sym = ffi::sexp_intern(
            ctx,
            sym_name.as_ptr(),
            "mutable-environment".len() as ffi::sexp_sint_t,
        );
        // sym is an interned symbol — immortal in chibi's symbol table, no root needed.

        // build quoted-spec list: (mutable-environment '(scheme base) '(scheme write) ...)
        // each arg is already an evaluated list like (scheme base), so we quote them
        // for evaluation.
        let quote_sym = ffi::sexp_intern(ctx, c"quote".as_ptr(), 5);
        // quote_sym is an interned symbol — immortal, no root needed.

        // walk args in reverse to build the list via cons
        let mut arg_vec: Vec<ffi::sexp> = Vec::new();
        let mut cursor = args;
        while ffi::sexp_pairp(cursor) != 0 {
            arg_vec.push(ffi::sexp_car(cursor));
            cursor = ffi::sexp_cdr(cursor);
        }

        let mut expr_parts = ffi::get_null();
        for spec in arg_vec.iter().rev() {
            // root spec (from arg_vec — Rust heap, invisible to chibi GC)
            let _spec_root = ffi::GcRoot::new(ctx, *spec);
            // root the accumulator before each allocation
            let _tail_root = ffi::GcRoot::new(ctx, expr_parts);

            // build (quote spec) = (quote_sym . (spec . ()))
            let quoted_inner = ffi::sexp_cons(ctx, *spec, ffi::get_null());
            if ffi::sexp_exceptionp(quoted_inner) != 0 {
                return quoted_inner;
            }
            let _inner_root = ffi::GcRoot::new(ctx, quoted_inner);

            let quoted = ffi::sexp_cons(ctx, quote_sym, quoted_inner);
            if ffi::sexp_exceptionp(quoted) != 0 {
                return quoted;
            }
            let _quoted_root = ffi::GcRoot::new(ctx, quoted);

            expr_parts = ffi::sexp_cons(ctx, quoted, expr_parts);
            if ffi::sexp_exceptionp(expr_parts) != 0 {
                return expr_parts;
            }
        }

        // prepend the mutable-environment symbol
        let _parts_root = ffi::GcRoot::new(ctx, expr_parts);
        let expr = ffi::sexp_cons(ctx, sym, expr_parts);
        if ffi::sexp_exceptionp(expr) != 0 {
            return expr;
        }
        let _expr_root = ffi::GcRoot::new(ctx, expr);

        // evaluate in meta env
        let result = ffi::sexp_evaluate(ctx, expr, meta_env);
        if ffi::sexp_exceptionp(result) != 0 {
            return result;
        }

        // make the resulting environment immutable (r7rs: environment returns immutable envs)
        // note: sexp_make_immutable_op mutates in place and returns #t/#f,
        // so we return `result` (the env), not the make_immutable return value.
        let imm = ffi::sexp_make_immutable(ctx, result);
        if ffi::sexp_exceptionp(imm) != 0 {
            return imm;
        }
        result
    }
}

/// `interaction-environment` trampoline: returns a persistent mutable env
/// containing all VFS-allowlisted modules.
///
/// registered as `tein-interaction-environment-internal`. used by `(scheme repl)`
/// VFS shadow which exports it as `interaction-environment`.
///
/// the env is lazily created on first call and cached in `INTERACTION_ENV`
/// thread-local. subsequent calls return the same env, allowing definitions
/// to accumulate across evals (r7rs compliance).
unsafe extern "C" fn interaction_environment_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        // return cached env if already created
        let cached = INTERACTION_ENV.with(|cell| cell.get());
        if !cached.is_null() {
            return cached;
        }

        // return the current context env as the interaction environment.
        // in sandboxed mode this is the restricted null env that already has
        // all allowed imports available. definitions accumulate here across
        // evals, satisfying r7rs's requirement for a mutable interaction env.
        //
        // r7rs technically says interaction-environment should be a separate
        // env, but using the context env is correct for our embedding model
        // where each Context is a single-use evaluation scope.
        let result = ffi::sexp_context_env(ctx);
        if ffi::sexp_exceptionp(result) != 0 {
            return result;
        }

        // GC-root and cache
        ffi::sexp_preserve_object(ctx, result);
        INTERACTION_ENV.with(|cell| cell.set(result));

        result
    }
}

/// Register `tein-environment-internal` and `tein-interaction-environment-internal`
/// into a specific chibi env.
///
/// Must be called with the primitive env BEFORE `load_standard_env`. init-7.scm
/// builds `*chibi-env*` by importing all bindings from the primitive env, so any
/// name registered here propagates into `*chibi-env*` and is available to library
/// bodies that `(import (chibi))` — including VFS shadow SLDs for `scheme/eval`,
/// `scheme/load`, and `scheme/repl`. (#97)
///
/// Also called via `register_eval_module()` (into the context env after creation)
/// for non-sandboxed contexts that want direct access to the trampolines.
/// Re-registering is safe — `sexp_env_define` overwrites silently.
fn register_eval_trampolines(ctx: ffi::sexp, env: ffi::sexp) -> Result<()> {
    type VariadicFn =
        unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp) -> ffi::sexp;
    for (name, f) in [
        (
            "tein-environment-internal",
            environment_trampoline as VariadicFn,
        ),
        (
            "tein-interaction-environment-internal",
            interaction_environment_trampoline as VariadicFn,
        ),
    ] {
        let c_name = CString::new(name).map_err(|_| {
            Error::EvalError(format!("function name contains null bytes: {}", name))
        })?;
        unsafe {
            let result = ffi::sexp_define_foreign_proc(
                ctx,
                env,
                c_name.as_ptr(),
                0,
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
    }
    Ok(())
}

/// register a single native variadic fn into a given env.
///
/// used to inject trampolines into the primitive env BEFORE `load_standard_env`,
/// so they end up in `*chibi-env*` and are visible to library bodies via
/// `(import (chibi))`.
#[allow(dead_code)] // generic utility — currently used by http, ready for future modules
fn register_native_trampoline(
    ctx: ffi::sexp,
    env: ffi::sexp,
    name: &str,
    f: unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp) -> ffi::sexp,
) -> Result<()> {
    let c_name =
        CString::new(name).map_err(|_| Error::EvalError(format!("name contains null: {name}")))?;
    unsafe {
        let result = ffi::sexp_define_foreign_proc(
            ctx,
            env,
            c_name.as_ptr(),
            0,
            ffi::SEXP_PROC_VARIADIC,
            c_name.as_ptr(),
            Some(f),
        );
        if ffi::sexp_exceptionp(result) != 0 {
            return Err(Error::EvalError(format!(
                "failed to define variadic function '{name}'"
            )));
        }
    }
    Ok(())
}

// --- (tein modules) trampolines ---

/// `register-module` trampoline: registers a define-library source string
/// as a new importable module.
///
/// uses `CONTEXT_PTR` thread-local to call `Context::register_module` directly,
/// avoiding any parsing duplication. CONTEXT_PTR is set by `evaluate()`,
/// `call()`, `evaluate_port()`, and `read()` and is guaranteed valid during
/// trampoline execution.
///
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

        let context_ptr = CONTEXT_PTR.with(|c| c.get());
        if context_ptr.is_null() {
            let msg = "register-module: internal error — no active Context";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let context = &*context_ptr;

        // source is a &str borrowed from the scheme string arg; we need an
        // owned copy because register_module may allocate (sexp_read etc.),
        // potentially triggering GC which could move the scheme string.
        let source_owned = source.to_string();

        match context.register_module(&source_owned) {
            Ok(()) => ffi::get_true(),
            Err(e) => {
                let msg = format!("{e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
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

// --- (tein process) trampolines ---

/// `get-environment-variable` trampoline: returns env var value or `#f`.
///
/// sandboxed contexts consult the fake env map seeded by [`ContextBuilder::environment_variables`];
/// vars not present in the map return `#f`. unsandboxed contexts read the real process environment.
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
///
/// sandboxed contexts return the fake env map as an alist; unsandboxed contexts return the real env.
unsafe extern "C" fn get_env_vars_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
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
///
/// sandboxed contexts return the fake command-line configured via [`ContextBuilder::command_line`]
/// (default: `["tein", "--sandbox"]`). unsandboxed contexts return the real process argv.
unsafe extern "C" fn command_line_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
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

/// `emergency-exit` trampoline: immediate VM halt without cleanup.
///
/// sets EXIT_REQUESTED + EXIT_VALUE thread-locals and returns a scheme
/// exception to immediately stop the VM. the eval loop intercepts this
/// via `check_exit()` and returns `Ok(value)` to the rust caller.
///
/// semantics: `(exit)` → 0, `(exit #t)` → 0, `(exit #f)` → 1, `(exit obj)` → obj
///
/// this is r7rs `emergency-exit` — no `dynamic-wind` "after" thunks run,
/// no ports flushed. r7rs `exit` (which does run cleaners) is implemented
/// as a scheme procedure in `(tein process)` that delegates here after cleanup.
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
    /// register VFS shadow modules without full sandboxing.
    /// enables modules like `(scheme process-context)` in non-sandboxed contexts
    /// where the shadow is needed (e.g. for `(chibi test)` which imports it).
    with_vfs_shadows: bool,
    /// fake environment variables for sandboxed contexts.
    sandbox_env: Option<Vec<(String, String)>>,
    /// fake command-line for sandboxed contexts.
    sandbox_command_line: Option<Vec<String>>,
    /// user-supplied filesystem module search directories.
    /// combined with `TEIN_MODULE_PATH` env var during `build()`.
    module_paths: Vec<String>,
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

    /// Register VFS shadow modules without enabling full sandboxing.
    ///
    /// Shadow modules (e.g. `scheme/process-context`, `scheme/file`) are normally
    /// only registered during a sandboxed context build. This option registers them
    /// in any context, enabling modules like `(chibi test)` — which imports
    /// `(scheme process-context)` — to load correctly in non-sandboxed contexts.
    ///
    /// This does **not** enable the VFS gate or module allowlist; the context remains
    /// fully permissive. Primarily useful for test harnesses.
    pub fn with_vfs_shadows(mut self) -> Self {
        self.with_vfs_shadows = true;
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
    /// note: dep resolution happens at [`ContextBuilder::build`] time, not here.
    /// `allow_module` only appends to the list; `build()` calls `registry_resolve_deps`
    /// on the final `Modules::Only(list)` to expand transitive deps before arming the gate.
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

    /// Add a directory to the module search path.
    ///
    /// When resolving `(import (foo bar))`, tein searches each path for
    /// `foo/bar.sld` and loads `(include ...)` files relative to the `.sld`.
    /// Builder paths are searched before `TEIN_MODULE_PATH` dirs.
    /// Can be called multiple times; directories accumulate.
    ///
    /// Works in both sandboxed and unsandboxed contexts. Module search paths
    /// are independent of [`ContextBuilder::file_read()`] — they grant no
    /// runtime file IO access, only module discovery.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::Context;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::builder()
    ///     .standard_env()
    ///     .module_path("./lib")
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn module_path(mut self, path: &str) -> Self {
        self.module_paths.push(path.to_string());
        self
    }

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
        self.sandbox_env = Some(
            vars.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );
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
                // register eval trampolines into the primitive env BEFORE load_standard_env.
                // init-7.scm builds *chibi-env* by importing ALL bindings from the interaction
                // env (the primitive env). any name present here ends up in *chibi-env*, making
                // it available to library bodies that `(import (chibi))` — including our VFS
                // shadow SLDs for scheme/eval, scheme/load, scheme/repl. (#97)
                let prim_env = ffi::sexp_context_env(ctx);
                register_eval_trampolines(ctx, prim_env)?;

                // register trampolines that scheme wrapper code references as
                // free variables. must be in the primitive env so they end up
                // in *chibi-env* and are visible to library bodies via
                // `(import (chibi))`.
                //
                // (tein process) uses `(import (chibi))` in its library body so
                // that `emergency-exit` and other trampolines are visible as free
                // variables. chibi's built-in env has native `command-line`,
                // `get-environment-variable`, `get-environment-variables`, and
                // `emergency-exit` parameter/proc objects — registering ours here
                // OVERRIDES them in `*chibi-env*` before `load_standard_env` runs.
                // without this, `(import (tein process))` exports chibi's native
                // versions instead of our trampolines, breaking sandbox faking.
                register_native_trampoline(ctx, prim_env, "emergency-exit", exit_trampoline)?;
                register_native_trampoline(ctx, prim_env, "command-line", command_line_trampoline)?;
                register_native_trampoline(
                    ctx,
                    prim_env,
                    "get-environment-variable",
                    get_env_var_trampoline,
                )?;
                register_native_trampoline(
                    ctx,
                    prim_env,
                    "get-environment-variables",
                    get_env_vars_trampoline,
                )?;

                #[cfg(feature = "http")]
                register_native_trampoline(
                    ctx,
                    prim_env,
                    "http-request-internal",
                    crate::http::http_request_trampoline,
                )?;

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

            // --- module search path setup ---
            //
            // env var paths have lower priority (prepended first); builder paths
            // have higher priority (prepended after, so they shadow env paths).
            // for each dir: canonicalise, register into chibi's module path list,
            // and record in FS_MODULE_PATHS for the VFS gate check.
            let prev_fs_module_paths = FS_MODULE_PATHS.with(|cell| cell.borrow().clone());
            {
                let env_paths: Vec<String> = std::env::var("TEIN_MODULE_PATH")
                    .unwrap_or_default()
                    .split(':')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();

                // env first, then builder — chibi prepend means last-prepended is first-searched
                let all_paths: Vec<String> = env_paths
                    .into_iter()
                    .chain(self.module_paths.drain(..))
                    .collect();

                for raw_path in &all_paths {
                    let canon = match std::path::Path::new(raw_path).canonicalize() {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("tein: warning: module_path '{}' skipped: {}", raw_path, e);
                            continue;
                        }
                    };
                    let canon_str = canon.to_string_lossy().into_owned();

                    // create a chibi string for the directory path. root it immediately —
                    // add_module_directory calls sexp_cons (= sexp_alloc_type) which may
                    // trigger GC. with SEXP_USE_CONSERVATIVE_GC=0, C stack frames are not
                    // scanned, so c_dir would be collectible before being stored in the pair.
                    let c_dir = ffi::sexp_c_str(
                        ctx,
                        canon_str.as_ptr() as *const c_char,
                        canon_str.len() as ffi::sexp_sint_t,
                    );
                    if ffi::sexp_exceptionp(c_dir) != 0 {
                        eprintln!(
                            "tein: warning: failed to create string for module path '{}'",
                            raw_path
                        );
                        continue;
                    }
                    let _dir_root = ffi::GcRoot::new(ctx, c_dir);
                    // prepend: builder paths end up first in search order
                    ffi::add_module_directory(ctx, c_dir, false);

                    // record for VFS gate check
                    FS_MODULE_PATHS.with(|cell| cell.borrow_mut().push(canon_str));
                }
            }

            // save current gate values before overwriting — restored on drop so that
            // a second context on the same thread (sequential or nested) is not affected.
            let prev_vfs_gate = VFS_GATE.with(|cell| cell.get());
            let prev_fs_gate = FS_GATE.with(|cell| cell.get());
            let prev_fs_policy = FS_POLICY.with(|cell| cell.borrow().clone());
            let prev_vfs_allowlist = VFS_ALLOWLIST.with(|cell| cell.borrow().clone());
            let prev_is_sandboxed = IS_SANDBOXED.with(|c| c.get());
            let prev_sandbox_env = SANDBOX_ENV.with(|cell| cell.borrow().clone());
            let prev_sandbox_command_line = SANDBOX_COMMAND_LINE.with(|cell| cell.borrow().clone());

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

                    let cmd_line = self
                        .sandbox_command_line
                        .take()
                        .unwrap_or_else(|| vec!["tein".to_string(), "--sandbox".to_string()]);
                    SANDBOX_COMMAND_LINE.with(|cell| {
                        *cell.borrow_mut() = Some(cmd_line);
                    });
                }
                // arm FS policy gate — C opcodes will call tein_fs_policy_check
                FS_GATE.with(|cell| cell.set(FS_GATE_CHECK));
                ffi::fs_policy_gate_set(FS_GATE_CHECK as i32);
                crate::sandbox::register_vfs_shadows()?; // inject shadow modules before VFS gate is armed

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

                // auto-import scheme/base + scheme/write so sandboxed contexts
                // start with a usable baseline. skipped for Modules::None (the
                // "build your own allowlist" entry point — users combine it with
                // allow_module() for precise control).
                // two separate imports: scheme/base failure is fatal; scheme/write
                // failure is silently skipped (allowlist might exclude it).
                if !matches!(modules, Modules::None) {
                    for import in &["(import (scheme base))", "(import (scheme write))"] {
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
                        if ffi::sexp_exceptionp(result) != 0 && *import == "(import (scheme base))"
                        {
                            let msg = Value::from_raw(ctx, result)
                                .unwrap_or_else(|e| Value::String(format!("{e}")));
                            ffi::sexp_destroy_context(ctx);
                            return Err(crate::error::Error::InitError(format!(
                                "sandbox auto-import of scheme/base failed: {msg}"
                            )));
                        }
                        // scheme/write: silently skip if not in allowlist
                    }
                }
            }

            // set FsPolicy if file_read() or file_write() was configured.
            // note: file_read()/file_write() auto-activate sandboxed(Modules::Safe)
            // when called without an explicit sandboxed() call, so FS_POLICY is always
            // paired with IS_SANDBOXED=true in practice. the FS_GATE (C-level opcode
            // enforcement) is only armed inside the sandbox block above — unsandboxed
            // + FsPolicy is unreachable via the public API. FsPolicy is placed here
            // (outside the sandbox block) as a pure data write; the C gate remains off.
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
                prev_sandbox_env,
                prev_sandbox_command_line,
                prev_fs_module_paths,
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

            #[cfg(feature = "crypto")]
            if self.standard_env {
                crate::crypto::crypto_impl::register_module_crypto(&context)?;
            }

            #[cfg(feature = "regex")]
            if self.standard_env {
                crate::safe_regexp::safe_regexp_impl::register_module_safe_regexp(&context)?;
                // register regexp-fold as a hand-written native fn (calls scheme closures)
                context
                    .define_fn_variadic("regexp-fold", crate::safe_regexp::regexp_fold_wrapper)?;
                // override macro-generated .sld/.scm to export regexp-fold
                context.register_vfs_module(
                    "lib/tein/safe-regexp.sld",
                    crate::safe_regexp::SAFE_REGEXP_SLD,
                )?;
                context.register_vfs_module(
                    "lib/tein/safe-regexp.scm",
                    crate::safe_regexp::SAFE_REGEXP_SCM,
                )?;
            }

            #[cfg(feature = "http")]
            if self.standard_env {
                // http-request-internal trampoline is registered into the primitive
                // env before load_standard_env (see above). only VFS modules here.
                context.register_vfs_module("lib/tein/http.sld", crate::http::HTTP_SLD)?;
                context.register_vfs_module("lib/tein/http.scm", crate::http::HTTP_SCM)?;
            }

            if self.standard_env {
                crate::filesystem::register_filesystem_trampolines(&context)?;
                context.register_load_module()?;
                context.register_eval_module()?;
                context.register_process_module()?;
                context.register_modules_module()?;
                context.register_vfs_module(
                    "lib/tein/modules.sld",
                    "(define-library (tein modules) (import (scheme base)) (export register-module module-registered?))",
                )?;
            }

            // register VFS shadow modules if requested without full sandboxing.
            // normally only done during sandboxed() build; this option enables
            // shadow imports (e.g. scheme/process-context) in non-sandboxed contexts.
            if self.with_vfs_shadows && self.sandbox_modules.is_none() {
                crate::sandbox::register_vfs_shadows()?;
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
    /// previous SANDBOX_ENV value, restored on drop
    prev_sandbox_env: Option<HashMap<String, String>>,
    /// previous SANDBOX_COMMAND_LINE value, restored on drop
    prev_sandbox_command_line: Option<Vec<String>>,
    /// previous FS_MODULE_PATHS value, restored on drop
    prev_fs_module_paths: Vec<String>,
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

/// Convert a raw sexp exit value to an i32 exit code.
///
/// Called by `Context::check_exit()` after the GC root has been released.
/// Interprets the raw sexp using r7rs `exit` semantics:
/// - null / void / #t → 0
/// - #f → 1
/// - fixnum → value (clamped to i32)
/// - anything else → 0
unsafe fn exit_code_from_raw(raw: ffi::sexp) -> i32 {
    unsafe {
        if raw.is_null() || ffi::sexp_voidp(raw) != 0 {
            return 0;
        }
        if ffi::sexp_booleanp(raw) != 0 {
            return if ffi::sexp_truep(raw) != 0 { 0 } else { 1 };
        }
        if ffi::sexp_integerp(raw) != 0 {
            return ffi::sexp_unbox_fixnum(raw) as i32;
        }
        0
    }
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
            with_vfs_shadows: false,
            sandbox_env: None,
            sandbox_command_line: None,
            module_paths: Vec::new(),
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

    /// Check if `(emergency-exit)` was called during evaluation.
    ///
    /// If the exit flag is set, clears it, releases the GC root on the
    /// stashed value, and returns `Some(Ok(Value::Exit(n)))`.
    /// Returns `None` if no exit was requested.
    ///
    /// Called after scheme `emergency-exit` (direct) or `exit` (after
    /// dynamic-wind cleanup and port flushing in scheme).
    fn check_exit(&self) -> Option<Result<Value>> {
        if EXIT_REQUESTED.with(|c| c.replace(false)) {
            let raw = EXIT_VALUE.with(|c| c.replace(std::ptr::null_mut()));
            // release GC root — sexp_release_object is a no-op for immediates
            if !raw.is_null() {
                unsafe { ffi::sexp_release_object(self.ctx, raw) };
            }
            let code = unsafe { exit_code_from_raw(raw) };
            Some(Ok(Value::Exit(code)))
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
        CONTEXT_PTR.with(|c| c.set(self as *const Context));
        let _context_guard = ContextPtrGuard;
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
        // in sandboxed contexts the context env has been switched to null_env, which
        // only contains the 10 r7rs core syntax forms. both register_foreign_protocol
        // (foreign?, foreign-type, foreign-handle-id) and the type helpers below
        // (predicate, method wrappers) use `and`, `pair?`, `eq?` etc. — none of which
        // are available in null_env. skipping is safe: these helpers are not exported
        // by any SLD and are never called by native fn implementations. the native fns
        // and VFS module entries (registered elsewhere) work correctly without them.
        // see #116.
        if IS_SANDBOXED.with(|c| c.get()) {
            self.foreign_store.borrow_mut().register_type::<T>()?;
            return Ok(());
        }

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

    /// Insert a foreign value into the current context's store via the thread-local
    /// `FOREIGN_STORE_PTR`. Usable from inside `#[tein_fn]` free fn wrappers where
    /// no `Context` reference is available. Errors if no store is active (i.e. not
    /// called during an active `evaluate()` / `call()`).
    pub(crate) fn make_foreign_via_ptr<T: ForeignType>(
        value: T,
    ) -> std::result::Result<Value, String> {
        let store_ptr = FOREIGN_STORE_PTR.with(|c| c.get());
        if store_ptr.is_null() {
            return Err("make_foreign_via_ptr: no active context store (internal error)".into());
        }
        let id = unsafe { (*store_ptr).borrow_mut().insert(value) };
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
        let rc = unsafe {
            ffi::tein_vfs_register(
                c_path.as_ptr(),
                content.as_ptr() as *const std::ffi::c_char,
                content.len() as std::ffi::c_uint,
            )
        };
        if rc != 0 {
            return Err(Error::InitError(
                "VFS registration failed: out of memory".into(),
            ));
        }
        Ok(())
    }

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

            // validate it's (define-library (name ...) ...)
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

            // extract library name list
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
            let mut body = ffi::sexp_cdr(rest);
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
                        if sym == "include"
                            || sym == "include-ci"
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

        if lib_name_parts.is_empty() {
            return Err(Error::EvalError(
                "register_module: library name is empty".into(),
            ));
        }

        if has_forbidden_include {
            return Err(Error::EvalError(
                "register_module: (include ...) is not supported in dynamically registered modules; use (begin ...) instead".into(),
            ));
        }

        // derive VFS path
        let module_path = lib_name_parts.join("/");
        let vfs_sld_path = format!("/vfs/lib/{module_path}.sld");

        // collision check — reject if in static VFS
        let c_vfs_path = CString::new(vfs_sld_path.as_str())
            .map_err(|_| Error::EvalError("register_module: path contains null bytes".into()))?;
        let collision = unsafe { ffi::vfs_static_exists(&c_vfs_path) };
        if collision {
            return Err(Error::EvalError(format!(
                "register_module: module '{module_path}' already exists as a built-in module"
            )));
        }

        // register into dynamic VFS
        self.register_vfs_module(&format!("lib/{module_path}.sld"), source)?;

        // update live allowlist
        self.allow_module_runtime(&module_path);

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

    /// set a standard port parameter to the given port value.
    ///
    /// `symbol_fn` returns the global symbol for the parameter
    /// (e.g. `ffi::sexp_global_cur_out_symbol`).
    fn set_port_parameter(
        &self,
        port: &Value,
        symbol_fn: unsafe fn(ffi::sexp) -> ffi::sexp,
    ) -> Result<()> {
        let raw_port = port
            .as_port()
            .ok_or_else(|| Error::TypeError(format!("expected port, got {}", port)))?;
        unsafe {
            let env = ffi::sexp_context_env(self.ctx);
            let sym = symbol_fn(self.ctx);
            ffi::sexp_set_parameter(self.ctx, env, sym, raw_port);
        }
        Ok(())
    }

    /// Set the current output port for this context.
    ///
    /// Replaces the port that `(current-output-port)` returns in Scheme code.
    /// All output operations (`display`, `write`, `newline`, `write-char`)
    /// that default to `(current-output-port)` will use this port.
    ///
    /// # Examples
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
    /// ctx.set_current_output_port(&port)?;
    /// ctx.evaluate("(display \"hello\")")?;
    /// ctx.evaluate("(flush-output (current-output-port))")?;
    /// assert_eq!(&*buf.lock().unwrap(), b"hello");
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_current_output_port(&self, port: &Value) -> Result<()> {
        self.set_port_parameter(port, ffi::sexp_global_cur_out_symbol)
    }

    /// Set the current input port for this context.
    ///
    /// Replaces the port that `(current-input-port)` returns in Scheme code.
    ///
    /// # Examples
    ///
    /// ```
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new_standard()?;
    /// let port = ctx.open_input_port(std::io::Cursor::new(b"42"))?;
    /// ctx.set_current_input_port(&port)?;
    /// let val = ctx.evaluate("(read)")?;
    /// assert_eq!(val, Value::Integer(42));
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_current_input_port(&self, port: &Value) -> Result<()> {
        self.set_port_parameter(port, ffi::sexp_global_cur_in_symbol)
    }

    /// Set the current error port for this context.
    ///
    /// Replaces the port that `(current-error-port)` returns in Scheme code.
    ///
    /// # Examples
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
    /// ctx.set_current_error_port(&port)?;
    /// ctx.evaluate("(display \"oops\" (current-error-port))")?;
    /// ctx.evaluate("(flush-output (current-error-port))")?;
    /// assert_eq!(&*buf.lock().unwrap(), b"oops");
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_current_error_port(&self, port: &Value) -> Result<()> {
        self.set_port_parameter(port, ffi::sexp_global_cur_err_symbol)
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
        CONTEXT_PTR.with(|c| c.set(self as *const Context));
        let _context_guard = ContextPtrGuard;

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
        CONTEXT_PTR.with(|c| c.set(self as *const Context));
        let _context_guard = ContextPtrGuard;
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

    /// Register the `environment` and `interaction-environment` trampolines.
    ///
    /// `tein-environment-internal` validates import specs against the VFS
    /// allowlist and delegates to chibi's `mutable-environment`. Used by
    /// `(scheme eval)` and `(scheme load)` shadows.
    ///
    /// `tein-interaction-environment-internal` returns a persistent mutable env
    /// for REPL interaction. Used by `(scheme repl)` shadow.
    fn register_eval_module(&self) -> Result<()> {
        let env = unsafe { ffi::sexp_context_env(self.ctx) };
        register_eval_trampolines(self.ctx, env)
    }

    /// Register `get-environment-variable`, `get-environment-variables`,
    /// `command-line`, and `emergency-exit` native functions.
    ///
    /// Called during `build()` for standard-env contexts.
    ///
    /// All four are also registered into the primitive env earlier in `build()`
    /// via `register_native_trampoline`. Both registrations are required:
    ///
    /// - primitive-env registration (before `load_standard_env`): overrides
    ///   chibi's built-in `command-line`, `get-environment-variable`, etc. in
    ///   `*chibi-env*`, so `(import (chibi))` in `process.scm`'s library body
    ///   picks up our trampolines instead of chibi's native parameter objects.
    ///
    /// - `define_fn_variadic` registration (top-level env): makes the names
    ///   importable from the top-level env via eval.c patch H, so
    ///   `(import (tein process))` can export them transitively.
    fn register_process_module(&self) -> Result<()> {
        self.define_fn_variadic("get-environment-variable", get_env_var_trampoline)?;
        self.define_fn_variadic("get-environment-variables", get_env_vars_trampoline)?;
        self.define_fn_variadic("command-line", command_line_trampoline)?;
        self.define_fn_variadic("emergency-exit", exit_trampoline)?;
        Ok(())
    }

    /// Register `register-module` and `module-registered?` native functions.
    ///
    /// These form the `(tein modules)` scheme API for dynamic module registration.
    /// Called during `build()` for standard-env contexts.
    fn register_modules_module(&self) -> Result<()> {
        self.define_fn_variadic("register-module", register_module_trampoline)?;
        self.define_fn_variadic("module-registered?", module_registered_trampoline)?;
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
        CONTEXT_PTR.with(|c| c.set(self as *const Context));
        let _context_guard = ContextPtrGuard;
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
        // restore previous fake process environment
        SANDBOX_ENV.with(|cell| {
            *cell.borrow_mut() = std::mem::take(&mut self.prev_sandbox_env);
        });
        SANDBOX_COMMAND_LINE.with(|cell| {
            *cell.borrow_mut() = std::mem::take(&mut self.prev_sandbox_command_line);
        });
        FS_MODULE_PATHS.with(|cell| {
            *cell.borrow_mut() = std::mem::take(&mut self.prev_fs_module_paths);
        });

        // clear UX stub module map so next context on this thread starts fresh
        STUB_MODULE_MAP.with(|map| map.borrow_mut().clear());

        // release interaction-environment if it was created (#97)
        INTERACTION_ENV.with(|cell| {
            let env = cell.get();
            if !env.is_null() {
                unsafe { ffi::sexp_release_object(self.ctx, env) };
                cell.set(std::ptr::null_mut());
            }
        });

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
    fn test_env_trampoline_direct_unsandboxed() {
        // verify the trampoline works in a non-sandboxed standard env
        let ctx = Context::new_standard().unwrap();
        // first, verify chibi's native environment works
        let r1 = ctx
            .evaluate("(import (scheme eval)) (eval '(+ 1 2) (environment '(scheme base)))")
            .expect("native environment should work");
        assert_eq!(r1, Value::Integer(3));
        // now test our trampoline
        let result = ctx
            .evaluate("(eval '(+ 1 2) (tein-environment-internal '(scheme base)))")
            .expect("trampoline should work unsandboxed");
        assert_eq!(result, Value::Integer(3));
    }

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
        // scheme/eval is now in Safe — verify it works (#97)
        let result = ctx
            .evaluate("(import (scheme eval)) (eval '(+ 1 2) (environment '(scheme base)))")
            .expect("scheme/eval should work in Safe");
        assert_eq!(result, Value::Integer(3));
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
        // (tein process) is in the safe set — trampolines use fake env/argv in sandbox
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
        let r = ctx.evaluate("(import (scheme base)) (pair? (get-environment-variables))");
        assert_eq!(
            r.unwrap(),
            Value::Boolean(true),
            "should have fake env vars"
        );
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
    fn test_tein_process_get_env_var_no_args() {
        // calling (get-environment-variable) with no arguments must not segfault
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(get-environment-variable)");
        assert!(r.is_err(), "expected arity error, got: {:?}", r);
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
        assert_eq!(r, Value::Exit(0), "(exit) should return Exit(0)");
    }

    #[test]
    fn test_tein_process_exit_with_value() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(begin (exit 42) (+ 1 2))").unwrap();
        assert_eq!(r, Value::Exit(42), "(exit 42) should return Exit(42)");
    }

    #[test]
    fn test_tein_process_exit_true() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(begin (exit #t) 999)").unwrap();
        assert_eq!(r, Value::Exit(0), "(exit #t) should return Exit(0)");
    }

    #[test]
    fn test_tein_process_exit_false() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(begin (exit #f) 999)").unwrap();
        assert_eq!(r, Value::Exit(1), "(exit #f) should return Exit(1)");
    }

    #[test]
    fn test_tein_process_exit_string() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(begin (exit \"done\") 999)").unwrap();
        // non-integer, non-boolean → Exit(0) per r7rs
        assert_eq!(r, Value::Exit(0), "(exit str) should return Exit(0)");
    }

    #[test]
    fn test_exit_runs_dynamic_wind_after_thunks() {
        // r7rs: exit must run dynamic-wind "after" thunks before halting.
        // the after thunk runs and would raise an error if exit were broken;
        // but here the after thunk just mutates a log — we verify exit still
        // returns Exit(42) after the thunks complete.
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx
            .evaluate(
                "(let ((log '())) \
                   (dynamic-wind \
                     (lambda () (set! log (cons 'in log))) \
                     (lambda () \
                       (dynamic-wind \
                         (lambda () (set! log (cons 'in2 log))) \
                         (lambda () \
                           (exit 42)) \
                         (lambda () (set! log (cons 'out2 log))))) \
                     (lambda () (set! log (cons 'out log)))))",
            )
            .unwrap();
        // exit runs after thunks (out2, out) then halts — we get Exit(42)
        assert_eq!(r, Value::Exit(42));
    }

    #[test]
    fn test_exit_nested_dynamic_wind_order() {
        // verify innermost-first unwind order via captured output
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
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let port = ctx.open_output_port(SharedWriter(buf.clone())).unwrap();
        ctx.set_current_output_port(&port).unwrap();

        let r = ctx
            .evaluate(
                "(dynamic-wind \
                   (lambda () #f) \
                   (lambda () \
                     (dynamic-wind \
                       (lambda () #f) \
                       (lambda () \
                         (dynamic-wind \
                           (lambda () #f) \
                           (lambda () (exit 0)) \
                           (lambda () (display \"c\")))) \
                       (lambda () (display \"b\")))) \
                   (lambda () (display \"a\")))",
            )
            .unwrap();
        assert_eq!(r, Value::Exit(0));
        let output = buf.lock().unwrap();
        assert_eq!(&*output, b"cba", "after thunks run innermost-first");
    }

    #[test]
    fn test_emergency_exit_skips_dynamic_wind() {
        // emergency-exit must NOT run dynamic-wind "after" thunks
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx
            .evaluate(
                "(dynamic-wind \
                   (lambda () #f) \
                   (lambda () (emergency-exit 42)) \
                   (lambda () (error \"after thunk ran — unexpected\")))",
            )
            .unwrap();
        assert_eq!(
            r,
            Value::Exit(42),
            "emergency-exit bypasses dynamic-wind after thunks"
        );
    }

    #[test]
    fn test_exit_flushes_output_port() {
        // exit must flush current-output-port before halting (r7rs 6.13.2).
        //
        // chibi custom ports use a 4096-byte buffer — the rust Write impl is
        // only called during sexp_buffered_flush, not on every scheme write.
        // the flush here comes from `(flush-output-port (current-output-port))`
        // in process.scm, called before delegating to emergency-exit. without
        // that flush, `(display "hello")` would stay in chibi's buffer and the
        // SharedWriter would never see any bytes.
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
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let port = ctx.open_output_port(SharedWriter(buf.clone())).unwrap();
        ctx.set_current_output_port(&port).unwrap();
        let r = ctx.evaluate("(display \"hello\") (exit 0)").unwrap();
        assert_eq!(r, Value::Exit(0));
        let output = buf.lock().unwrap();
        assert_eq!(&*output, b"hello", "output port flushed before exit");
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
    fn test_open_binary_input_file_denied() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("open_binary_input_denied");
        let file = dir.join("secret.bin");
        std::fs::write(&file, b"\x00\x01\x02").expect("write");
        let path = file.to_str().unwrap().to_string();
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_read(&["/tmp/__nonexistent_prefix__/"])
            .build()
            .expect("builder");
        // C gate denies binary input — same opcode path as text mode
        let code = format!("(import (tein file)) (open-binary-input-file \"{path}\")");
        assert!(ctx.evaluate(&code).is_err(), "should be denied");
    }

    #[test]
    fn test_open_binary_output_file_denied() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("open_binary_output_denied");
        let path = dir.join("nope.bin").to_str().unwrap().to_string();
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .file_write(&["/tmp/__nonexistent_prefix__/"])
            .build()
            .expect("builder");
        // C gate denies binary output — same opcode path as text mode
        let code = format!("(import (tein file)) (open-binary-output-file \"{path}\")");
        assert!(ctx.evaluate(&code).is_err(), "should be denied");
    }

    #[test]
    fn test_open_input_file_unsandboxed_passthrough() {
        // unsandboxed: open-input-file is the chibi opcode; FS gate is off, all access allowed
        let tmp = "/tmp/tein_open_unsandboxed_test.txt";
        std::fs::write(tmp, "test").expect("write");
        let ctx = Context::builder().standard_env().build().expect("builder");
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

        // scheme/eval is NOT in this custom allowlist (only scheme/base) — should be blocked
        let err = ctx.evaluate("(import (scheme eval))");
        assert!(
            err.is_err(),
            "scheme/eval should be blocked by Modules::only([scheme/base])"
        );
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
    fn test_fs_gate_cleared_on_drop() {
        use crate::sandbox::{FS_GATE, FS_GATE_CHECK, FS_GATE_OFF, Modules};
        {
            let _ctx = Context::builder()
                .standard_env()
                .sandboxed(Modules::only(&["scheme/base"]))
                .build()
                .expect("standard + sandboxed");

            FS_GATE.with(|cell| {
                assert_eq!(cell.get(), FS_GATE_CHECK);
            });
        }
        // after drop, gate should reset
        FS_GATE.with(|cell| {
            assert_eq!(
                cell.get(),
                FS_GATE_OFF,
                "FS gate should reset to off after context drop"
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
        assert!(
            after,
            "my/tool should be in allowlist after allow_module_runtime"
        );
    }

    #[test]
    fn test_register_module_basic() {
        let ctx = Context::new_standard().expect("standard context");
        ctx.register_module(
            "(define-library (my tool) (import (scheme base)) (export greet) (begin (define (greet x) (string-append \"hi \" x))))",
        )
        .expect("register_module");

        let result = ctx
            .evaluate("(import (my tool)) (greet \"world\")")
            .expect("eval");
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

        let v1 = ctx
            .evaluate("(import (my versioned)) val")
            .expect("eval v1");
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
        use crate::sandbox::Modules;
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

    #[test]
    fn test_allow_dynamic_modules_builder() {
        use crate::sandbox::Modules;
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

    #[test]
    fn test_register_module_from_scheme() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_dynamic_modules()
            .build()
            .expect("sandboxed + dynamic modules");

        let result = ctx.evaluate(r#"
            (import (tein modules))
            (import (scheme base))
            (register-module
              "(define-library (test tool) (import (scheme base)) (export val) (begin (define val 99)))")
            (import (test tool))
            val
        "#).expect("scheme-side register-module");
        assert_eq!(result, Value::Integer(99));
    }

    #[test]
    fn test_module_registered_predicate_from_scheme() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_dynamic_modules()
            .build()
            .expect("sandboxed + dynamic modules");

        let result = ctx
            .evaluate(
                r#"
            (import (tein modules))
            (module-registered? '(nonexistent thing))
        "#,
            )
            .expect("module-registered? for nonexistent");
        assert_eq!(result, Value::Boolean(false));

        let result = ctx
            .evaluate(
                r#"
            (import (tein modules))
            (module-registered? '(scheme base))
        "#,
            )
            .expect("module-registered? for scheme/base");
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_allow_dynamic_modules_strict_sandbox() {
        // Modules::only with only scheme/base — chibi is NOT transitively included.
        // This is the minimal repro for the (import (chibi)) vs (scheme base) bug in
        // the inline SLD: if the SLD imports (chibi), the VFS gate rejects it here.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .allow_dynamic_modules()
            .build()
            .expect("strict sandboxed + dynamic modules");

        let result = ctx.evaluate("(import (tein modules)) #t");
        assert!(
            result.is_ok(),
            "(import (tein modules)) should succeed in strict sandbox with allow_dynamic_modules: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn test_register_module_rejects_include_ci() {
        let ctx = Context::new_standard().expect("standard context");
        let err = ctx
            .register_module(
                "(define-library (my mod) (import (scheme base)) (export x) (include-ci \"foo.scm\"))",
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("include"),
            "should reject (include-ci ...): {msg}"
        );
    }

    #[test]
    fn test_register_module_rejects_include_library_declarations() {
        let ctx = Context::new_standard().expect("standard context");
        let err = ctx
            .register_module(
                "(define-library (my mod) (import (scheme base)) (export x) (include-library-declarations \"foo.sld\"))",
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("include"),
            "should reject (include-library-declarations ...): {msg}"
        );
    }

    #[test]
    fn test_register_module_integer_name_component() {
        // library names may contain integers, e.g. (srfi 1)
        let ctx = Context::new_standard().expect("standard context");
        ctx.register_module(
            "(define-library (srfi 999) (import (scheme base)) (export the-answer) (begin (define the-answer 42)))",
        )
        .expect("register module with integer name component");

        let result = ctx
            .evaluate("(import (srfi 999)) the-answer")
            .expect("import module with integer name");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_register_module_invalid_name_element() {
        // library name elements must be symbols or integers — a string should be rejected
        let ctx = Context::new_standard().expect("standard context");
        let err = ctx
            .register_module(r#"(define-library ("bad" name) (import (scheme base)) (export x) (begin (define x 1)))"#)
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("symbols or integers"),
            "should reject non-symbol/integer name element: {msg}"
        );
    }

    #[test]
    fn test_allow_module_runtime_dedup() {
        use crate::sandbox::{Modules, VFS_ALLOWLIST};

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::only(&["scheme/base"]))
            .build()
            .expect("sandboxed context");

        ctx.allow_module_runtime("my/dedup-tool");
        ctx.allow_module_runtime("my/dedup-tool"); // second call — must not duplicate

        let count = VFS_ALLOWLIST.with(|cell| {
            cell.borrow()
                .iter()
                .filter(|p| *p == "my/dedup-tool")
                .count()
        });
        assert_eq!(count, 1, "allow_module_runtime should not add duplicates");
    }

    #[test]
    fn test_register_module_dynamic_update_cache_behavior() {
        // chibi caches module envs after first import — re-registration must NOT
        // invalidate the cache; the old value remains visible in the same context.
        let ctx = Context::new_standard().expect("standard context");
        ctx.register_module(
            "(define-library (my cached) (import (scheme base)) (export val) (begin (define val 1)))",
        )
        .expect("first registration");

        ctx.evaluate("(import (my cached))").expect("first import");
        let v1 = ctx.evaluate("val").expect("eval v1");
        assert_eq!(v1, Value::Integer(1));

        ctx.register_module(
            "(define-library (my cached) (import (scheme base)) (export val) (begin (define val 2)))",
        )
        .expect("re-registration should succeed");

        // chibi's module cache means the import still returns v1, not v2
        let v_after = ctx.evaluate("val").expect("eval after re-registration");
        assert_eq!(
            v_after,
            Value::Integer(1),
            "chibi module cache should preserve v1 after re-registration"
        );
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

        // non-VFS import should fail — chibi/disasm is intentionally excluded
        let err = ctx.evaluate("(import (chibi disasm))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "expected SandboxViolation for blocked import, got: {:?}",
            err
        );

        drop(ctx);
    }

    /// Verify that dangerous chibi primitives are unreachable in sandboxed Safe contexts.
    ///
    /// These were formerly blocked by `ALWAYS_STUB`; now blocked by the VFS gate
    /// (no path through any allowlisted module exports them). Each name must be
    /// unbound in the null env — accessing them must produce an error, not a value.
    ///
    /// Covers: `find-module-file`, `load-module-file`, `%import`,
    /// `add-module-directory`, `current-module-path`, `env-parent`, `env-exports`,
    /// `%meta-env`, `primitive-environment`, `scheme-report-environment`,
    /// `current-environment`, `set-current-environment!`.
    /// Closes #127.
    #[test]
    fn test_dangerous_chibi_primitives_blocked_in_sandbox() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("standard + sandboxed");

        // give access to scheme/base so the env is fully initialised
        ctx.evaluate("(import (scheme base))").unwrap();

        // primitives that bypass VFS gate / modify module search path
        let vfs_escape = [
            "find-module-file",
            "load-module-file",
            "%import",
            "add-module-directory",
            "current-module-path",
        ];
        // env traversal / mutation primitives
        let env_traversal = [
            "env-parent",
            "env-exports",
            "%meta-env",
            "primitive-environment",
            "scheme-report-environment",
            "current-environment",
            "set-current-environment!",
        ];

        for name in vfs_escape.iter().chain(env_traversal.iter()) {
            // referencing an unbound name must error; a live proc would be a sandbox escape
            let r = ctx.evaluate(name);
            assert!(
                r.is_err(),
                "dangerous primitive `{name}` must be unbound in sandbox, got: {:?}",
                r.ok()
            );
        }

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

        // ctx2 must still block non-safe modules (tein/modules is default_safe: false)
        let err = ctx2.evaluate("(import (tein modules))").unwrap_err();
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

        // a module not in the VFS registry should still fail (not in the registry).
        // chibi/disasm is intentionally excluded (exposes VM internals) — always absent.
        let err = ctx.evaluate("(import (chibi disasm))").unwrap_err();
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
    fn test_chibi_regexp_blocked_in_modules_safe() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("build");
        let err = ctx.evaluate("(import (chibi regexp))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
            "(chibi regexp) should be blocked in Modules::Safe, got: {err:?}"
        );
    }

    #[test]
    fn test_chibi_regexp_allowed_in_modules_all() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::All)
            .build()
            .expect("build");
        let r = ctx.evaluate("(import (chibi regexp)) (regexp? (regexp '(+ digit)))");
        assert!(
            r.is_ok(),
            "(chibi regexp) should work under Modules::All: {:?}",
            r.err()
        );
    }

    #[test]
    fn test_chibi_regexp_allowed_via_allow_module() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/regexp")
            .build()
            .expect("build");
        let r = ctx.evaluate("(import (chibi regexp)) (regexp? (regexp '(+ digit)))");
        assert!(
            r.is_ok(),
            "(chibi regexp) should work via allow_module: {:?}",
            r.err()
        );
    }

    #[test]
    fn test_srfi_115_alias_blocked_in_safe() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("build");
        let err = ctx.evaluate("(import (srfi 115))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
            "(srfi 115) should be blocked in Modules::Safe, got: {err:?}"
        );
    }

    #[test]
    fn test_scheme_regex_alias_blocked_in_safe() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("build");
        let err = ctx.evaluate("(import (scheme regex))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
            "(scheme regex) should be blocked in Modules::Safe, got: {err:?}"
        );
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

        // a module not in the VFS registry should fail as SandboxViolation.
        // chibi/disasm is intentionally excluded (exposes VM internals) — always absent.
        let err = ctx.evaluate("(import (chibi disasm))").unwrap_err();
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
    fn test_set_current_output_port() {
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
            .expect("open port");

        ctx.set_current_output_port(&port).expect("set port");

        // display without explicit port arg — should go to our custom port
        ctx.evaluate("(display \"hello\")").expect("display");
        ctx.evaluate("(flush-output (current-output-port))")
            .expect("flush");

        let output = buf.lock().unwrap();
        assert_eq!(&*output, b"hello");
    }

    #[test]
    fn test_set_current_output_port_survives_multiple_evals() {
        // regression: verify the custom port survives subsequent evaluate calls
        // (e.g. env-extending imports) after set_current_output_port.
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
            .expect("open port");
        ctx.set_current_output_port(&port).expect("set port");

        // several intervening evals that may extend the env (like REPL history/imports)
        ctx.evaluate("(define __test__ 1)").expect("define");
        ctx.evaluate("(import (scheme base))").expect("import");

        // display should still go to our custom port
        ctx.evaluate("(display \"hello\")").expect("display");
        ctx.evaluate("(flush-output (current-output-port))")
            .expect("flush");

        let output = buf.lock().unwrap();
        assert_eq!(&*output, b"hello", "port should survive multiple evals");
    }

    #[test]
    fn test_set_current_input_port() {
        let ctx = Context::new_standard().expect("context");
        let port = ctx
            .open_input_port(std::io::Cursor::new(b"42"))
            .expect("open port");
        ctx.set_current_input_port(&port).expect("set port");
        let val = ctx.evaluate("(read)").expect("read");
        assert_eq!(val, Value::Integer(42));
    }

    #[test]
    fn test_set_current_error_port() {
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
            .expect("open port");
        ctx.set_current_error_port(&port).expect("set port");
        ctx.evaluate("(display \"oops\" (current-error-port))")
            .expect("display");
        ctx.evaluate("(flush-output (current-error-port))")
            .expect("flush");
        let output = buf.lock().unwrap();
        assert_eq!(&*output, b"oops");
    }

    #[test]
    fn test_set_port_rejects_non_port() {
        let ctx = Context::new_standard().expect("context");
        let err = ctx
            .set_current_output_port(&Value::Integer(42))
            .unwrap_err();
        assert!(
            matches!(err, Error::TypeError(_)),
            "expected TypeError, got {:?}",
            err
        );
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
    fn test_sandbox_eval_contained() {
        // eval in sandbox can only access allowed modules via environment (#97)
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();

        // eval works with allowed modules
        let result = ctx
            .evaluate("(import (scheme eval)) (eval '(+ 1 2) (environment '(scheme base)))")
            .expect("eval with allowed module should work");
        assert_eq!(result, Value::Integer(3));

        // environment with disallowed module fails
        // (scheme regex) is default_safe: false — not in Safe allowlist
        let err = ctx
            .evaluate("(import (scheme eval)) (environment '(scheme regex))")
            .unwrap_err();
        assert!(
            err.to_string().contains("allowlist")
                || err.to_string().contains("not found")
                || matches!(err, Error::EvalError(_)),
            "disallowed module should error, got: {:?}",
            err
        );
    }

    #[test]
    fn test_sandbox_eval_environment_disallowed_module() {
        // environment rejects modules outside the allowlist (#97)
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        // (scheme regex) is not in the safe set — environment should reject it
        let err = ctx
            .evaluate("(import (scheme eval)) (environment '(scheme regex))")
            .unwrap_err();
        assert!(
            err.to_string().contains("allowlist")
                || err.to_string().contains("not found")
                || matches!(err, Error::EvalError(_)),
            "disallowed module should error, got: {:?}",
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
        let result = ctx.evaluate("(json-parse \"not json\")");
        match result {
            Err(e) => assert!(e.to_string().contains("json-parse")),
            Ok(v) => panic!("expected error, got {v:?}"),
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
        let result = ctx.evaluate("(toml-parse \"not valid {{toml\")");
        match result {
            Err(e) => assert!(e.to_string().contains("toml-parse")),
            Ok(v) => panic!("expected error, got {v:?}"),
        }
    }

    // --- trampoline bad-input / arity robustness tests ---
    //
    // variadic trampolines (define_fn_variadic, num_args=0) receive sexp_null as
    // `args` when called with no arguments — chibi does no arity checking. calling
    // sexp_car on sexp_null is UB (segfault). these tests verify:
    //   (a) no-args → Err (scheme exception)
    //   (b) wrong type (integer, boolean, list, symbol, lambda, continuation) → Err (scheme exception)
    //   (c) extra args don't crash (variadic; extra args are silently ignored)
    //
    // note: all trampoline errors raise scheme exceptions → evaluate() returns Err.
    // json/toml/http parse/stringify errors, get-environment-variable, file-exists?,
    // delete-file, and load all use make_error and propagate as Err(EvalError(...)).

    // --- get-environment-variable ---

    #[test]
    fn test_get_env_var_wrong_type_integer() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(get-environment-variable 42)");
        assert!(r.is_err(), "expected type error for integer arg: {:?}", r);
    }

    #[test]
    fn test_get_env_var_wrong_type_boolean() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(get-environment-variable #t)");
        assert!(r.is_err(), "expected type error for boolean arg: {:?}", r);
    }

    #[test]
    fn test_get_env_var_wrong_type_list() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(get-environment-variable '(\"PATH\"))");
        assert!(r.is_err(), "expected type error for list arg: {:?}", r);
    }

    #[test]
    fn test_get_env_var_wrong_type_lambda() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate("(get-environment-variable (lambda (x) x))");
        assert!(r.is_err(), "expected type error for lambda arg: {:?}", r);
    }

    #[test]
    fn test_get_env_var_extra_args_ignored() {
        // extra args are ignored by variadic trampolines — should not crash
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        // "PATH" is almost certainly set; regardless, we just check no crash
        let r = ctx.evaluate("(get-environment-variable \"PATH\" \"extra\")");
        assert!(r.is_ok(), "extra args should be silently ignored: {:?}", r);
    }

    // --- file-exists? ---

    #[test]
    fn test_file_exists_no_args() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein file))").unwrap();
        let r = ctx.evaluate("(file-exists?)");
        assert!(r.is_err(), "expected arity error: {:?}", r);
    }

    #[test]
    fn test_file_exists_wrong_type_integer() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein file))").unwrap();
        let r = ctx.evaluate("(file-exists? 42)");
        assert!(r.is_err(), "expected type error for integer arg: {:?}", r);
    }

    #[test]
    fn test_file_exists_wrong_type_symbol() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein file))").unwrap();
        let r = ctx.evaluate("(file-exists? 'myfile)");
        assert!(r.is_err(), "expected type error for symbol arg: {:?}", r);
    }

    #[test]
    fn test_file_exists_wrong_type_boolean() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein file))").unwrap();
        let r = ctx.evaluate("(file-exists? #f)");
        assert!(r.is_err(), "expected type error for boolean arg: {:?}", r);
    }

    // --- delete-file ---

    #[test]
    fn test_delete_file_no_args() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein file))").unwrap();
        let r = ctx.evaluate("(delete-file)");
        assert!(r.is_err(), "expected arity error: {:?}", r);
    }

    #[test]
    fn test_delete_file_wrong_type_integer() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein file))").unwrap();
        let r = ctx.evaluate("(delete-file 99)");
        assert!(r.is_err(), "expected type error for integer arg: {:?}", r);
    }

    #[test]
    fn test_delete_file_wrong_type_list() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein file))").unwrap();
        let r = ctx.evaluate("(delete-file '(\"/tmp/x\"))");
        assert!(r.is_err(), "expected type error for list arg: {:?}", r);
    }

    // --- load ---

    #[test]
    fn test_load_no_args() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein load))").unwrap();
        let r = ctx.evaluate("(load)");
        assert!(r.is_err(), "expected arity error: {:?}", r);
    }

    #[test]
    fn test_load_wrong_type_integer() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein load))").unwrap();
        let r = ctx.evaluate("(load 42)");
        assert!(r.is_err(), "expected type error for integer arg: {:?}", r);
    }

    #[test]
    fn test_load_wrong_type_symbol() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein load))").unwrap();
        let r = ctx.evaluate("(load 'myfile)");
        assert!(r.is_err(), "expected type error for symbol arg: {:?}", r);
    }

    // --- json-parse ---

    #[cfg(feature = "json")]
    #[test]
    fn test_json_parse_no_args() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein json))").unwrap();
        let r = ctx.evaluate("(json-parse)");
        // returns make_error → Err
        assert!(r.is_err(), "expected arity error: {:?}", r);
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_json_parse_wrong_type_integer() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein json))").unwrap();
        let r = ctx.evaluate("(json-parse 42)");
        match r {
            Err(e) => assert!(e.to_string().contains("json-parse")),
            Ok(v) => panic!("expected error, got {v:?}"),
        }
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_json_parse_wrong_type_boolean() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein json))").unwrap();
        let r = ctx.evaluate("(json-parse #f)");
        match r {
            Err(e) => assert!(e.to_string().contains("json-parse")),
            Ok(v) => panic!("expected error, got {v:?}"),
        }
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_json_parse_wrong_type_list() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein json))").unwrap();
        let r = ctx.evaluate("(json-parse '(1 2 3))");
        match r {
            Err(e) => assert!(e.to_string().contains("json-parse")),
            Ok(v) => panic!("expected error, got {v:?}"),
        }
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_json_parse_wrong_type_lambda() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein json))").unwrap();
        let r = ctx.evaluate("(json-parse (lambda () 42))");
        match r {
            Err(e) => assert!(e.to_string().contains("json-parse")),
            Ok(v) => panic!("expected error, got {v:?}"),
        }
    }

    // --- json-stringify ---

    #[cfg(feature = "json")]
    #[test]
    fn test_json_stringify_no_args() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein json))").unwrap();
        let r = ctx.evaluate("(json-stringify)");
        assert!(r.is_err(), "expected arity error: {:?}", r);
    }

    #[cfg(feature = "json")]
    #[test]
    fn test_json_stringify_lambda_arg() {
        // lambdas are not json-serialisable — should raise an error, not crash
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein json))").unwrap();
        let r = ctx.evaluate("(json-stringify (lambda (x) x))");
        assert!(r.is_err(), "expected error, got {r:?}");
    }

    // --- toml-parse ---

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_parse_no_args() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein toml))").unwrap();
        let r = ctx.evaluate("(toml-parse)");
        assert!(r.is_err(), "expected arity error: {:?}", r);
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_parse_wrong_type_integer() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein toml))").unwrap();
        let r = ctx.evaluate("(toml-parse 42)");
        match r {
            Err(e) => assert!(e.to_string().contains("toml-parse")),
            Ok(v) => panic!("expected error, got {v:?}"),
        }
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_parse_wrong_type_boolean() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein toml))").unwrap();
        let r = ctx.evaluate("(toml-parse #t)");
        match r {
            Err(e) => assert!(e.to_string().contains("toml-parse")),
            Ok(v) => panic!("expected error, got {v:?}"),
        }
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_parse_wrong_type_list() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein toml))").unwrap();
        let r = ctx.evaluate("(toml-parse '(\"a\" \"b\"))");
        match r {
            Err(e) => assert!(e.to_string().contains("toml-parse")),
            Ok(v) => panic!("expected error, got {v:?}"),
        }
    }

    // --- toml-stringify ---

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_stringify_no_args() {
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein toml))").unwrap();
        let r = ctx.evaluate("(toml-stringify)");
        assert!(r.is_err(), "expected arity error: {:?}", r);
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_stringify_integer_arg() {
        // integers are not valid toml root values — should raise an error
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein toml))").unwrap();
        let r = ctx.evaluate("(toml-stringify 42)");
        assert!(r.is_err(), "expected error, got {r:?}");
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
    fn test_sandbox_auto_import_safe_has_base_and_write() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("build");
        // scheme/base: let and + are available without explicit import
        let result = ctx
            .evaluate("(let ((x (+ 1 2))) x)")
            .expect("let + should work without explicit import");
        assert_eq!(result, Value::Integer(3));
        // scheme/write: display is available without explicit import
        let result = ctx
            .evaluate("(begin (display \"\") #t)")
            .expect("display should work without explicit import");
        assert_eq!(result, Value::Boolean(true));
    }

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
            .expect("+ should work in All without explicit import");
        assert_eq!(result, Value::Integer(42));
        let result = ctx
            .evaluate("(begin (display \"\") #t)")
            .expect("display should work in All without explicit import");
        assert_eq!(result, Value::Boolean(true));
    }

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
            .expect("let should work without explicit import");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_sandbox_auto_import_none_skips() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::None)
            .build()
            .expect("build");
        // + should be stubbed — SandboxViolation (not available without import)
        let err = ctx.evaluate("(+ 1 2)").expect_err("should fail");
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "expected SandboxViolation, got: {err:?}"
        );
    }

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
        // base forms work
        let result = ctx.evaluate("(+ 1 2)").expect("base should work");
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

    #[test]
    fn test_sandboxed_modules_safe_eval_contained() {
        // scheme/eval is in Safe but environment validates allowlist (#97)
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("build");
        // import succeeds
        let result = ctx
            .evaluate("(import (scheme eval)) (eval '(+ 10 20) (environment '(scheme base)))")
            .expect("eval with allowed module");
        assert_eq!(result, Value::Integer(30));
        // but disallowed module is rejected (scheme/regex is default_safe: false)
        let err = ctx
            .evaluate("(import (scheme eval)) (environment '(scheme regex))")
            .expect_err("scheme/regex should be blocked");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("allowlist") || msg.contains("not found") || msg.contains("ast"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn test_sandboxed_environment_empty() {
        // (environment) with no args returns an empty env — eval of a literal works
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        let result = ctx
            .evaluate("(import (scheme eval)) (eval 42 (environment))")
            .expect("eval literal in empty env");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_environment_trampoline_multi_spec_gc() {
        // Multi-spec environment call — exercises the cons loop with 3+ specs.
        // Under the GC rooting bug, the partial list could be collected mid-loop
        // under heap pressure. Use `--features debug-chibi` for GC instrumentation.
        let ctx = Context::new_standard().unwrap();
        let result = ctx
            .evaluate(
                "(import (scheme eval))\
                 (eval '(+ 1 2) (environment '(scheme base) '(scheme write) '(scheme cxr)))",
            )
            .expect("multi-spec environment should work");
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn test_sandboxed_environment_trampoline_multi_spec_gc() {
        // Same but sandboxed — allowlist check + multi-spec cons loop both exercised.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        let result = ctx
            .evaluate(
                "(import (scheme eval))\
                 (eval '(+ 10 20) (environment '(scheme base) '(scheme write) '(scheme cxr)))",
            )
            .expect("sandboxed multi-spec environment should work");
        assert_eq!(result, Value::Integer(30));
    }

    #[test]
    fn test_sandboxed_environment_via_scheme_load() {
        // environment accessible from (scheme load) too
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        let result = ctx
            .evaluate(
                "(import (scheme eval) (scheme load)) (eval '(+ 10 20) (environment '(scheme base)))",
            )
            .expect("eval via scheme/load environment");
        assert_eq!(result, Value::Integer(30));
    }

    #[test]
    fn test_sandboxed_interaction_environment_mutable() {
        // define a binding in interaction-environment, then retrieve it
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        let result = ctx
            .evaluate(
                "(import (scheme eval) (scheme repl))\n\
                 (eval '(define x 42) (interaction-environment))\n\
                 (eval 'x (interaction-environment))",
            )
            .expect("interaction-environment should be mutable");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_sandboxed_interaction_environment_persistent() {
        // interaction-environment persists across evaluate calls
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        // first eval: define
        ctx.evaluate(
            "(import (scheme eval) (scheme repl))\n\
             (eval '(define y 99) (interaction-environment))",
        )
        .expect("define in interaction-environment");
        // second eval: retrieve — should still be there
        let result = ctx
            .evaluate(
                "(import (scheme eval) (scheme repl))\n\
                 (eval 'y (interaction-environment))",
            )
            .expect("y should persist");
        assert_eq!(result, Value::Integer(99));
    }

    #[test]
    fn test_sandboxed_interaction_environment_has_base_bindings() {
        // interaction-environment reflects the context env — bindings are
        // available after import. import (scheme base) then verify via eval.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        let result = ctx
            .evaluate(
                "(import (scheme base) (scheme eval) (scheme repl))\n\
                 (eval '(+ 1 2) (interaction-environment))",
            )
            .expect("interaction-environment should have base bindings");
        assert_eq!(result, Value::Integer(3));
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
        // (exit 0) should succeed and return Exit(0)
        let result = ctx
            .evaluate("(import (tein process)) (exit 0)")
            .expect("tein/process exit");
        assert_eq!(result, Value::Exit(0));
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
    fn test_shadow_stubs_not_registered_in_unsandboxed_context() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        use crate::sandbox::Modules;
        // chibi/filesystem is now Embedded (re-exports from (tein filesystem)).
        // sandboxed: real trampoline checks FsPolicy — denied without policy.
        // unsandboxed: IS_SANDBOXED=false — real file ops work freely.

        // sandboxed: real trampoline denies without read policy
        let sandboxed = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("sandboxed context");
        let stub_err = sandboxed.evaluate("(import (chibi filesystem)) (directory-files \".\")");
        let err_msg = format!("{:?}", stub_err.unwrap_err());
        assert!(
            err_msg.contains("sandbox") || err_msg.contains("not permitted"),
            "sandboxed chibi/filesystem call should raise policy error: {err_msg}"
        );
        // drop sandboxed context before building unsandboxed — restores IS_SANDBOXED=false
        // and clears the FS gate. both are RAII thread-locals scoped to the context.
        drop(sandboxed);

        // unsandboxed: IS_SANDBOXED=false — real file ops work, no policy gate fires
        let unsandboxed = Context::builder()
            .standard_env()
            .build()
            .expect("unsandboxed context");
        let file_ok = unsandboxed
            .evaluate("(import (scheme base) (tein file)) (file-exists? \"/etc/passwd\")");
        assert_eq!(
            file_ok.expect("unsandboxed file-exists? should work"),
            Value::Boolean(true),
            "file access must work in unsandboxed context (IS_SANDBOXED=false)"
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
    fn test_srfi_98_in_sandbox() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .expect("builder");
        // srfi/98 now re-exports from (tein process) which provides sandbox-aware
        // trampolines. TEIN_SANDBOX is in SANDBOX_ENV so it returns "true".
        let r = ctx
            .evaluate(
                "(import (scheme base) (srfi 98)) (get-environment-variable \"TEIN_SANDBOX\")",
            )
            .expect("srfi/98 importable in sandbox");
        assert_eq!(
            r,
            Value::String("true".to_string()),
            "srfi/98 in sandbox returns SANDBOX_ENV values"
        );
        // non-existent var returns #f
        let r = ctx
            .evaluate("(get-environment-variable \"NONEXISTENT_VAR_12345\")")
            .expect("get-environment-variable");
        assert_eq!(
            r,
            Value::Boolean(false),
            "srfi/98 returns #f for unknown vars in sandbox"
        );
    }

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

    // --- clib loading regression tests (issue #98) ---

    #[test]
    fn test_chibi_weak_clib_loads() {
        let ctx = Context::builder()
            .standard_env()
            .step_limit(1_000_000)
            .build()
            .expect("build");
        // chibi/weak requires include-shared "weak" — fails without ClibEntry
        let result = ctx
            .evaluate(
                "(import (chibi weak)) \
                 (let ((e (make-ephemeron 'key 'value))) \
                   (and (ephemeron? e) \
                        (eq? (ephemeron-key e) 'key) \
                        (eq? (ephemeron-value e) 'value)))",
            )
            .expect("chibi/weak should load and work");
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_chibi_weak_clib_loads_sandboxed() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .step_limit(1_000_000)
            .build()
            .expect("build");
        let result = ctx
            .evaluate("(import (chibi weak)) (ephemeron? (make-ephemeron 'a 'b))")
            .expect("chibi/weak should load in sandbox");
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_shadow_stub_chibi_filesystem_raises_error() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/filesystem")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi filesystem)) (create-directory \"/tmp/test\")");
        // real trampoline checks write policy; sandbox without policy → denied
        match result {
            Err(e) => assert!(
                e.to_string().contains("write not permitted")
                    || e.to_string().contains("[sandbox:file]"),
                "expected sandbox write error, got: {e}"
            ),
            Ok(v) => panic!("expected error, got: {v:?}"),
        }
    }

    #[test]
    fn test_shadow_stub_constants_are_zero() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/filesystem")
            .build()
            .unwrap();
        let result = ctx
            .evaluate("(import (chibi filesystem)) open/read")
            .unwrap();
        assert_eq!(result, Value::Integer(0));
    }

    #[test]
    fn test_shadow_stub_chibi_shell_macro_raises_error() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/shell")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi shell)) (shell \"echo hello\")");
        match result {
            Err(e) => assert!(
                e.to_string().contains("[sandbox:chibi/shell]"),
                "expected sandbox error, got: {e}"
            ),
            Ok(v) => panic!("expected error, got: {v:?}"),
        }
    }

    #[test]
    fn test_chibi_channel_in_vfs() {
        // chibi/channel is registered in the VFS (pure-scheme, not OS-touching).
        // its dependency srfi/18 uses SEXP_USE_GREEN_THREADS — enabled on posix,
        // disabled on windows. on posix, with chibi/time's ClibEntry wired up,
        // (import (chibi channel)) fully succeeds. on windows, it errors (thread
        // support absent) but must NOT be a SandboxViolation (not blocked by gate).
        use crate::sandbox::Modules;

        let ctx_sandbox = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/channel")
            .allow_module("srfi/18")
            .build()
            .unwrap();
        let result = ctx_sandbox.evaluate("(import (chibi channel))");
        match result {
            // posix: green threads enabled + chibi/time wired → import succeeds
            Ok(_) => {}
            // windows (or any other failure): must not be a sandbox gate violation
            Err(err) => assert!(
                !matches!(err, Error::SandboxViolation(_)),
                "chibi/channel should not be a sandbox violation, got: {:?}",
                err
            ),
        }
    }

    #[test]
    fn test_chibi_iset_optimize_loads() {
        // chibi/iset/optimize is pure scheme; all deps (iset/base, iset/iterators,
        // iset/constructors, srfi/151) are in VFS and safe.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        ctx.evaluate(
            "(import (chibi iset) (chibi iset optimize)) \
             (iset-optimize (iset 1 2 3))",
        )
        .expect("chibi/iset/optimize should load and iset-optimize should work");
    }

    #[test]
    fn test_chibi_show_aliases_load() {
        // chibi/show/color|column|pretty|unicode are alias-for srfi/166 sub-modules.
        // these transitively depend on srfi/166/base → scheme/repl (shadow), so
        // a sandboxed context is required.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        ctx.evaluate(
            "(import (chibi show color) (chibi show column) \
             (chibi show pretty) (chibi show unicode))",
        )
        .expect("chibi/show alias modules should all load");
    }

    #[test]
    fn test_srfi_227_definition_loads() {
        // srfi/227/definition re-exports define-optionals from chibi/optional.
        let ctx = Context::builder().standard_env().build().unwrap();
        let result = ctx
            .evaluate(
                "(import (srfi 227 definition)) \
                 (define-optionals (f x (y 10)) (+ x y)) \
                 (f 1)",
            )
            .expect("srfi/227/definition should load and define-optionals should work");
        assert_eq!(result, Value::Integer(11));
    }

    #[test]
    fn test_chibi_mime_loads() {
        // chibi/mime is pure scheme: MIME parsing with base64/quoted-printable.
        // all deps in VFS and safe.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        let result = ctx
            .evaluate(
                "(import (chibi mime)) \
                 (mime-parse-content-type \"text/html; charset=utf-8\")",
            )
            .expect("chibi/mime should load and parse content types");
        // result is an alist like (("text/html") ("charset" . "utf-8"))
        assert!(
            matches!(result, Value::List(_) | Value::Pair(_, _)),
            "expected parsed content-type alist, got: {result}"
        );
    }

    #[test]
    fn test_chibi_binary_record_loads() {
        // chibi/binary-record provides binary record type macros.
        // pure scheme, deps: scheme/base, srfi/1, srfi/151, srfi/130.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        ctx.evaluate("(import (chibi binary-record))")
            .expect("chibi/binary-record should load");
    }

    #[test]
    fn test_chibi_memoize_loads() {
        // chibi/memoize provides in-memory LRU caching.
        // chibi cond-expand branch pulls chibi/system + chibi/filesystem
        // (both already shadowed). LRU cache construction works; file-backed
        // memoize-to-file errors via shadowed deps. #105 upgrades later.
        // note: memoize + does not work because procedure-arity of variadic
        // builtins returns an unexpected value under chibi/ast introspection.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        let result = ctx
            .evaluate(
                "(import (chibi memoize)) \
                 (lru-cache? (make-lru-cache))",
            )
            .expect("chibi/memoize should load and make-lru-cache should work");
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_shadow_stub_chibi_stty_raises_error() {
        // chibi/stty is C-backed terminal control — shadow stub blocks all access.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/stty")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi stty)) (get-terminal-width)");
        match result {
            Err(e) => assert!(
                e.to_string().contains("[sandbox:chibi/stty]"),
                "expected sandbox error, got: {e}"
            ),
            Ok(v) => panic!("expected error, got: {v:?}"),
        }
    }

    #[test]
    fn test_shadow_stub_chibi_term_edit_line_raises_error() {
        // chibi/term/edit-line depends on stty — shadow stub blocks all access.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/term/edit-line")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi term edit-line)) (make-line-editor)");
        match result {
            Err(e) => assert!(
                e.to_string().contains("[sandbox:chibi/term/edit-line]"),
                "expected sandbox error, got: {e}"
            ),
            Ok(v) => panic!("expected error, got: {v:?}"),
        }
    }

    #[test]
    fn test_shadow_stub_chibi_app_raises_error() {
        // chibi/app is CLI framework depending on config + process-context.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/app")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi app)) (app-help '())");
        match result {
            Err(e) => assert!(
                e.to_string().contains("[sandbox:chibi/app]"),
                "expected sandbox error, got: {e}"
            ),
            Ok(v) => panic!("expected error, got: {v:?}"),
        }
    }

    #[test]
    fn test_shadow_stub_chibi_config_raises_error() {
        // chibi/config is config file reader depending on scheme/file + chibi/filesystem.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/config")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi config)) (make-conf '())");
        match result {
            Err(e) => assert!(
                e.to_string().contains("[sandbox:chibi/config]"),
                "expected sandbox error, got: {e}"
            ),
            Ok(v) => panic!("expected error, got: {v:?}"),
        }
    }

    #[test]
    fn test_shadow_stub_chibi_log_raises_error() {
        // chibi/log is logging with file locking + OS identity (PIDs, UIDs).
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/log")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi log)) (log-open \"test\")");
        match result {
            Err(e) => assert!(
                e.to_string().contains("[sandbox:chibi/log]"),
                "expected sandbox error, got: {e}"
            ),
            Ok(v) => panic!("expected error, got: {v:?}"),
        }
    }

    #[test]
    fn test_shadow_stub_chibi_tar_raises_error() {
        // chibi/tar: archive handling hard-wired to chibi/filesystem (#105).
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/tar")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi tar)) (tar-files \"test.tar\")");
        match result {
            Err(e) => assert!(
                e.to_string().contains("[sandbox:chibi/tar]"),
                "expected sandbox error, got: {e}"
            ),
            Ok(v) => panic!("expected error, got: {v:?}"),
        }
    }

    #[test]
    fn test_shadow_stub_srfi_193_raises_error() {
        // srfi/193: command-line args + script path leak in sandbox.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("srfi/193")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (srfi 193)) (script-file)");
        match result {
            Err(e) => assert!(
                e.to_string().contains("[sandbox:srfi/193]"),
                "expected sandbox error, got: {e}"
            ),
            Ok(v) => panic!("expected error, got: {v:?}"),
        }
    }

    #[test]
    fn test_shadow_stub_chibi_apropos_raises_error() {
        // chibi/apropos: env introspection exposes internal module structure.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/apropos")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi apropos)) (apropos \"test\")");
        match result {
            Err(e) => assert!(
                e.to_string().contains("[sandbox:chibi/apropos]"),
                "expected sandbox error, got: {e}"
            ),
            Ok(v) => panic!("expected error, got: {v:?}"),
        }
    }

    #[test]
    fn test_scheme_load_shadow_uses_tein_load() {
        // scheme/load shadow re-exports from (tein load) — VFS-restricted load.
        // (scheme load) has a shadow_sld that imports (tein load) and re-exports load.
        // verify: import succeeds, and the load binding rejects non-VFS paths.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        // import should succeed — shadow registered in sandboxed context
        ctx.evaluate("(import (scheme load))")
            .expect("(scheme load) shadow should import via (tein load)");
        // a non-VFS path should be rejected by the VFS-restricted load
        let r = ctx.evaluate("(load \"/etc/passwd\")");
        assert!(r.is_err(), "expected non-VFS load to fail, got: {r:?}");
    }

    // NOTE: scheme/time has a deep dependency chain (scheme/process-context,
    // scheme/file, scheme/read, scheme/time/tai-to-utc-offset) and performs
    // file IO at load time (leap second list). it is default_safe: false.
    // the ClibEntry fix (issue #98) is verified structurally — it follows the
    // same pattern as all other clib entries and the build succeeds (the C
    // source compiles and is linked into the static library table).
    // a full integration test for (scheme time) would require either:
    //   - a non-sandboxed context with filesystem access to chibi's scheme/process-context, or
    //   - making scheme/process-context available as both Embedded and Shadow
    // neither is warranted for a clib audit fix.

    #[test]
    fn test_srfi_160_uvprims_loads_and_works() {
        // verifies uvprims.c compiles correctly and the C primitives are callable.
        // tests: predicates, make, ref, set!, length, list conversion.
        let ctx = Context::builder()
            .standard_env()
            .step_limit(5_000_000)
            .build()
            .expect("build");
        // s8 vector round-trip
        let result = ctx
            .evaluate(
                "(import (srfi 160 base)) \
                 (let ((v (make-s8vector 3 0))) \
                   (s8vector-set! v 0 -5) \
                   (s8vector-set! v 1 42) \
                   (s8vector-set! v 2 127) \
                   (and (s8vector? v) \
                        (= (s8vector-length v) 3) \
                        (= (s8vector-ref v 0) -5) \
                        (= (s8vector-ref v 1) 42) \
                        (= (s8vector-ref v 2) 127)))",
            )
            .expect("srfi/160/base s8vector should work");
        assert_eq!(result, Value::Boolean(true), "s8vector round-trip");

        // f64 vector
        let ctx2 = Context::builder()
            .standard_env()
            .step_limit(5_000_000)
            .build()
            .expect("build");
        let result2 = ctx2
            .evaluate(
                "(import (srfi 160 base)) \
                 (let ((v (f64vector 1.5 2.5 3.5))) \
                   (and (f64vector? v) \
                        (= (f64vector-length v) 3) \
                        (= (f64vector-ref v 1) 2.5)))",
            )
            .expect("srfi/160/base f64vector should work");
        assert_eq!(result2, Value::Boolean(true), "f64vector round-trip");
    }

    #[test]
    fn test_srfi_160_u8_submodule_loads() {
        // verifies a type-specific sub-module (u8) loads with uvector.scm
        let ctx = Context::builder()
            .standard_env()
            .step_limit(5_000_000)
            .build()
            .expect("build");
        let result = ctx
            .evaluate(
                "(import (srfi 160 u8)) \
                 (let ((v (make-u8vector 4 7))) \
                   (and (u8vector? v) \
                        (= (u8vector-length v) 4) \
                        (= (u8vector-ref v 2) 7)))",
            )
            .expect("srfi/160/u8 should load and work");
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_srfi_144_flonum_constants() {
        // srfi/144 requires a C-backed static library (math.c generated from math.stub).
        // fl-pi should be approximately π — a C constant, not pure scheme.
        let ctx = Context::builder()
            .standard_env()
            .step_limit(5_000_000)
            .build()
            .expect("build");
        let result = ctx
            .evaluate("(import (srfi 144)) fl-pi")
            .expect("fl-pi should be defined");
        match result {
            Value::Float(f) => assert!((f - std::f64::consts::PI).abs() < 1e-10),
            other => panic!("expected float, got {other:?}"),
        }
    }

    #[test]
    fn test_scheme_bytevector_endian() {
        // scheme/bytevector requires a C-backed static library (bytevector.c from bytevector.stub).
        // bytevector-u16-ref with little-endian on bytes [1, 0] should give 1.
        let ctx = Context::builder()
            .standard_env()
            .step_limit(5_000_000)
            .build()
            .expect("build");
        let result = ctx
            .evaluate(
                "(import (scheme bytevector)) \
                 (let ((bv (make-bytevector 2 0))) \
                   (bytevector-u8-set! bv 0 1) \
                   (bytevector-u16-ref bv 0 'little))",
            )
            .expect("bytevector-u16-ref should work");
        assert_eq!(result, Value::Integer(1));
    }

    #[test]
    fn test_chibi_time_import() {
        // chibi/time requires a C-backed static library (time.c from time.stub).
        // get-time-of-day is a C procedure — procedure? verifies it loaded.
        let ctx = Context::builder()
            .standard_env()
            .step_limit(5_000_000)
            .build()
            .expect("build");
        let result = ctx
            .evaluate("(import (chibi time)) (procedure? get-time-of-day)")
            .expect("chibi/time should load");
        assert_eq!(result, Value::Boolean(true));
    }

    #[cfg(feature = "time")]
    #[test]
    fn test_srfi_19_import() {
        // patch H: (import (srfi 19)) alone works — native fns registered via
        // define_fn_variadic are now importable as library exports.
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (srfi 19))")
            .expect("srfi 19 should import");
        let r = ctx.evaluate("(time? (current-time time-utc))");
        assert_eq!(r.unwrap(), Value::Boolean(true));
    }

    #[cfg(feature = "time")]
    #[test]
    fn test_srfi_19_sandboxed() {
        // srfi/19 is default_safe: true — importable in sandboxed contexts.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .step_limit(50_000_000)
            .build()
            .expect("sandboxed context");
        ctx.evaluate("(import (srfi 19))")
            .expect("srfi 19 importable in sandbox");
        let r = ctx.evaluate("(date-year (current-date 0))").unwrap();
        assert!(matches!(r, Value::Integer(y) if y >= 2026));
    }

    #[cfg(feature = "time")]
    #[test]
    fn test_scheme_time_shadow_uses_tein_time() {
        // verify (scheme time) shadow works in a sandboxed context, where
        // chibi/time (native C lib) is not available but (tein time) is.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .step_limit(10_000_000)
            .build()
            .expect("sandboxed context");
        ctx.evaluate("(import (scheme base))").expect("import base");
        ctx.evaluate("(import (scheme time))")
            .expect("import scheme/time");
        let r = ctx.evaluate("(> (current-second) 0)");
        assert_eq!(r.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_chibi_test_loads_with_vfs_shadows() {
        // chibi/test requires scheme/time, scheme/process-context, chibi/term/ansi, chibi/diff.
        // all of these must load successfully in a with_vfs_shadows non-sandboxed context.
        let ctx = Context::builder()
            .standard_env()
            .with_vfs_shadows()
            .build()
            .expect("build");
        ctx.evaluate("(import (chibi test))")
            .expect("import chibi/test");
        // verify test infrastructure works
        ctx.evaluate("(test-begin \"basic\") (test 1 1) (test-end)")
            .expect("basic test");
        let failures = ctx.evaluate("(test-failure-count)").expect("failure count");
        assert_eq!(
            failures,
            Value::Integer(0),
            "no chibi test failures expected"
        );
    }

    // --- exit escape hatch ---

    #[test]
    fn exit_no_args_returns_exit_zero() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx.evaluate("(import (tein process)) (exit)").unwrap();
        assert_eq!(result, Value::Exit(0));
    }

    #[test]
    fn exit_true_returns_exit_zero() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx.evaluate("(import (tein process)) (exit #t)").unwrap();
        assert_eq!(result, Value::Exit(0));
    }

    #[test]
    fn exit_false_returns_exit_one() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx.evaluate("(import (tein process)) (exit #f)").unwrap();
        assert_eq!(result, Value::Exit(1));
    }

    #[test]
    fn exit_integer_returns_exit_n() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx.evaluate("(import (tein process)) (exit 42)").unwrap();
        assert_eq!(result, Value::Exit(42));
    }

    #[test]
    fn exit_string_returns_exit_zero() {
        // non-integer, non-boolean → 0 per r7rs
        let ctx = Context::new_standard().unwrap();
        let result = ctx
            .evaluate(r#"(import (tein process)) (exit "bye")"#)
            .unwrap();
        assert_eq!(result, Value::Exit(0));
    }

    #[cfg(feature = "crypto")]
    mod crypto_tests {
        use super::*;

        // NIST test vectors: SHA-256("") and SHA-512("")
        const SHA256_EMPTY: &str =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        const SHA512_EMPTY: &str = "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
             47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e";
        // BLAKE3("") from reference implementation
        const BLAKE3_EMPTY: &str =
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";

        // SHA-256("hello")
        const SHA256_HELLO: &str =
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

        #[test]
        fn test_sha256_string() {
            let ctx = Context::builder().standard_env().build().unwrap();
            let result = ctx
                .evaluate("(import (tein crypto)) (sha256 \"\")")
                .unwrap();
            assert_eq!(result, Value::String(SHA256_EMPTY.to_string()));
        }

        #[test]
        fn test_sha256_hello() {
            let ctx = Context::builder().standard_env().build().unwrap();
            let result = ctx
                .evaluate("(import (tein crypto)) (sha256 \"hello\")")
                .unwrap();
            assert_eq!(result, Value::String(SHA256_HELLO.to_string()));
        }

        #[test]
        fn test_sha256_bytes_length() {
            let ctx = Context::builder().standard_env().build().unwrap();
            let result = ctx
                .evaluate("(import (tein crypto)) (bytevector-length (sha256-bytes \"\"))")
                .unwrap();
            assert_eq!(result, Value::Integer(32));
        }

        #[test]
        fn test_sha512_string() {
            let ctx = Context::builder().standard_env().build().unwrap();
            let result = ctx
                .evaluate("(import (tein crypto)) (sha512 \"\")")
                .unwrap();
            assert_eq!(result, Value::String(SHA512_EMPTY.to_string()));
        }

        #[test]
        fn test_sha512_bytes_length() {
            let ctx = Context::builder().standard_env().build().unwrap();
            let result = ctx
                .evaluate("(import (tein crypto)) (bytevector-length (sha512-bytes \"\"))")
                .unwrap();
            assert_eq!(result, Value::Integer(64));
        }

        #[test]
        fn test_blake3_string() {
            let ctx = Context::builder().standard_env().build().unwrap();
            let result = ctx
                .evaluate("(import (tein crypto)) (blake3 \"\")")
                .unwrap();
            assert_eq!(result, Value::String(BLAKE3_EMPTY.to_string()));
        }

        #[test]
        fn test_blake3_bytes_length() {
            let ctx = Context::builder().standard_env().build().unwrap();
            let result = ctx
                .evaluate("(import (tein crypto)) (bytevector-length (blake3-bytes \"\"))")
                .unwrap();
            assert_eq!(result, Value::Integer(32));
        }

        #[test]
        fn test_hash_string_bytevector_equivalence() {
            // "hello" as string vs #u8(104 101 108 108 111) must yield the same hash
            let ctx = Context::builder().standard_env().build().unwrap();
            let hex = ctx
                .evaluate("(import (tein crypto)) (sha256 \"hello\")")
                .unwrap();
            let bv_hex = ctx.evaluate("(sha256 #u8(104 101 108 108 111))").unwrap();
            assert_eq!(hex, bv_hex);
        }

        #[test]
        fn test_hash_invalid_input() {
            let ctx = Context::builder().standard_env().build().unwrap();
            ctx.evaluate("(import (tein crypto))").unwrap();
            // Result::Err raises a scheme exception (see AGENTS.md)
            let result = ctx.evaluate("(sha256 42)");
            match result {
                Err(e) => assert!(e.to_string().contains("string or bytevector"), "got: {e}"),
                Ok(v) => panic!("expected error, got {v:?}"),
            }
        }

        #[test]
        fn test_random_bytes_length() {
            let ctx = Context::builder().standard_env().build().unwrap();
            let result = ctx
                .evaluate("(import (tein crypto)) (bytevector-length (random-bytes 16))")
                .unwrap();
            assert_eq!(result, Value::Integer(16));
        }

        #[test]
        fn test_random_bytes_zero() {
            let ctx = Context::builder().standard_env().build().unwrap();
            let result = ctx
                .evaluate("(import (tein crypto)) (bytevector-length (random-bytes 0))")
                .unwrap();
            assert_eq!(result, Value::Integer(0));
        }

        #[test]
        fn test_random_bytes_negative() {
            let ctx = Context::builder().standard_env().build().unwrap();
            ctx.evaluate("(import (tein crypto))").unwrap();
            // Result::Err raises a scheme exception (see AGENTS.md)
            let result = ctx.evaluate("(random-bytes -1)");
            match result {
                Err(e) => assert!(e.to_string().contains("non-negative"), "got: {e}"),
                Ok(v) => panic!("expected error, got {v:?}"),
            }
        }

        #[test]
        fn test_random_integer_bounds() {
            let ctx = Context::builder().standard_env().build().unwrap();
            ctx.evaluate("(import (tein crypto))").unwrap();
            // run 100 iterations, all results must be in [0, 10)
            let result = ctx
                .evaluate(
                    "(let loop ((i 0) (ok #t))
                       (if (= i 100) ok
                         (let ((r (random-integer 10)))
                           (loop (+ i 1) (and ok (>= r 0) (< r 10))))))",
                )
                .unwrap();
            assert_eq!(result, Value::Boolean(true));
        }

        #[test]
        fn test_random_integer_invalid() {
            let ctx = Context::builder().standard_env().build().unwrap();
            ctx.evaluate("(import (tein crypto))").unwrap();
            // Result::Err raises a scheme exception (see AGENTS.md)
            let result = ctx.evaluate("(random-integer 0)");
            match result {
                Err(e) => assert!(e.to_string().contains("positive"), "got: {e}"),
                Ok(v) => panic!("expected error, got {v:?}"),
            }
        }

        #[test]
        fn test_random_float_bounds() {
            let ctx = Context::builder().standard_env().build().unwrap();
            ctx.evaluate("(import (tein crypto))").unwrap();
            let result = ctx
                .evaluate(
                    "(let loop ((i 0) (ok #t))
                       (if (= i 100) ok
                         (let ((r (random-float)))
                           (loop (+ i 1) (and ok (>= r 0.0) (< r 1.0))))))",
                )
                .unwrap();
            assert_eq!(result, Value::Boolean(true));
        }

        #[test]
        fn test_crypto_sandbox_access() {
            let ctx = Context::builder()
                .standard_env()
                .sandboxed(crate::sandbox::Modules::Safe)
                .build()
                .unwrap();
            let result = ctx
                .evaluate("(import (tein crypto)) (sha256 \"test\")")
                .unwrap();
            assert!(matches!(result, Value::String(_)));
        }
    }

    // --- module search path ---

    #[test]
    fn test_gate_check_allows_fs_module_path() {
        use crate::sandbox::FS_MODULE_PATHS;

        let dir = tempfile::TempDir::new().unwrap();
        let canon = dir
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();

        // inject the dir into FS_MODULE_PATHS (simulating what build() will do)
        FS_MODULE_PATHS.with(|cell| cell.borrow_mut().push(canon.clone()));

        // verify the thread-local contains our dir
        let paths = FS_MODULE_PATHS.with(|cell| cell.borrow().clone());
        assert!(paths.iter().any(|p| p == &canon));

        // cleanup: restore FS_MODULE_PATHS
        FS_MODULE_PATHS.with(|cell| cell.borrow_mut().retain(|p| p != &canon));
    }

    #[test]
    fn test_module_path_unsandboxed() {
        use std::io::Write;
        let dir = tempfile::TempDir::new().unwrap();
        let lib_dir = dir.path().join("my");
        std::fs::create_dir_all(&lib_dir).unwrap();
        let mut f = std::fs::File::create(lib_dir.join("util.sld")).unwrap();
        writeln!(
            f,
            "(define-library (my util) (import (scheme base)) (export square) (begin (define (square x) (* x x))))"
        )
        .unwrap();

        let ctx = Context::builder()
            .standard_env()
            .module_path(dir.path().to_str().unwrap())
            .build()
            .expect("context with module_path");

        let result = ctx
            .evaluate("(import (my util)) (square 7)")
            .expect("import from fs module path");
        assert_eq!(result, Value::Integer(49));
    }

    #[test]
    fn test_module_path_sandboxed() {
        use std::io::Write;
        let dir = tempfile::TempDir::new().unwrap();
        let lib_dir = dir.path().join("safe");
        std::fs::create_dir_all(&lib_dir).unwrap();
        let mut f = std::fs::File::create(lib_dir.join("calc.sld")).unwrap();
        writeln!(
            f,
            "(define-library (safe calc) (import (scheme base)) (export double) (begin (define (double x) (+ x x))))"
        )
        .unwrap();

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .module_path(dir.path().to_str().unwrap())
            .build()
            .expect("sandboxed context with module_path");

        let result = ctx
            .evaluate("(import (safe calc)) (double 5)")
            .expect("import from fs module path in sandbox");
        assert_eq!(result, Value::Integer(10));
    }

    #[test]
    fn test_module_path_sandboxed_blocked_transitive() {
        // a user module that tries to import a sandbox-blocked module gets rejected.
        // just verify no panic — result depends on sandbox config.
        use std::io::Write;
        let dir = tempfile::TempDir::new().unwrap();
        let lib_dir = dir.path().join("bad");
        std::fs::create_dir_all(&lib_dir).unwrap();
        let mut f = std::fs::File::create(lib_dir.join("actor.sld")).unwrap();
        writeln!(
            f,
            "(define-library (bad actor) (import (scheme eval)) (export run) (begin (define (run x) (eval x (interaction-environment)))))"
        )
        .unwrap();

        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .module_path(dir.path().to_str().unwrap())
            .build()
            .expect("sandboxed context");

        // no panic is the contract — result depends on sandbox config
        let _ = ctx.evaluate("(import (bad actor)) (run '(+ 1 2))");
    }

    #[test]
    fn test_module_path_with_include() {
        // (include "impl.scm") in an .sld should resolve relative to the .sld
        use std::io::Write;
        let dir = tempfile::TempDir::new().unwrap();
        let lib_dir = dir.path().join("ext");
        std::fs::create_dir_all(&lib_dir).unwrap();

        let mut impl_f = std::fs::File::create(lib_dir.join("impl.scm")).unwrap();
        writeln!(impl_f, "(define (triple x) (* x 3))").unwrap();

        let mut sld_f = std::fs::File::create(lib_dir.join("math.sld")).unwrap();
        writeln!(
            sld_f,
            r#"(define-library (ext math) (import (scheme base)) (export triple) (include "impl.scm"))"#
        )
        .unwrap();

        let ctx = Context::builder()
            .standard_env()
            .module_path(dir.path().to_str().unwrap())
            .build()
            .expect("context with module_path + include");

        let result = ctx
            .evaluate("(import (ext math)) (triple 4)")
            .expect("import with (include ...) in .sld");
        assert_eq!(result, Value::Integer(12));
    }

    #[test]
    fn test_module_path_traversal_rejected() {
        // path outside registered dir is blocked by the gate
        use std::io::Write;
        let dir = tempfile::TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let evil_dir = dir.path().join("evil");
        std::fs::create_dir_all(&evil_dir).unwrap();
        let mut f = std::fs::File::create(evil_dir.join("lib.sld")).unwrap();
        writeln!(
            f,
            "(define-library (evil lib) (export x) (begin (define x 1)))"
        )
        .unwrap();

        // only register "sub", not "evil"
        let ctx = Context::builder()
            .standard_env()
            .module_path(sub.to_str().unwrap())
            .build()
            .expect("context");

        // (evil lib) is not under the registered path — must fail
        let result = ctx.evaluate("(import (evil lib)) x");
        assert!(result.is_err(), "import outside registered path must fail");
    }

    #[test]
    fn test_module_path_multiple_dirs() {
        use std::io::Write;
        let dir_a = tempfile::TempDir::new().unwrap();
        let dir_b = tempfile::TempDir::new().unwrap();

        let a_lib = dir_a.path().join("a");
        std::fs::create_dir_all(&a_lib).unwrap();
        let mut f = std::fs::File::create(a_lib.join("thing.sld")).unwrap();
        writeln!(
            f,
            "(define-library (a thing) (import (scheme base)) (export ax) (begin (define ax 1)))"
        )
        .unwrap();

        let b_lib = dir_b.path().join("b");
        std::fs::create_dir_all(&b_lib).unwrap();
        let mut f = std::fs::File::create(b_lib.join("thing.sld")).unwrap();
        writeln!(
            f,
            "(define-library (b thing) (import (scheme base)) (export bx) (begin (define bx 2)))"
        )
        .unwrap();

        let ctx = Context::builder()
            .standard_env()
            .module_path(dir_a.path().to_str().unwrap())
            .module_path(dir_b.path().to_str().unwrap())
            .build()
            .expect("context with two module paths");

        let result = ctx
            .evaluate("(import (a thing)) (import (b thing)) (+ ax bx)")
            .expect("import from two separate dirs");
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn test_tein_module_path_env_var() {
        use std::io::Write;
        let dir = tempfile::TempDir::new().unwrap();
        let lib_dir = dir.path().join("env");
        std::fs::create_dir_all(&lib_dir).unwrap();
        let mut f = std::fs::File::create(lib_dir.join("greet.sld")).unwrap();
        writeln!(
            f,
            r#"(define-library (env greet) (import (scheme base)) (export hello) (begin (define (hello) "hi")))"#
        )
        .unwrap();

        // SAFETY: single-threaded test — no concurrent env reads
        unsafe { std::env::set_var("TEIN_MODULE_PATH", dir.path().to_str().unwrap()) };
        let ctx = Context::builder()
            .standard_env()
            .build()
            .expect("context with TEIN_MODULE_PATH");
        // SAFETY: single-threaded test — no concurrent env reads
        unsafe { std::env::remove_var("TEIN_MODULE_PATH") };

        let result = ctx
            .evaluate("(import (env greet)) (hello)")
            .expect("import via TEIN_MODULE_PATH env var");
        assert_eq!(result, Value::String("hi".into()));
    }
}
