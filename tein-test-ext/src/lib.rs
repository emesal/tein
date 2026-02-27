//! Test extension for verifying the cdylib extension system.
//!
//! This crate compiles to a `.so` / `.dylib` and is loaded at test time
//! by `tein/tests/ext_loading.rs`.

use tein_macros::tein_module;

#[tein_module("testext", ext = true)]
mod testext_impl {
    /// add two integers
    #[tein_fn]
    pub fn add(a: i64, b: i64) -> i64 {
        a + b
    }

    /// multiply two floats
    #[tein_fn]
    pub fn multiply(a: f64, b: f64) -> f64 {
        a * b
    }

    /// greet someone
    #[tein_fn]
    pub fn greet(name: String) -> String {
        format!("hello, {}!", name)
    }

    /// check if a number is positive
    #[tein_fn]
    pub fn positive_q(n: i64) -> bool {
        n > 0
    }

    /// return nothing (void)
    #[tein_fn]
    pub fn noop() {}

    /// divide a by b, returning an error on division by zero
    #[tein_fn]
    pub fn safe_div(a: i64, b: i64) -> Result<i64, String> {
        if b == 0 {
            Err("division by zero".to_string())
        } else {
            Ok(a / b)
        }
    }

    /// greeting string constant
    #[tein_const]
    pub const GREETING: &str = "hello from testext";

    /// the answer to life, the universe, and everything
    #[tein_const]
    pub const ANSWER: i64 = 42;

    /// a simple counter type
    #[tein_type]
    pub struct Counter {
        pub n: i64,
    }

    #[tein_methods]
    impl Counter {
        /// get current value
        pub fn get(&self) -> i64 {
            self.n
        }

        /// increment by one, returning the new value
        pub fn increment(&mut self) -> i64 {
            self.n += 1;
            self.n
        }

        /// add an amount, returning the new value
        pub fn add(&mut self, amount: i64) -> i64 {
            self.n += amount;
            self.n
        }
    }
}
