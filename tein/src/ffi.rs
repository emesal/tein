//! raw ffi bindings to chibi-scheme c api
//!
//! this module contains unsafe bindings to the underlying chibi-scheme library.
//! users should prefer the safe wrappers in the parent modules.
//!
//! # Safety
//!
//! All functions in this module are `unsafe` and require:
//! - `sexp` pointers must be valid, non-null pointers obtained from chibi-scheme
//! - `ctx` (context) pointers must be live and not yet destroyed
//! - String pointers (`*const c_char`) must be valid null-terminated C strings
//! - Calling functions on invalid or destroyed sexp values is undefined behavior
//! - The caller must ensure proper memory management across the FFI boundary
//!
//! # Intentionally omitted
//!
//! `sexp_register_type` / `sexp_register_simple_type` are NOT exposed here.
//! chibi's C-level type registration ties into the GC finaliser system which
//! has known bugs (M19-M21 in chibi-scheme-review.md). tein's `ForeignType`
//! protocol stores objects rust-side, avoiding these issues entirely.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(missing_docs)]
#![allow(clippy::missing_safety_doc)]

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};

// opaque types from chibi
pub type sexp = *mut c_void;
pub type sexp_sint_t = c_long;
pub type sexp_uint_t = c_ulong;

unsafe extern "C" {
    // context management
    pub fn sexp_make_eval_context(
        context: sexp,
        stack: sexp,
        env: sexp,
        size: sexp_uint_t,
        max_size: sexp_uint_t,
    ) -> sexp;

    pub fn sexp_destroy_context(ctx: sexp);

    // evaluation
    pub fn sexp_eval_string(ctx: sexp, str: *const c_char, len: sexp_sint_t, env: sexp) -> sexp;

    // version param is a tagged fixnum (sexp), not sexp_uint_t
    pub fn sexp_load_standard_env(ctx: sexp, env: sexp, version: sexp) -> sexp;

    // standard ports (via tein shim — wraps sexp_load_standard_ports with stdin/stdout/stderr)
    pub fn tein_sexp_load_standard_ports(ctx: sexp, env: sexp) -> sexp;

    // copy a named binding from one env to another (searches direct + rename bindings).
    // returns 1 if found and copied, 0 if not found.
    pub fn tein_env_copy_named(
        ctx: sexp,
        src_env: sexp,
        dst_env: sexp,
        name: *const c_char,
        name_len: sexp_sint_t,
    ) -> c_int;

    // character operations (via tein shim)
    pub fn tein_sexp_charp(x: sexp) -> c_int;
    pub fn tein_sexp_unbox_character(x: sexp) -> c_int;
    pub fn tein_sexp_make_character(n: c_int) -> sexp;

    // bytevector operations (via tein shim)
    pub fn tein_sexp_bytesp(x: sexp) -> c_int;
    pub fn tein_sexp_bytes_data(x: sexp) -> *mut c_char;
    pub fn tein_sexp_bytes_length(x: sexp) -> sexp_uint_t;
    pub fn tein_sexp_make_bytes(ctx: sexp, len: sexp_uint_t, init: c_uchar) -> sexp;

    // numeric tower operations (via tein shim)
    pub fn tein_sexp_bignump(x: sexp) -> c_int;
    pub fn tein_sexp_ratiop(x: sexp) -> c_int;
    pub fn tein_sexp_complexp(x: sexp) -> c_int;
    pub fn tein_sexp_bignum_sign(x: sexp) -> c_int;
    pub fn tein_sexp_bignum_to_string(ctx: sexp, x: sexp) -> sexp;
    pub fn tein_sexp_ratio_numerator(x: sexp) -> sexp;
    pub fn tein_sexp_ratio_denominator(x: sexp) -> sexp;
    pub fn tein_sexp_complex_real(x: sexp) -> sexp;
    pub fn tein_sexp_complex_imag(x: sexp) -> sexp;
    pub fn tein_sexp_string_to_number(ctx: sexp, str: sexp, base: c_int) -> sexp;
    pub fn tein_sexp_make_ratio(ctx: sexp, num: sexp, den: sexp) -> sexp;
    pub fn tein_sexp_make_complex(ctx: sexp, real: sexp, imag: sexp) -> sexp;

    // port operations (via tein shim)
    pub fn tein_sexp_portp(x: sexp) -> c_int;
    pub fn tein_sexp_iportp(x: sexp) -> c_int;
    pub fn tein_sexp_oportp(x: sexp) -> c_int;

    // type checking (via tein shim)
    pub fn tein_sexp_integerp(x: sexp) -> c_int;
    pub fn tein_sexp_flonump(x: sexp) -> c_int;
    pub fn tein_sexp_stringp(x: sexp) -> c_int;
    pub fn tein_sexp_symbolp(x: sexp) -> c_int;
    pub fn tein_sexp_booleanp(x: sexp) -> c_int;
    pub fn tein_sexp_pairp(x: sexp) -> c_int;
    pub fn tein_sexp_nullp(x: sexp) -> c_int;
    pub fn tein_sexp_exceptionp(x: sexp) -> c_int;

    // value extraction (via tein shim)
    pub fn tein_sexp_unbox_fixnum(x: sexp) -> sexp_sint_t;
    pub fn tein_sexp_flonum_value(x: sexp) -> f64;
    pub fn tein_sexp_string_data(x: sexp) -> *const c_char;
    pub fn tein_sexp_string_size(x: sexp) -> sexp_uint_t;
    pub fn tein_sexp_symbol_to_string(ctx: sexp, x: sexp) -> sexp;

    // pair operations (via tein shim)
    pub fn tein_sexp_car(x: sexp) -> sexp;
    pub fn tein_sexp_cdr(x: sexp) -> sexp;

    // vector operations (via tein shim)
    pub fn tein_sexp_vectorp(x: sexp) -> c_int;
    pub fn tein_sexp_vector_length(x: sexp) -> sexp_uint_t;
    pub fn tein_sexp_vector_data(x: sexp) -> *mut sexp;

    // exception details (via tein shim)
    pub fn tein_sexp_exception_message(x: sexp) -> sexp;
    pub fn tein_sexp_exception_irritants(x: sexp) -> sexp;

    // value construction (via tein shim)
    pub fn tein_sexp_make_fixnum(n: sexp_sint_t) -> sexp;
    pub fn tein_sexp_make_flonum(ctx: sexp, f: f64) -> sexp;
    pub fn tein_sexp_make_boolean(b: c_int) -> sexp;
    pub fn tein_get_void() -> sexp;

    // string construction
    pub fn sexp_c_string(ctx: sexp, str: *const c_char, slen: sexp_sint_t) -> sexp;

    // foreign function registration
    pub fn tein_sexp_define_foreign(
        ctx: sexp,
        env: sexp,
        name: *const c_char,
        num_args: c_int,
        fname: *const c_char,
        f: Option<unsafe extern "C" fn(sexp, sexp, sexp_sint_t) -> sexp>,
    ) -> sexp;

    // foreign function registration (procedure-wrapped, supports variadic)
    pub fn tein_sexp_define_foreign_proc(
        ctx: sexp,
        env: sexp,
        name: *const c_char,
        num_args: c_int,
        flags: c_int,
        fname: *const c_char,
        f: Option<unsafe extern "C" fn(sexp, sexp, sexp_sint_t, sexp) -> sexp>,
    ) -> sexp;

    // interning symbols
    pub fn tein_sexp_intern(ctx: sexp, str: *const c_char, len: sexp_sint_t) -> sexp;

    // context (via tein shim)
    pub fn tein_sexp_context_env(ctx: sexp) -> sexp;

    // constants (via tein shim)
    pub fn tein_get_true() -> sexp;
    pub fn tein_get_false() -> sexp;
    pub fn tein_get_null() -> sexp;
    pub fn tein_get_eof() -> sexp;

    // multi-expression evaluation (via tein shim)
    pub fn tein_sexp_eofp(x: sexp) -> c_int;
    pub fn tein_sexp_open_input_string(ctx: sexp, str: sexp) -> sexp;
    pub fn tein_sexp_read(ctx: sexp, port: sexp) -> sexp;
    pub fn tein_sexp_evaluate(ctx: sexp, obj: sexp, env: sexp) -> sexp;

    // gc preservation for rust-side references (via tein shim)
    pub fn tein_sexp_preserve_object(ctx: sexp, x: sexp);
    pub fn tein_sexp_release_object(ctx: sexp, x: sexp);

    // procedure/application support (via tein shim)
    pub fn tein_sexp_procedurep(x: sexp) -> c_int;
    pub fn tein_sexp_opcodep(x: sexp) -> c_int;
    pub fn tein_sexp_opcode_name(op: sexp) -> sexp;
    pub fn tein_sexp_applicablep(x: sexp) -> c_int;

    // procedure application (chibi SEXP_API — not a macro)
    pub fn sexp_apply(ctx: sexp, proc: sexp, args: sexp) -> sexp;

    // fuel control (step limiting)
    pub fn tein_fuel_arm(ctx: sexp, total_fuel: sexp_sint_t);
    pub fn tein_fuel_disarm(ctx: sexp);
    pub fn tein_fuel_exhausted(ctx: sexp) -> c_int;

    // environment manipulation (sandboxing)
    pub fn tein_sexp_make_null_env(ctx: sexp, version: sexp) -> sexp;
    pub fn tein_sexp_make_primitive_env(ctx: sexp, version: sexp) -> sexp;
    pub fn tein_sexp_env_define(ctx: sexp, env: sexp, sym: sexp, val: sexp) -> sexp;
    pub fn tein_sexp_env_ref(ctx: sexp, env: sexp, sym: sexp, dflt: sexp) -> sexp;
    pub fn tein_sexp_context_env_set(ctx: sexp, env: sexp);

    // error construction (for policy violation exceptions)
    pub fn tein_make_error(ctx: sexp, msg: *const c_char, len: sexp_sint_t) -> sexp;

    // module import policy (for sandboxed standard env)
    pub fn tein_module_policy_set(policy: c_int);

    // pair/list construction (via tein shim)
    pub fn tein_sexp_cons(ctx: sexp, head: sexp, tail: sexp) -> sexp;

    // vector construction (via tein shim)
    pub fn tein_sexp_make_vector(ctx: sexp, len: sexp_uint_t, dflt: sexp) -> sexp;
    pub fn tein_sexp_vector_set(vec: sexp, i: sexp_uint_t, val: sexp);

    // custom port creation (via tein shim → chibi io lib)
    pub fn tein_make_custom_input_port(ctx: sexp, read_proc: sexp) -> sexp;
    pub fn tein_make_custom_output_port(ctx: sexp, write_proc: sexp) -> sexp;

    // reader dispatch table (# syntax extensions)
    pub fn tein_reader_dispatch_set(ctx: sexp, c: c_int, proc: sexp) -> c_int;
    pub fn tein_reader_dispatch_unset(ctx: sexp, c: c_int) -> c_int;
    pub fn tein_reader_dispatch_get(c: c_int) -> sexp;
    pub fn tein_reader_dispatch_chars(ctx: sexp) -> sexp;
    pub fn tein_reader_dispatch_clear(ctx: sexp);
    pub fn tein_reader_char_is_reserved(c: c_int) -> c_int;

    // macro expansion hook
    pub fn tein_macro_expand_hook_set(ctx: sexp, proc: sexp);
    pub fn tein_macro_expand_hook_get() -> sexp;
    pub fn tein_macro_expand_hook_clear(ctx: sexp);

    // runtime VFS registration (tein_shim.c dynamic VFS table)
    pub fn tein_vfs_register(key: *const c_char, content: *const c_char, length: c_uint);
    pub fn tein_vfs_clear_dynamic();
}

