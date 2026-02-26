//! scheme-level integration tests via `(tein test)` assertion framework.
//!
//! each `.scm` file is embedded at compile time and evaluated in a fresh
//! standard context. assertions in scheme raise errors on failure, which
//! propagate as `Error::EvalError` and fail the cargo test.

use tein::{Context, tein_module};

/// run a scheme test file in a fresh standard context with `(tein test)` loaded.
fn run_scheme_test(source: &str) {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein test))")
        .expect("import tein test");
    ctx.evaluate(source).expect("scheme test failed");
}

// ── module test infrastructure ───────────────────────────────────────────────

#[tein_module("testmod")]
mod testmod {
    #[tein_fn]
    pub fn greet(name: String) -> String {
        format!("hello, {}!", name)
    }

    #[tein_fn]
    pub fn add(a: i64, b: i64) -> i64 {
        a + b
    }

    #[tein_type(name = "counter")]
    pub struct Counter {
        pub n: i64,
    }

    #[tein_methods]
    impl Counter {
        pub fn get(&self) -> i64 {
            self.n
        }
        pub fn increment(&mut self) -> i64 {
            self.n += 1;
            self.n
        }
    }
}

/// run a scheme test that needs a `#[tein_module]` registered first.
fn run_scheme_test_with_module(source: &str) {
    let ctx = Context::new_standard().expect("context");
    testmod::register_module_testmod(&ctx).expect("register testmod");
    ctx.evaluate("(import (tein test))")
        .expect("import tein test");
    ctx.evaluate(source)
        .expect("scheme test with module failed");
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
fn test_scheme_control_flow() {
    run_scheme_test(include_str!("scheme/control_flow.scm"));
}

#[test]
fn test_scheme_binding_forms() {
    run_scheme_test(include_str!("scheme/binding_forms.scm"));
}

#[test]
fn test_scheme_tail_calls() {
    run_scheme_test(include_str!("scheme/tail_calls.scm"));
}

#[test]
fn test_scheme_closures() {
    run_scheme_test(include_str!("scheme/closures.scm"));
}

#[test]
fn test_scheme_continuations() {
    run_scheme_test(include_str!("scheme/continuations.scm"));
}

#[test]
fn test_scheme_error_handling() {
    run_scheme_test(include_str!("scheme/error_handling.scm"));
}

#[test]
fn test_scheme_records() {
    run_scheme_test(include_str!("scheme/records.scm"));
}

#[test]
fn test_scheme_bytevectors() {
    run_scheme_test(include_str!("scheme/bytevectors.scm"));
}

#[test]
fn test_scheme_io() {
    run_scheme_test(include_str!("scheme/io.scm"));
}

#[test]
fn test_scheme_macros() {
    run_scheme_test(include_str!("scheme/macros.scm"));
}

#[test]
fn test_scheme_quasiquote() {
    run_scheme_test(include_str!("scheme/quasiquote.scm"));
}

#[test]
fn test_scheme_case_lambda() {
    run_scheme_test(include_str!("scheme/case_lambda.scm"));
}

#[test]
fn test_scheme_lazy() {
    run_scheme_test(include_str!("scheme/lazy.scm"));
}

#[test]
fn test_scheme_numbers_extended() {
    run_scheme_test(include_str!("scheme/numbers_extended.scm"));
}

#[test]
fn test_scheme_scheme_eval() {
    run_scheme_test(include_str!("scheme/scheme_eval.scm"));
}

#[test]
fn test_scheme_tein_foreign() {
    run_scheme_test(include_str!("scheme/tein_foreign.scm"));
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
            "import",
            "define",
            "define-syntax",
            "syntax-rules",
            "set!",
            "if",
            "let",
            "lambda",
            "begin",
            "quote",
        ])
        .step_limit(5_000_000)
        .build()
        .expect("sandboxed context");
    ctx.evaluate("(import (tein test))")
        .expect("import tein test");
    ctx.evaluate("(import (tein reader))")
        .expect("import tein reader");
    ctx.evaluate("(import (tein macro))")
        .expect("import tein macro");
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

#[test]
fn test_scheme_tein_module() {
    run_scheme_test_with_module(include_str!("scheme/tein_module.scm"));
}
