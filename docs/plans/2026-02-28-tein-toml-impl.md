# `(tein toml)` Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** add `(tein toml)` format module with parse/stringify and tagged datetime representation, closes #77

**Architecture:** mirrors `(tein json)` — `toml.rs` for conversion logic, trampolines in `context.rs`, VFS module in chibi fork, feature-gated behind `toml` cargo feature. parse goes through `toml::Value` → `tein::Value`. stringify builds `toml::Value` from raw chibi sexps then delegates to `toml::to_string()`.

**Tech Stack:** `toml` crate 1.0 (parse + display features only, no serde), chibi-scheme VFS

**Design doc:** `docs/plans/2026-02-28-tein-toml-design.md`

---

### Task 1: add `toml` dependency and cargo feature

**Files:**
- Modify: `tein/Cargo.toml`

**Step 1: add the dependency and feature**

in `tein/Cargo.toml`, add to `[dependencies]`:
```toml
toml_crate = { package = "toml", version = "1.0", default-features = false, features = ["parse", "display"], optional = true }
```

add to `[features]`:
```toml
toml = ["dep:toml_crate"]
```

update default:
```toml
default = ["json", "toml"]
```

**Step 2: verify it compiles**

Run: `cargo build -p tein`

**Step 3: verify feature gating works**

Run: `cargo build -p tein --no-default-features`

both should succeed.

**Step 4: commit**

```
feat: add toml crate dependency behind `toml` cargo feature (#77)
```

---

### Task 2: create `toml.rs` — `toml_parse`

**Files:**
- Create: `tein/src/toml.rs`

**Step 1: write failing test for basic parse**

in `tein/src/toml.rs`, create the module with a test that calls `toml_parse`:

```rust
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
/// tables become alists, arrays become lists, datetimes become tagged
/// lists `(toml-datetime "iso-string")`.
pub fn toml_parse(input: &str) -> Result<Value> {
    let tv: toml_crate::Value = input
        .parse()
        .map_err(|e| Error::EvalError(format!("toml-parse: {e}")))?;
    toml_value_to_value(tv)
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
```

**Step 2: wire up the module in `lib.rs`**

in `tein/src/lib.rs`, add after the json module gate (line ~61):

```rust
#[cfg(feature = "toml")]
mod toml;
```

**Step 3: run tests**

Run: `cargo test -p tein --lib toml`

expected: all parse tests pass.

**Step 4: commit**

```
feat(toml): add toml_parse — TOML string to scheme Value (#77)
```

---

### Task 3: add `toml_stringify_raw` to `toml.rs`

**Files:**
- Modify: `tein/src/toml.rs`

**Step 1: write the stringify function and tests**

add to `tein/src/toml.rs`, after `toml_value_to_value`:

```rust
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
        toml_crate::Value::Table(t) => toml_crate::to_string(&t)
            .map_err(|e| Error::EvalError(format!("toml-stringify: {e}"))),
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
        let sym = match std::str::from_utf8(std::slice::from_raw_parts(
            sym_ptr as *const u8,
            sym_len,
        )) {
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
```

add the `ffi` import at the top of the file:

```rust
use crate::{Error, Result, Value, ffi};
```

**Step 2: add stringify tests to the test module**

add after the existing tests in the `mod tests` block (these are rust-level tests using `toml_parse` then verifying the round-trip; the raw-sexp stringify path gets tested via scheme integration tests):

```rust
    // --- toml_stringify tests (via rust-only round-trip) ---

    /// helper: parse then re-stringify via the rust-only Value path.
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
                if items.len() == 2 {
                    if let Value::Symbol(tag) = &items[0] {
                        if tag == DATETIME_TAG {
                            if let Value::String(s) = &items[1] {
                                let dt: toml_crate::value::Datetime =
                                    s.parse().map_err(|e| {
                                        Error::EvalError(format!("toml-stringify: {e}"))
                                    })?;
                                return Ok(toml_crate::Value::Datetime(dt));
                            }
                        }
                    }
                }
                // alist check
                let is_alist = items.iter().all(|v| {
                    matches!(v, Value::Pair(k, _) if matches!(k.as_ref(), Value::String(_)))
                });
                if is_alist {
                    let mut table = toml_crate::map::Map::new();
                    for item in items {
                        if let Value::Pair(k, v) = item {
                            if let Value::String(key) = k.as_ref() {
                                table.insert(key.clone(), value_to_toml_value(v)?);
                            }
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
```

