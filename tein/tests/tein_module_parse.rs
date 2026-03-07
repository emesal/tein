//! compile-time test: `#[tein_module]` parses and generates valid code.
//!
//! this test exercises the macro expansion path without running scheme.
//! runtime integration is covered by `tein_module_naming.rs` and
//! `scheme_tests.rs::test_scheme_tein_module`.

use tein::tein_module;

#[tein_module("parse-test")]
mod parse_test {
    #[tein_fn]
    pub fn hello() -> i64 {
        42
    }

    #[tein_const]
    #[allow(dead_code)]
    pub const MAX_ITEMS: i64 = 100;
}

#[test]
fn test_module_generates_register_fn() {
    // verify the generated register function exists and has the right signature.
    // we can't call it without a context, but its existence proves codegen worked.
    let _: fn(&tein::Context) -> tein::Result<()> = parse_test::register_module_parse_test;
}
