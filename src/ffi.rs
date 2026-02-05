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

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(missing_docs)]
#![allow(clippy::missing_safety_doc)]

use std::os::raw::{c_char, c_int, c_long, c_ulong, c_void};

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

    pub fn sexp_load_standard_env(ctx: sexp, env: sexp, version: sexp_uint_t) -> sexp;

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

    // pair/list construction (via tein shim)
    pub fn tein_sexp_cons(ctx: sexp, head: sexp, tail: sexp) -> sexp;

    // vector construction (via tein shim)
    pub fn tein_sexp_make_vector(ctx: sexp, len: sexp_uint_t, dflt: sexp) -> sexp;
    pub fn tein_sexp_vector_set(vec: sexp, i: sexp_uint_t, val: sexp);
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