**Step 3: run tests**

Run: `cargo test -p tein --lib toml`

expected: all tests pass.

**Step 4: commit**

```
feat(toml): add toml_stringify_raw — scheme Value to TOML string (#77)
```

---

### Task 4: add trampolines and registration in `context.rs`

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: add trampolines**

after the json trampolines (~line 841), add:

```rust
// --- toml trampolines (gated behind "toml" feature) ---

#[cfg(feature = "toml")]
/// Trampoline for `toml-parse`: takes one scheme string argument, returns parsed value.
///
/// On parse error or type mismatch, returns a scheme string with the error message.
/// This matches tein's convention for native function errors (see AGENTS.md).
unsafe extern "C" fn toml_parse_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let str_sexp = ffi::sexp_car(args);
        if ffi::sexp_stringp(str_sexp) == 0 {
            let msg = "toml-parse: expected string argument";
            let c_msg = CString::new(msg).unwrap_or_default();
            return ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let data = ffi::sexp_string_data(str_sexp);
        let len = ffi::sexp_string_size(str_sexp) as usize;
        let input = match std::str::from_utf8(std::slice::from_raw_parts(data as *const u8, len)) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("toml-parse: invalid UTF-8: {e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                return ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };
        match crate::toml::toml_parse(input) {
            Ok(value) => match value.to_raw(ctx) {
                Ok(raw) => raw,
                Err(e) => {
                    let msg = format!("toml-parse: {e}");
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

#[cfg(feature = "toml")]
/// Trampoline for `toml-stringify`: takes one scheme value, returns TOML string.
///
/// Works directly on raw chibi sexps via `toml::toml_stringify_raw` to preserve
/// alist structure, then delegates to `toml::to_string()` for correct formatting.
///
/// On conversion error, returns a scheme string with the error message.
/// This matches tein's convention for native function errors (see AGENTS.md).
unsafe extern "C" fn toml_stringify_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let val_sexp = ffi::sexp_car(args);
        match crate::toml::toml_stringify_raw(ctx, val_sexp) {
            Ok(toml_str) => {
                let c_str = CString::new(toml_str.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_str.as_ptr(), toml_str.len() as ffi::sexp_sint_t)
            }
            Err(e) => {
                let msg = format!("{e}");
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}
```

**Step 2: add `register_toml_module`**

after `register_json_module()` (~line 2397), add:

```rust
    #[cfg(feature = "toml")]
    /// Register `toml-parse` and `toml-stringify` native functions.
    ///
    /// Called during `build()` for standard-env contexts. the VFS module
    /// `(tein toml)` exports these names, making them available via
    /// `(import (tein toml))`.
    fn register_toml_module(&self) -> Result<()> {
        self.define_fn_variadic("toml-parse", toml_parse_trampoline)?;
        self.define_fn_variadic("toml-stringify", toml_stringify_trampoline)?;
        Ok(())
    }
```

**Step 3: add call site in `build()`**

after the json registration block (~line 1415), add:

```rust
            #[cfg(feature = "toml")]
            if self.standard_env {
                context.register_toml_module()?;
            }
```

**Step 4: verify it compiles**

Run: `cargo build -p tein`

**Step 5: commit**

```
feat(toml): add trampolines and registration in context.rs (#77)
```

---

### Task 5: add VFS module files and build.rs gating

**Files:**
- Create: `target/chibi-scheme/lib/tein/toml.sld` (in the chibi fork)
- Create: `target/chibi-scheme/lib/tein/toml.scm` (in the chibi fork)
- Modify: `tein/build.rs`

**Step 1: create VFS files in the chibi fork**

`target/chibi-scheme/lib/tein/toml.sld`:
```scheme
(define-library (tein toml)
  (import (scheme base))
  (export toml-parse toml-stringify)
  (include "toml.scm"))
```

