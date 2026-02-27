//! `(tein json)` — bidirectional JSON ↔ scheme value conversion.
//!
//! JSON parsing goes through `serde_json::Value` (preserving `null` vs `[]`/`{}`)
//! then maps to scheme `Value`. JSON stringifying works directly on raw chibi sexps
//! (bypassing `Value::from_raw`) to preserve alist structure — chibi's pair representation
//! collapses `(key . val)` into a proper list when val is itself a proper list, which would
//! lose the dotted-pair structure needed to detect alists. the `'null` symbol distinguishes
//! JSON null from scheme `'()` (empty list/array).
//!
//! ## representation
//!
//! | JSON         | scheme                    |
//! |--------------|---------------------------|
//! | object `{}`  | alist `((key . val) ...)` |
//! | array `[]`   | list `(...)`              |
//! | string       | string                    |
//! | integer      | integer / bignum          |
//! | float        | flonum                    |
//! | `true/false` | `#t / #f`                 |
//! | `null`       | `'null` symbol            |

use crate::{Error, Result, Value, ffi};
use tein_sexp::{Sexp, SexpKind};

/// parse a JSON string into a scheme `Value`.
///
/// JSON null becomes `Value::Symbol("null")` to distinguish from `Value::Nil`
/// (empty list/array). empty `[]` becomes `Value::Nil` and empty `{}` becomes
/// `Value::Nil`.
pub fn json_parse(input: &str) -> Result<Value> {
    let jv: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| Error::EvalError(format!("json-parse: {e}")))?;
    json_value_to_value(jv)
}

/// stringify a scheme `Value` as JSON.
///
/// `Value::Symbol("null")` becomes JSON `null`. values that can't be
/// represented in JSON (procedures, ports, etc.) produce an error.
///
/// note: this path is for rust callers using `Value` directly. the scheme
/// trampoline uses `json_stringify_raw` to preserve alist structure.
pub fn json_stringify(value: &Value) -> Result<String> {
    use crate::sexp_bridge;
    use tein_sexp::Sexp;
    let sexp = sexp_bridge::value_to_sexp(value)?;
    let sexp = remap_null_symbol_to_nil(sexp);
    serde_json::to_string(&sexp)
        .map_err(|e| Error::EvalError(format!("json-stringify: {e}")))
}

/// stringify a raw chibi sexp as JSON.
///
/// works directly on raw sexps to preserve alist structure. chibi's pair
/// representation collapses dotted pairs into proper lists when the cdr is
/// a proper list (e.g. `("x" . (("y" . 1)))` → proper list `("x" ("y" . 1))`
/// after `Value::from_raw` — losing the pair structure). this function
/// detects alist entries at the chibi level by checking the car's type.
///
/// an alist is detected when a proper list's every element is a cons pair
/// whose car is a string or symbol.
///
/// # safety
/// ctx and sexp must be valid chibi sexp pointers. called from trampoline.
pub unsafe fn json_stringify_raw(ctx: ffi::sexp, sexp: ffi::sexp) -> Result<String> {
    unsafe { json_sexp_to_value(ctx, sexp, 0) }
}

/// maximum nesting depth for recursive sexp-to-json conversion.
const MAX_DEPTH: usize = 10_000;

