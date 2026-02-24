//! error types for tein
//!
//! all fallible operations in tein return [`Result<T>`](Result), which uses
//! [`Error`] as the error type. [`Error`] implements `std::error::Error`
//! and converts from `std::io::Error` and utf-8 errors.
//!
//! # when each variant occurs
//!
//! | variant | cause |
//! |---------|-------|
//! | [`Error::EvalError`] | scheme evaluation error (syntax, unbound variable, etc.) |
//! | [`Error::TypeError`] | rust↔scheme type conversion failure |
//! | [`Error::InitError`] | context creation or thread communication failure |
//! | [`Error::Utf8Error`] | invalid utf-8 in scheme string/symbol |
//! | [`Error::IoError`] | file or port IO failure |
//! | [`Error::StepLimitExceeded`] | evaluation exceeded [`ContextBuilder::step_limit()`](crate::ContextBuilder::step_limit) |
//! | [`Error::Timeout`] | evaluation exceeded [`TimeoutContext`](crate::TimeoutContext) deadline |
//! | [`Error::SandboxViolation`] | blocked module import, denied file access, or missing preset |
//!
//! # example
//!
//! ```
//! use tein::{Context, Error};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ctx = Context::builder().step_limit(100).build()?;
//!
//! match ctx.evaluate("((lambda () (define (f) (f)) (f)))") {
//!     Err(Error::StepLimitExceeded) => { /* expected */ }
//!     other => panic!("unexpected: {:?}", other),
//! }
//! # Ok(())
//! # }
//! ```

use std::fmt;

/// result type alias for tein operations
pub type Result<T> = std::result::Result<T, Error>;

/// errors that can occur when working with scheme contexts
#[derive(Debug, Clone)]
pub enum Error {
    /// scheme evaluation error
    EvalError(String),

    /// type conversion error
    TypeError(String),

    /// context initialization error
    InitError(String),

    /// utf-8 conversion error
    Utf8Error(String),

    /// file io error
    IoError(String),

    /// evaluation exceeded the configured step limit
    StepLimitExceeded,

    /// evaluation exceeded the configured wall-clock timeout
    Timeout,

    /// evaluation was blocked by sandbox policy (not a code bug)
    ///
    /// indicates the scheme code attempted something explicitly restricted
    /// by the context's configuration: a blocked module import, denied
    /// file access, or use of a primitive not included in the active presets.
    SandboxViolation(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::EvalError(msg) => write!(f, "scheme evaluation error: {}", msg),
            Error::TypeError(msg) => write!(f, "type error: {}", msg),
            Error::InitError(msg) => write!(f, "initialization error: {}", msg),
            Error::Utf8Error(msg) => write!(f, "utf-8 error: {}", msg),
            Error::IoError(msg) => write!(f, "io error: {}", msg),
            Error::StepLimitExceeded => write!(f, "step limit exceeded"),
            Error::Timeout => write!(f, "evaluation timed out"),
            Error::SandboxViolation(msg) => write!(f, "sandbox violation: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::str::Utf8Error> for Error {
    fn from(err: std::str::Utf8Error) -> Self {
        Error::Utf8Error(err.to_string())
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(err: std::string::FromUtf8Error) -> Self {
        Error::Utf8Error(err.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::IoError(err.to_string())
    }
}
