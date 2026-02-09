//! pure rust s-expression parser, printer, and serde data format
//!
//! `tein-sexp` provides a complete s-expression toolkit with zero required
//! dependencies. the optional `serde` feature adds serialization/deserialization
//! support, enabling free conversion to/from json, toml, yaml, and more.
//!
//! # quick start
//!
//! ```
//! use tein_sexp::{Sexp, SexpKind};
//!
//! // construct programmatically
//! let expr = Sexp::list(vec![
//!     Sexp::symbol("define"),
//!     Sexp::symbol("x"),
//!     Sexp::integer(42),
//! ]);
//! assert_eq!(expr.to_string(), "(define x 42)");
//! ```

#![warn(missing_docs)]

pub mod ast;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod printer;

#[cfg(feature = "serde")]
pub mod serde;

// re-exports for convenience
pub use ast::{Comment, CommentKind, Sexp, SexpKind, Span};
pub use error::ParseError;
