//! test that #[tein_fn] free fns can accept Value arguments.

use tein::{Context, Value, tein_module};

#[tein_module("valarg")]
mod valarg {
    /// return #t if the argument is a string, #f otherwise
    #[tein_fn(name = "string-value?")]
    pub fn string_value_q(v: Value) -> bool {
        matches!(v, Value::String(_))
    }
}

#[test]
fn test_value_arg_string() {
    let ctx = Context::new_standard().unwrap();
    valarg::register_module_valarg(&ctx).unwrap();
    ctx.evaluate("(import (tein valarg))").unwrap();
    assert_eq!(
        ctx.evaluate(r#"(string-value? "hello")"#).unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn test_value_arg_integer() {
    let ctx = Context::new_standard().unwrap();
    valarg::register_module_valarg(&ctx).unwrap();
    ctx.evaluate("(import (tein valarg))").unwrap();
    assert_eq!(
        ctx.evaluate("(string-value? 42)").unwrap(),
        Value::Boolean(false)
    );
}

#[test]
fn test_value_arg_boolean() {
    let ctx = Context::new_standard().unwrap();
    valarg::register_module_valarg(&ctx).unwrap();
    ctx.evaluate("(import (tein valarg))").unwrap();
    assert_eq!(
        ctx.evaluate("(string-value? #t)").unwrap(),
        Value::Boolean(false)
    );
}
