//! `(tein toml)` — bidirectional TOML ↔ scheme value conversion.
//!
//! TOML parsing goes through `toml_crate::Value` then maps to scheme `Value`.
//! TOML stringifying builds a `toml_crate::Value` tree from raw chibi sexps
//! then delegates to `toml_crate::to_string()` for correct formatting.
//!
//! ## representation
//!
//! | TOML            | scheme                                       |
//! |-----------------|----------------------------------------------|
//! | table `{}`      | alist `((key . val) ...)`                    |
//! | empty table     | `'()` (same ambiguity as json — accepted)    |
//! | array `[]`      | list `(...)`                                 |
//! | empty `[]`      | `'()`                                        |
//! | string          | string                                       |
//! | integer         | integer                                      |
//! | float           | flonum (includes inf, nan)                   |
//! | `true/false`    | `#t / #f`                                    |
//! | datetime        | `(toml-datetime "...")`  tagged list          |

use crate::{Error, Result, Value, ffi};

/// the symbol tag used for TOML datetime values.
///
/// all four TOML datetime variants (offset datetime, local datetime,
/// local date, local time) use this same tag — the string content
/// distinguishes them.
const DATETIME_TAG: &str = "toml-datetime";

/// parse a TOML string into a scheme `Value`.
///
/// the input must be a complete TOML document (not a bare value). tables
/// become alists, arrays become lists, datetimes become tagged lists
/// `(toml-datetime "iso-string")`.
pub fn toml_parse(input: &str) -> Result<Value> {
    let table: toml_crate::Table = input
        .parse()
        .map_err(|e| Error::EvalError(format!("toml-parse: {e}")))?;
    toml_value_to_value(toml_crate::Value::Table(table))
}

/// convert a `toml_crate::Value` into a scheme `Value`.
fn toml_value_to_value(tv: toml_crate::Value) -> Result<Value> {
    match tv {
        toml_crate::Value::String(s) => Ok(Value::String(s)),
        toml_crate::Value::Integer(i) => Ok(Value::Integer(i)),
        toml_crate::Value::Float(f) => Ok(Value::Float(f)),
        toml_crate::Value::Boolean(b) => Ok(Value::Boolean(b)),
        toml_crate::Value::Datetime(dt) => Ok(Value::List(vec![
            Value::Symbol(DATETIME_TAG.to_string()),
            Value::String(dt.to_string()),
        ])),
        toml_crate::Value::Array(arr) => {
            if arr.is_empty() {
                Ok(Value::Nil)
            } else {
                let items: Result<Vec<Value>> = arr.into_iter().map(toml_value_to_value).collect();
                Ok(Value::List(items?))
            }
        }
        toml_crate::Value::Table(map) => {
            if map.is_empty() {
                Ok(Value::Nil)
            } else {
                let entries: Result<Vec<Value>> = map
                    .into_iter()
                    .map(|(k, v)| {
                        let val = toml_value_to_value(v)?;
                        Ok(Value::Pair(Box::new(Value::String(k)), Box::new(val)))
                    })
                    .collect();
                Ok(Value::List(entries?))
            }
        }
    }
}

/// stringify a raw chibi sexp as TOML.
///
/// builds a `toml_crate::Value` tree from the raw sexp, then delegates to
/// `toml_crate::to_string()` for correct TOML formatting. works directly on
/// raw sexps (like json) to preserve alist structure.
///
/// datetime detection: a two-element list `(toml-datetime "...")` where
/// the car is the symbol `toml-datetime` and the cadr is a string.
///
/// # safety
/// ctx and sexp must be valid chibi sexp pointers. called from trampoline.
pub unsafe fn toml_stringify_raw(ctx: ffi::sexp, sexp: ffi::sexp) -> Result<String> {
    let tv = unsafe { sexp_to_toml_value(ctx, sexp, 0)? };
    // toml_crate::to_string requires a Table at the top level
    match tv {
        toml_crate::Value::Table(t) => {
            toml_crate::to_string(&t).map_err(|e| Error::EvalError(format!("toml-stringify: {e}")))
        }
        _ => Err(Error::TypeError(
            "toml-stringify: top-level value must be a table (alist)".to_string(),
        )),
    }
}

/// maximum nesting depth for recursive sexp-to-toml conversion.
const MAX_DEPTH: usize = 10_000;