// convenience wrappers that call our shim layer
#[inline]
pub unsafe fn sexp_integerp(x: sexp) -> c_int {
    unsafe { tein_sexp_integerp(x) }
}

#[inline]
pub unsafe fn sexp_flonump(x: sexp) -> c_int {
    unsafe { tein_sexp_flonump(x) }
}

#[inline]
pub unsafe fn sexp_stringp(x: sexp) -> c_int {
    unsafe { tein_sexp_stringp(x) }
}

#[inline]
pub unsafe fn sexp_symbolp(x: sexp) -> c_int {
    unsafe { tein_sexp_symbolp(x) }
}

#[inline]
pub unsafe fn sexp_booleanp(x: sexp) -> c_int {
    unsafe { tein_sexp_booleanp(x) }
}

#[inline]
pub unsafe fn sexp_pairp(x: sexp) -> c_int {
    unsafe { tein_sexp_pairp(x) }
}

#[inline]
pub unsafe fn sexp_nullp(x: sexp) -> c_int {
    unsafe { tein_sexp_nullp(x) }
}

#[inline]
pub unsafe fn sexp_exceptionp(x: sexp) -> c_int {
    unsafe { tein_sexp_exceptionp(x) }
}

#[inline]
pub unsafe fn sexp_unbox_fixnum(x: sexp) -> sexp_sint_t {
    unsafe { tein_sexp_unbox_fixnum(x) }
}