/// convert a chibi sexp directly to a JSON string, preserving alist structure.
///
/// alist detection: a proper list is an alist iff every element is a cons pair
/// with a string or symbol car. scheme's `(key . val)` is stored as a cons pair
/// regardless of whether val is a proper list, so `sexp_pairp(car)` and
/// `sexp_stringp(sexp_car(car))` reliably detects alist entries.
unsafe fn json_sexp_to_value(
    ctx: ffi::sexp,
    sexp: ffi::sexp,
    depth: usize,
) -> Result<String> {
    if depth > MAX_DEPTH {
        return Err(Error::EvalError(
            "json-stringify: maximum nesting depth exceeded".to_string(),
        ));
    }
    unsafe {
        if ffi::sexp_booleanp(sexp) != 0 {
            if sexp == ffi::sexp_make_boolean(true) {
                return Ok("true".to_string());
            } else {
                return Ok("false".to_string());
            }
        }
        if ffi::sexp_nullp(sexp) != 0 {
            // scheme '() (empty list/empty object) → JSON null
            return Ok("null".to_string());
        }
        if ffi::sexp_symbolp(sexp) != 0 {
            // check for 'null symbol → JSON null
            let sym_ptr = ffi::sexp_string_data(ffi::sexp_symbol_to_string(ctx, sexp));
            let sym_len = ffi::sexp_string_size(ffi::sexp_symbol_to_string(ctx, sexp)) as usize;
            let sym = std::str::from_utf8(std::slice::from_raw_parts(sym_ptr as *const u8, sym_len))
                .map_err(|e| Error::EvalError(format!("json-stringify: symbol UTF-8 error: {e}")))?;
            if sym == "null" {
                return Ok("null".to_string());
            }
            return Err(Error::TypeError(format!(
                "json-stringify: cannot convert symbol '{sym}' to JSON (only 'null is allowed)"
            )));
        }
        if ffi::sexp_stringp(sexp) != 0 {
            let ptr = ffi::sexp_string_data(sexp);
            let len = ffi::sexp_string_size(sexp) as usize;
            let s = std::str::from_utf8(std::slice::from_raw_parts(ptr as *const u8, len))
                .map_err(|e| Error::EvalError(format!("json-stringify: string UTF-8 error: {e}")))?;
            return serde_json::to_string(s)
                .map_err(|e| Error::EvalError(format!("json-stringify: {e}")));
        }
        if ffi::sexp_integerp(sexp) != 0 && ffi::sexp_flonump(sexp) == 0 {
            let n = ffi::sexp_unbox_fixnum(sexp);
            return Ok(n.to_string());
        }
        if ffi::sexp_flonump(sexp) != 0 {
            let f = ffi::sexp_flonum_value(sexp);
            return serde_json::to_string(&f)
                .map_err(|e| Error::EvalError(format!("json-stringify: {e}")));
        }
        if ffi::sexp_pairp(sexp) != 0 {
            // collect the list elements to decide: alist or array?
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

            if is_proper && !elems.is_empty() {
                // check if every element is a cons pair with a string/symbol car → alist (JSON object)
                let all_alist = elems.iter().all(|&elem| {
                    ffi::sexp_pairp(elem) != 0 && {
                        let k = ffi::sexp_car(elem);
                        ffi::sexp_stringp(k) != 0 || ffi::sexp_symbolp(k) != 0
                    }
                });

                if all_alist {
                    // JSON object: { "key": val, ... }
                    let mut out = String::from("{");
                    for (i, &elem) in elems.iter().enumerate() {
                        let k = ffi::sexp_car(elem);
                        let v = ffi::sexp_cdr(elem);
                        // key: string or symbol
                        let key = if ffi::sexp_stringp(k) != 0 {
                            let ptr = ffi::sexp_string_data(k);
                            let len = ffi::sexp_string_size(k) as usize;
                            std::str::from_utf8(
                                std::slice::from_raw_parts(ptr as *const u8, len),
                            )
                            .map_err(|e| Error::EvalError(format!("json-stringify: key UTF-8: {e}")))?
                            .to_string()
                        } else {
                            // symbol key
                            let ss = ffi::sexp_symbol_to_string(ctx, k);
                            let ptr = ffi::sexp_string_data(ss);
                            let len = ffi::sexp_string_size(ss) as usize;
                            std::str::from_utf8(
                                std::slice::from_raw_parts(ptr as *const u8, len),
                            )
                            .map_err(|e| Error::EvalError(format!("json-stringify: key UTF-8: {e}")))?
                            .to_string()
                        };
                        let key_json = serde_json::to_string(&key)
                            .map_err(|e| Error::EvalError(format!("json-stringify: {e}")))?;
                        let val_json = json_sexp_to_value(ctx, v, depth + 1)?;
                        if i > 0 {
                            out.push(',');
                        }
                        out.push_str(&key_json);
                        out.push(':');
                        out.push_str(&val_json);
                    }
                    out.push('}');
                    return Ok(out);
                }
            }

            if is_proper {
                // JSON array: [ elem, ... ]
                let mut out = String::from("[");
                for (i, &elem) in elems.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&json_sexp_to_value(ctx, elem, depth + 1)?);
                }
                out.push(']');
                return Ok(out);
            }

            // improper list (dotted pair) — not valid JSON
            return Err(Error::TypeError(
                "json-stringify: cannot convert improper list (dotted pair) to JSON".to_string(),
            ));
        }

        Err(Error::TypeError(format!(
            "json-stringify: cannot convert scheme value to JSON"
        )))
    }
}

