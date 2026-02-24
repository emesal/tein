//! scheme evaluation context

use crate::{
    Value,
    error::{Error, Result},
    ffi,
    foreign::{ForeignStore, ForeignType},
    port::PortStore,
    sandbox::{FS_POLICY, FsPolicy, MODULE_POLICY, ModulePolicy, Preset},
};
use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::os::raw::c_char;
use std::path::Path;

/// RAII guard that clears the FOREIGN_STORE_PTR thread-local on drop.
///
/// ensures the pointer is nulled on all exit paths (early returns, `?`, panic).
struct ForeignStoreGuard;

impl Drop for ForeignStoreGuard {
    fn drop(&mut self) {
        FOREIGN_STORE_PTR.with(|c| c.set(std::ptr::null()));
    }
}

// --- original proc thread-locals for IO wrappers ---
//
// when file_read() or file_write() is configured, we capture the original
// chibi primitives before switching to the restricted env, then store them
// here so our wrapper functions can delegate after policy checks.

// original procs for the 4 wrapped file-opening primitives.
// indexed by IoOp discriminant.
thread_local! {
    static ORIGINAL_PROCS: [Cell<ffi::sexp>; 4] = const {
        [
            Cell::new(std::ptr::null_mut()),
            Cell::new(std::ptr::null_mut()),
            Cell::new(std::ptr::null_mut()),
            Cell::new(std::ptr::null_mut()),
        ]
    };
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
}

// --- implementations of the 4 foreign protocol dispatch functions ---

/// dispatch a method call: (foreign-call obj 'method arg ...)
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

/// list methods of the foreign object in the first arg: (foreign-methods obj)
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