`target/chibi-scheme/lib/tein/toml.scm`:
```scheme
;;; (tein toml) — bidirectional TOML <-> scheme value conversion
;;;
;;; toml-parse and toml-stringify are registered by the rust runtime
;;; via define_fn_variadic when a standard-env context is built.
;;; this file is included by toml.sld for module definition.
```

**Step 2: add VFS gating in build.rs**

after `VFS_FILES_JSON` (~line 91), add:

```rust
/// VFS files gated behind the "toml" cargo feature.
const VFS_FILES_TOML: &[&str] = &["lib/tein/toml.sld", "lib/tein/toml.scm"];
```

in `main()`, after the json VFS extend (~line 184), add:

```rust
    if cfg!(feature = "toml") {
        vfs_files.extend_from_slice(VFS_FILES_TOML);
    }
```

**Step 3: verify it compiles and VFS loads**

Run: `cargo build -p tein`

**Step 4: commit**

```
feat(toml): add VFS module files and build.rs gating (#77)
```

---

### Task 6: add integration tests

**Files:**
- Create: `tein/tests/scheme/toml.scm`
- Modify: `tein/tests/scheme_tests.rs`
- Modify: `tein/src/context.rs` (add rust integration tests)

**Step 1: create scheme integration test**

`tein/tests/scheme/toml.scm`:
```scheme
;;; (tein toml) integration tests

(import (tein toml))

;; --- toml-parse ---

;; basic types
(let ((v (toml-parse "x = 42")))
  (test-equal "parse/integer" 42 (cdr (car v))))

(let ((v (toml-parse "x = 3.14")))
  (test-equal "parse/float" 3.14 (cdr (car v))))

(let ((v (toml-parse "x = \"hello\"")))
  (test-equal "parse/string" "hello" (cdr (car v))))

(let ((v (toml-parse "x = true")))
  (test-equal "parse/true" #t (cdr (car v))))

(let ((v (toml-parse "x = false")))
  (test-equal "parse/false" #f (cdr (car v))))

;; arrays
(let ((v (toml-parse "x = [1, 2, 3]")))
  (test-equal "parse/array" '(1 2 3) (cdr (car v))))

(let ((v (toml-parse "x = []")))
  (test-equal "parse/empty-array" '() (cdr (car v))))

;; nested table
(let ((v (toml-parse "[server]\nhost = \"localhost\"\nport = 8080")))
  (test-true "parse/nested-table" (list? v))
  (test-equal "parse/nested-key" "server" (car (car v)))
  (test-true "parse/nested-val-is-alist" (list? (cdr (car v)))))

;; datetime — all 4 variants
(let ((v (toml-parse "dt = 1979-05-27T07:32:00Z")))
  (test-equal "parse/datetime-offset"
    (list 'toml-datetime "1979-05-27T07:32:00Z")
    (cdr (car v))))

(let ((v (toml-parse "dt = 1979-05-27T07:32:00")))
  (test-equal "parse/datetime-local"
    (list 'toml-datetime "1979-05-27T07:32:00")
    (cdr (car v))))

(let ((v (toml-parse "dt = 1979-05-27")))
  (test-equal "parse/datetime-date"
    (list 'toml-datetime "1979-05-27")
    (cdr (car v))))

(let ((v (toml-parse "dt = 07:32:00")))
  (test-equal "parse/datetime-time"
    (list 'toml-datetime "07:32:00")
    (cdr (car v))))

;; --- toml-stringify ---

(test-true "stringify/simple"
  (string? (toml-stringify '(("name" . "tein")))))

;; --- round-trip ---

(test-equal "round-trip/datetime"
  (list 'toml-datetime "1979-05-27T07:32:00Z")
  (cdr (car (toml-parse (toml-stringify
    '(("dt" . (toml-datetime "1979-05-27T07:32:00Z"))))))))
```

**Step 2: add scheme test runner entry**

in `tein/tests/scheme_tests.rs`, after the json test (~line 173), add:

```rust
#[cfg(feature = "toml")]
#[test]
fn test_scheme_toml() {
    run_scheme_test(include_str!("scheme/toml.scm"));
}
```

**Step 3: add rust integration tests in `context.rs`**

at the end of the test module in `context.rs` (before closing `}`), add:

```rust
    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_parse_table() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein toml))").expect("import");
        let result = ctx
            .evaluate("(toml-parse \"name = \\\"tein\\\"\\nversion = 1\")")
            .expect("parse");
        match result {
            Value::List(items) => assert_eq!(items.len(), 2),
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_parse_datetime() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein toml))").expect("import");
        let result = ctx
            .evaluate("(cdr (car (toml-parse \"dt = 1979-05-27T07:32:00Z\")))")
            .expect("parse");
        // should be a tagged list (toml-datetime "1979-05-27T07:32:00Z")
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], Value::Symbol("toml-datetime".to_string()));
                assert_eq!(
                    items[1],
                    Value::String("1979-05-27T07:32:00Z".to_string())
                );
            }
            other => panic!("expected tagged list, got {other:?}"),
        }
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_stringify_table() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein toml))").expect("import");
        let result = ctx
            .evaluate("(toml-stringify '((\"name\" . \"tein\")))")
            .expect("stringify");
        match result {
            Value::String(s) => assert!(s.contains("name = \"tein\"")),
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_round_trip_via_scheme() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein toml))").expect("import");
        let result = ctx
            .evaluate(
                "(cdr (car (toml-parse (toml-stringify '((\"x\" . 42))))))",
            )
            .expect("round-trip");
        assert_eq!(result, Value::Integer(42));
    }

    #[cfg(feature = "toml")]
    #[test]
    fn test_toml_parse_invalid() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein toml))").expect("import");
        let result = ctx
            .evaluate("(toml-parse \"not valid {{toml\")")
            .expect("parse");
        match result {
            Value::String(msg) => assert!(msg.contains("toml-parse")),
            other => panic!("expected error string, got {other:?}"),
        }
    }
```

**Step 4: run all tests**

Run: `cargo test -p tein toml`

expected: all toml tests pass (unit + integration + scheme).

**Step 5: commit**

```
feat(toml): add integration tests — scheme and rust (#77)
```

---

### Task 7: update docs and feature-gate verification

**Files:**
- Modify: `tein/src/lib.rs` (feature flags table)
- Modify: `tein/AGENTS.md` (architecture + commands)

**Step 1: update feature flags table in lib.rs**

add the toml row to the feature flags table (~line 42):

```
//! | `toml`  | yes     | Enables `(tein toml)` module with `toml-parse` and `toml-stringify`. Pulls in `toml` crate. |
```

**Step 2: update AGENTS.md**

- add `toml.rs` to architecture section:
  ```
  toml.rs    — toml_parse (TOML string → Value) + toml_stringify_raw (raw sexp → TOML string);
               datetimes as tagged lists (toml-datetime "..."). registered via trampolines in context.rs.
               feature-gated behind `toml` cargo feature
  ```
- add `lib/tein/toml.sld` / `lib/tein/toml.scm` to the chibi-scheme tree listing
- update test count in commands section

**Step 3: verify feature gating**

Run: `cargo build -p tein --no-default-features --features toml`
Run: `cargo build -p tein --no-default-features`
Run: `cargo test -p tein --no-default-features`

all should succeed (the no-features build has no toml/json).

**Step 4: run full test suite**

Run: `just test`

**Step 5: run lint**

Run: `just lint`

**Step 6: commit**

```
docs: update lib.rs feature table and AGENTS.md for (tein toml) (#77)
```

---

### Task 8: final verification and cleanup

**Step 1: run full test suite one more time**

Run: `just test`

**Step 2: check for any AGENTS.md notes**

review the implementation for any gotchas or quirks worth documenting.

known items to consider:
- `toml-datetime` tagged list convention
- toml top-level must be a table (alist) — `toml-stringify` errors on non-table
- empty table/array ambiguity (same as json)
- the dep is renamed `toml_crate` to avoid feature name collision

**Step 3: halt for review**

the branch is ready for PR or further work.