/// convert a `serde_json::Value` into a scheme `Value`, preserving null vs empty.
fn json_value_to_value(jv: serde_json::Value) -> Result<Value> {
    match jv {
        serde_json::Value::Null => Ok(Value::Symbol("null".to_string())),
        serde_json::Value::Bool(b) => Ok(Value::Boolean(b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::Float(f))
            } else {
                // u64 that overflows i64 — treat as bignum string
                Ok(Value::Bignum(n.to_string()))
            }
        }
        serde_json::Value::String(s) => Ok(Value::String(s)),
        serde_json::Value::Array(items) => {
            let values: Result<Vec<Value>> = items.into_iter().map(json_value_to_value).collect();
            let values = values?;
            if values.is_empty() {
                Ok(Value::Nil)
            } else {
                Ok(Value::List(values))
            }
        }
        serde_json::Value::Object(map) => {
            // JSON object → alist: `((key . val) ...)`
            let entries: Result<Vec<Value>> = map
                .into_iter()
                .map(|(k, v)| {
                    let val = json_value_to_value(v)?;
                    Ok(Value::Pair(Box::new(Value::String(k)), Box::new(val)))
                })
                .collect();
            let entries = entries?;
            if entries.is_empty() {
                Ok(Value::Nil)
            } else {
                Ok(Value::List(entries))
            }
        }
    }
}