/// list all registered type names: (foreign-types)
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
            let c_name = match CString::new(*name) {
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

/// list method names for a named type: (foreign-type-methods "type-name")
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

// --- custom port trampolines ---

/// extern "C" trampoline for custom input port reads.
///
/// called by chibi via sexp_apply when the custom port's buffer needs refilling.
/// args from scheme: (port-id buffer start end).
/// reads from the rust Read object in PortStore, copies bytes into the scheme
/// string buffer, returns fixnum byte count.
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
        let start = ffi::sexp_unbox_fixnum(start_sexp) as usize;
        let end = ffi::sexp_unbox_fixnum(end_sexp) as usize;
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

/// extern "C" trampoline for custom output port writes.
///
/// called by chibi via sexp_apply when data needs flushing to the port.
/// args from scheme: (port-id buffer start end).
/// writes bytes from the scheme string buffer to the rust Write object
/// in PortStore, returns fixnum byte count.
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
        let start = ffi::sexp_unbox_fixnum(start_sexp) as usize;
        let end = ffi::sexp_unbox_fixnum(end_sexp) as usize;
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

/// extern "C" wrapper for `(tein-reader-set! char proc)`.
///
/// registers a reader dispatch handler for `#char` syntax. rejects reserved
/// r7rs characters with a descriptive error.
unsafe extern "C" fn reader_set_wrapper(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        // args is a list: (char proc)
        if ffi::sexp_nullp(args) != 0 {
            let msg = "set-reader!: expected (set-reader! char proc)";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let ch_sexp = ffi::sexp_car(args);
        let rest = ffi::sexp_cdr(args);
        if ffi::sexp_nullp(rest) != 0 {
            let msg = "set-reader!: expected (set-reader! char proc)";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let proc_sexp = ffi::sexp_car(rest);

        if ffi::sexp_charp(ch_sexp) == 0 {
            let msg = "set-reader!: first argument must be a character";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        let c = ffi::sexp_unbox_character(ch_sexp);
        let result = ffi::reader_dispatch_set(c, proc_sexp);
        match result {
            0 => ffi::get_void(),
            -1 => {
                let ch = char::from(c as u8);
                let msg = format!(
                    "reader dispatch #{} is reserved by r7rs and cannot be overridden",
                    ch
                );
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
            _ => {
                let msg = "set-reader!: character out of ASCII range";
                let c_msg = CString::new(msg).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}

/// extern "C" wrapper for `(tein-reader-unset! char)`.
///
/// removes a reader dispatch handler for `#char` syntax.
unsafe extern "C" fn reader_unset_wrapper(
    _ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let ch_sexp = ffi::sexp_car(args);
        if ffi::sexp_charp(ch_sexp) == 0 {
            return ffi::get_void();
        }
        let c = ffi::sexp_unbox_character(ch_sexp);
        ffi::reader_dispatch_unset(c);
        ffi::get_void()
    }
}

/// extern "C" wrapper for `(tein-reader-dispatch-chars)`.
///
/// returns a list of characters with active dispatch handlers.
unsafe extern "C" fn reader_chars_wrapper(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe { ffi::reader_dispatch_chars(ctx) }
}

/// extern "C" wrapper for `(set-macro-expand-hook! proc)`.
///
/// sets the thread-local macro expansion hook. the hook receives
/// `(name unexpanded expanded env)` after each macro expansion and
/// returns the form to use (return `expanded` unchanged for observation).
unsafe extern "C" fn macro_expand_hook_set_wrapper(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        if ffi::sexp_nullp(args) != 0 {
            let msg = "set-macro-expand-hook!: expected a procedure argument";
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

/// extern "C" wrapper for `(unset-macro-expand-hook!)`.
///
/// clears the thread-local macro expansion hook.
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

/// extern "C" wrapper for `(macro-expand-hook)`.
///
/// returns the current hook procedure or `#f` if none is set.
unsafe extern "C" fn macro_expand_hook_get_wrapper(
    _ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    unsafe { ffi::macro_expand_hook_get() }
}

/// register protocol native fns into the context's current env.
///
/// called during `build()` for standard env contexts, *before* sandbox
/// restriction. defines native functions that back the `(tein reader)`
/// and `(tein macro)` VFS modules. by registering into the source env
/// before sandboxing, these are available via `(import ...)` even in
/// restricted contexts.
unsafe fn register_protocol_fns(ctx: ffi::sexp) {
    let protocol_fns: &[(
        &str,
        unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp) -> ffi::sexp,
    )] = &[
        // reader dispatch protocol
        ("set-reader!", reader_set_wrapper),
        ("unset-reader!", reader_unset_wrapper),
        ("reader-dispatch-chars", reader_chars_wrapper),
        // macro expansion hook protocol
        ("set-macro-expand-hook!", macro_expand_hook_set_wrapper),
        ("unset-macro-expand-hook!", macro_expand_hook_unset_wrapper),
        ("macro-expand-hook", macro_expand_hook_get_wrapper),
    ];
    unsafe {
        for (name, f) in protocol_fns {
            // re-fetch env each iteration — sexp_define_foreign_proc allocates
            // (interning symbols, creating procedures and env cells), which can
            // trigger GC. since env is not GC-rooted here, a stale pointer
            // would corrupt the binding list.
            let env = ffi::sexp_context_env(ctx);
            let c_name = CString::new(*name).unwrap();
            let f_typed: Option<
                unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t) -> ffi::sexp,
            > = std::mem::transmute::<*const std::ffi::c_void, _>(*f as *const std::ffi::c_void);
            ffi::sexp_define_foreign_proc(
                ctx,
                env,
                c_name.as_ptr(),
                0,
                ffi::SEXP_PROC_VARIADIC,
                c_name.as_ptr(),
                f_typed,
            );
        }
    }
}

/// the 4 file-opening primitives we wrap with policy checks
#[derive(Clone, Copy)]
#[allow(clippy::enum_variant_names)] // variants mirror scheme primitive names
enum IoOp {
    InputFile = 0,
    BinaryInputFile = 1,
    OutputFile = 2,
    BinaryOutputFile = 3,
}

impl IoOp {
    /// scheme primitive name for this operation
    const fn name(self) -> &'static str {
        match self {
            IoOp::InputFile => "open-input-file",
            IoOp::BinaryInputFile => "open-binary-input-file",
            IoOp::OutputFile => "open-output-file",
            IoOp::BinaryOutputFile => "open-binary-output-file",
        }
    }

    /// whether this is a read or write operation
    const fn is_read(self) -> bool {
        matches!(self, IoOp::InputFile | IoOp::BinaryInputFile)
    }

    /// all operations as a slice for iteration
    const ALL: [IoOp; 4] = [
        IoOp::InputFile,
        IoOp::BinaryInputFile,
        IoOp::OutputFile,
        IoOp::BinaryOutputFile,
    ];
}

/// sandbox stub for disallowed bindings
///
/// registered under the name of each known preset primitive that wasn't
/// included in the context's allowlist. when called, raises a scheme exception
/// with a `[sandbox:binding]` sentinel that `extract_exception_error` converts
/// to `Error::SandboxViolation`.
///
/// the stub extracts its own name from the opcode's name slot (set by
/// `sexp_define_foreign_proc` at registration time), so one function serves
/// all stubbed bindings.
unsafe extern "C" fn sandbox_stub(
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
        } else {
            "unknown"
        };
        let msg = format!(
            "[sandbox:binding] '{}' is not available in this sandbox",
            name
        );
        let c_msg = CString::new(msg.as_str()).unwrap_or_default();
        ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
    }
}

/// shared policy check + delegation for all file-open wrappers
///
/// extracts the filename from the first arg, checks against FsPolicy,
/// and either delegates to the original primitive or returns a policy error.
unsafe fn check_and_delegate(ctx: ffi::sexp, args: ffi::sexp, op: IoOp) -> ffi::sexp {
    unsafe {
        // extract filename string from first arg
        let first_arg = ffi::sexp_car(args);
        if ffi::sexp_stringp(first_arg) == 0 {
            let msg = "open-file: expected string argument";
            let c_msg = msg.as_ptr() as *const c_char;
            return ffi::make_error(ctx, c_msg, msg.len() as ffi::sexp_sint_t);
        }

        let c_str = ffi::sexp_string_data(first_arg);
        let len = ffi::sexp_string_size(first_arg) as usize;
        let path =
            std::str::from_utf8(std::slice::from_raw_parts(c_str as *const u8, len)).unwrap_or("");

        // check policy
        let allowed = FS_POLICY.with(|cell| {
            let policy = cell.borrow();
            match &*policy {
                Some(p) => {
                    if op.is_read() {
                        p.check_read(path)
                    } else {
                        p.check_write(path)
                    }
                }
                None => false,
            }
        });

        if !allowed {
            let op_kind = if op.is_read() { "read" } else { "write" };
            let msg = format!("[sandbox:file] {} ({} not permitted)", path, op_kind);
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        // delegate to original primitive
        let original = ORIGINAL_PROCS.with(|procs| procs[op as usize].get());
        ffi::sexp_apply_proc(ctx, original, args)
    }
}

// 4 wrapper functions — one per file-opening primitive.
// each is a thin shim that calls check_and_delegate with the right IoOp.

unsafe extern "C" fn wrapper_open_input_file(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { check_and_delegate(ctx, args, IoOp::InputFile) }
}

unsafe extern "C" fn wrapper_open_binary_input_file(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { check_and_delegate(ctx, args, IoOp::BinaryInputFile) }
}

unsafe extern "C" fn wrapper_open_output_file(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { check_and_delegate(ctx, args, IoOp::OutputFile) }
}

unsafe extern "C" fn wrapper_open_binary_output_file(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { check_and_delegate(ctx, args, IoOp::BinaryOutputFile) }
}

/// get the wrapper function pointer for a given IoOp
fn wrapper_fn_for(
    op: IoOp,
) -> unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp) -> ffi::sexp {
    match op {
        IoOp::InputFile => wrapper_open_input_file,
        IoOp::BinaryInputFile => wrapper_open_binary_input_file,
        IoOp::OutputFile => wrapper_open_output_file,
        IoOp::BinaryOutputFile => wrapper_open_binary_output_file,
    }
}

// --- default sizes ---

const DEFAULT_HEAP_SIZE: usize = 8 * 1024 * 1024;
const DEFAULT_HEAP_MAX: usize = 128 * 1024 * 1024;

/// builder for configuring a scheme context before creation
///
/// provides a fluent api for setting heap sizes, step limits,
/// and environment restrictions (sandboxing).
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
    allowed_primitives: Option<Vec<&'static str>>,
    file_read_prefixes: Option<Vec<String>>,
    file_write_prefixes: Option<Vec<String>>,
}

impl ContextBuilder {
    /// set the initial heap size in bytes (default: 8mb)
    pub fn heap_size(mut self, size: usize) -> Self {
        self.heap_size = size;
        self
    }

    /// set the maximum heap size in bytes (default: 128mb)
    pub fn heap_max(mut self, size: usize) -> Self {
        self.heap_max = size;
        self
    }

    /// set the maximum number of vm steps per evaluation call
    ///
    /// when the limit is reached, evaluation returns `Error::StepLimitExceeded`.
    /// fuel resets before each `evaluate()` or `call()` invocation.
    pub fn step_limit(mut self, limit: u64) -> Self {
        self.step_limit = Some(limit);
        self
    }

    /// enable the r7rs standard environment
    ///
    /// loads `(scheme base)` and supporting modules via the embedded VFS,
    /// providing `define-record-type`, `import`, `map`, `for-each`, etc.
    /// standard ports (stdin/stdout/stderr) are also initialised.
    ///
    /// when combined with presets, the standard env is loaded first, then
    /// the sandbox restricts it — so sandboxed code can use allowed r7rs
    /// procedures that aren't bare primitives.
    pub fn standard_env(mut self) -> Self {
        self.standard_env = true;
        self
    }

    /// add all primitives from a preset to the allowlist
    ///
    /// activating any preset switches the context to restricted mode:
    /// only explicitly allowed primitives (plus core syntax) are available.
    /// presets are additive — calling this multiple times combines them.
    pub fn preset(mut self, preset: &Preset) -> Self {
        let list = self.allowed_primitives.get_or_insert_with(Vec::new);
        for name in preset.primitives {
            if !list.contains(name) {
                list.push(name);
            }
        }
        self
    }

    /// add individual primitives to the allowlist
    ///
    /// like `preset()`, activates restricted mode. additive with presets.
    pub fn allow(mut self, names: &[&'static str]) -> Self {
        let list = self.allowed_primitives.get_or_insert_with(Vec::new);
        for name in names {
            if !list.contains(name) {
                list.push(name);
            }
        }
        self
    }

    /// convenience: allow arithmetic + math + lists + vectors + strings + characters + type predicates
    ///
    /// suitable for pure computation with no side effects or mutation.
    pub fn pure_computation(self) -> Self {
        use crate::sandbox::*;
        self.preset(&ARITHMETIC)
            .preset(&MATH)
            .preset(&LISTS)
            .preset(&VECTORS)
            .preset(&STRINGS)
            .preset(&CHARACTERS)
            .preset(&TYPE_PREDICATES)
    }

    /// convenience: pure_computation + mutation + string_ports + stdout_only + exceptions
    ///
    /// suitable for most sandboxed use cases that don't need file/network io.
    pub fn safe(self) -> Self {
        use crate::sandbox::*;
        self.pure_computation()
            .preset(&MUTATION)
            .preset(&STRING_PORTS)
            .preset(&STDOUT_ONLY)
            .preset(&EXCEPTIONS)
    }

    /// allow file reading from paths under the given prefixes
    ///
    /// activates restricted mode and registers policy-checked wrapper
    /// functions for `open-input-file` and `open-binary-input-file`.
    /// also adds port-reading support primitives (read, read-char, etc.).
    ///
    /// prefixes should be absolute paths (e.g. "/config/", "/data/").
    /// paths are canonicalised before checking, so symlinks and `..`
    /// traversals are resolved.
    pub fn file_read(mut self, prefixes: &[&str]) -> Self {
        let list = self.file_read_prefixes.get_or_insert_with(Vec::new);
        for p in prefixes {
            list.push(p.to_string());
        }
        // ensure restricted mode is active (IO wrappers require restricted env)
        self.allowed_primitives.get_or_insert_with(Vec::new);
        // ensure support primitives are in the allowlist
        self = self.preset(&crate::sandbox::FILE_READ_SUPPORT);
        self
    }

    /// allow file writing to paths under the given prefixes
    ///
    /// activates restricted mode and registers policy-checked wrapper
    /// functions for `open-output-file` and `open-binary-output-file`.
    /// also adds port-writing support primitives (write, write-char, etc.).
    ///
    /// parent directories must exist; files will be created as needed (r7rs).
    /// prefixes should be absolute paths (e.g. "/tmp/", "/output/").
    pub fn file_write(mut self, prefixes: &[&str]) -> Self {
        let list = self.file_write_prefixes.get_or_insert_with(Vec::new);
        for p in prefixes {
            list.push(p.to_string());
        }
        // ensure restricted mode is active (IO wrappers require restricted env)
        self.allowed_primitives.get_or_insert_with(Vec::new);
        // ensure support primitives are in the allowlist
        self = self.preset(&crate::sandbox::FILE_WRITE_SUPPORT);
        self
    }

    /// check if a step limit has been configured
    pub(crate) fn has_step_limit(&self) -> bool {
        self.step_limit.is_some()
    }

    /// build the configured context
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

            // register protocol native fns into the full standard env before
            // sandbox restriction. these are needed for (import (tein reader))
            // and (import (tein macro)) to work — the VFS modules re-export
            // these bindings. placed here so they exist in the source env that
            // sandboxing copies from, rather than in the restricted env.
            if self.standard_env {
                register_protocol_fns(ctx);
            }

            // activate VFS-only module policy if both standard_env and
            // sandbox (presets) are configured. this restricts (import ...)
            // to only load modules from the embedded VFS, blocking
            // filesystem-based modules like (chibi process).
            // set early so it's active during sandbox setup (which may
            // trigger transitive module loads).
            let has_module_policy = self.standard_env && self.allowed_primitives.is_some();
            if has_module_policy {
                MODULE_POLICY.with(|cell| cell.set(ModulePolicy::VfsOnly));
                ffi::module_policy_set(ModulePolicy::VfsOnly as i32);
            }

            // extract IO prefixes before borrowing self for allowed_primitives
            let file_read_prefixes = self.file_read_prefixes.take();
            let file_write_prefixes = self.file_write_prefixes.take();
            let has_io = file_read_prefixes.is_some() || file_write_prefixes.is_some();

            // apply environment restrictions if presets are active.
            // source_env is the context's current env — either the bare primitive
            // env or the enriched standard env if standard_env was loaded above.
            if let Some(ref allowed) = self.allowed_primitives {
                let source_env = ffi::sexp_context_env(ctx);
                let version = ffi::sexp_make_fixnum(7);
                let null_env = ffi::sexp_make_null_env(ctx, version);

                if ffi::sexp_exceptionp(null_env) != 0 {
                    ffi::sexp_destroy_context(ctx);
                    return Err(Error::InitError(
                        "failed to create null environment".to_string(),
                    ));
                }

                // root both envs — intern, env_copy_named, and define_foreign_proc
                // all allocate and can trigger GC. source_env is replaced as the
                // context's env by null_env, so it becomes unreachable; null_env
                // is only a rust local until sexp_context_env_set.
                let _source_env_guard = ffi::GcRoot::new(ctx, source_env);
                let _null_env_guard = ffi::GcRoot::new(ctx, null_env);

                // if IO wrappers needed, capture original procs from full env first
                if has_io {
                    let undefined = ffi::get_void();
                    for op in IoOp::ALL {
                        let name = op.name();
                        let c_name = CString::new(name).unwrap();
                        let sym =
                            ffi::sexp_intern(ctx, c_name.as_ptr(), name.len() as ffi::sexp_sint_t);
                        let val = ffi::sexp_env_ref(ctx, source_env, sym, undefined);
                        if val != undefined {
                            ORIGINAL_PROCS.with(|procs| procs[op as usize].set(val));
                        }
                    }
                }

                // copy allowed primitives from the source env into the restricted env.
                // uses env_copy_named which searches both direct bindings and
                // rename bindings (needed when standard_env is active, since the
                // module system stores most bindings as renames).
                for name in allowed {
                    let c_name = CString::new(*name).map_err(|_| {
                        ffi::sexp_destroy_context(ctx);
                        Error::InitError(format!("primitive name contains null bytes: {}", name))
                    })?;
                    ffi::env_copy_named(
                        ctx,
                        source_env,
                        null_env,
                        c_name.as_ptr(),
                        name.len() as ffi::sexp_sint_t,
                    );
                }

                ffi::sexp_context_env_set(ctx, null_env);

                // register wrapper functions in the restricted env
                if has_io {
                    let read_ops = file_read_prefixes.is_some();
                    let write_ops = file_write_prefixes.is_some();

                    for op in IoOp::ALL {
                        let want = if op.is_read() { read_ops } else { write_ops };
                        if !want {
                            continue;
                        }
                        let name = op.name();
                        let c_name = CString::new(name).unwrap();
                        let wrapper = wrapper_fn_for(op);
                        // transmute to match the 3-arg signature ffi expects
                        let f_typed: Option<
                            unsafe extern "C" fn(
                                ffi::sexp,
                                ffi::sexp,
                                ffi::sexp_sint_t,
                            ) -> ffi::sexp,
                        > = std::mem::transmute::<*const std::ffi::c_void, _>(
                            wrapper as *const std::ffi::c_void,
                        );
                        ffi::sexp_define_foreign_proc(
                            ctx,
                            null_env,
                            c_name.as_ptr(),
                            0,
                            ffi::SEXP_PROC_VARIADIC,
                            c_name.as_ptr(),
                            f_typed,
                        );
                    }

                    // set up the FsPolicy thread-local
                    FS_POLICY.with(|cell| {
                        *cell.borrow_mut() = Some(FsPolicy {
                            read_prefixes: file_read_prefixes.unwrap_or_default(),
                            write_prefixes: file_write_prefixes.unwrap_or_default(),
                        });
                    });
                }

                // register sandbox stubs for known primitives that weren't allowed.
                // this gives callers a clear SandboxViolation instead of "undefined variable".
                {
                    use crate::sandbox::ALL_PRESETS;
                    let stub_fn: Option<
                        unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t) -> ffi::sexp,
                    > = std::mem::transmute::<*const std::ffi::c_void, _>(
                        sandbox_stub as *const std::ffi::c_void,
                    );

                    for preset in ALL_PRESETS {
                        for name in preset.primitives {
                            if !allowed.contains(name) {
                                let c_name = CString::new(*name).unwrap();
                                ffi::sexp_define_foreign_proc(
                                    ctx,
                                    null_env,
                                    c_name.as_ptr(),
                                    0,
                                    ffi::SEXP_PROC_VARIADIC,
                                    c_name.as_ptr(),
                                    stub_fn,
                                );
                            }
                        }
                    }
                }
            }

            let context = Context {
                ctx,
                step_limit: self.step_limit,
                has_io_wrappers: has_io,
                has_module_policy,
                foreign_store: RefCell::new(ForeignStore::new()),
                has_foreign_protocol: Cell::new(false),
                port_store: RefCell::new(PortStore::new()),
                has_port_protocol: Cell::new(false),
            };

            Ok(context)
        }
    }

    /// build a managed context on a dedicated thread (persistent mode)
    ///
    /// the init closure runs once after context creation. state accumulates
    /// across evaluations. use `reset()` to tear down and rebuild.
    pub fn build_managed(
        self,
        init: impl Fn(&Context) -> Result<()> + Send + 'static,
    ) -> Result<crate::managed::ThreadLocalContext> {
        crate::managed::ThreadLocalContext::new(self, crate::managed::Mode::Persistent, init)
    }

    /// build a managed context on a dedicated thread (fresh mode)
    ///
    /// the init closure runs before every evaluation — context is rebuilt
    /// each time. no state persists between calls. `reset()` is a no-op.
    pub fn build_managed_fresh(
        self,
        init: impl Fn(&Context) -> Result<()> + Send + 'static,
    ) -> Result<crate::managed::ThreadLocalContext> {
        crate::managed::ThreadLocalContext::new(self, crate::managed::Mode::Fresh, init)
    }
}

