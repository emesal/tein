//! integration tests for `(tein time)`.

use tein::{Context, Value};

fn ctx() -> Context {
    Context::new_standard().expect("context")
}

#[test]
fn test_current_second_returns_flonum() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let val = ctx.evaluate("(current-second)").expect("current-second");
    assert!(
        matches!(val, Value::Float(_)),
        "expected float, got {:?}",
        val
    );
}

#[test]
fn test_current_second_is_positive() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let val = ctx.evaluate("(current-second)").expect("current-second");
    if let Value::Float(f) = val {
        assert!(f > 0.0, "current-second should be positive, got {}", f);
    } else {
        panic!("expected float");
    }
}

#[test]
fn test_current_second_is_recent() {
    // sanity check: should be after 2025-01-01 (~1735689600)
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let val = ctx.evaluate("(current-second)").expect("current-second");
    if let Value::Float(f) = val {
        assert!(f > 1_735_689_600.0, "timestamp too old: {}", f);
    } else {
        panic!("expected float");
    }
}

#[test]
fn test_current_jiffy_returns_integer() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let val = ctx.evaluate("(current-jiffy)").expect("current-jiffy");
    assert!(
        matches!(val, Value::Integer(_)),
        "expected integer, got {:?}",
        val
    );
}

#[test]
fn test_current_jiffy_is_non_negative() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let val = ctx.evaluate("(current-jiffy)").expect("current-jiffy");
    if let Value::Integer(n) = val {
        assert!(n >= 0, "jiffies should be non-negative, got {}", n);
    } else {
        panic!("expected integer");
    }
}

#[test]
fn test_current_jiffy_is_monotonic() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let a = ctx.evaluate("(current-jiffy)").expect("jiffy a");
    let b = ctx.evaluate("(current-jiffy)").expect("jiffy b");
    if let (Value::Integer(a), Value::Integer(b)) = (a, b) {
        assert!(b >= a, "jiffies not monotonic: {} then {}", a, b);
    } else {
        panic!("expected integers");
    }
}

#[test]
fn test_jiffies_per_second_value() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    assert_eq!(
        ctx.evaluate("jiffies-per-second").unwrap(),
        Value::Integer(1_000_000_000)
    );
}

#[test]
fn test_time_in_sandbox() {
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(tein::sandbox::Modules::Safe)
        .build()
        .expect("sandboxed context");
    ctx.evaluate("(import (tein time))")
        .expect("import in sandbox");

    // current-second works
    let val = ctx
        .evaluate("(current-second)")
        .expect("current-second in sandbox");
    assert!(
        matches!(val, Value::Float(_)),
        "expected float, got {:?}",
        val
    );

    // current-jiffy works
    let val = ctx
        .evaluate("(current-jiffy)")
        .expect("current-jiffy in sandbox");
    assert!(
        matches!(val, Value::Integer(_)),
        "expected integer, got {:?}",
        val
    );

    // jiffies-per-second is correct
    assert_eq!(
        ctx.evaluate("jiffies-per-second").unwrap(),
        Value::Integer(1_000_000_000)
    );
}
