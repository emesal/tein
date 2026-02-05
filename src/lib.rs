//! # tein
//!
//! > *branch and rune-stick*
//!
//! embeddable r7rs scheme interpreter for rust, built on chibi-scheme.
//!
//! ## quick start
//!
//! ```
//! use tein::{Context, Value};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ctx = Context::new()?;
//! let result = ctx.evaluate("(+ 1 2 3)")?;
//! assert_eq!(result, Value::Integer(6));
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

mod context;
mod error;
mod ffi;
mod value;

pub use context::Context;
pub use error::{Error, Result};
pub use value::Value;

/// raw ffi types for advanced use (foreign functions, etc.)
///
/// these are thin wrappers over chibi's c api. use them to build
/// foreign functions that can be registered with [`Context::define_fn0`]
/// through [`Context::define_fn3`].
#[allow(missing_docs)]
pub mod raw {
    pub use crate::ffi::{
        get_false, get_null, get_true, get_void, sexp_booleanp, sexp_c_str, sexp_car, sexp_cdr,
        sexp_exceptionp, sexp_flonum_value, sexp_flonump, sexp_integerp, sexp_make_boolean,
        sexp_make_fixnum, sexp_make_flonum, sexp_nullp, sexp_pairp, sexp_stringp, sexp_symbolp,
        sexp_unbox_fixnum, sexp_vectorp,
    };
    pub use crate::ffi::{sexp, sexp_sint_t};
}
