//! scheme-level integration tests via `(tein test)` assertion framework.
//!
//! each `.scm` file is embedded at compile time and evaluated in a fresh
//! standard context. assertions in scheme raise errors on failure, which
//! propagate as `Error::EvalError` and fail the cargo test.

use tein::Context;

/// run a scheme test file in a fresh standard context with `(tein test)` loaded.
fn run_scheme_test(source: &str) {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein test))").expect("import tein test");
    ctx.evaluate(source).expect("scheme test failed");
}

#[test]
fn test_scheme_arithmetic() {
    run_scheme_test(include_str!("scheme/arithmetic.scm"));
}

#[test]
fn test_scheme_lists() {
    run_scheme_test(include_str!("scheme/lists.scm"));
}

#[test]
fn test_scheme_strings() {
    run_scheme_test(include_str!("scheme/strings.scm"));
}

#[test]
fn test_scheme_types() {
    run_scheme_test(include_str!("scheme/types.scm"));
}

#[test]
fn test_scheme_reader_macro() {
    // tests issue #31 fix: reader/macro fns via import in standard context
    run_scheme_test(include_str!("scheme/reader_macro.scm"));
}

#[test]
fn test_scheme_reader_macro_sandbox() {
    // tests issue #31 fix: reader/macro fns via import in sandboxed context
    use tein::sandbox::*;
    let ctx = Context::builder()
        .standard_env()
        .preset(&ARITHMETIC)
        .preset(&LISTS)
        .preset(&STRINGS)
        .preset(&TYPE_PREDICATES)
        .preset(&CHARACTERS)
        .preset(&MUTATION)
        .preset(&EXCEPTIONS)
        .allow(&[
            "import", "define", "define-syntax", "syntax-rules",
            "set!", "if", "let", "lambda", "begin", "quote",
        ])
        .step_limit(5_000_000)
        .build()
        .expect("sandboxed context");
    ctx.evaluate("(import (tein test))").expect("import tein test");
    ctx.evaluate("(import (tein reader))").expect("import tein reader");
    ctx.evaluate("(import (tein macro))").expect("import tein macro");
    // simplified reader/macro test for sandbox — avoids standard library
    // fns like `member` that aren't in the primitive presets
    ctx.evaluate(
        r#"
        ;; reader dispatch
        (set-reader! #\j (lambda (port) 42))
        (test-equal "sandbox/reader-basic" 42 #j)

        ;; introspection — chars list has at least one entry
        (test-true "sandbox/reader-chars" (pair? (reader-dispatch-chars)))
        (unset-reader! #\j)
        (test-equal "sandbox/reader-unset" '() (reader-dispatch-chars))

        ;; macro hook
        (define-syntax double (syntax-rules () ((double x) (+ x x))))
        (define hook-fired #f)
        (set-macro-expand-hook!
          (lambda (name unexpanded expanded env)
            (set! hook-fired #t)
            expanded))
        (double 5)
        (test-true "sandbox/macro-hook-fired" hook-fired)
        (unset-macro-expand-hook!)
        (test-false "sandbox/macro-hook-unset" (macro-expand-hook))
        "#,
    )
    .expect("sandboxed reader/macro test failed");
}
