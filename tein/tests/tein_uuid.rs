//! integration tests for `(tein uuid)`.

use tein::{Context, Value};

fn ctx() -> Context {
    Context::new_standard().expect("context")
}

#[test]
fn test_make_uuid_returns_string() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    let val = ctx.evaluate("(make-uuid)").expect("make-uuid");
    assert!(matches!(val, Value::String(_)), "expected string, got {:?}", val);
}

#[test]
fn test_make_uuid_format() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    let val = ctx.evaluate("(make-uuid)").expect("make-uuid");
    if let Value::String(s) = val {
        assert_eq!(s.len(), 36, "uuid wrong length: {}", s);
        let parts: Vec<&str> = s.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!((parts[0].len(), parts[1].len(), parts[2].len(), parts[3].len(), parts[4].len()),
                   (8, 4, 4, 4, 12));
        assert!(parts[2].starts_with('4'), "not v4: {}", s);
    } else {
        panic!("expected string");
    }
}

#[test]
fn test_make_uuid_unique() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    let a = ctx.evaluate("(make-uuid)").expect("uuid a");
    let b = ctx.evaluate("(make-uuid)").expect("uuid b");
    assert_ne!(a, b);
}

#[test]
fn test_uuid_predicate_valid() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    assert_eq!(ctx.evaluate("(uuid? (make-uuid))").unwrap(), Value::Boolean(true));
}

#[test]
fn test_uuid_predicate_invalid_string() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    assert_eq!(ctx.evaluate(r#"(uuid? "nope")"#).unwrap(), Value::Boolean(false));
}

#[test]
fn test_uuid_predicate_non_string() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    assert_eq!(ctx.evaluate("(uuid? 42)").unwrap(), Value::Boolean(false));
    assert_eq!(ctx.evaluate("(uuid? #t)").unwrap(), Value::Boolean(false));
    assert_eq!(ctx.evaluate("(uuid? '())").unwrap(), Value::Boolean(false));
}

#[test]
fn test_uuid_nil_value() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    assert_eq!(
        ctx.evaluate("uuid-nil").unwrap(),
        Value::String("00000000-0000-0000-0000-000000000000".to_string())
    );
}

#[test]
fn test_uuid_nil_is_valid() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    assert_eq!(ctx.evaluate("(uuid? uuid-nil)").unwrap(), Value::Boolean(true));
}

#[test]
fn test_uuid_docs() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import uuid");
    ctx.evaluate("(import (tein uuid docs))").expect("import uuid docs");
    ctx.evaluate("(import (tein docs))").expect("import docs");
    let desc = ctx.evaluate("(describe uuid-docs)").expect("describe");
    if let Value::String(s) = desc {
        assert!(s.contains("make-uuid"), "docs missing make-uuid: {}", s);
        assert!(s.contains("uuid?"), "docs missing uuid?: {}", s);
        assert!(s.contains("uuid-nil"), "docs missing uuid-nil: {}", s);
    } else {
        panic!("describe returned non-string: {:?}", desc);
    }
}
