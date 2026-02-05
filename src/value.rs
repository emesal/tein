//! scheme value representation

use crate::{error::{Error, Result}, ffi};
use std::fmt;

/// maximum nesting depth for recursive value conversion.
/// prevents stack overflow on deeply nested or (theoretically) circular structures.
const MAX_DEPTH: usize = 10_000;

/// a scheme value
///
/// represents the result of evaluating scheme code.
/// this is a safe wrapper around chibi's internal sexp type.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// integer value
    Integer(i64),

    /// floating point value
    Float(f64),

    /// string value
    String(String),

    /// symbol value
    Symbol(String),

    /// boolean value
    Boolean(bool),

    /// proper list (converted to vec)
    List(Vec<Value>),

    /// pair (improper list or dotted pair)
    Pair(Box<Value>, Box<Value>),

    /// vector (scheme `#(...)`)
    Vector(Vec<Value>),

    /// nil/empty list
    Nil,

    /// unspecified value (like void in c)
    Unspecified,

    /// other scheme values we don't yet handle
    Other(String),
}

impl Value {
    /// convert from raw chibi sexp to safe value
    ///
    /// # safety
    /// ctx and raw must be valid pointers from chibi-scheme
    pub(crate) unsafe fn from_raw(ctx: ffi::sexp, raw: ffi::sexp) -> Result<Self> {
        unsafe { Self::from_raw_depth(ctx, raw, 0) }
    }

    /// recursive inner conversion with depth tracking to prevent stack overflow
    unsafe fn from_raw_depth(ctx: ffi::sexp, raw: ffi::sexp, depth: usize) -> Result<Self> {
        if depth > MAX_DEPTH {
            return Err(Error::EvalError(
                "value nesting depth exceeded maximum".to_string(),
            ));
        }

        unsafe {
        if ffi::sexp_exceptionp(raw) != 0 {
            return Err(Error::EvalError(Self::extract_exception_message(ctx, raw)));
        }

        // check floats before integers because sexp_integerp matches some floats
        if ffi::sexp_flonump(raw) != 0 {
            let f = ffi::sexp_flonum_value(raw);
            return Ok(Value::Float(f));
        }

        if ffi::sexp_integerp(raw) != 0 {
            let i = ffi::sexp_unbox_fixnum(raw);
            return Ok(Value::Integer(i as i64));
        }

        if ffi::sexp_booleanp(raw) != 0 {
            let true_val = ffi::get_true();
            return Ok(Value::Boolean(raw == true_val));
        }

        if ffi::sexp_nullp(raw) != 0 {
            return Ok(Value::Nil);
        }

        if raw == ffi::get_void() {
            return Ok(Value::Unspecified);
        }

        if ffi::sexp_stringp(raw) != 0 {
            let str_ptr = ffi::sexp_string_data(raw);
            let str_len = ffi::sexp_string_size(raw);
            let bytes = std::slice::from_raw_parts(str_ptr as *const u8, str_len as usize);
            let s = String::from_utf8(bytes.to_vec())?;
            return Ok(Value::String(s));
        }

        if ffi::sexp_symbolp(raw) != 0 {
            let str_sexp = ffi::sexp_symbol_to_string(ctx, raw);
            let str_ptr = ffi::sexp_string_data(str_sexp);
            let str_len = ffi::sexp_string_size(str_sexp);
            let bytes = std::slice::from_raw_parts(str_ptr as *const u8, str_len as usize);
            let s = String::from_utf8(bytes.to_vec())?;
            return Ok(Value::Symbol(s));
        }

        if ffi::sexp_vectorp(raw) != 0 {
            let len = ffi::sexp_vector_length(raw) as usize;
            let data = ffi::sexp_vector_data(raw);
            let mut items = Vec::with_capacity(len);
            for i in 0..len {
                let elem = *data.add(i);
                items.push(Value::from_raw_depth(ctx, elem, depth + 1)?);
            }
            return Ok(Value::Vector(items));
        }

        if ffi::sexp_pairp(raw) != 0 {
            if Self::is_proper_list(raw) {
                let mut items = Vec::new();
                let mut current = raw;

                while ffi::sexp_pairp(current) != 0 {
                    let car = ffi::sexp_car(current);
                    items.push(Value::from_raw_depth(ctx, car, depth + 1)?);
                    current = ffi::sexp_cdr(current);
                }

                return Ok(Value::List(items));
            } else {
                let car = ffi::sexp_car(raw);
                let cdr = ffi::sexp_cdr(raw);
                return Ok(Value::Pair(
                    Box::new(Value::from_raw_depth(ctx, car, depth + 1)?),
                    Box::new(Value::from_raw_depth(ctx, cdr, depth + 1)?),
                ));
            }
        }

        // fallback for other types
        Ok(Value::Other("<unhandled-type>".to_string()))
        }
    }