#[inline]
pub unsafe fn sexp_flonum_value(x: sexp) -> f64 {
    unsafe { tein_sexp_flonum_value(x) }
}

#[inline]
pub unsafe fn sexp_string_data(x: sexp) -> *const c_char {
    unsafe { tein_sexp_string_data(x) }
}

#[inline]
pub unsafe fn sexp_string_size(x: sexp) -> sexp_uint_t {
    unsafe { tein_sexp_string_size(x) }
}

#[inline]
pub unsafe fn sexp_symbol_to_string(ctx: sexp, sym: sexp) -> sexp {
    unsafe { tein_sexp_symbol_to_string(ctx, sym) }
}

#[inline]
pub unsafe fn sexp_car(x: sexp) -> sexp {
    unsafe { tein_sexp_car(x) }
}

#[inline]
pub unsafe fn sexp_cdr(x: sexp) -> sexp {
    unsafe { tein_sexp_cdr(x) }
}

// vector operations
#[inline]
pub unsafe fn sexp_vectorp(x: sexp) -> c_int {
    unsafe { tein_sexp_vectorp(x) }
}

#[inline]
pub unsafe fn sexp_vector_length(x: sexp) -> sexp_uint_t {
    unsafe { tein_sexp_vector_length(x) }
}

#[inline]
pub unsafe fn sexp_vector_data(x: sexp) -> *mut sexp {
    unsafe { tein_sexp_vector_data(x) }
}