/// convert a raw chibi sexp to a `toml_crate::Value`.
unsafe fn sexp_to_toml_value(
    ctx: ffi::sexp,
    sexp: ffi::sexp,
    depth: usize,
) -> Result<toml_crate::Value> {
    if depth > MAX_DEPTH {
        return Err(Error::EvalError(
            "toml-stringify: maximum nesting depth exceeded".to_string(),
        ));
    }
    unsafe {
        if ffi::sexp_booleanp(sexp) != 0 {
            return Ok(toml_crate::Value::Boolean(
                sexp == ffi::sexp_make_boolean(true),
            ));
        }
        if ffi::sexp_nullp(sexp) != 0 {
            return Ok(toml_crate::Value::Array(vec![]));
        }
        if ffi::sexp_stringp(sexp) != 0 {
            let s = sexp_to_str(sexp)?;
            return Ok(toml_crate::Value::String(s));
        }
        if ffi::sexp_integerp(sexp) != 0 && ffi::sexp_flonump(sexp) == 0 {
            let n = ffi::sexp_unbox_fixnum(sexp);
            return Ok(toml_crate::Value::Integer(n));
        }
        if ffi::sexp_flonump(sexp) != 0 {
            let f = ffi::sexp_flonum_value(sexp);
            return Ok(toml_crate::Value::Float(f));
        }
        if ffi::sexp_pairp(sexp) != 0 {
            // check for datetime tag: (toml-datetime "...")
            if is_datetime_tagged(ctx, sexp) {
                let str_sexp = ffi::sexp_car(ffi::sexp_cdr(sexp));
                let s = sexp_to_str(str_sexp)?;
                let dt: toml_crate::value::Datetime = s.parse().map_err(|e| {
                    Error::EvalError(format!("toml-stringify: invalid datetime '{s}': {e}"))
                })?;
                return Ok(toml_crate::Value::Datetime(dt));
            }

            // collect list elements
            let mut elems: Vec<ffi::sexp> = Vec::new();
            let mut cur = sexp;
            let mut is_proper = true;
            while ffi::sexp_pairp(cur) != 0 {
                elems.push(ffi::sexp_car(cur));
                cur = ffi::sexp_cdr(cur);
            }
            if ffi::sexp_nullp(cur) == 0 {
                is_proper = false;
            }

            if !is_proper {
                return Err(Error::TypeError(
                    "toml-stringify: cannot convert improper list (dotted pair) to TOML"
                        .to_string(),
                ));
            }

            if !elems.is_empty() {
                // alist check: every element is a pair with a string car
                let all_alist = elems.iter().all(|&elem| {
                    ffi::sexp_pairp(elem) != 0 && ffi::sexp_stringp(ffi::sexp_car(elem)) != 0
                });

                if all_alist {
                    let mut table = toml_crate::map::Map::new();
                    for &elem in &elems {
                        let k = sexp_to_str(ffi::sexp_car(elem))?;
                        let v = sexp_to_toml_value(ctx, ffi::sexp_cdr(elem), depth + 1)?;
                        table.insert(k, v);
                    }
                    return Ok(toml_crate::Value::Table(table));
                }
            }

            // plain array
            let mut arr = Vec::with_capacity(elems.len());
            for &elem in &elems {
                arr.push(sexp_to_toml_value(ctx, elem, depth + 1)?);
            }
            return Ok(toml_crate::Value::Array(arr));
        }

        Err(Error::TypeError(
            "toml-stringify: cannot convert scheme value to TOML".to_string(),
        ))
    }
}

/// check if a sexp is a `(toml-datetime "...")` tagged list.
///
/// matches: a two-element proper list where car is the symbol `toml-datetime`
/// and cadr is a string.
unsafe fn is_datetime_tagged(ctx: ffi::sexp, sexp: ffi::sexp) -> bool {
    unsafe {
        if ffi::sexp_pairp(sexp) == 0 {
            return false;
        }
        let car = ffi::sexp_car(sexp);
        if ffi::sexp_symbolp(car) == 0 {
            return false;
        }
        // check symbol name is "toml-datetime"
        let sym_str = ffi::sexp_symbol_to_string(ctx, car);
        let sym_ptr = ffi::sexp_string_data(sym_str);
        let sym_len = ffi::sexp_string_size(sym_str) as usize;
        let sym =
            match std::str::from_utf8(std::slice::from_raw_parts(sym_ptr as *const u8, sym_len)) {
                Ok(s) => s,
                Err(_) => return false,
            };
        if sym != DATETIME_TAG {
            return false;
        }
        // check cdr is a pair with a string car and null cdr (two-element list)
        let cdr = ffi::sexp_cdr(sexp);
        if ffi::sexp_pairp(cdr) == 0 {
            return false;
        }
        if ffi::sexp_stringp(ffi::sexp_car(cdr)) == 0 {
            return false;
        }
        // must be exactly two elements (cdr of cdr is null)
        ffi::sexp_nullp(ffi::sexp_cdr(cdr)) != 0
    }
}

