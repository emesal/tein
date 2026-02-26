//! # tein
//!
//! > *branch and rune-stick*
//!
//! Embeddable R7RS Scheme interpreter for Rust, built on
//! [chibi-scheme](https://github.com/ashinn/chibi-scheme). Safe Rust API
//! wrapping unsafe C FFI — zero runtime dependencies, full r7rs-small
//! compliance, ~200kb footprint.
//!
//! ## Quick start
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
//!
//! ## Features
//!
//! - **Sandboxing** — Restrict environments with composable [`sandbox`] presets,
//!   step limits, wall-clock timeouts, and file IO policies via [`ContextBuilder`]
//! - **`#[tein_fn]`** — Define Scheme-callable functions in Rust with automatic
//!   type conversion (see [`tein_fn`])
//! - **Foreign types** — Expose Rust types as Scheme objects with method dispatch
//!   and introspection (see [`foreign`])
//! - **Custom ports** — Bridge Rust `Read`/`Write` into Scheme's port system
//! - **Reader extensions** — Register custom `#` dispatch characters
//! - **Macro expansion hooks** — Intercept and transform macro expansions
//! - **Managed contexts** — Thread-safe evaluation via [`ThreadLocalContext`]
//!   with persistent or fresh-per-evaluation modes (see [`managed`])
//! - **Timeouts** — Wall-clock deadlines via [`TimeoutContext`]
//!
//! ## Safety model
//!
//! [`Context`] is intentionally `!Send + !Sync`. Chibi-Scheme contexts are not
//! thread-safe — one context per thread. For cross-thread use, wrap in
//! [`ThreadLocalContext`] or [`TimeoutContext`], which run Scheme on a dedicated
//! thread and proxy requests over channels.

#![warn(missing_docs)]

mod context;
mod error;
mod ffi;
pub mod foreign;
pub mod managed;
mod port;
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

/// Re-export the `#[tein_fn]` proc macro for defining scheme-callable functions.
pub use tein_macros::tein_fn;

/// Re-export the `#[tein_module]` proc macro for defining scheme modules.
pub use tein_macros::tein_module;

/// Re-export the `#[tein_type]` proc macro for marking structs in a `#[tein_module]`.
pub use tein_macros::tein_type;

/// Re-export the `#[tein_methods]` proc macro for exposing impl blocks in a `#[tein_module]`.
pub use tein_macros::tein_methods;

/// Deprecated: use [`tein_fn`] instead.
#[allow(deprecated)]
pub use tein_macros::scheme_fn;

/// Raw FFI types for advanced use (foreign functions, proc macro generated code, etc.)
///
/// These are thin wrappers over Chibi's C API. The `#[tein_fn]` proc macro
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
