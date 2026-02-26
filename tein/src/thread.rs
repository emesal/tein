//! shared protocol types for thread-based context wrappers
//!
//! both [`TimeoutContext`](crate::TimeoutContext) and
//! [`ThreadLocalContext`](crate::managed::ThreadLocalContext) run a
//! [`Context`](crate::Context) on a dedicated thread and communicate
//! via channels. this module contains the shared request/response
//! protocol and the `SendableValue` wrapper.

use crate::Value;
use crate::error::Result;

/// function pointer type for variadic foreign functions
pub(crate) type ForeignFnPtr = unsafe extern "C" fn(
    crate::ffi::sexp,
    crate::ffi::sexp,
    crate::ffi::sexp_sint_t,
    crate::ffi::sexp,
) -> crate::ffi::sexp;

/// request sent to a context thread
pub(crate) enum Request {
    /// evaluate a string of scheme code
    Evaluate(String),
    /// call a procedure with arguments
    Call(SendableValue, Vec<SendableValue>),
    /// register a variadic foreign function
    DefineFnVariadic {
        /// scheme name for the function
        name: String,
        /// the raw function pointer
        f: ForeignFnPtr,
    },
    /// rebuild the context from the stored builder + init closure
    Reset,
    /// shut down the context thread
    Shutdown,
}

// SAFETY: Request contains SendableValue which wraps Value (may hold raw
// sexp pointers). safe because values only travel to the context thread
// where the context that created them lives.
unsafe impl Send for Request {}

/// response from a context thread
pub(crate) enum Response {
    /// result of evaluate or call
    Value(Result<Value>),
    /// result of define_fn_variadic
    Defined(Result<()>),
    /// result of reset
    Reset(Result<()>),
}

// SAFETY: Response contains Result<Value> which may hold Value::Procedure
// (a raw *mut c_void). safe because values only travel between the caller
// and the single context thread — Procedure pointers are only
// dereferenced on the context thread where the context lives.
unsafe impl Send for Response {}

/// wrapper allowing a Value to be sent across threads
///
/// # safety
/// safe because values are only ever sent *back* to the thread that owns
/// the context. Procedure values contain raw sexp pointers that are only
/// valid on the context thread — this wrapper ensures they travel back
/// to where they came from.
pub(crate) struct SendableValue(pub(crate) Value);

// SAFETY: see struct-level doc. values only travel between the caller
// and the single context thread, and Procedure pointers are only
// dereferenced on the context thread.
unsafe impl Send for SendableValue {}