/// extract a rust string from a chibi string sexp.
unsafe fn sexp_to_str(sexp: ffi::sexp) -> Result<String> {
    unsafe {
        let ptr = ffi::sexp_string_data(sexp);
        let len = ffi::sexp_string_size(sexp) as usize;
        let s = std::str::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len))
            .map_err(|e| Error::EvalError(format!("toml-stringify: UTF-8 error: {e}")))?;
        Ok(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_string() {
        let v = toml_parse("val = \"hello\"").unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(k, v) => {
                    assert_eq!(**k, Value::String("val".to_string()));
                    assert_eq!(**v, Value::String("hello".to_string()));
                }
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_integer() {
        let v = toml_parse("x = 42").unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(_, v) => assert_eq!(**v, Value::Integer(42)),
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_float() {
        let v = toml_parse("x = 1.5").unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(_, v) => assert_eq!(**v, Value::Float(1.5)),
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_boolean() {
        let v = toml_parse("x = true").unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(_, v) => assert_eq!(**v, Value::Boolean(true)),
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_datetime_offset() {
        let v = toml_parse("dt = 1979-05-27T07:32:00Z").unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(_, v) => {
                    assert_eq!(
                        **v,
                        Value::List(vec![
                            Value::Symbol("toml-datetime".to_string()),
                            Value::String("1979-05-27T07:32:00Z".to_string()),
                        ])
                    );
                }
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_datetime_local() {
        let v = toml_parse("dt = 1979-05-27T07:32:00").unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(_, val) => match val.as_ref() {
                    Value::List(inner) => {
                        assert_eq!(inner[0], Value::Symbol("toml-datetime".to_string()));
                        assert_eq!(inner[1], Value::String("1979-05-27T07:32:00".to_string()));
                    }
                    other => panic!("expected tagged list, got {other:?}"),
                },
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_datetime_date_only() {
        let v = toml_parse("dt = 1979-05-27").unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(_, val) => match val.as_ref() {
                    Value::List(inner) => {
                        assert_eq!(inner[0], Value::Symbol("toml-datetime".to_string()));
                        assert_eq!(inner[1], Value::String("1979-05-27".to_string()));
                    }
                    other => panic!("expected tagged list, got {other:?}"),
                },
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_datetime_time_only() {
        let v = toml_parse("dt = 07:32:00").unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(_, val) => match val.as_ref() {
                    Value::List(inner) => {
                        assert_eq!(inner[0], Value::Symbol("toml-datetime".to_string()));
                        assert_eq!(inner[1], Value::String("07:32:00".to_string()));
                    }
                    other => panic!("expected tagged list, got {other:?}"),
                },
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_array() {
        let v = toml_parse("x = [1, 2, 3]").unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(_, v) => {
                    assert_eq!(
                        **v,
                        Value::List(vec![
                            Value::Integer(1),
                            Value::Integer(2),
                            Value::Integer(3),
                        ])
                    );
                }
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_empty_array() {
        let v = toml_parse("x = []").unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(_, v) => assert_eq!(**v, Value::Nil),
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_nested_table() {
        let v = toml_parse("[server]\nhost = \"localhost\"\nport = 8080").unwrap();
        match &v {
            Value::List(items) => {
                assert_eq!(items.len(), 1);
                match &items[0] {
                    Value::Pair(k, v) => {
                        assert_eq!(**k, Value::String("server".to_string()));
                        // v is an alist with host and port
                        match v.as_ref() {
                            Value::List(inner) => assert_eq!(inner.len(), 2),
                            other => panic!("expected nested alist, got {other:?}"),
                        }
                    }
                    other => panic!("expected pair, got {other:?}"),
                }
            }
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_inf_nan() {
        let v = toml_parse("a = inf\nb = -inf\nc = nan").unwrap();
        match &v {
            Value::List(items) => {
                match &items[0] {
                    Value::Pair(_, v) => assert_eq!(**v, Value::Float(f64::INFINITY)),
                    other => panic!("expected pair, got {other:?}"),
                }
                match &items[1] {
                    Value::Pair(_, v) => assert_eq!(**v, Value::Float(f64::NEG_INFINITY)),
                    other => panic!("expected pair, got {other:?}"),
                }
                match &items[2] {
                    Value::Pair(_, v) => match v.as_ref() {
                        Value::Float(f) => assert!(f.is_nan()),
                        other => panic!("expected float, got {other:?}"),
                    },
                    other => panic!("expected pair, got {other:?}"),
                }
            }
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_invalid_toml() {
        let err = toml_parse("not valid toml {{{}").unwrap_err();
        assert!(err.to_string().contains("toml-parse"));
    }

    // --- toml_stringify tests (via rust-only round-trip) ---

    /// helper: convert a scheme `Value` back to a `toml_crate::Value`.
    /// this tests the parse logic; the raw-sexp stringify is tested
    /// via scheme integration tests in tests/scheme/toml.scm.
    fn value_to_toml_value(value: &Value) -> std::result::Result<toml_crate::Value, Error> {
        match value {
            Value::String(s) => Ok(toml_crate::Value::String(s.clone())),
            Value::Integer(i) => Ok(toml_crate::Value::Integer(*i)),
            Value::Float(f) => Ok(toml_crate::Value::Float(*f)),
            Value::Boolean(b) => Ok(toml_crate::Value::Boolean(*b)),
            Value::Nil => Ok(toml_crate::Value::Array(vec![])),
            Value::List(items) => {
                // check for datetime tag
                if items.len() == 2
                    && let Value::Symbol(tag) = &items[0]
                    && tag == DATETIME_TAG
                    && let Value::String(s) = &items[1]
                {
                    let dt: toml_crate::value::Datetime = s
                        .parse()
                        .map_err(|e| Error::EvalError(format!("toml-stringify: {e}")))?;
                    return Ok(toml_crate::Value::Datetime(dt));
                }
                // alist check
                let is_alist = items.iter().all(
                    |v| matches!(v, Value::Pair(k, _) if matches!(k.as_ref(), Value::String(_))),
                );
                if is_alist {
                    let mut table = toml_crate::map::Map::new();
                    for item in items {
                        if let Value::Pair(k, v) = item
                            && let Value::String(key) = k.as_ref()
                        {
                            table.insert(key.clone(), value_to_toml_value(v)?);
                        }
                    }
                    Ok(toml_crate::Value::Table(table))
                } else {
                    let arr: std::result::Result<Vec<_>, _> =
                        items.iter().map(value_to_toml_value).collect();
                    Ok(toml_crate::Value::Array(arr?))
                }
            }
            other => Err(Error::TypeError(format!(
                "toml-stringify: cannot convert {other} to TOML"
            ))),
        }
    }

    fn toml_stringify(value: &Value) -> Result<String> {
        let tv = value_to_toml_value(value)?;
        match tv {
            toml_crate::Value::Table(t) => toml_crate::to_string(&t)
                .map_err(|e| Error::EvalError(format!("toml-stringify: {e}"))),
            _ => Err(Error::TypeError(
                "toml-stringify: top-level must be a table".to_string(),
            )),
        }
    }

    #[test]
    fn stringify_simple_table() {
        let v = toml_parse("name = \"tein\"\nversion = 1").unwrap();
        let s = toml_stringify(&v).unwrap();
        assert!(s.contains("name = \"tein\""));
        assert!(s.contains("version = 1"));
    }

    #[test]
    fn stringify_nested_table() {
        let v = toml_parse("[server]\nhost = \"localhost\"").unwrap();
        let s = toml_stringify(&v).unwrap();
        assert!(s.contains("[server]"));
        assert!(s.contains("host = \"localhost\""));
    }

    #[test]
    fn stringify_datetime_round_trip() {
        let input = "dt = 1979-05-27T07:32:00Z";
        let v = toml_parse(input).unwrap();
        let s = toml_stringify(&v).unwrap();
        assert!(s.contains("1979-05-27T07:32:00Z"));
        // must NOT be quoted (would be "1979-05-27T07:32:00Z" if treated as string)
        assert!(!s.contains("\"1979-05-27T07:32:00Z\""));
    }

    #[test]
    fn stringify_local_date_round_trip() {
        let input = "dt = 1979-05-27";
        let v = toml_parse(input).unwrap();
        let s = toml_stringify(&v).unwrap();
        assert!(s.contains("1979-05-27"));
    }

    #[test]
    fn stringify_local_time_round_trip() {
        let input = "dt = 07:32:00";
        let v = toml_parse(input).unwrap();
        let s = toml_stringify(&v).unwrap();
        assert!(s.contains("07:32:00"));
    }

    #[test]
    fn stringify_inf_nan() {
        let input = "a = inf\nb = -inf\nc = nan";
        let v = toml_parse(input).unwrap();
        let s = toml_stringify(&v).unwrap();
        assert!(s.contains("inf"));
        assert!(s.contains("-inf"));
        assert!(s.contains("nan"));
    }

    #[test]
    fn stringify_error_not_table() {
        // toml top level must be a table
        let err = toml_stringify(&Value::String("hello".to_string())).unwrap_err();
        assert!(err.to_string().contains("table"));
    }
}