/// a scheme evaluation context
///
/// this is the main entry point for evaluating scheme code.
/// each context maintains its own heap and environment.
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
    has_io_wrappers: bool,
    has_module_policy: bool,
    /// per-context store for foreign type registrations and live instances
    foreign_store: RefCell<ForeignStore>,
    /// whether foreign protocol dispatch functions are registered
    has_foreign_protocol: Cell<bool>,
    /// per-context store for custom port backing objects (Read/Write impls)
    port_store: RefCell<PortStore>,
    /// whether port protocol dispatch functions are registered
    has_port_protocol: Cell<bool>,
}

impl Context {
    /// create a new scheme context with default settings
    ///
    /// initializes a chibi-scheme context with:
    /// - 8mb initial heap
    /// - 128mb max heap
    /// - full primitive environment (no restrictions)
    /// - no step limit
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// create a new context with the r7rs standard environment
    ///
    /// equivalent to `Context::builder().standard_env().build()`.
    /// provides `(scheme base)` and supporting modules — `map`, `for-each`,
    /// `import`, `define-record-type`, etc.
    pub fn new_standard() -> Result<Self> {
        Self::builder().standard_env().build()
    }

    /// create a builder for configuring a context
    pub fn builder() -> ContextBuilder {
        ContextBuilder {
            heap_size: DEFAULT_HEAP_SIZE,
            heap_max: DEFAULT_HEAP_MAX,
            step_limit: None,
            standard_env: false,
            allowed_primitives: None,
            file_read_prefixes: None,
            file_write_prefixes: None,
        }
    }