// character operations
#[inline]
pub unsafe fn sexp_charp(x: sexp) -> c_int {
    unsafe { tein_sexp_charp(x) }
}

#[inline]
pub unsafe fn sexp_unbox_character(x: sexp) -> c_int {
    unsafe { tein_sexp_unbox_character(x) }
}

#[inline]
pub unsafe fn sexp_make_character(n: c_int) -> sexp {
    unsafe { tein_sexp_make_character(n) }
}

// bytevector operations
#[inline]
pub unsafe fn sexp_bytesp(x: sexp) -> c_int {
    unsafe { tein_sexp_bytesp(x) }
}

#[inline]
pub unsafe fn sexp_bytes_data(x: sexp) -> *mut c_char {
    unsafe { tein_sexp_bytes_data(x) }
}

#[inline]
pub unsafe fn sexp_bytes_length(x: sexp) -> sexp_uint_t {
    unsafe { tein_sexp_bytes_length(x) }
}

#[inline]
pub unsafe fn sexp_make_bytes(ctx: sexp, len: sexp_uint_t, init: u8) -> sexp {
    unsafe { tein_sexp_make_bytes(ctx, len, init as c_uchar) }
}

// numeric tower operations

#[inline]
pub unsafe fn sexp_bignump(x: sexp) -> c_int {
    unsafe { tein_sexp_bignump(x) }
}

#[inline]
pub unsafe fn sexp_ratiop(x: sexp) -> c_int {
    unsafe { tein_sexp_ratiop(x) }
}

#[inline]
pub unsafe fn sexp_complexp(x: sexp) -> c_int {
    unsafe { tein_sexp_complexp(x) }
}

#[inline]
pub unsafe fn sexp_bignum_sign(x: sexp) -> c_int {
    unsafe { tein_sexp_bignum_sign(x) }
}

