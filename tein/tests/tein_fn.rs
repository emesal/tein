//! integration tests for the #[tein_fn] proc macro

use tein::{Context, Value, tein_fn};

// --- basic types ---

#[tein_fn]
fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[tein_fn]
fn greet(name: String) -> String {
    format!("hello, {}!", name)
}

#[tein_fn]
fn negate(b: bool) -> bool {
    !b
}

#[tein_fn]
fn multiply_float(a: f64, b: f64) -> f64 {
    a * b
}

// --- no args ---

#[tein_fn]
fn random_number() -> i64 {
    42
}

// --- mixed types ---

#[tein_fn]
fn format_pair(key: String, val: i64) -> String {
    format!("{}: {}", key, val)
}

// --- error propagation ---

#[tein_fn]
fn safe_div(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 {
        Err("division by zero".to_string())
    } else {
        Ok(a / b)
    }
}

// --- void return ---

#[tein_fn]
fn do_nothing() {
    // side-effect only function
}

// --- tests ---

#[test]
fn test_tein_fn_add() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("add", __tein_add).expect("define");
    let result = ctx.evaluate("(add 3 4)").expect("eval");
    assert_eq!(result, Value::Integer(7));
}

#[test]
fn test_tein_fn_greet() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("greet", __tein_greet)
        .expect("define");
    let result = ctx.evaluate(r#"(greet "world")"#).expect("eval");
    assert_eq!(result, Value::String("hello, world!".to_string()));
}

#[test]
fn test_tein_fn_negate() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("negate", __tein_negate)
        .expect("define");
    let result = ctx.evaluate("(negate #t)").expect("eval");
    assert_eq!(result, Value::Boolean(false));
}

#[test]
fn test_tein_fn_float() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("mul", __tein_multiply_float)
        .expect("define");
    let result = ctx.evaluate("(mul 2.5 4.0)").expect("eval");
    assert_eq!(result, Value::Float(10.0));
}

#[test]
fn test_tein_fn_no_args() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("random-number", __tein_random_number)
        .expect("define");
    let result = ctx.evaluate("(random-number)").expect("eval");
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn test_tein_fn_mixed_types() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("format-pair", __tein_format_pair)
        .expect("define");
    let result = ctx.evaluate(r#"(format-pair "age" 30)"#).expect("eval");
    assert_eq!(result, Value::String("age: 30".to_string()));
}

#[test]
fn test_tein_fn_result_ok() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("safe-div", __tein_safe_div)
        .expect("define");
    let result = ctx.evaluate("(safe-div 10 3)").expect("eval");
    assert_eq!(result, Value::Integer(3));
}

#[test]
fn test_tein_fn_result_err() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("safe-div", __tein_safe_div)
        .expect("define");
    // division by zero returns error string (not an exception, since chibi
    // treats the raw return as a value — the string will be the result)
    let result = ctx.evaluate("(safe-div 10 0)").expect("eval");
    match result {
        Value::String(s) => assert!(s.contains("division by zero"), "got: {}", s),
        _ => panic!("expected error string, got {:?}", result),
    }
}

#[test]
fn test_tein_fn_wrong_arg_type() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("add", __tein_add).expect("define");
    // pass string where integer expected
    let result = ctx.evaluate(r#"(add "hello" 1)"#).expect("eval");
    match result {
        Value::String(s) => assert!(
            s.contains("expected i64"),
            "expected type error message, got: {}",
            s
        ),
        _ => panic!("expected error string, got {:?}", result),
    }
}

#[test]
fn test_tein_fn_void_return() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("do-nothing", __tein_do_nothing)
        .expect("define");
    let result = ctx.evaluate("(do-nothing)").expect("eval");
    assert!(result.is_unspecified(), "expected void, got {:?}", result);
}

#[test]
fn test_tein_fn_panic_safety() {
    #[tein_fn]
    fn panicker() -> i64 {
        panic!("oh no!");
    }

    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("panicker", __tein_panicker)
        .expect("define");
    // should not crash — panic is caught, returns error string
    let result = ctx.evaluate("(panicker)").expect("eval");
    match result {
        Value::String(s) => assert!(s.contains("panic"), "expected panic message, got: {}", s),
        _ => panic!("expected error string from panic, got {:?}", result),
    }
}

#[test]
fn test_tein_fn_float_int_coercion() {
    // passing an integer where f64 is expected should auto-coerce
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("mul", __tein_multiply_float)
        .expect("define");
    let result = ctx.evaluate("(mul 3 4.0)").expect("eval");
    assert_eq!(result, Value::Float(12.0));
}
