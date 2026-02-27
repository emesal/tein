//! bidirectional `Value ↔ Sexp` bridge.
//!
//! converts between the runtime `Value` type (chibi-scheme interop) and the
//! pure-rust `Sexp` AST (serde interop). this is the shared layer that format
//! modules (`(tein json)`, `(tein toml)`, etc.) build upon.
//!
//! ## mapping
//!
//! | `Value`        | `Sexp`         | notes                                      |
//! |----------------|----------------|--------------------------------------------|
//! | `Integer`      | `Integer`      | direct                                     |
//! | `Float`        | `Float`        | direct                                     |
//! | `Bignum`       | `Bignum`       | direct (decimal string)                    |
//! | `Rational`     | `Rational`     | recursive on components                    |
//! | `Complex`      | `Complex`      | recursive on components                    |
//! | `String`       | `String`       | direct                                     |
//! | `Symbol`       | `Symbol`       | direct                                     |
//! | `Boolean`      | `Boolean`      | direct                                     |
//! | `Char`         | `Char`         | direct                                     |
//! | `List`         | `List`         | recursive                                  |
//! | `Vector`       | `Vector`       | recursive                                  |
//! | `Bytevector`   | `Bytevector`   | direct                                     |
//! | `Nil`          | `Nil`          | direct                                     |
//! | `Pair(a, b)`   | `DottedList`   | flatten right-recursive pairs              |
//! | `DottedList`   | `Pair`         | nest into right-recursive pairs (reverse)  |
//! | opaque types   | —              | error (Procedure, Port, etc.)              |

// sexp_bridge is a shared layer for format modules (json, toml, yaml, ...).
// not all fns are called from current code — suppress dead_code until more
// format modules are added.
#![allow(dead_code)]

use crate::{Error, Result, Value};
use tein_sexp::Sexp;

/// maximum nesting depth for recursive conversion (matches `Value::MAX_DEPTH`).
const MAX_DEPTH: usize = 10_000;

/// convert a `Value` to a `Sexp`.
pub fn value_to_sexp(value: &Value) -> Result<Sexp> {
    value_to_sexp_depth(value, 0)
}

/// convert a `Sexp` to a `Value`.
pub fn sexp_to_value(sexp: &Sexp) -> Result<Value> {
    sexp_to_value_depth(sexp, 0)
}

fn value_to_sexp_depth(value: &Value, depth: usize) -> Result<Sexp> {
    if depth > MAX_DEPTH {
        return Err(Error::EvalError(
            "value_to_sexp: maximum nesting depth exceeded".to_string(),
        ));
    }
    match value {
        Value::Integer(n) => Ok(Sexp::integer(*n)),
        Value::Float(f) => Ok(Sexp::float(*f)),
        Value::Bignum(s) => Ok(Sexp::bignum(s.as_str())),
        Value::Rational(n, d) => Ok(Sexp::rational(
            value_to_sexp_depth(n, depth + 1)?,
            value_to_sexp_depth(d, depth + 1)?,
        )),
        Value::Complex(r, i) => Ok(Sexp::complex(
            value_to_sexp_depth(r, depth + 1)?,
            value_to_sexp_depth(i, depth + 1)?,
        )),
        Value::String(s) => Ok(Sexp::string(s.as_str())),
        Value::Symbol(s) => Ok(Sexp::symbol(s.as_str())),
        Value::Boolean(b) => Ok(Sexp::boolean(*b)),
        Value::Char(c) => Ok(Sexp::char(*c)),
        Value::Bytevector(bv) => Ok(Sexp::bytevector(bv.clone())),
        Value::Nil => Ok(Sexp::nil()),
        Value::List(items) => {
            let sexps: Result<Vec<Sexp>> = items
                .iter()
                .map(|v| value_to_sexp_depth(v, depth + 1))
                .collect();
            Ok(Sexp::list(sexps?))
        }
        Value::Vector(items) => {
            let sexps: Result<Vec<Sexp>> = items
                .iter()
                .map(|v| value_to_sexp_depth(v, depth + 1))
                .collect();
            Ok(Sexp::vector(sexps?))
        }
        Value::Pair(a, b) => {
            // flatten right-recursive pairs: (a . (b . (c . d))) → DottedList [a, b, c] d
            let mut heads = vec![value_to_sexp_depth(a, depth + 1)?];
            let mut current = b.as_ref();
            let mut pair_depth = depth + 1;
            loop {
                pair_depth += 1;
                if pair_depth > MAX_DEPTH {
                    return Err(Error::EvalError(
                        "value_to_sexp: maximum nesting depth exceeded".to_string(),
                    ));
                }
                match current {
                    Value::Pair(car, cdr) => {
                        heads.push(value_to_sexp_depth(car, pair_depth)?);
                        current = cdr.as_ref();
                    }
                    Value::Nil => {
                        // proper list encoded as pairs — emit as List
                        return Ok(Sexp::list(heads));
                    }
                    tail => {
                        let tail_sexp = value_to_sexp_depth(tail, pair_depth)?;
                        return Ok(Sexp::dotted_list(heads, tail_sexp));
                    }
                }
            }
        }
        // opaque types cannot be represented in the pure-rust AST
        Value::Procedure(_) => Err(Error::TypeError(
            "cannot convert procedure to sexp".to_string(),
        )),
        Value::Port(_) => Err(Error::TypeError("cannot convert port to sexp".to_string())),
        Value::HashTable(_) => Err(Error::TypeError(
            "cannot convert hash-table to sexp".to_string(),
        )),
        Value::Foreign { type_name, .. } => Err(Error::TypeError(format!(
            "cannot convert foreign object ({type_name}) to sexp"
        ))),
        Value::Other(desc) => Err(Error::TypeError(format!(
            "cannot convert opaque value ({desc}) to sexp"
        ))),
        Value::Unspecified => Err(Error::TypeError(
            "cannot convert unspecified to sexp".to_string(),
        )),
    }
}