/// converts a bignum to a decimal string sexp. allocates (opens string port).
#[inline]
pub unsafe fn sexp_bignum_to_string(ctx: sexp, x: sexp) -> sexp {
    unsafe { tein_sexp_bignum_to_string(ctx, x) }
}

#[inline]
pub unsafe fn sexp_ratio_numerator(x: sexp) -> sexp {
    unsafe { tein_sexp_ratio_numerator(x) }
}

#[inline]
pub unsafe fn sexp_ratio_denominator(x: sexp) -> sexp {
    unsafe { tein_sexp_ratio_denominator(x) }
}

#[inline]
pub unsafe fn sexp_complex_real(x: sexp) -> sexp {
    unsafe { tein_sexp_complex_real(x) }
}

#[inline]
pub unsafe fn sexp_complex_imag(x: sexp) -> sexp {
    unsafe { tein_sexp_complex_imag(x) }
}

/// parses a string sexp as a number in the given base. allocates.
#[inline]
pub unsafe fn sexp_string_to_number(ctx: sexp, s: sexp, base: c_int) -> sexp {
    unsafe { tein_sexp_string_to_number(ctx, s, base) }
}

#[inline]
pub unsafe fn sexp_make_ratio(ctx: sexp, num: sexp, den: sexp) -> sexp {
    unsafe { tein_sexp_make_ratio(ctx, num, den) }
}

#[inline]
pub unsafe fn sexp_make_complex(ctx: sexp, real: sexp, imag: sexp) -> sexp {
    unsafe { tein_sexp_make_complex(ctx, real, imag) }
}

// port operations
#[inline]
pub unsafe fn sexp_portp(x: sexp) -> c_int {
    unsafe { tein_sexp_portp(x) }
}

#[inline]
pub unsafe fn sexp_iportp(x: sexp) -> c_int {
    unsafe { tein_sexp_iportp(x) }
}

#[inline]
pub unsafe fn sexp_oportp(x: sexp) -> c_int {
    unsafe { tein_sexp_oportp(x) }
}

// exception details
#[inline]
pub unsafe fn sexp_exception_message(x: sexp) -> sexp {
    unsafe { tein_sexp_exception_message(x) }
}

#[inline]
pub unsafe fn sexp_exception_irritants(x: sexp) -> sexp {
    unsafe { tein_sexp_exception_irritants(x) }
}

// value construction
#[inline]
pub unsafe fn sexp_make_fixnum(n: sexp_sint_t) -> sexp {
    unsafe { tein_sexp_make_fixnum(n) }
}

#[inline]
pub unsafe fn sexp_make_flonum(ctx: sexp, f: f64) -> sexp {
    unsafe { tein_sexp_make_flonum(ctx, f) }
}

#[inline]
pub unsafe fn sexp_make_boolean(b: bool) -> sexp {
    unsafe { tein_sexp_make_boolean(if b { 1 } else { 0 }) }
}

#[inline]
pub unsafe fn get_void() -> sexp {
    unsafe { tein_get_void() }
}

#[inline]
pub unsafe fn sexp_c_str(ctx: sexp, s: *const c_char, len: sexp_sint_t) -> sexp {
    unsafe { sexp_c_string(ctx, s, len) }
}

#[inline]
pub unsafe fn sexp_define_foreign(
    ctx: sexp,
    env: sexp,
    name: *const c_char,
    num_args: c_int,
    fname: *const c_char,
    f: Option<unsafe extern "C" fn(sexp, sexp, sexp_sint_t) -> sexp>,
) -> sexp {
    unsafe { tein_sexp_define_foreign(ctx, env, name, num_args, fname, f) }
}

#[inline]
pub unsafe fn sexp_intern(ctx: sexp, s: *const c_char, len: sexp_sint_t) -> sexp {
    unsafe { tein_sexp_intern(ctx, s, len) }
}

#[inline]
pub unsafe fn sexp_context_env(ctx: sexp) -> sexp {
    unsafe { tein_sexp_context_env(ctx) }
}

// get scheme constants
#[inline]
pub unsafe fn get_true() -> sexp {
    unsafe { tein_get_true() }
}

#[inline]
pub unsafe fn get_false() -> sexp {
    unsafe { tein_get_false() }
}

