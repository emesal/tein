//! serde integration for s-expressions
//!
//! serialize and deserialize rust types to/from s-expression text.
//! enabled with the `serde` cargo feature.
//!
//! # serialization mapping
//!
//! | rust type | s-expression |
//! |---|---|
//! | bool | `#t` / `#f` |
//! | integers | `42` |
//! | floats | `3.14` |
//! | char | `#\a` |
//! | string | `"hello"` |
//! | Option None | `()` |
//! | Option Some(x) | `x` |
//! | unit / unit struct | `()` |
//! | sequence / tuple | `(1 2 3)` |
//! | map / struct | `((key . val) ...)` |
//! | enum unit variant | `symbol` |
//! | enum newtype | `(variant value)` |
//! | enum tuple | `(variant val1 val2 ...)` |
//! | enum struct | `(variant (field . val) ...)` |

mod de;
mod ser;

pub use de::{from_sexp, from_str};
pub use ser::to_sexp;

use crate::error::ParseError;
use crate::printer;
use std::io;

/// serialize a value to compact s-expression text
pub fn to_string<T: serde::Serialize>(value: &T) -> Result<String, ParseError> {
    let sexp = to_sexp(value)?;
    Ok(printer::to_string(&sexp))
}

/// serialize a value to pretty-printed s-expression text
pub fn to_string_pretty<T: serde::Serialize>(value: &T) -> Result<String, ParseError> {
    let sexp = to_sexp(value)?;
    Ok(printer::to_string_pretty(&sexp))
}

/// deserialize a value from a reader containing s-expression text
///
/// reads all available input, then parses it as a single s-expression.
/// for streaming use, read the input manually and call [`from_str`].
pub fn from_reader<R: io::Read, T: serde::de::DeserializeOwned>(
    mut reader: R,
) -> Result<T, ParseError> {
    let mut input = String::new();
    reader
        .read_to_string(&mut input)
        .map_err(|e| ParseError::no_span(format!("io error: {e}")))?;
    from_str(&input)
}

/// serialize a value to a writer as compact s-expression text
pub fn to_writer<W: io::Write, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), ParseError> {
    let sexp = to_sexp(value)?;
    write!(writer, "{}", printer::to_string(&sexp))
        .map_err(|e| ParseError::no_span(format!("io error: {e}")))
}

/// serialize a value to a writer as pretty-printed s-expression text
pub fn to_writer_pretty<W: io::Write, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), ParseError> {
    let sexp = to_sexp(value)?;
    write!(writer, "{}", printer::to_string_pretty(&sexp))
        .map_err(|e| ParseError::no_span(format!("io error: {e}")))
}