fn sexp_to_value_depth(sexp: &Sexp, depth: usize) -> Result<Value> {
    use tein_sexp::SexpKind;

    if depth > MAX_DEPTH {
        return Err(Error::EvalError(
            "sexp_to_value: maximum nesting depth exceeded".to_string(),
        ));
    }
    match &sexp.kind {
        SexpKind::Integer(n) => Ok(Value::Integer(*n)),
        SexpKind::Float(f) => Ok(Value::Float(*f)),
        SexpKind::Bignum(s) => Ok(Value::Bignum(s.clone())),
        SexpKind::Rational(n, d) => Ok(Value::Rational(
            Box::new(sexp_to_value_depth(n, depth + 1)?),
            Box::new(sexp_to_value_depth(d, depth + 1)?),
        )),
        SexpKind::Complex(r, i) => Ok(Value::Complex(
            Box::new(sexp_to_value_depth(r, depth + 1)?),
            Box::new(sexp_to_value_depth(i, depth + 1)?),
        )),
        SexpKind::String(s) => Ok(Value::String(s.clone())),
        SexpKind::Symbol(s) => Ok(Value::Symbol(s.clone())),
        SexpKind::Boolean(b) => Ok(Value::Boolean(*b)),
        SexpKind::Char(c) => Ok(Value::Char(*c)),
        SexpKind::Bytevector(bv) => Ok(Value::Bytevector(bv.clone())),
        SexpKind::Nil => Ok(Value::Nil),
        SexpKind::List(items) => {
            let values: Result<Vec<Value>> = items
                .iter()
                .map(|s| sexp_to_value_depth(s, depth + 1))
                .collect();
            Ok(Value::List(values?))
        }
        SexpKind::Vector(items) => {
            let values: Result<Vec<Value>> = items
                .iter()
                .map(|s| sexp_to_value_depth(s, depth + 1))
                .collect();
            Ok(Value::Vector(values?))
        }
        SexpKind::DottedList(heads, tail) => {
            // nest into right-recursive pairs: DottedList [a, b, c] d → (a . (b . (c . d)))
            let tail_val = sexp_to_value_depth(tail, depth + 1)?;
            heads.iter().rev().try_fold(tail_val, |acc, head| {
                let head_val = sexp_to_value_depth(head, depth + 1)?;
                Ok(Value::Pair(Box::new(head_val), Box::new(acc)))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tein_sexp::Sexp;

    // --- round-trip tests ---

    #[test]
    fn round_trip_integer() {
        let v = Value::Integer(42);
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::integer(42));
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_float() {
        let v = Value::Float(2.5);
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::float(2.5));
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_bignum() {
        let v = Value::Bignum("12345678901234567890".to_string());
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::bignum("12345678901234567890"));
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_rational() {
        let v = Value::Rational(Box::new(Value::Integer(1)), Box::new(Value::Integer(3)));
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::rational(Sexp::integer(1), Sexp::integer(3)));
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_complex() {
        let v = Value::Complex(Box::new(Value::Float(1.0)), Box::new(Value::Float(2.0)));
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::complex(Sexp::float(1.0), Sexp::float(2.0)));
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_string() {
        let v = Value::String("hello".to_string());
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::string("hello"));
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_symbol() {
        let v = Value::Symbol("foo".to_string());
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::symbol("foo"));
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_boolean() {
        for b in [true, false] {
            let v = Value::Boolean(b);
            let s = value_to_sexp(&v).unwrap();
            assert_eq!(s, Sexp::boolean(b));
            assert_eq!(sexp_to_value(&s).unwrap(), v);
        }
    }

    #[test]
    fn round_trip_char() {
        let v = Value::Char('λ');
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::char('λ'));
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_bytevector() {
        let v = Value::Bytevector(vec![1, 2, 3]);
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::bytevector(vec![1, 2, 3]));
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_nil() {
        let v = Value::Nil;
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::nil());
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_list() {
        let v = Value::List(vec![Value::Integer(1), Value::String("two".to_string())]);
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::list(vec![Sexp::integer(1), Sexp::string("two")]));
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_vector() {
        let v = Value::Vector(vec![Value::Boolean(true)]);
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::vector(vec![Sexp::boolean(true)]));
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_pair_to_dotted_list() {
        // (1 . 2) → DottedList [1] 2
        let v = Value::Pair(Box::new(Value::Integer(1)), Box::new(Value::Integer(2)));
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(
            s,
            Sexp::dotted_list(vec![Sexp::integer(1)], Sexp::integer(2))
        );
    }

    #[test]
    fn round_trip_nested_pairs_flatten() {
        // (1 . (2 . 3)) → DottedList [1, 2] 3
        let v = Value::Pair(
            Box::new(Value::Integer(1)),
            Box::new(Value::Pair(
                Box::new(Value::Integer(2)),
                Box::new(Value::Integer(3)),
            )),
        );
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(
            s,
            Sexp::dotted_list(vec![Sexp::integer(1), Sexp::integer(2)], Sexp::integer(3))
        );
    }

    #[test]
    fn round_trip_pair_nil_becomes_list() {
        // (1 . (2 . ())) → List [1, 2]  (proper list)
        let v = Value::Pair(
            Box::new(Value::Integer(1)),
            Box::new(Value::Pair(
                Box::new(Value::Integer(2)),
                Box::new(Value::Nil),
            )),
        );
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::list(vec![Sexp::integer(1), Sexp::integer(2)]));
    }

    #[test]
    fn dotted_list_to_pair() {
        // DottedList [1, 2] 3 → (1 . (2 . 3))
        let s = Sexp::dotted_list(vec![Sexp::integer(1), Sexp::integer(2)], Sexp::integer(3));
        let v = sexp_to_value(&s).unwrap();
        assert_eq!(
            v,
            Value::Pair(
                Box::new(Value::Integer(1)),
                Box::new(Value::Pair(
                    Box::new(Value::Integer(2)),
                    Box::new(Value::Integer(3)),
                ))
            )
        );
    }

    // --- error cases ---

    #[test]
    fn error_on_unspecified() {
        let err = value_to_sexp(&Value::Unspecified).unwrap_err();
        assert!(err.to_string().contains("unspecified"));
    }

    #[test]
    fn error_on_other() {
        let err = value_to_sexp(&Value::Other("unknown".to_string())).unwrap_err();
        assert!(err.to_string().contains("unknown"));
    }

    // --- alist structure tests (important for json) ---

    #[test]
    fn value_alist_becomes_sexp_alist() {
        // ((key . val)) → sexp alist (which serializes as JSON object)
        let v = Value::List(vec![Value::Pair(
            Box::new(Value::String("name".to_string())),
            Box::new(Value::String("tein".to_string())),
        )]);
        let s = value_to_sexp(&v).unwrap();
        let expected = Sexp::list(vec![Sexp::dotted_list(
            vec![Sexp::string("name")],
            Sexp::string("tein"),
        )]);
        assert_eq!(s, expected);
        assert!(s.is_alist());
    }
}