#[inline]
pub unsafe fn get_null() -> sexp {
    unsafe { tein_get_null() }
}

// foreign function registration (procedure-wrapped, supports variadic)
/// Flag indicating a variadic foreign function (rest-args).
///
/// Note: C defines this as `sexp_uint_t` (unsigned) but we use `c_int` (signed)
/// here. The value 1 is safe for both, and the `flags` field is passed through
/// `tein_sexp_define_foreign_proc` which takes `int`.
pub const SEXP_PROC_VARIADIC: c_int = 1;

#[inline]
pub unsafe fn sexp_define_foreign_proc(
    ctx: sexp,
    env: sexp,
    name: *const c_char,
    num_args: c_int,
    flags: c_int,
    fname: *const c_char,
    f: Option<unsafe extern "C" fn(sexp, sexp, sexp_sint_t, sexp) -> sexp>,
) -> sexp {
    unsafe { tein_sexp_define_foreign_proc(ctx, env, name, num_args, flags, fname, f) }
}

/// Construct a Scheme user exception with the given message.
///
/// `msg` must be a valid nul-terminated C string pointer. `len` is passed
/// through for API symmetry but is unused by the C implementation (which
/// treats `msg` as a C string). `CString::new(s).unwrap_or_default()` is
/// safe to use at all call sites because `tein_make_error` ignores `len`
/// and reads `msg` as a nul-terminated string; the empty-string fallback
/// just produces a Scheme exception with an empty message.
#[inline]
pub unsafe fn make_error(ctx: sexp, msg: *const c_char, len: sexp_sint_t) -> sexp {
    unsafe { tein_make_error(ctx, msg, len) }
}

// pair/list construction
#[inline]
pub unsafe fn sexp_cons(ctx: sexp, head: sexp, tail: sexp) -> sexp {
    unsafe { tein_sexp_cons(ctx, head, tail) }
}

// vector construction
#[inline]
pub unsafe fn sexp_make_vector(ctx: sexp, len: sexp_uint_t, dflt: sexp) -> sexp {
    unsafe { tein_sexp_make_vector(ctx, len, dflt) }
}

#[inline]
pub unsafe fn sexp_vector_set(vec: sexp, i: sexp_uint_t, val: sexp) {
    unsafe { tein_sexp_vector_set(vec, i, val) }
}

// procedure/application support
#[inline]
pub unsafe fn sexp_procedurep(x: sexp) -> c_int {
    unsafe { tein_sexp_procedurep(x) }
}

#[inline]
pub unsafe fn sexp_opcodep(x: sexp) -> c_int {
    unsafe { tein_sexp_opcodep(x) }
}

/// extract the name (scheme string) from an opcode/foreign-fn object
#[inline]
pub unsafe fn sexp_opcode_name(op: sexp) -> sexp {
    unsafe { tein_sexp_opcode_name(op) }
}

#[inline]
pub unsafe fn sexp_applicablep(x: sexp) -> c_int {
    unsafe { tein_sexp_applicablep(x) }
}

#[inline]
pub unsafe fn sexp_apply_proc(ctx: sexp, proc: sexp, args: sexp) -> sexp {
    unsafe { sexp_apply(ctx, proc, args) }
}

// eof constant
#[inline]
pub unsafe fn get_eof() -> sexp {
    unsafe { tein_get_eof() }
}

// multi-expression evaluation
#[inline]
pub unsafe fn sexp_eofp(x: sexp) -> c_int {
    unsafe { tein_sexp_eofp(x) }
}

#[inline]
pub unsafe fn sexp_open_input_string(ctx: sexp, str: sexp) -> sexp {
    unsafe { tein_sexp_open_input_string(ctx, str) }
}

#[inline]
pub unsafe fn sexp_read(ctx: sexp, port: sexp) -> sexp {
    unsafe { tein_sexp_read(ctx, port) }
}

#[inline]
pub unsafe fn sexp_evaluate(ctx: sexp, obj: sexp, env: sexp) -> sexp {
    unsafe { tein_sexp_evaluate(ctx, obj, env) }
}

