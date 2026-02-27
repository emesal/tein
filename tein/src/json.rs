//! `(tein json)` — bidirectional JSON ↔ scheme value conversion.
//!
//! JSON parsing goes through `serde_json::Value` (preserving `null` vs `[]`/`{}`)
//! then maps to scheme `Value`. JSON stringifying goes through the `sexp_bridge` and
//! `tein_sexp::Sexp`'s serde `Serialize` impl. the `'null` symbol distinguishes JSON
//! null from scheme `'()` (empty list/array).
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

use crate::sexp_bridge;
use crate::{Error, Result, Value};
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
pub fn json_stringify(value: &Value) -> Result<String> {
    let sexp = sexp_bridge::value_to_sexp(value)?;
    let sexp = remap_null_symbol_to_nil(sexp);
    serde_json::to_string(&sexp)
        .map_err(|e| Error::EvalError(format!("json-stringify: {e}")))
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
