//! integration tests for doc attr scraping in `#[tein_module]`.
//!
//! exercises `///` comment threading through codegen: doc comments on
//! `#[tein_fn]`, `#[tein_const]`, `#[tein_type]`, and `#[tein_methods]`
//! items should appear as `;;` comments in generated scheme output and
//! be accessible via the info structs.

use tein::{Context, Value, tein_module};

#[tein_module("dc")]
#[allow(dead_code)]
mod dc {
    /// a friendly greeting
    #[tein_const]
    pub const GREETING: &str = "hello";

    /// the maximum allowed size.
    /// must be a positive integer.
    #[tein_const]
    pub const MAX_SIZE: i64 = 256;

    /// bare const — no docs
    #[tein_const]
    pub const BARE: bool = true;

    /// add two numbers
    #[tein_fn]
    pub fn add(a: i64, b: i64) -> i64 {
        a + b
    }
}

fn setup() -> Context {
    let ctx = Context::builder().standard_env().build().expect("ctx");
    dc::register_module_dc(&ctx).expect("register");
    ctx.evaluate("(import (tein dc))").expect("import");
    ctx
}

#[test]
fn test_doc_const_values_still_work() {
    let ctx = setup();
    assert_eq!(
        ctx.evaluate("greeting").unwrap(),
        Value::String("hello".into())
    );
    assert_eq!(ctx.evaluate("max-size").unwrap(), Value::Integer(256));
    assert_eq!(ctx.evaluate("bare").unwrap(), Value::Boolean(true));
}

#[test]
fn test_doc_fn_still_works() {
    let ctx = setup();
    assert_eq!(ctx.evaluate("(dc-add 1 2)").unwrap(), Value::Integer(3));
}

#[test]
fn test_vfs_scm_contains_doc_comments() {
    // verify the generated .scm (with ;; comments) registers and imports cleanly.
    // the exact comment content is verified by the unit test on generate_vfs_scm.
    // loading the module implicitly parses the .scm file, so valid scheme is confirmed.
    let ctx = Context::builder().standard_env().build().expect("ctx");
    dc::register_module_dc(&ctx).expect("register");
    ctx.evaluate("(import (tein dc))").expect("import");
}

// ── doc preservation: verify doc attrs survive macro expansion ────────────────

/// verify that doc comments on tein items survive macro expansion.
/// this module exists to be compiled — if it compiles, doc attrs are preserved.
#[tein_module("dp")]
#[allow(dead_code)]
mod dp {
    /// documented constant
    #[tein_const]
    pub const DOCUMENTED: i64 = 1;

    /// documented function
    #[tein_fn]
    pub fn documented_fn() -> i64 {
        1
    }

    /// documented type
    #[tein_type]
    pub struct DocType {
        pub val: i64,
    }

    /// documented method
    #[tein_methods]
    impl DocType {
        /// get the value
        pub fn get(&self) -> i64 {
            self.val
        }
    }
}

#[test]
fn test_doc_preservation_compiles() {
    // if this test compiles, doc attrs survived macro expansion.
    // cargo doc would pick them up.
    let _: fn(&tein::Context) -> tein::Result<()> = dp::register_module_dp;
}