/// add a sexp to the global preservatives list, preventing GC collection.
/// must be paired with `sexp_release_object` when the reference is no longer needed.
/// use this for rust-side references that survive across allocation points.
#[inline]
pub unsafe fn sexp_preserve_object(ctx: sexp, x: sexp) {
    unsafe { tein_sexp_preserve_object(ctx, x) }
}

/// remove a sexp from the global preservatives list, allowing GC collection.
#[inline]
pub unsafe fn sexp_release_object(ctx: sexp, x: sexp) {
    unsafe { tein_sexp_release_object(ctx, x) }
}

// fuel control
#[inline]
pub unsafe fn fuel_arm(ctx: sexp, total_fuel: sexp_sint_t) {
    unsafe { tein_fuel_arm(ctx, total_fuel) }
}

#[inline]
pub unsafe fn fuel_disarm(ctx: sexp) {
    unsafe { tein_fuel_disarm(ctx) }
}

#[inline]
pub unsafe fn fuel_exhausted(ctx: sexp) -> c_int {
    unsafe { tein_fuel_exhausted(ctx) }
}

// environment manipulation (sandboxing)
#[inline]
pub unsafe fn sexp_make_null_env(ctx: sexp, version: sexp) -> sexp {
    unsafe { tein_sexp_make_null_env(ctx, version) }
}

#[inline]
pub unsafe fn sexp_make_primitive_env(ctx: sexp, version: sexp) -> sexp {
    unsafe { tein_sexp_make_primitive_env(ctx, version) }
}

#[inline]
pub unsafe fn sexp_env_define(ctx: sexp, env: sexp, sym: sexp, val: sexp) -> sexp {
    unsafe { tein_sexp_env_define(ctx, env, sym, val) }
}

#[inline]
pub unsafe fn sexp_env_ref(ctx: sexp, env: sexp, sym: sexp, dflt: sexp) -> sexp {
    unsafe { tein_sexp_env_ref(ctx, env, sym, dflt) }
}

#[inline]
pub unsafe fn sexp_context_env_set(ctx: sexp, env: sexp) {
    unsafe { tein_sexp_context_env_set(ctx, env) }
}

// standard environment + ports
#[inline]
pub unsafe fn load_standard_env(ctx: sexp, env: sexp, version: sexp) -> sexp {
    unsafe { sexp_load_standard_env(ctx, env, version) }
}

#[inline]
pub unsafe fn load_standard_ports(ctx: sexp, env: sexp) -> sexp {
    unsafe { tein_sexp_load_standard_ports(ctx, env) }
}

/// set the module import policy at C level.
/// 0 = unrestricted, 1 = vfs-all, 2 = allowlist (rust callback).
#[inline]
pub unsafe fn module_policy_set(policy: i32) {
    unsafe { tein_module_policy_set(policy as c_int) }
}

/// called from C (`tein_shim.c`) when module policy is Allowlist (policy 2).
/// checks the module path against the thread-local allowlist.
///
/// the path arrives as e.g. `/vfs/lib/tein/json.sld` or `/vfs/lib/srfi/69/hash`.
/// we strip the `/vfs/lib/` prefix and check if any allowlist entry is a prefix
/// of the remainder.
#[unsafe(no_mangle)]
extern "C" fn tein_module_allowlist_check(path: *const c_char) -> c_int {
    use crate::sandbox::MODULE_ALLOWLIST;

    let path_str = unsafe { CStr::from_ptr(path) }.to_str().unwrap_or("");
    let suffix = path_str.strip_prefix("/vfs/lib/").unwrap_or(path_str);

    MODULE_ALLOWLIST.with(|cell| {
        let list = cell.borrow();
        if list.iter().any(|prefix| suffix.starts_with(prefix.as_str())) {
            1
        } else {
            0
        }
    })
}

/// RAII guard that roots a `sexp` on chibi's global preservatives list.
///
/// prevents GC from collecting the guarded object while the guard is alive.
/// calls `sexp_preserve_object` on creation and `sexp_release_object` on drop,
/// so early returns and panics are handled automatically.
///
/// # safety
///
/// `ctx` and `obj` must be valid chibi-scheme pointers.
/// `obj` must be a heap-allocated sexp (not a fixnum or other immediate value).
/// the guard must not outlive the context.
pub struct GcRoot {
    ctx: sexp,
    obj: sexp,
}

