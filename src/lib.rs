//! # tein
//!
//! > *branch and rune-stick*
//!
//! embeddable r7rs scheme interpreter for rust, built on chibi-scheme.
//!
//! ## quick start
//!
//! ```ignore
//! use tein::Context;
//!
//! let ctx = Context::new();
//! let result = ctx.evaluate("(+ 1 2 3)")?;
//! println!("result: {}", result);
//! ```

#![warn(missing_docs)]

mod ffi;
mod context;
mod value;
mod error;

pub use context::Context;
pub use value::Value;
pub use error::{Error, Result};

/// raw ffi types for advanced use (foreign functions, etc.)
///
/// these are thin wrappers over chibi's c api. use them to build
/// foreign functions that can be registered with [`Context::define_fn_raw`].
#[allow(missing_docs)]
pub mod raw {
    pub use crate::ffi::{sexp, sexp_sint_t};
    pub use crate::ffi::{
        sexp_make_fixnum, sexp_make_flonum, sexp_make_boolean,
        sexp_c_str, get_void, get_null, get_true, get_false,
        sexp_exceptionp, sexp_car, sexp_cdr,
        sexp_unbox_fixnum, sexp_flonum_value,
        sexp_integerp, sexp_flonump, sexp_stringp, sexp_symbolp,
        sexp_booleanp, sexp_pairp, sexp_nullp, sexp_vectorp,
    };
}
