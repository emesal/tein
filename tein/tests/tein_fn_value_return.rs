//! test that #[tein_fn] correctly handles Value return types.
use tein::{Context, Value};
use tein_macros::tein_module;

#[tein_module("valret")]
mod valret {
    /// return a list value directly
    #[tein_fn(name = "make-pair")]
    pub fn make_pair(a: i64, b: i64) -> Value {
        Value::List(vec![Value::Integer(a), Value::Integer(b)])
    }

    /// return Value or error string
    #[tein_fn(name = "maybe-vec")]
    pub fn maybe_vec(n: i64) -> Result<Value, String> {
        if n >= 0 {
            Ok(Value::Vector(vec![Value::Integer(n)]))
        } else {
            Err("negative".into())
        }
    }
}

#[test]
fn value_return_direct() {
    let ctx = Context::new_standard().unwrap();
    valret::register_module_valret(&ctx).unwrap();
    let result = ctx
        .evaluate("(import (tein valret)) (make-pair 1 2)")
        .unwrap();
    assert_eq!(
        result,
        Value::List(vec![Value::Integer(1), Value::Integer(2)])
    );
}

#[test]
fn value_return_result_ok() {
    let ctx = Context::new_standard().unwrap();
    valret::register_module_valret(&ctx).unwrap();
    let result = ctx
        .evaluate("(import (tein valret)) (vector-ref (maybe-vec 42) 0)")
        .unwrap();
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn value_return_result_err() {
    let ctx = Context::new_standard().unwrap();
    valret::register_module_valret(&ctx).unwrap();
    let result = ctx
        .evaluate("(import (tein valret)) (maybe-vec -1)")
        .unwrap();
    assert_eq!(result, Value::String("negative".into()));
}
