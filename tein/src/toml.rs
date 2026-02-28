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

use crate::{Error, Result, Value};

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
                let items: Result<Vec<Value>> =
                    arr.into_iter().map(toml_value_to_value).collect();
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
        let v = toml_parse("x = 3.14").unwrap();
        match &v {
            Value::List(items) => match &items[0] {
                Value::Pair(_, v) => assert_eq!(**v, Value::Float(3.14)),
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
                        assert_eq!(
                            inner[1],
                            Value::String("1979-05-27T07:32:00".to_string())
                        );
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
}