/// recursively remap `Sexp::Symbol("null")` → `Sexp::Nil` before serialization.
///
/// ensures scheme `'null` becomes JSON `null` in the output.
fn remap_null_symbol_to_nil(sexp: Sexp) -> Sexp {
    match sexp.kind {
        SexpKind::Symbol(ref s) if s == "null" => Sexp::nil(),
        SexpKind::List(items) => {
            Sexp::list(items.into_iter().map(remap_null_symbol_to_nil).collect())
        }
        SexpKind::DottedList(heads, tail) => Sexp::dotted_list(
            heads.into_iter().map(remap_null_symbol_to_nil).collect(),
            remap_null_symbol_to_nil(*tail),
        ),
        SexpKind::Vector(items) => {
            Sexp::vector(items.into_iter().map(remap_null_symbol_to_nil).collect())
        }
        _ => sexp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- json_parse tests ---

    #[test]
    fn parse_object() {
        let v = json_parse(r#"{"name": "tein", "version": 1}"#).unwrap();
        // alist with string keys
        match &v {
            Value::List(items) => {
                assert_eq!(items.len(), 2);
                // serde_json preserves insertion order
                match &items[0] {
                    Value::Pair(k, v) => {
                        assert_eq!(**k, Value::String("name".to_string()));
                        assert_eq!(**v, Value::String("tein".to_string()));
                    }
                    other => panic!("expected pair, got {other:?}"),
                }
            }
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_array() {
        let v = json_parse("[1, 2, 3]").unwrap();
        assert_eq!(
            v,
            Value::List(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3),
            ])
        );
    }

    #[test]
    fn parse_null_becomes_symbol() {
        let v = json_parse("null").unwrap();
        assert_eq!(v, Value::Symbol("null".to_string()));
    }

    #[test]
    fn parse_null_in_array() {
        let v = json_parse("[1, null, 3]").unwrap();
        assert_eq!(
            v,
            Value::List(vec![
                Value::Integer(1),
                Value::Symbol("null".to_string()),
                Value::Integer(3),
            ])
        );
    }

    #[test]
    fn parse_null_in_object() {
        let v = json_parse(r#"{"x": null}"#).unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(_, val) => {
                    assert_eq!(**val, Value::Symbol("null".to_string()));
                }
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_booleans() {
        assert_eq!(json_parse("true").unwrap(), Value::Boolean(true));
        assert_eq!(json_parse("false").unwrap(), Value::Boolean(false));
    }

    #[test]
    fn parse_string() {
        assert_eq!(
            json_parse(r#""hello""#).unwrap(),
            Value::String("hello".to_string())
        );
    }

    #[test]
    fn parse_integer() {
        assert_eq!(json_parse("42").unwrap(), Value::Integer(42));
        assert_eq!(json_parse("-7").unwrap(), Value::Integer(-7));
    }

    #[test]
    fn parse_float() {
        assert_eq!(json_parse("3.14").unwrap(), Value::Float(3.14));
    }

    #[test]
    fn parse_empty_array() {
        // [] → empty list → Value::Nil (same as scheme '())
        assert_eq!(json_parse("[]").unwrap(), Value::Nil);
    }

    #[test]
    fn parse_empty_object() {
        assert_eq!(json_parse("{}").unwrap(), Value::Nil);
    }

    #[test]
    fn parse_nested() {
        let v = json_parse(r#"{"a": [1, {"b": 2}]}"#).unwrap();
        // just verify it doesn't error — structure is complex
        match v {
            Value::List(_) => {} // alist
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn parse_unicode() {
        assert_eq!(
            json_parse(r#""こんにちは""#).unwrap(),
            Value::String("こんにちは".to_string())
        );
    }

    #[test]
    fn parse_invalid_json() {
        let err = json_parse("{bad}").unwrap_err();
        assert!(err.to_string().contains("json-parse"));
    }

    // --- json_stringify tests ---

    #[test]
    fn stringify_alist_as_object() {
        let v = Value::List(vec![Value::Pair(
            Box::new(Value::String("name".to_string())),
            Box::new(Value::String("tein".to_string())),
        )]);
        let json = json_stringify(&v).unwrap();
        assert_eq!(json, r#"{"name":"tein"}"#);
    }

    #[test]
    fn stringify_list_as_array() {
        let v = Value::List(vec![Value::Integer(1), Value::Integer(2)]);
        let json = json_stringify(&v).unwrap();
        assert_eq!(json, "[1,2]");
    }

    #[test]
    fn stringify_null_symbol_as_null() {
        let v = Value::Symbol("null".to_string());
        let json = json_stringify(&v).unwrap();
        assert_eq!(json, "null");
    }

    #[test]
    fn stringify_boolean() {
        assert_eq!(json_stringify(&Value::Boolean(true)).unwrap(), "true");
        assert_eq!(json_stringify(&Value::Boolean(false)).unwrap(), "false");
    }

    #[test]
    fn stringify_nil_as_null() {
        // Nil → Sexp::Nil → serialize_unit() → JSON null
        let json = json_stringify(&Value::Nil).unwrap();
        assert_eq!(json, "null");
    }

    // --- round-trip tests ---

    #[test]
    fn round_trip_object() {
        let json = r#"{"a":1,"b":"two"}"#;
        let v = json_parse(json).unwrap();
        let result = json_stringify(&v).unwrap();
        assert_eq!(result, json);
    }

    #[test]
    fn round_trip_array() {
        let json = "[1,2,3]";
        let v = json_parse(json).unwrap();
        assert_eq!(json_stringify(&v).unwrap(), json);
    }

    #[test]
    fn round_trip_null() {
        let v = json_parse("null").unwrap();
        assert_eq!(json_stringify(&v).unwrap(), "null");
    }

    #[test]
    fn round_trip_nested() {
        let json = r#"{"nested":{"x":10}}"#;
        let v = json_parse(json).unwrap();
        assert_eq!(json_stringify(&v).unwrap(), json);
    }

    #[test]
    fn round_trip_mixed() {
        let json = r#"[1,"two",true,null,{"key":"val"}]"#;
        let v = json_parse(json).unwrap();
        assert_eq!(json_stringify(&v).unwrap(), json);
    }

    #[test]
    fn stringify_error_on_unspecified() {
        let err = json_stringify(&Value::Unspecified).unwrap_err();
        assert!(err.to_string().contains("unspecified"));
    }
}
