//! compile test: #[tein_module] parses without errors (no codegen exercised yet)

use tein::{tein_fn, tein_module};

#[tein_module("test-parse")]
mod test_parse {
    #[tein_fn]
    fn hello() -> i64 {
        42
    }
}

#[test]
fn test_module_parses() {
    // if this compiles, parsing succeeded
    assert!(true);
}