    /// check if a sexp is a proper list (ends in nil)
    ///
    /// uses tortoise-and-hare cycle detection to avoid infinite loops
    /// on circular lists constructed via set-cdr!.
    unsafe fn is_proper_list(sexp: ffi::sexp) -> bool {
        unsafe {
        let mut tortoise = sexp;
        let mut hare = sexp;

        loop {
            if ffi::sexp_pairp(hare) == 0 {
                return ffi::sexp_nullp(hare) != 0;
            }
            hare = ffi::sexp_cdr(hare);

            if ffi::sexp_pairp(hare) == 0 {
                return ffi::sexp_nullp(hare) != 0;
            }
            hare = ffi::sexp_cdr(hare);

            tortoise = ffi::sexp_cdr(tortoise);

            if hare == tortoise {
                return false; // cycle detected
            }
        }
        }
    }

    /// extract a human-readable message from a chibi exception
    unsafe fn extract_exception_message(ctx: ffi::sexp, exn: ffi::sexp) -> String {
        unsafe {
        let msg_sexp = ffi::sexp_exception_message(exn);
        let message = if ffi::sexp_stringp(msg_sexp) != 0 {
            let ptr = ffi::sexp_string_data(msg_sexp);
            let len = ffi::sexp_string_size(msg_sexp) as usize;
            let bytes = std::slice::from_raw_parts(ptr as *const u8, len);
            std::string::String::from_utf8_lossy(bytes).into_owned()
        } else {
            "unknown error".to_owned()
        };

        // append irritants if present (the values that caused the error)
        let irritants = ffi::sexp_exception_irritants(exn);
        if ffi::sexp_pairp(irritants) != 0 {
            if let Ok(val) = Value::from_raw(ctx, irritants) {
                return format!("{}: {}", message, val);
            }
        }

        message
        }
    }

    /// convert a rust value to a raw chibi sexp
    ///
    /// useful for returning values from foreign functions registered
    /// with [`Context::define_fn0`] through [`Context::define_fn3`].
    ///
    /// currently supports: Integer, Float, Boolean, String, Symbol, Nil, Unspecified.
    /// returns an error for List, Pair, Vector, and Other (use the raw API for these).
    ///
    /// # safety
    /// ctx must be a valid chibi context pointer
    pub unsafe fn to_raw(&self, ctx: ffi::sexp) -> Result<ffi::sexp> {
        unsafe {
        match self {
            Value::Integer(n) => Ok(ffi::sexp_make_fixnum(*n as ffi::sexp_sint_t)),
            Value::Float(f) => Ok(ffi::sexp_make_flonum(ctx, *f)),
            Value::Boolean(b) => Ok(ffi::sexp_make_boolean(*b)),
            Value::String(s) => {
                let c_str = std::ffi::CString::new(s.as_str()).map_err(|_| {
                    Error::TypeError("string contains null bytes".to_string())
                })?;
                Ok(ffi::sexp_c_str(ctx, c_str.as_ptr(), s.len() as ffi::sexp_sint_t))
            }
            Value::Symbol(s) => {
                let c_str = std::ffi::CString::new(s.as_str()).map_err(|_| {
                    Error::TypeError("symbol contains null bytes".to_string())
                })?;
                Ok(ffi::sexp_intern(ctx, c_str.as_ptr(), s.len() as ffi::sexp_sint_t))
            }
            Value::Nil => Ok(ffi::get_null()),
            Value::Unspecified => Ok(ffi::get_void()),
            Value::List(_) => Err(Error::TypeError(
                "cannot convert List to raw sexp (use raw API for compound types)".to_string(),
            )),
            Value::Pair(_, _) => Err(Error::TypeError(
                "cannot convert Pair to raw sexp (use raw API for compound types)".to_string(),
            )),
            Value::Vector(_) => Err(Error::TypeError(
                "cannot convert Vector to raw sexp (use raw API for compound types)".to_string(),
            )),
            Value::Other(desc) => Err(Error::TypeError(format!(
                "cannot convert Other({}) to raw sexp", desc,
            ))),
        }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(i) => write!(f, "{}", i),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::String(s) => {
                write!(f, "\"")?;
                for ch in s.chars() {
                    match ch {
                        '"' => write!(f, "\\\"")?,
                        '\\' => write!(f, "\\\\")?,
                        '\n' => write!(f, "\\n")?,
                        '\r' => write!(f, "\\r")?,
                        '\t' => write!(f, "\\t")?,
                        c => write!(f, "{}", c)?,
                    }
                }
                write!(f, "\"")
            }
            Value::Symbol(s) => write!(f, "{}", s),
            Value::Boolean(b) => write!(f, "{}", if *b { "#t" } else { "#f" }),
            Value::List(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
            Value::Pair(car, cdr) => {
                write!(f, "({} . {})", car, cdr)
            }
            Value::Vector(items) => {
                write!(f, "#(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
            Value::Nil => write!(f, "()"),
            Value::Unspecified => write!(f, "#<unspecified>"),
            Value::Other(s) => write!(f, "#<{}>", s),
        }
    }
}
