//! integration tests for `#[tein_const]` in `#[tein_module]`.
//!
//! exercises literal types (string, integer, float, bool), naming conventions
//! (SCREAMING_SNAKE → kebab-case), and `name = "..."` override.

use tein::{Context, Value, tein_module};

#[tein_module("tc")]
#[allow(dead_code)]
mod tc {
    #[tein_const]
    pub const GREETING: &str = "hello";

    #[tein_const]
    pub const MAX_SIZE: i64 = 256;

    #[tein_const]
    pub const SCALE_FACTOR: f64 = 2.5;

    #[tein_const]
    pub const ENABLED: bool = true;

    #[tein_const]
    pub const DISABLED: bool = false;

    #[tein_const]
    pub const NEGATIVE: i64 = -42;

    #[tein_const(name = "custom-name")]
    pub const OVERRIDDEN: &str = "custom";

    #[tein_fn]
    pub fn dummy() -> i64 {
        0
    }
}

fn setup() -> Context {
    let ctx = Context::builder().standard_env().build().expect("ctx");
    tc::register_module_tc(&ctx).expect("register");
    ctx.evaluate("(import (tein tc))").expect("import");
    ctx
}

#[test]
fn test_const_string() {
    let ctx = setup();
    let r = ctx.evaluate("greeting").expect("eval");
    assert_eq!(r, Value::String("hello".into()));
}

#[test]
fn test_const_integer() {
    let ctx = setup();
    let r = ctx.evaluate("max-size").expect("eval");
    assert_eq!(r, Value::Integer(256));
}

#[test]
fn test_const_float() {
    let ctx = setup();
    let r = ctx.evaluate("scale-factor").expect("eval");
    assert_eq!(r, Value::Float(2.5));
}

#[test]
fn test_const_bool_true() {
    let ctx = setup();
    let r = ctx.evaluate("enabled").expect("eval");
    assert_eq!(r, Value::Boolean(true));
}

#[test]
fn test_const_bool_false() {
    let ctx = setup();
    let r = ctx.evaluate("disabled").expect("eval");
    assert_eq!(r, Value::Boolean(false));
}

#[test]
fn test_const_negative() {
    let ctx = setup();
    let r = ctx.evaluate("negative").expect("eval");
    assert_eq!(r, Value::Integer(-42));
}

#[test]
fn test_const_name_override() {
    let ctx = setup();
    let r = ctx.evaluate("custom-name").expect("eval");
    assert_eq!(r, Value::String("custom".into()));
}

#[test]
fn test_const_coexists_with_fn() {
    let ctx = setup();
    // const works
    let c = ctx.evaluate("greeting").expect("const");
    assert_eq!(c, Value::String("hello".into()));
    // fn works alongside
    let f = ctx.evaluate("(tc-dummy)").expect("fn");
    assert_eq!(f, Value::Integer(0));
}