    /// set fuel before an evaluation call (if step limit is configured)
    fn arm_fuel(&self) {
        if let Some(limit) = self.step_limit {
            unsafe {
                ffi::fuel_arm(self.ctx, limit as ffi::sexp_sint_t);
            }
        }
    }

    /// check if fuel was exhausted after an evaluation call, then disarm
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

    /// evaluate one or more scheme expressions
    ///
    /// evaluates all expressions in the string sequentially, returning the
    /// result of the last expression. this enables natural scripting patterns
    /// like defining values and then using them.
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

                // evaluate the expression
                result = ffi::sexp_evaluate(self.ctx, expr, env);

                // check fuel exhaustion before exception status
                // (fuel exhaustion returns a normal-looking value, not an exception)
                self.check_fuel()?;

                // evaluation error
                if ffi::sexp_exceptionp(result) != 0 {
                    return Value::from_raw(self.ctx, result);
                }
            }

            Value::from_raw(self.ctx, result)
        }
    }

    /// load and evaluate a scheme file
    ///
    /// reads the file contents and evaluates all expressions sequentially,
    /// returning the result of the last expression. this is the file-based
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
    /// # errors
    ///
    /// returns [`Error::IoError`] if the file cannot be read, or evaluation
    /// errors if the scheme code is invalid.
    pub fn load_file<P: AsRef<Path>>(&self, path: P) -> Result<Value> {
        let contents = std::fs::read_to_string(path.as_ref())?;
        self.evaluate(&contents)
    }

    /// register a foreign function as a scheme primitive
    ///
    /// all arguments are passed as a single scheme list via the `args` parameter.
    /// this is the universal registration method — use `#[scheme_fn]` for ergonomic
    /// wrappers that handle argument extraction and return conversion automatically.
    ///
    /// the function receives all arguments as a single scheme list in the `args`
    /// parameter. chibi passes `(ctx, self, nargs, args)` where args is a proper
    /// list of all actual arguments.
    ///
    /// this uses `sexp_define_foreign_proc_aux` with `SEXP_PROC_VARIADIC`,
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
            let env = ffi::sexp_context_env(self.ctx);
            let f_typed: Option<
                unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t) -> ffi::sexp,
            > = std::mem::transmute::<*const std::ffi::c_void, _>(f as *const std::ffi::c_void);
            let result = ffi::sexp_define_foreign_proc(
                self.ctx,
                env,
                c_name.as_ptr(),
                0, // num_args = 0 (variadic handles its own arity)
                ffi::SEXP_PROC_VARIADIC,
                c_name.as_ptr(),
                f_typed,
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

    /// raw context pointer for internal use (tests, examples, proc macros)
    #[cfg(test)]
    pub(crate) fn ctx_ptr(&self) -> ffi::sexp {
        self.ctx
    }

    /// register a rust type with the foreign object protocol.
    ///
    /// makes the type's methods callable from scheme. auto-registers:
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

    /// wrap a rust value as a scheme foreign object.
    ///
    /// stores it in the ForeignStore and returns a `Value::Foreign`
    /// that scheme code can pass around, inspect, and use with `foreign-call`.
    ///
    /// the value lives until the Context is dropped.
    pub fn foreign_value<T: ForeignType>(&self, value: T) -> Result<Value> {
        let id = self.foreign_store.borrow_mut().insert(value);
        Ok(Value::Foreign {
            handle_id: id,
            type_name: T::type_name().to_string(),
        })
    }

    /// borrow a foreign object immutably.
    ///
    /// returns an error if the value isn't `Foreign`, the handle is stale,
    /// or the type doesn't match `T`.
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

    /// register the custom port protocol dispatch functions.
    ///
    /// called automatically by `open_input_port`/`open_output_port` on first use.
    fn register_port_protocol(&self) -> Result<()> {
        self.define_fn_variadic("tein-port-read", port_read_trampoline)?;
        self.define_fn_variadic("tein-port-write", port_write_trampoline)?;
        Ok(())
    }

    /// register a reader dispatch handler for `#ch` syntax.
    ///
    /// the handler must be a scheme procedure taking one argument (the input
    /// port) and returning a datum. reserved r7rs characters (`#t`, `#f`,
    /// `#\\`, `#(`, numeric prefixes, etc.) cannot be overridden.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new_standard()?;
    /// let handler = ctx.evaluate("(lambda (port) 42)")?;
    /// ctx.register_reader('j', &handler)?;
    /// assert_eq!(ctx.evaluate("#j")?, Value::Integer(42));
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_reader(&self, ch: char, handler: &Value) -> Result<()> {
        let raw_proc = handler
            .as_procedure()
            .ok_or_else(|| Error::TypeError("handler must be a procedure".into()))?;
        let c = ch as std::ffi::c_int;
        unsafe {
            let result = ffi::reader_dispatch_set(c, raw_proc);
            match result {
                0 => Ok(()),
                -1 => Err(Error::EvalError(format!(
                    "reader dispatch #{} is reserved by r7rs and cannot be overridden",
                    ch
                ))),
                _ => Err(Error::EvalError("character out of ASCII range".into())),
            }
        }
    }

    /// set a scheme procedure as the macro expansion hook.
    ///
    /// the hook receives `(name unexpanded expanded env)` after each macro
    /// expansion and returns the form to use (replace-and-reanalyze semantics).
    /// return `expanded` unchanged for observation-only mode.
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

    /// wrap a rust `Read` as a scheme input port.
    ///
    /// returns a `Value::Port` that scheme code can pass to `read`,
    /// `read-char`, `read-line`, etc. the backing `Read` lives in the
    /// per-context `PortStore` until the context is dropped.
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

        // create scheme closure capturing port ID
        let closure_code = format!(
            "(lambda (buf start end) (tein-port-read {} buf start end))",
            port_id
        );

        // need PORT_STORE_PTR set for the evaluate call
        PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
        let _guard = PortStoreGuard;

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

    /// wrap a rust `Write` as a scheme output port.
    ///
    /// returns a `Value::Port` that scheme code can pass to `write`,
    /// `display`, `write-char`, etc. the backing `Write` lives in the
    /// per-context `PortStore` until the context is dropped.
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

        let closure_code = format!(
            "(lambda (buf start end) (tein-port-write {} buf start end))",
            port_id
        );

        PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
        let _guard = PortStoreGuard;

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

    /// read one s-expression from a port.
    ///
    /// returns the parsed but unevaluated expression.
    /// returns `Value::Unspecified` at end-of-input (EOF).
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
            if ffi::sexp_exceptionp(result) != 0 {
                return Value::from_raw(self.ctx, result);
            }
            Value::from_raw(self.ctx, result)
        }
    }

    /// read and evaluate all expressions from a port.
    ///
    /// reads s-expressions one at a time, evaluating each in sequence.
    /// returns the result of the last expression evaluated, or
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
        self.arm_fuel();

        unsafe {
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
                if ffi::sexp_exceptionp(result) != 0 {
                    return Value::from_raw(self.ctx, result);
                }
                last = Value::from_raw(self.ctx, result)?;
            }
            Ok(last)
        }
    }

    /// register the foreign object protocol dispatch functions.
    ///
    /// called automatically by `register_foreign_type` on first use.
    /// registers both the rust-side dispatch functions and the pure-scheme
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

    /// call a scheme procedure from rust
    ///
    /// invokes a `Value::Procedure` (lambda, named function, or builtin)
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
    /// # errors
    ///
    /// returns [`Error::TypeError`] if `proc` is not a `Value::Procedure`,
    /// or [`Error::EvalError`] if the scheme call raises an exception.
    pub fn call(&self, proc: &Value, args: &[Value]) -> Result<Value> {
        let raw_proc = proc
            .as_procedure()
            .ok_or_else(|| Error::TypeError(format!("expected procedure, got {}", proc)))?;

        FOREIGN_STORE_PTR.with(|c| c.set(&self.foreign_store as *const _));
        let _foreign_guard = ForeignStoreGuard;
        PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
        let _port_guard = PortStoreGuard;
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

            if ffi::sexp_exceptionp(result) != 0 {
                return Value::from_raw(self.ctx, result);
            }

            Value::from_raw(self.ctx, result)
        }
    }

    /// get the raw context pointer for advanced ffi use
    ///
    /// # safety
    /// the returned pointer is only valid for the lifetime of this context.
    /// do not call `sexp_destroy_context` on it.
    pub fn raw_ctx(&self) -> ffi::sexp {
        self.ctx
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // clean up module policy if active
        if self.has_module_policy {
            MODULE_POLICY.with(|cell| cell.set(ModulePolicy::Unrestricted));
            unsafe { ffi::module_policy_set(ModulePolicy::Unrestricted as i32) };
        }

        // clean up IO wrapper state if active
        if self.has_io_wrappers {
            FS_POLICY.with(|cell| {
                *cell.borrow_mut() = None;
            });
            ORIGINAL_PROCS.with(|procs| {
                for p in procs {
                    p.set(std::ptr::null_mut());
                }
            });
        }

        // clear reader dispatch table so the next context on this thread
        // starts with a clean slate (dispatch state is thread-local in C)
        unsafe { ffi::reader_dispatch_clear() };

        // clear macro expansion hook (thread-local in C)
        unsafe { ffi::macro_expand_hook_clear(self.ctx) };

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

    // --- phase 2: restricted environments + presets ---

    #[test]
    fn test_arithmetic_only_env() {
        let ctx = Context::builder()
            .preset(&crate::sandbox::ARITHMETIC)
            .build()
            .expect("builder");
        let result = ctx.evaluate("(+ 1 2)").expect("should work");
        assert_eq!(result, Value::Integer(3));
        let err = ctx.evaluate("(cons 1 2)").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "cons should produce SandboxViolation in arithmetic-only env, got: {:?}",
            err
        );
    }

    #[test]
    fn test_syntax_forms_always_available() {
        let ctx = Context::builder()
            .preset(&crate::sandbox::ARITHMETIC)
            .build()
            .expect("builder");
        let result = ctx
            .evaluate("(define x 5) (if #t (+ x 1) 0)")
            .expect("should work");
        assert_eq!(result, Value::Integer(6));

        let result = ctx
            .evaluate("((lambda (a b) (+ a b)) 3 4)")
            .expect("lambda");
        assert_eq!(result, Value::Integer(7));

        let result = ctx.evaluate("(begin (+ 1 1) (+ 2 2))").expect("begin");
        assert_eq!(result, Value::Integer(4));

        let result = ctx.evaluate("(quote hello)").expect("quote");
        assert_eq!(result, Value::Symbol("hello".into()));
    }

    #[test]
    fn test_preset_composition() {
        let ctx = Context::builder()
            .preset(&crate::sandbox::ARITHMETIC)
            .preset(&crate::sandbox::LISTS)
            .build()
            .expect("builder");
        let result = ctx.evaluate("(+ 1 2)").expect("arithmetic");
        assert_eq!(result, Value::Integer(3));
        let result = ctx.evaluate("(car (cons 1 2))").expect("lists");
        assert_eq!(result, Value::Integer(1));
    }

    #[test]
    fn test_allow_individual_primitives() {
        let ctx = Context::builder()
            .allow(&["+", "-"])
            .build()
            .expect("builder");
        let result = ctx.evaluate("(+ 10 (- 5 3))").expect("should work");
        assert_eq!(result, Value::Integer(12));
        let err = ctx.evaluate("(* 2 3)");
        assert!(err.is_err(), "* should be undefined");
    }

    #[test]
    fn test_no_preset_full_env() {
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
    fn test_pure_computation_convenience() {
        let ctx = Context::builder()
            .pure_computation()
            .build()
            .expect("builder");
        let r = ctx.evaluate("(+ 1 2)").expect("arithmetic");
        assert_eq!(r, Value::Integer(3));
        let r = ctx.evaluate("(car (cons 1 2))").expect("lists");
        assert_eq!(r, Value::Integer(1));
        let r = ctx.evaluate("(string? \"hello\")").expect("strings");
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_safe_convenience() {
        let ctx = Context::builder().safe().build().expect("builder");
        let r = ctx
            .evaluate("(define x (cons 1 2)) (set-car! x 99) (car x)")
            .expect("should work");
        assert_eq!(r, Value::Integer(99));
    }

    #[test]
    fn test_foreign_fn_works_in_restricted_env() {
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
            .preset(&crate::sandbox::ARITHMETIC)
            .build()
            .expect("builder");
        ctx.define_fn_variadic("add100", add100).expect("define fn");
        let result = ctx.evaluate("(add100 5)").expect("should work");
        assert_eq!(result, Value::Integer(105));
    }

    #[test]
    fn test_file_io_absent_in_safe_preset() {
        let ctx = Context::builder().safe().build().expect("builder");
        let err = ctx.evaluate("(open-input-file \"/etc/passwd\")");
        assert!(err.is_err(), "file io should be unavailable in safe preset");
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

    // --- phase 4: parameterised IO presets ---
    //
    // IO tests use thread-local state (FS_POLICY, ORIGINAL_PROCS) so they
    // must not run concurrently on the same thread. we use a mutex to
    // serialise them.

    static IO_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// helper: create a temp directory with a known prefix for IO tests
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
            .safe()
            .file_read(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        let code = format!(
            r#"(define p (open-input-file "{}")) (define r (read p)) (close-input-port p) r"#,
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
            .safe()
            .file_read(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        let err = ctx
            .evaluate("(open-input-file \"/etc/passwd\")")
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
            .safe()
            .file_write(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        let code = format!(
            r#"(define p (open-output-file "{}")) (write-char #\X p) (close-output-port p)"#,
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
            .safe()
            .file_write(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        let err = ctx.evaluate("(open-output-file \"/tmp/tein-io-test-nope.txt\")");
        assert!(err.is_err(), "write to unallowed path should be denied");
    }

    #[test]
    fn test_file_read_path_traversal() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let dir = io_test_dir("traversal");
        let canon_dir = dir.canonicalize().unwrap();

        let ctx = Context::builder()
            .safe()
            .file_read(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        // try to escape via ../ — canonicalisation should catch this
        let evil_path = format!("{}/../../../etc/passwd", dir.display());
        let code = format!(r#"(open-input-file "{}")"#, evil_path);
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
                .safe()
                .file_read(&[canon_dir.to_str().unwrap()])
                .build()
                .expect("builder");

            // the symlink points outside the allowed prefix, so should be denied
            let code = format!(r#"(open-input-file "{}")"#, link.display());
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
            .safe()
            .file_write(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        let code = format!(
            r#"(define p (open-output-file "{}")) (write-char #\Y p) (close-output-port p)"#,
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
            .safe()
            .file_write(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        // parent dir doesn't exist, so check_write fails (can't canonicalise parent)
        let code = format!(
            r#"(open-output-file "{}/nonexistent_subdir/file.txt")"#,
            dir.display()
        );
        let err = ctx.evaluate(&code);
        assert!(err.is_err(), "write with non-existent parent should fail");
    }

    #[test]
    fn test_file_read_without_policy() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        // safe preset without file_read — open-input-file should be absent
        let ctx = Context::builder().safe().build().expect("builder");
        let err = ctx.evaluate("(open-input-file \"/etc/passwd\")");
        assert!(
            err.is_err(),
            "open-input-file should be undefined without file_read()"
        );
    }

    #[test]
    fn test_file_write_without_policy() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        // safe preset without file_write — open-output-file should be absent
        let ctx = Context::builder().safe().build().expect("builder");
        let err = ctx.evaluate("(open-output-file \"/tmp/nope.txt\")");
        assert!(
            err.is_err(),
            "open-output-file should be undefined without file_write()"
        );
    }

    #[test]
    fn test_file_io_with_safe_preset() {
        let _lock = IO_TEST_LOCK.lock().unwrap();
        // .safe().file_read() should compose correctly
        let dir = io_test_dir("safe_compose");
        let file = dir.join("data.txt");
        std::fs::write(&file, "42").expect("write");
        let canon_dir = dir.canonicalize().unwrap();

        let ctx = Context::builder()
            .safe()
            .file_read(&[canon_dir.to_str().unwrap()])
            .build()
            .expect("builder");

        // arithmetic still works
        let r = ctx.evaluate("(+ 1 2)").expect("arithmetic");
        assert_eq!(r, Value::Integer(3));

        // mutation still works
        let r = ctx
            .evaluate("(define x (cons 1 2)) (set-car! x 99) (car x)")
            .expect("mutation");
        assert_eq!(r, Value::Integer(99));

        // file read works
        let code = format!(
            r#"(define p (open-input-file "{}")) (read p)"#,
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
            .safe()
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
        // standard_env + presets: sandbox copies from the enriched standard env,
        // but only bindings explicitly in the allowlist are available.
        // "map" and "for-each" come from (scheme base), not from C primitives,
        // so they must be explicitly allowed.
        use crate::sandbox::*;
        let ctx = Context::builder()
            .standard_env()
            .preset(&ARITHMETIC)
            .preset(&LISTS)
            .allow(&["map", "for-each"])
            .build()
            .expect("standard + sandbox");

        // map is allowed and comes from the standard env
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

        // for-each works too (side-effect only, returns void)
        // use define instead of let since for-each's closure sees the
        // restricted env, and define creates top-level bindings.
        ctx.evaluate("(define sandbox-sum 0)").expect("define");
        ctx.evaluate("(for-each (lambda (x) (set! sandbox-sum (+ sandbox-sum x))) '(1 2 3))")
            .expect("for-each in sandbox");
        let r = ctx.evaluate("sandbox-sum").expect("read sum");
        assert_eq!(r, Value::Integer(6));

        // display is NOT in the allowlist — should be blocked
        let err = ctx.evaluate("(display 42)");
        assert!(err.is_err(), "display should be blocked by sandbox");
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

    // --- module policy ---

    #[test]
    fn test_module_policy_blocks_non_vfs() {
        // a sandboxed standard-env context should activate VfsOnly policy,
        // blocking attempts to import filesystem-based modules.
        use crate::sandbox::*;
        let ctx = Context::builder()
            .standard_env()
            .preset(&ARITHMETIC)
            .build()
            .expect("standard + sandbox");

        MODULE_POLICY.with(|cell| {
            assert_eq!(
                cell.get(),
                ModulePolicy::VfsOnly,
                "sandboxed standard env should activate VfsOnly policy"
            );
        });

        drop(ctx);
    }

    #[test]
    fn test_module_policy_unrestricted_without_sandbox() {
        // standard env without sandbox should leave module policy unrestricted
        let ctx = Context::new_standard().expect("new_standard");

        MODULE_POLICY.with(|cell| {
            assert_eq!(
                cell.get(),
                ModulePolicy::Unrestricted,
                "unsandboxed standard env should be unrestricted"
            );
        });

        drop(ctx);
    }

    #[test]
    fn test_module_policy_cleared_on_drop() {
        use crate::sandbox::*;
        {
            let _ctx = Context::builder()
                .standard_env()
                .preset(&ARITHMETIC)
                .build()
                .expect("standard + sandbox");

            MODULE_POLICY.with(|cell| {
                assert_eq!(cell.get(), ModulePolicy::VfsOnly);
            });
        }
        // after drop, policy should reset
        MODULE_POLICY.with(|cell| {
            assert_eq!(
                cell.get(),
                ModulePolicy::Unrestricted,
                "module policy should reset to unrestricted after context drop"
            );
        });
    }

    #[test]
    fn test_module_policy_not_set_without_standard_env() {
        // sandbox without standard_env should NOT activate module policy
        // (there's no module system to restrict)
        use crate::sandbox::*;
        let ctx = Context::builder()
            .preset(&ARITHMETIC)
            .build()
            .expect("sandbox without standard env");

        MODULE_POLICY.with(|cell| {
            assert_eq!(
                cell.get(),
                ModulePolicy::Unrestricted,
                "non-standard-env sandbox should not set module policy"
            );
        });

        drop(ctx);
    }

    #[test]
    fn test_module_policy_blocks_filesystem_import() {
        // sandboxed standard-env contexts with import allowed should block
        // filesystem-based modules like (chibi process) via VfsOnly policy
        // while still allowing VFS-based imports like (scheme write).
        use crate::sandbox::*;
        let ctx = Context::builder()
            .standard_env()
            .preset(&ARITHMETIC)
            .allow(&["import"])
            .build()
            .expect("standard + sandbox");

        // VFS import should succeed
        let r = ctx.evaluate("(import (scheme write))");
        assert!(
            r.is_ok(),
            "(import (scheme write)) should succeed under VfsOnly: {:?}",
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
        // sandboxed standard-env contexts with import allowed should be able
        // to import VFS modules and use their bindings at runtime.
        use crate::sandbox::*;
        let ctx = Context::builder()
            .standard_env()
            .preset(&ARITHMETIC)
            .allow(&["import"])
            .build()
            .expect("standard + sandbox");

        // import scheme write — VFS module with dependencies (srfi 38, etc.)
        let r = ctx.evaluate("(import (scheme write))");
        assert!(r.is_ok(), "(import (scheme write)) failed: {:?}", r.err());

        // verify imported binding works — display returns void, write returns void
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
        use crate::sandbox::*;
        let _lock = IO_TEST_LOCK.lock().unwrap();
        let ctx = Context::builder()
            .standard_env()
            .preset(&ARITHMETIC)
            .file_read(&["/allowed/"])
            .build()
            .expect("builder");

        let err = ctx
            .evaluate("(open-input-file \"/etc/passwd\")")
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
        use crate::sandbox::*;
        let ctx = Context::builder()
            .standard_env()
            .preset(&ARITHMETIC)
            .allow(&["import"])
            .build()
            .expect("standard + sandbox");

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
    fn test_sandbox_stub_binding_violation() {
        // arithmetic-only context should have stubs for known non-allowed primitives
        use crate::sandbox::*;
        let ctx = Context::builder()
            .preset(&ARITHMETIC)
            .build()
            .expect("builder");

        // cons is in LISTS preset, not allowed — should produce SandboxViolation
        let err = ctx.evaluate("(cons 1 2)").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "expected SandboxViolation for stubbed binding, got: {:?}",
            err
        );
        let msg = format!("{}", err);
        assert!(
            msg.contains("not available in this sandbox"),
            "expected 'not available in this sandbox', got: {}",
            msg
        );
        assert!(
            msg.contains("cons"),
            "expected stub message to name 'cons', got: {}",
            msg
        );
    }

    #[test]
    fn test_sandbox_stub_does_not_shadow_allowed() {
        // allowed primitives should work normally, not be replaced by stubs
        use crate::sandbox::*;
        let ctx = Context::builder()
            .preset(&ARITHMETIC)
            .preset(&LISTS)
            .build()
            .expect("builder");

        let result = ctx.evaluate("(cons 1 2)").expect("cons should work");
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

    /// register TestCounter and a scheme-callable constructor (make-test-counter)
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
        // reader dispatch fns are registered in standard env by build(),
        // available directly or via (import (tein reader)).
        // handler returns a self-evaluating value (number).
        ctx.evaluate("(set-reader! #\\j (lambda (port) 42))")
            .expect("set-reader");
        let result = ctx.evaluate("#j").expect("eval #j");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_reader_dispatch_reserved_char() {
        let ctx = Context::new_standard().expect("context");
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
        ctx.evaluate("(set-reader! #\\j (lambda (port) 42))")
            .expect("set");
        assert_eq!(ctx.evaluate("#j").unwrap(), Value::Integer(42));
        ctx.evaluate("(unset-reader! #\\j)").expect("unset");
        assert!(ctx.evaluate("#j").is_err());
    }

    #[test]
    fn test_reader_dispatch_chars_introspection() {
        let ctx = Context::new_standard().expect("context");
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
        // verify (import (tein reader)) works for sandboxed contexts
        // that need explicit import
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein reader))").expect("import");
        ctx.evaluate("(set-reader! #\\j (lambda (port) 42))")
            .expect("set-reader");
        assert_eq!(ctx.evaluate("#j").unwrap(), Value::Integer(42));
    }

    #[test]
    fn test_register_reader_from_rust() {
        let ctx = Context::new_standard().expect("context");
        let handler = ctx.evaluate("(lambda (port) 42)").expect("handler");
        ctx.register_reader('j', &handler).expect("register");
        let result = ctx.evaluate("#j").expect("eval");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_register_reader_reserved_from_rust() {
        let ctx = Context::new_standard().expect("context");
        let handler = ctx.evaluate("(lambda (port) 42)").expect("handler");
        let err = ctx.register_reader('t', &handler).unwrap_err();
        assert!(format!("{}", err).contains("reserved"));
    }

    // --- macro expansion hook tests ---

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
            ctx.evaluate(
                "(set-macro-expand-hook!
                   (lambda (name unexpanded expanded env) expanded))",
            )
            .expect("set");
        }
        let ctx2 = Context::new_standard().expect("context2");
        let hook = ctx2.evaluate("(macro-expand-hook)").expect("get");
        assert_eq!(hook, Value::Boolean(false));
    }

    #[test]
    fn test_macro_expand_hook_sandbox() {
        // sandboxed context with macro hook fns allowed directly.
        // VFS module re-export doesn't work in sandboxes (see #31),
        // so we allow the native fns via the sandbox allowlist instead.
        use crate::sandbox::*;
        let ctx = Context::builder()
            .standard_env()
            .preset(&ARITHMETIC)
            .allow(&[
                "define-syntax",
                "syntax-rules",
                "set!",
                "define",
                "set-macro-expand-hook!",
                "unset-macro-expand-hook!",
                "macro-expand-hook",
            ])
            .build()
            .expect("sandboxed context");
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
}