impl GcRoot {
    /// root `obj` in `ctx`'s global preservatives list.
    ///
    /// # safety
    ///
    /// `ctx` must be a live context. `obj` must be a valid heap-allocated sexp.
    #[inline]
    pub unsafe fn new(ctx: sexp, obj: sexp) -> Self {
        unsafe { sexp_preserve_object(ctx, obj) };
        Self { ctx, obj }
    }

    /// the rooted sexp pointer
    #[inline]
    pub fn get(&self) -> sexp {
        self.obj
    }
}

impl Drop for GcRoot {
    fn drop(&mut self) {
        unsafe { sexp_release_object(self.ctx, self.obj) };
    }
}

/// create a custom input port backed by a scheme read procedure.
#[inline]
pub unsafe fn make_custom_input_port(ctx: sexp, read_proc: sexp) -> sexp {
    unsafe { tein_make_custom_input_port(ctx, read_proc) }
}

/// create a custom output port backed by a scheme write procedure.
#[inline]
pub unsafe fn make_custom_output_port(ctx: sexp, write_proc: sexp) -> sexp {
    unsafe { tein_make_custom_output_port(ctx, write_proc) }
}

/// copy a named binding from src_env to dst_env, searching both direct
/// bindings and rename bindings (module system). returns true if found.
#[inline]
pub unsafe fn env_copy_named(
    ctx: sexp,
    src_env: sexp,
    dst_env: sexp,
    name: *const std::os::raw::c_char,
    name_len: sexp_sint_t,
) -> bool {
    unsafe { tein_env_copy_named(ctx, src_env, dst_env, name, name_len) != 0 }
}

// --- reader dispatch table ---

/// register a reader dispatch handler for `#c` syntax.
///
/// the handler proc is GC-preserved in the dispatch table and released
/// when overwritten, unset, or cleared. returns 0 on success, -1 if the
/// character is reserved, -2 if out of range.
#[inline]
pub unsafe fn reader_dispatch_set(ctx: sexp, c: c_int, proc: sexp) -> c_int {
    unsafe { tein_reader_dispatch_set(ctx, c, proc) }
}

/// remove a reader dispatch handler for `#c` syntax.
///
/// releases the GC-preserved handler. returns 0 on success, -2 if out of range.
#[inline]
pub unsafe fn reader_dispatch_unset(ctx: sexp, c: c_int) -> c_int {
    unsafe { tein_reader_dispatch_unset(ctx, c) }
}

/// get the reader dispatch handler for `#c`, or SEXP_FALSE if none.
#[inline]
pub unsafe fn reader_dispatch_get(c: c_int) -> sexp {
    unsafe { tein_reader_dispatch_get(c) }
}

/// return a list of characters with active dispatch handlers.
#[inline]
pub unsafe fn reader_dispatch_chars(ctx: sexp) -> sexp {
    unsafe { tein_reader_dispatch_chars(ctx) }
}

/// clear all reader dispatch handlers, releasing GC-preserved procs.
#[inline]
pub unsafe fn reader_dispatch_clear(ctx: sexp) {
    unsafe { tein_reader_dispatch_clear(ctx) }
}

/// check if a character is reserved by r7rs and cannot be dispatched.
#[inline]
pub unsafe fn reader_char_is_reserved(c: c_int) -> bool {
    unsafe { tein_reader_char_is_reserved(c) != 0 }
}

/// set the macro expansion hook procedure, or SEXP_FALSE to clear.
/// GC-safe: preserves the proc and releases any previous hook.
#[inline]
pub unsafe fn macro_expand_hook_set(ctx: sexp, proc: sexp) {
    unsafe { tein_macro_expand_hook_set(ctx, proc) }
}

/// get the current macro expansion hook, or SEXP_FALSE if none.
#[inline]
pub unsafe fn macro_expand_hook_get() -> sexp {
    unsafe { tein_macro_expand_hook_get() }
}

/// clear the macro expansion hook, releasing the GC reference.
#[inline]
pub unsafe fn macro_expand_hook_clear(ctx: sexp) {
    unsafe { tein_macro_expand_hook_clear(ctx) }
}
