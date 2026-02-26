//! parse error with source span

use crate::ast::Span;
use std::fmt;

/// an error encountered while parsing (or serializing/deserializing) s-expressions
#[derive(Debug, Clone)]
pub struct ParseError {
    /// human-readable description of what went wrong
    pub message: String,
    /// source location where the error occurred
    pub span: Span,
}

impl ParseError {
    /// create a new parse error at the given span
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    /// create a parse error with no source location
    pub fn no_span(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: Span::NONE,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.span.is_none() {
            write!(f, "{}", self.message)
        } else {
            write!(
                f,
                "{} at line {}, column {}",
                self.message, self.span.line, self.span.column,
            )
        }
    }
}

impl std::error::Error for ParseError {}

#[cfg(feature = "serde")]
impl serde::ser::Error for ParseError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::no_span(msg.to_string())
    }
}

#[cfg(feature = "serde")]
impl serde::de::Error for ParseError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::no_span(msg.to_string())
    }
}

/// convenience result type for parse operations
pub type Result<T> = std::result::Result<T, ParseError>;
