//! Integration tests for the cdylib extension system.
//!
//! These tests load `tein-test-ext` as a shared library and exercise
//! the full extension lifecycle: loading, VFS registration, function
//! calls, constants, foreign types, and documentation.
//!
//! The extension must be built before running these tests:
//!
//! ```text
//! cargo build -p tein-test-ext
//! cargo test -p tein -- ext
//! ```

use tein::{Context, Value};

/// resolve the path to the test extension shared library.
///
/// prefers `CARGO_TARGET_DIR` env var (set by the project's cargo alias),
/// falls back to `<workspace>/target/`.
fn ext_lib_path() -> std::path::PathBuf {
    let target_dir = if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        std::path::PathBuf::from(dir)
    } else {
        // default: workspace root / target
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop(); // tein/ → workspace root
        path.push("target");
        path
    };

    let mut path = target_dir;
    path.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });

    #[cfg(target_os = "linux")]
    path.push("libtein_test_ext.so");
    #[cfg(target_os = "macos")]
    path.push("libtein_test_ext.dylib");
    #[cfg(target_os = "windows")]
    path.push("tein_test_ext.dll");

    path
}

#[test]
fn test_ext_load_and_import() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load extension");
    ctx.evaluate("(import (tein testext))").expect("import");
}

#[test]
fn test_ext_free_fn_integer() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx.evaluate("(testext-add 20 22)").expect("eval");
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn test_ext_free_fn_float() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx.evaluate("(testext-multiply 2.5 4.0)").expect("eval");
    assert_eq!(result, Value::Float(10.0));
}

#[test]
fn test_ext_free_fn_string() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx.evaluate("(testext-greet \"world\")").expect("eval");
    assert_eq!(result, Value::String("hello, world!".to_string()));
}

#[test]
fn test_ext_free_fn_bool() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    assert_eq!(
        ctx.evaluate("(testext-positive? 5)").expect("eval"),
        Value::Boolean(true)
    );
    assert_eq!(
        ctx.evaluate("(testext-positive? -3)").expect("eval"),
        Value::Boolean(false)
    );
}

#[test]
fn test_ext_free_fn_void() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx.evaluate("(testext-noop)").expect("eval");
    assert_eq!(result, Value::Unspecified);
}

#[test]
fn test_ext_free_fn_result_ok() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx.evaluate("(testext-safe-div 10 2)").expect("eval");
    assert_eq!(result, Value::Integer(5));
}

#[test]
fn test_ext_free_fn_result_err() {
    // Result::Err returns a scheme string (not an exception) — same as internal mode.
    // see tein/tests/tein_fn.rs :: test_tein_fn_result_err for precedent.
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx
        .evaluate("(testext-safe-div 10 0)")
        .expect("eval (returns string on error)");
    match result {
        Value::String(s) => assert!(
            s.contains("division by zero"),
            "expected 'division by zero' in error string, got: {s}"
        ),
        other => panic!("expected error string from Result::Err, got {other:?}"),
    }
}

#[test]
fn test_ext_constants() {
    // constants use screaming_snake → kebab scheme names, no module prefix.
    // GREETING → "greeting", ANSWER → "answer" (same convention as internal modules).
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    assert_eq!(
        ctx.evaluate("greeting").expect("eval greeting"),
        Value::String("hello from testext".to_string())
    );
    assert_eq!(
        ctx.evaluate("answer").expect("eval answer"),
        Value::Integer(42)
    );
}

#[test]
fn test_ext_load_nonexistent() {
    let ctx = Context::new_standard().expect("context");
    let result = ctx.load_extension("/nonexistent/path/libnope.so");
    assert!(result.is_err(), "expected error for missing library");
}

#[test]
fn test_ext_foreign_type_predicate() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    // counter? should be a procedure even without foreign value creation
    assert_eq!(
        ctx.evaluate("(procedure? counter?)").expect("pred is proc"),
        Value::Boolean(true),
    );
    assert_eq!(
        ctx.evaluate("(counter? 42)").expect("pred false on int"),
        Value::Boolean(false),
    );
}

#[test]
fn test_ext_foreign_type_convenience_proc_names() {
    // convenience procs should use the correct (non-doubled) name prefix (#69).
    // ext method names arrive already prefixed (e.g. "counter-get"), so the
    // generated (define ...) must use them directly, not wrap in type-name again.
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");

    // correct names should be bound as procedures
    for name in &["counter-get", "counter-increment", "counter-add"] {
        assert_eq!(
            ctx.evaluate(&format!("(procedure? {})", name)).expect(name),
            Value::Boolean(true),
            "{} should be a procedure",
            name,
        );
    }

    // doubled-prefix names should NOT be bound (#69 regression)
    for name in &[
        "counter-counter-get",
        "counter-counter-increment",
        "counter-counter-add",
    ] {
        let result = ctx.evaluate(&format!("(procedure? {})", name));
        assert!(
            result.is_err(),
            "{} should not be defined (double prefix bug)",
            name,
        );
    }
}

#[test]
fn test_ext_foreign_type_method_error_message() {
    // error message should use correct (non-doubled) prefix
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let err = ctx.evaluate("(counter-get 42)").unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("counter-get: expected counter"),
        "expected clear error with correct prefix, got: {msg}"
    );
}

#[test]
fn test_ext_docs_sublibrary() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))")
        .expect("import testext");
    ctx.evaluate("(import (tein testext docs))")
        .expect("import docs sub-library");
    let result = ctx.evaluate("testext-docs").expect("eval docs alist");
    match result {
        Value::List(_) | Value::Pair(_, _) => {} // alist is a list of pairs
        other => panic!("expected list for docs alist, got {other:?}"),
    }
}
