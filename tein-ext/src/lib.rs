//! C ABI type definitions for tein extension modules.
//!
//! This crate defines the stable interface between the tein host and
//! dynamically loaded cdylib extensions. Only C-compatible types cross
//! the boundary — no rust ABI fragility.
//!
//! Extension crates depend on this crate (and `tein-macros`), but never
//! on `tein` itself. The `#[tein_module("name", ext = true)]` macro
//! generates all the plumbing.

use std::ffi::{c_char, c_int, c_long, c_void};

// ── opaque pointer types ─────────────────────────────────────────────────────

/// Opaque chibi-scheme context pointer.
///
/// Never dereferenced by extension code — only received from the host
/// and passed back through API calls.
#[repr(C)]
pub struct OpaqueCtx {
    _private: [u8; 0],
}

/// Opaque chibi-scheme value pointer (sexp).
///
/// Never dereferenced by extension code — only received from the host
/// and passed back through API calls. In chibi's type system, both
/// context and value are `sexp`; we use separate types to prevent
/// accidental misuse.
#[repr(C)]
pub struct OpaqueVal {
    _private: [u8; 0],
}

// ── API version ──────────────────────────────────────────────────────────────

/// Current API version. Checked by extensions at init time.
/// Bump when adding fields to `TeinExtApi`.
pub const TEIN_EXT_API_VERSION: u32 = 2;

// ── error codes ──────────────────────────────────────────────────────────────

/// Extension initialised successfully.
pub const TEIN_EXT_OK: i32 = 0;

/// API version mismatch — extension requires a newer host.
pub const TEIN_EXT_ERR_VERSION: i32 = -1;

/// Extension-defined initialisation error.
pub const TEIN_EXT_ERR_INIT: i32 = -2;

// ── function type aliases ────────────────────────────────────────────────────

/// Variadic scheme function signature — same ABI as chibi's native fns.
///
/// `ctx`, `self_`, and `args` are all `sexp` in chibi's type system
/// (hence `*mut OpaqueVal`, not `*mut OpaqueCtx`).
pub type SexpFn = unsafe extern "C" fn(
    ctx: *mut OpaqueVal,
    self_: *mut OpaqueVal,
    n: c_long,
    args: *mut OpaqueVal,
) -> *mut OpaqueVal;

/// Foreign type method function pointer.
///
/// - `obj`: pointer to the rust object (`&mut T` as `*mut c_void`)
/// - `ctx`: opaque context for value construction
/// - `api`: API table for calling host primitives
/// - `n`: argument count
/// - `args`: scheme argument list (sexp)
pub type TeinMethodFn = unsafe extern "C" fn(
    obj: *mut c_void,
    ctx: *mut OpaqueCtx,
    api: *const TeinExtApi,
    n: c_long,
    args: *mut OpaqueVal,
) -> *mut OpaqueVal;

/// Extension init entry point. Every cdylib extension exports this symbol.
pub type TeinExtInitFn = unsafe extern "C" fn(ctx: *mut OpaqueCtx, api: *const TeinExtApi) -> i32;

// ── foreign type descriptors ─────────────────────────────────────────────────

/// Describes a foreign type for registration across the C boundary.
///
/// Emitted as a `static` by the macro in cdylib extensions.
#[repr(C)]
pub struct TeinTypeDesc {
    /// Type name as UTF-8 (not null-terminated).
    pub type_name: *const c_char,
    /// Length of `type_name` in bytes.
    pub type_name_len: usize,
    /// Pointer to array of method descriptors.
    pub methods: *const TeinMethodDesc,
    /// Number of methods.
    pub method_count: usize,
}

// Safety: TeinTypeDesc only contains raw pointers to static data emitted by
// the macro. The pointers are valid for the process lifetime and never mutated.
unsafe impl Send for TeinTypeDesc {}
unsafe impl Sync for TeinTypeDesc {}

/// Describes a single method on a foreign type.
#[repr(C)]
pub struct TeinMethodDesc {
    /// Method name as UTF-8 (not null-terminated).
    pub name: *const c_char,
    /// Length of `name` in bytes.
    pub name_len: usize,
    /// The method function pointer.
    pub func: TeinMethodFn,
    /// Whether the method requires mutable access to the object.
    pub is_mut: bool,
}

// Safety: same as TeinTypeDesc — static data, never mutated.
unsafe impl Send for TeinMethodDesc {}
unsafe impl Sync for TeinMethodDesc {}

// ── the API vtable ───────────────────────────────────────────────────────────

/// Stable C ABI function pointer table, populated by the tein host and
/// passed to extensions at init time.
///
/// Versioned and append-only — new fields go at the end. Extensions
/// check `version` before accessing any field.
///
/// Function signatures mirror `tein::raw::*` but use opaque pointer
/// types. The host fills each slot with a trampoline that casts back
/// to `sexp` and calls the real chibi function.
#[repr(C)]
pub struct TeinExtApi {
    /// API version — must be >= `TEIN_EXT_API_VERSION`.
    pub version: u32,

