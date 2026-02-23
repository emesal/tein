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
pub mod foreign;
pub mod managed;
pub mod sandbox;
mod thread;
mod timeout;
mod value;

pub use context::{Context, ContextBuilder};
pub use error::{Error, Result};
pub use foreign::{ForeignType, MethodContext, MethodFn};
pub use managed::{Mode, ThreadLocalContext};
pub use timeout::TimeoutContext;
pub use value::Value;

/// re-export the `#[scheme_fn]` proc macro for ergonomic foreign function definition
pub use tein_macros::scheme_fn;

/// raw ffi types for advanced use (foreign functions, proc macro generated code, etc.)
///
/// these are thin wrappers over chibi's c api. the `#[scheme_fn]` proc macro
/// generates code that references these symbols, so they must remain public.
#[allow(missing_docs)]
pub mod raw {
    pub use crate::ffi::{GcRoot, sexp, sexp_sint_t};
    pub use crate::ffi::{
        get_false, get_null, get_true, get_void, sexp_booleanp, sexp_bytes_data, sexp_bytes_length,
        sexp_bytesp, sexp_c_str, sexp_car, sexp_cdr, sexp_charp, sexp_cons, sexp_exceptionp,
        sexp_flonum_value, sexp_flonump, sexp_integerp, sexp_make_boolean, sexp_make_bytes,
        sexp_make_character, sexp_make_fixnum, sexp_make_flonum, sexp_nullp, sexp_pairp,
        sexp_portp, sexp_string_data, sexp_string_size, sexp_stringp, sexp_symbolp,
        sexp_unbox_character, sexp_unbox_fixnum, sexp_vectorp,
    };
}
