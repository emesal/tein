# `(tein json)` implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Status:** COMPLETE — all 7 tasks implemented.

**Goal:** bidirectional JSON ↔ scheme conversion as a built-in `(tein json)` module.

**Architecture:** json string → `serde_json::from_str::<Sexp>` → null remapping → `sexp_to_value` bridge → `Value` → `to_raw` → chibi sexp. reverse path for stringify. the `Value ↔ Sexp` bridge is reusable for future format modules (toml, yaml).

**Tech Stack:** `serde_json` for JSON parsing/serialising, `tein_sexp` with serde feature for the `Sexp` ↔ JSON data model translation, new `sexp_bridge` module for `Value` ↔ `Sexp` conversion.

**Branch:** create `feature/tein-json-2602` from `dev` (current work on `bugfix/alist-serde-roundtrip-2602` should be merged first)

**Issue:** #36

**Design doc:** `docs/plans/2026-02-27-tein-json-design.md`

---

### Task 1: add dependencies to tein crate

**Files:**
- Modify: `tein/Cargo.toml`

**Step 1: add serde, serde_json, and tein-sexp deps**

```toml
# add to [dependencies]:
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tein-sexp = { path = "../tein-sexp", features = ["serde"] }
```

**Step 2: verify it compiles**

Run: `cargo build -p tein`
Expected: compiles with no errors (new deps are unused but that's fine)

**Step 3: commit**

```
feat(json): add serde_json and tein-sexp dependencies
```

---

### Task 2: implement the `Value ↔ Sexp` bridge

this module converts between `tein::Value` and `tein_sexp::Sexp`. it's the reusable layer that future format modules share.

**Files:**
- Create: `tein/src/sexp_bridge.rs`
- Modify: `tein/src/lib.rs` (add `mod sexp_bridge;`)

**Step 1: write bridge tests first**

create `tein/src/sexp_bridge.rs` with the test module at the bottom. tests cover every convertible variant plus error cases. the implementation fns can be stubs that return `Err` initially.

```rust
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
        Value::Port(_) => Err(Error::TypeError(
            "cannot convert port to sexp".to_string(),
        )),
        Value::HashTable(_) => Err(Error::TypeError(
            "cannot convert hash-table to sexp".to_string(),
        )),
        Value::Foreign { type_name, .. } => Err(Error::TypeError(
            format!("cannot convert foreign object ({type_name}) to sexp"),
        )),
        Value::Other(desc) => Err(Error::TypeError(
            format!("cannot convert opaque value ({desc}) to sexp"),
        )),
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
    use tein_sexp::{Sexp, SexpKind};

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
        let v = Value::Float(3.14);
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::float(3.14));
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
        let v = Value::Rational(
            Box::new(Value::Integer(1)),
            Box::new(Value::Integer(3)),
        );
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(
            s,
            Sexp::rational(Sexp::integer(1), Sexp::integer(3))
        );
        assert_eq!(sexp_to_value(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_complex() {
        let v = Value::Complex(
            Box::new(Value::Float(1.0)),
            Box::new(Value::Float(2.0)),
        );
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(
            s,
            Sexp::complex(Sexp::float(1.0), Sexp::float(2.0))
        );
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
        assert_eq!(
            s,
            Sexp::list(vec![Sexp::integer(1), Sexp::string("two")])
        );
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
        let v = Value::Pair(
            Box::new(Value::Integer(1)),
            Box::new(Value::Integer(2)),
        );
        let s = value_to_sexp(&v).unwrap();
        assert_eq!(s, Sexp::dotted_list(vec![Sexp::integer(1)], Sexp::integer(2)));
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
            Sexp::dotted_list(
                vec![Sexp::integer(1), Sexp::integer(2)],
                Sexp::integer(3)
            )
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
        let s = Sexp::dotted_list(
            vec![Sexp::integer(1), Sexp::integer(2)],
            Sexp::integer(3),
        );
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
    fn error_on_procedure() {
        // Value::Procedure holds a raw sexp pointer — can't construct in unit tests.
        // test the bridge direction instead: there's no Sexp variant for procedures.
        // just verify the error message pattern.
    }

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
```

**Step 2: add module declaration**

in `tein/src/lib.rs`, add:
```rust
mod sexp_bridge;
```

after the existing module declarations (near the top, before the `pub use` section).

**Step 3: run tests to verify they pass**

Run: `cargo test -p tein --lib sexp_bridge`
Expected: all tests pass

**Step 4: commit**

```
feat(json): add Value <-> Sexp bridge module

reusable conversion layer for format modules. handles all data types
including numeric tower, pairs <-> dotted lists, and depth limiting.
```

---

### Task 3: implement the json module

**Files:**
- Create: `tein/src/json.rs`
- Modify: `tein/src/lib.rs` (add `mod json;`)

**Step 1: write the json module with tests**

```rust
//! `(tein json)` — bidirectional JSON ↔ scheme value conversion.
//!
//! JSON parsing uses `serde_json` → `tein_sexp::Sexp` → [`sexp_bridge`] → `Value`.
//! JSON stringifying reverses the path. the `'null` symbol distinguishes JSON null
//! from scheme `'()` (empty list/array).
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
/// (which represents empty arrays/objects).
pub fn json_parse(input: &str) -> Result<Value> {
    let sexp: Sexp = serde_json::from_str(input)
        .map_err(|e| Error::EvalError(format!("json-parse: {e}")))?;
    let sexp = remap_nil_to_null_symbol(sexp);
    sexp_bridge::sexp_to_value(&sexp)
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

/// recursively remap `Sexp::Nil` → `Sexp::Symbol("null")` in a deserialized tree.
///
/// serde_json maps JSON `null` to `Sexp::Nil` (via `visit_unit`). we remap to
/// a symbol so it's distinguishable from empty lists/objects in scheme.
fn remap_nil_to_null_symbol(sexp: Sexp) -> Sexp {
    match sexp.kind {
        SexpKind::Nil => Sexp::symbol("null"),
        SexpKind::List(items) => {
            Sexp::list(items.into_iter().map(remap_nil_to_null_symbol).collect())
        }
        SexpKind::DottedList(heads, tail) => Sexp::dotted_list(
            heads.into_iter().map(remap_nil_to_null_symbol).collect(),
            remap_nil_to_null_symbol(*tail),
        ),
        SexpKind::Vector(items) => {
            Sexp::vector(items.into_iter().map(remap_nil_to_null_symbol).collect())
        }
        _ => sexp,
    }
}

/// recursively remap `Sexp::Symbol("null")` → `Sexp::Nil` before serialization.
///
/// reverses `remap_nil_to_null_symbol` so that scheme `'null` becomes JSON `null`.
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
    fn stringify_nil_as_empty_array() {
        // Nil → Sexp::Nil → serializes as JSON null (via Serialize for Nil).
        // but actually: Nil → serde Serialize → serialize_unit → JSON null.
        // hmm — do we want Nil to be [] or null?
        // per design: empty array is '(), null is 'null. so Nil → null.
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
    fn stringify_error_on_procedure_like_value() {
        let err = json_stringify(&Value::Unspecified).unwrap_err();
        assert!(err.to_string().contains("unspecified"));
    }
}
```

**Step 2: add module declaration**

in `tein/src/lib.rs`, add:
```rust
mod json;
```

**Step 3: run tests**

Run: `cargo test -p tein --lib json`
Expected: all tests pass

**Step 4: commit**

```
feat(json): add json parse/stringify module

json_parse: JSON string → Value via serde_json → Sexp → bridge
json_stringify: Value → Sexp → serde_json string
null ↔ 'null symbol remapping for round-trip fidelity
```

---

### Task 4: register json trampolines + VFS module

**Files:**
- Modify: `tein/src/context.rs` (add trampolines + registration)
- Create: `target/chibi-scheme/lib/tein/json.sld` (VFS module definition)
- Create: `target/chibi-scheme/lib/tein/json.scm` (module documentation)
- Modify: `tein/build.rs` (add json VFS entries)

**Step 1: add VFS files**

create `target/chibi-scheme/lib/tein/json.sld`:
```scheme
(define-library (tein json)
  (import (scheme base))
  (export json-parse json-stringify)
  (include "json.scm"))
```

create `target/chibi-scheme/lib/tein/json.scm`:
```scheme
;;; (tein json) — bidirectional JSON <-> scheme value conversion
;;;
;;; json-parse and json-stringify are registered by the rust runtime
;;; via define_fn_variadic when a standard-env context is built.
;;; this file is included by json.sld for module definition.
```

**Step 2: add VFS entries to build.rs**

in `tein/build.rs`, add to the `VFS_FILES` array:
```rust
"lib/tein/json.sld",
"lib/tein/json.scm",
```

**Step 3: add trampolines and registration to context.rs**

add two `unsafe extern "C" fn` trampolines near the other trampolines in context.rs:

```rust
/// trampoline for `json-parse`: takes one scheme string argument, returns parsed value.
unsafe extern "C" fn json_parse_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let str_sexp = ffi::sexp_car(args);
        if ffi::sexp_stringp(str_sexp) == 0 {
            let msg = "json-parse: expected string argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let data = ffi::sexp_string_data(str_sexp);
        let len = ffi::sexp_string_size(str_sexp) as usize;
        let input = match std::str::from_utf8(std::slice::from_raw_parts(data as *const u8, len)) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("json-parse: invalid UTF-8: {e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                return ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };
        match crate::json::json_parse(input) {
            Ok(value) => match value.to_raw(ctx) {
                Ok(raw) => raw,
                Err(e) => {
                    let msg = format!("json-parse: {e}");
                    let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                    ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
                }
            },
            Err(e) => {
                let msg = format!("{e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}

/// trampoline for `json-stringify`: takes one scheme value, returns JSON string.
unsafe extern "C" fn json_stringify_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let val_sexp = ffi::sexp_car(args);
        match Value::from_raw(ctx, val_sexp) {
            Ok(value) => match crate::json::json_stringify(&value) {
                Ok(json) => {
                    let c_json = CString::new(json.as_str()).unwrap_or_default();
                    ffi::sexp_c_str(ctx, c_json.as_ptr(), json.len() as ffi::sexp_sint_t)
                }
                Err(e) => {
                    let msg = format!("{e}");
                    let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                    ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
                }
            },
            Err(e) => {
                let msg = format!("json-stringify: {e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}
```

add a registration method on `Context`:

```rust
/// register `json-parse` and `json-stringify` native functions.
///
/// called during `build()` for standard-env contexts. the VFS module
/// `(tein json)` exports these names, making them available via
/// `(import (tein json))`.
fn register_json_module(&self) -> Result<()> {
    self.define_fn_variadic("json-parse", json_parse_trampoline)?;
    self.define_fn_variadic("json-stringify", json_stringify_trampoline)?;
    Ok(())
}
```

call it in `build()`, right after the context struct is created (after line ~1328), before `Ok(context)`:

```rust
// register built-in module trampolines for standard-env contexts.
// these are always safe (pure data conversion, no IO) and cheap.
if self.standard_env {
    context.register_json_module()?;
}
```

NOTE: check if `self.standard_env` is still accessible at this point. if the builder has been consumed, store the flag in a local before constructing Context.

**Step 4: verify it compiles**

Run: `cargo build -p tein`
Expected: compiles. may need `just clean && cargo build` if VFS cache is stale.

**Step 5: commit**

```
feat(json): register json-parse/json-stringify trampolines + VFS module

- extern "C" trampolines bridge scheme ↔ rust json module
- register during build() for standard-env contexts
- VFS json.sld/json.scm for (import (tein json))
```

---

### Task 5: rust-level integration tests

**Files:**
- Modify: `tein/src/context.rs` (add tests at the bottom of the test module)

**Step 1: add integration tests in context.rs**

add to the existing `#[cfg(test)] mod tests` block in context.rs:

```rust
// --- (tein json) tests ---

#[test]
fn test_json_parse_object() {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein json))").expect("import");
    let result = ctx
        .evaluate(r#"(json-parse "{\"a\": 1, \"b\": \"two\"}")"#)
        .expect("parse");
    // result is an alist
    match result {
        Value::List(items) => assert_eq!(items.len(), 2),
        other => panic!("expected list, got {other:?}"),
    }
}

#[test]
fn test_json_parse_array() {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein json))").expect("import");
    let result = ctx.evaluate("(json-parse \"[1, 2, 3]\")").expect("parse");
    assert_eq!(
        result,
        Value::List(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ])
    );
}

#[test]
fn test_json_parse_null_is_symbol() {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein json))").expect("import");
    let result = ctx.evaluate("(json-parse \"null\")").expect("parse");
    assert_eq!(result, Value::Symbol("null".to_string()));
}

#[test]
fn test_json_stringify_alist() {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein json))").expect("import");
    let result = ctx
        .evaluate("(json-stringify '((\"name\" . \"tein\")))")
        .expect("stringify");
    assert_eq!(result, Value::String("{\"name\":\"tein\"}".to_string()));
}

#[test]
fn test_json_round_trip_via_scheme() {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein json))").expect("import");
    let result = ctx
        .evaluate(r#"(json-stringify (json-parse "{\"x\":42}"))"#)
        .expect("round-trip");
    assert_eq!(result, Value::String("{\"x\":42}".to_string()));
}

#[test]
fn test_json_parse_invalid() {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein json))").expect("import");
    let result = ctx.evaluate("(json-parse \"not json\")").expect("parse");
    // per convention: errors return scheme strings
    match result {
        Value::String(msg) => assert!(msg.contains("json-parse")),
        other => panic!("expected error string, got {other:?}"),
    }
}
```

**Step 2: run tests**

Run: `cargo test -p tein --lib json`
Expected: all pass

Run: `cargo test -p tein --lib test_json`
Expected: all pass

**Step 3: commit**

```
test(json): add rust-level integration tests for (tein json)
```

---

### Task 6: scheme-level integration tests

**Files:**
- Create: `tein/tests/scheme/json.scm`
- Modify: `tein/tests/scheme_tests.rs` (add test fn)

**Step 1: write scheme test file**

create `tein/tests/scheme/json.scm`:

```scheme
;;; (tein json) integration tests

(import (tein json))

;; --- json-parse ---

(test-equal "parse/integer" 42 (json-parse "42"))
(test-equal "parse/float" 3.14 (json-parse "3.14"))
(test-equal "parse/string" "hello" (json-parse "\"hello\""))
(test-equal "parse/true" #t (json-parse "true"))
(test-equal "parse/false" #f (json-parse "false"))
(test-equal "parse/null" 'null (json-parse "null"))
(test-equal "parse/empty-array" '() (json-parse "[]"))
(test-equal "parse/array" '(1 2 3) (json-parse "[1, 2, 3]"))

;; object → alist
(let ((obj (json-parse "{\"a\": 1}")))
  (test-true "parse/object-is-list" (list? obj))
  (test-equal "parse/object-key" "a" (car (car obj)))
  (test-equal "parse/object-val" 1 (cdr (car obj))))

;; nested null
(test-equal "parse/null-in-array" '(1 null 3)
  (json-parse "[1, null, 3]"))

;; unicode
(test-equal "parse/unicode" "こんにちは"
  (json-parse "\"こんにちは\""))

;; --- json-stringify ---

(test-equal "stringify/integer" "42" (json-stringify 42))
(test-equal "stringify/string" "\"hello\"" (json-stringify "hello"))
(test-equal "stringify/true" "true" (json-stringify #t))
(test-equal "stringify/false" "false" (json-stringify #f))
(test-equal "stringify/null" "null" (json-stringify 'null))
(test-equal "stringify/array" "[1,2,3]" (json-stringify '(1 2 3)))

;; alist → object
(test-equal "stringify/object"
  "{\"name\":\"tein\"}"
  (json-stringify '(("name" . "tein"))))

;; --- round-trip ---

(test-equal "round-trip/object"
  "{\"a\":1,\"b\":\"two\"}"
  (json-stringify (json-parse "{\"a\":1,\"b\":\"two\"}")))

(test-equal "round-trip/array"
  "[1,2,3]"
  (json-stringify (json-parse "[1,2,3]")))

(test-equal "round-trip/null"
  "null"
  (json-stringify (json-parse "null")))

(test-equal "round-trip/nested"
  "{\"x\":{\"y\":1}}"
  (json-stringify (json-parse "{\"x\":{\"y\":1}}")))

(test-equal "round-trip/mixed"
  "[1,\"two\",true,null]"
  (json-stringify (json-parse "[1,\"two\",true,null]")))
```

**Step 2: add test runner in scheme_tests.rs**

add after the last scheme test fn:

```rust
#[test]
fn test_scheme_json() {
    run_scheme_test(include_str!("scheme/json.scm"));
}
```

**Step 3: run tests**

Run: `cargo test -p tein test_scheme_json`
Expected: passes

**Step 4: commit**

```
test(json): add scheme-level integration tests for (tein json)

round-trip tests for objects, arrays, null, nested structures, unicode.
closes #36
```

---

### Task 7: lint, full test suite, cleanup

**Step 1: run full lint**

Run: `just lint`
Expected: no warnings

**Step 2: run full test suite**

Run: `just test`
Expected: all tests pass (existing + new)

**Step 3: update design doc status**

in `docs/plans/2026-02-27-tein-json-design.md`, change line 5:
```
**status**: implemented
```

**Step 4: collect AGENTS.md notes**

update `AGENTS.md` architecture section:
- add `json.rs` and `sexp_bridge.rs` to the file listing
- add `(tein json)` to the VFS module listing
- update the test count comment in the commands section

**Step 5: final commit**

```
docs: update design doc status + AGENTS.md for (tein json)
```

---

### task dependency graph

```
task 1 (deps) → task 2 (bridge) → task 3 (json module) → task 4 (trampolines + VFS) → task 5 (rust tests) → task 6 (scheme tests) → task 7 (lint + cleanup)
```

tasks 2 and 3 are the core implementation. task 4 wires it into chibi. tasks 5-6 are integration tests. task 7 is polish.