    // ── high-level registration ──────────────────────────────────────
    /// Register a VFS module entry (path + content).
    ///
    /// Path and content are UTF-8, not null-terminated, with explicit
    /// lengths. Returns 0 on success, negative on error.
    pub register_vfs_module: unsafe extern "C" fn(
        ctx: *mut OpaqueCtx,
        path: *const c_char,
        path_len: usize,
        content: *const c_char,
        content_len: usize,
    ) -> i32,

    /// Register a variadic scheme function.
    ///
    /// Name is UTF-8, not null-terminated. Returns 0 on success.
    pub define_fn_variadic: unsafe extern "C" fn(
        ctx: *mut OpaqueCtx,
        name: *const c_char,
        name_len: usize,
        f: SexpFn,
    ) -> i32,

    /// Register a foreign type from a `TeinTypeDesc`.
    ///
    /// Returns 0 on success, negative on error.
    pub register_foreign_type:
        unsafe extern "C" fn(ctx: *mut OpaqueCtx, desc: *const TeinTypeDesc) -> i32,

    // ── type predicates ──────────────────────────────────────────────
    // Return nonzero if the value matches the type, 0 otherwise.
    pub sexp_integerp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_flonump: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_stringp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_booleanp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_symbolp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_pairp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_nullp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_charp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_bytesp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_vectorp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_portp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_exceptionp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,

    // ── value extractors ─────────────────────────────────────────────
    /// Extract fixnum (integer) value. Caller must check `sexp_integerp` first.
    pub sexp_unbox_fixnum: unsafe extern "C" fn(*mut OpaqueVal) -> c_long,

    /// Extract flonum (f64) value. Caller must check `sexp_flonump` first.
    pub sexp_flonum_value: unsafe extern "C" fn(*mut OpaqueVal) -> f64,

    /// Get string data pointer. Caller must check `sexp_stringp` first.
    /// Returned pointer is valid for the lifetime of the sexp.
    pub sexp_string_data: unsafe extern "C" fn(*mut OpaqueVal) -> *const c_char,

    /// Get string byte length. Caller must check `sexp_stringp` first.
    pub sexp_string_size: unsafe extern "C" fn(*mut OpaqueVal) -> c_long,

    /// Extract character codepoint. Caller must check `sexp_charp` first.
    pub sexp_unbox_character: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,

    /// Get bytevector data pointer. Caller must check `sexp_bytesp` first.
    pub sexp_bytes_data: unsafe extern "C" fn(*mut OpaqueVal) -> *const c_char,

    /// Get bytevector length. Caller must check `sexp_bytesp` first.
    pub sexp_bytes_length: unsafe extern "C" fn(*mut OpaqueVal) -> c_long,

    /// Get car of a pair. Caller must check `sexp_pairp` first.
    pub sexp_car: unsafe extern "C" fn(*mut OpaqueVal) -> *mut OpaqueVal,

    /// Get cdr of a pair. Caller must check `sexp_pairp` first.
    pub sexp_cdr: unsafe extern "C" fn(*mut OpaqueVal) -> *mut OpaqueVal,

    // ── value constructors ───────────────────────────────────────────
    /// Create a fixnum (integer). Does not allocate.
    pub sexp_make_fixnum: unsafe extern "C" fn(c_long) -> *mut OpaqueVal,

    /// Create a flonum (f64). Allocates on the scheme heap.
    pub sexp_make_flonum: unsafe extern "C" fn(*mut OpaqueCtx, f64) -> *mut OpaqueVal,

    /// Create a boolean. Does not allocate.
    pub sexp_make_boolean: unsafe extern "C" fn(c_int) -> *mut OpaqueVal,

    /// Create a character. Does not allocate.
    pub sexp_make_character: unsafe extern "C" fn(c_int) -> *mut OpaqueVal,

    /// Create a scheme string from UTF-8 data. Allocates.
    /// `len` is byte length; pass -1 for null-terminated C strings.
    pub sexp_c_str: unsafe extern "C" fn(*mut OpaqueCtx, *const c_char, c_long) -> *mut OpaqueVal,

    /// Construct a pair (cons cell). Allocates.
    pub sexp_cons:
        unsafe extern "C" fn(*mut OpaqueCtx, *mut OpaqueVal, *mut OpaqueVal) -> *mut OpaqueVal,

    /// Create a bytevector of given length, filled with `init`. Allocates.
    pub sexp_make_bytes: unsafe extern "C" fn(*mut OpaqueCtx, c_long, u8) -> *mut OpaqueVal,

    /// Create a scheme exception (error object) from a message string.
    /// Equivalent to `(error msg)` in scheme. Catchable by `guard`.
    /// `len` is byte length; pass -1 for null-terminated C strings.
    pub make_error: unsafe extern "C" fn(*mut OpaqueCtx, *const c_char, c_long) -> *mut OpaqueVal,

    // ── sentinels ────────────────────────────────────────────────────
    // These return the canonical singleton values. Do not allocate.
    pub get_null: unsafe extern "C" fn() -> *mut OpaqueVal,
    pub get_true: unsafe extern "C" fn() -> *mut OpaqueVal,
    pub get_false: unsafe extern "C" fn() -> *mut OpaqueVal,
    pub get_void: unsafe extern "C" fn() -> *mut OpaqueVal,
}
