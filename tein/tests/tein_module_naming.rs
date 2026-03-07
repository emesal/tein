//! naming convention tests for `#[tein_module]`.
//!
//! exercises `_q` → `?`, `_bang` → `!`, `name = "..."` override,
//! and method naming with suffix transforms.

use tein::{Context, Value, tein_module};

#[tein_module("nm")]
mod nm {
    #[tein_fn]
    pub fn is_valid_q(x: i64) -> bool {
        x > 0
    }

    #[tein_fn]
    pub fn reset_bang() -> i64 {
        0
    }

    #[tein_fn(name = "nm-custom")]
    pub fn custom_override() -> i64 {
        99
    }

    #[tein_type(name = "widget")]
    pub struct Widget {
        pub val: i64,
    }

    #[tein_methods]
    impl Widget {
        pub fn active_q(&self) -> bool {
            self.val > 0
        }
        pub fn clear_bang(&mut self) -> i64 {
            self.val = 0;
            0
        }
    }
}

// --- tests ---

#[test]
fn test_naming_q_suffix() {
    let ctx = Context::builder().standard_env().build().expect("ctx");
    nm::register_module_nm(&ctx).expect("register");
    ctx.evaluate("(import (tein nm))").expect("import");
    let r = ctx.evaluate("(nm-is-valid? 5)").expect("eval");
    assert_eq!(r, Value::Boolean(true));
    let r2 = ctx.evaluate("(nm-is-valid? -1)").expect("eval neg");
    assert_eq!(r2, Value::Boolean(false));
}

#[test]
fn test_naming_bang_suffix() {
    let ctx = Context::builder().standard_env().build().expect("ctx");
    nm::register_module_nm(&ctx).expect("register");
    ctx.evaluate("(import (tein nm))").expect("import");
    let r = ctx.evaluate("(nm-reset!)").expect("eval");
    assert_eq!(r, Value::Integer(0));
}

#[test]
fn test_naming_override() {
    let ctx = Context::builder().standard_env().build().expect("ctx");
    nm::register_module_nm(&ctx).expect("register");
    ctx.evaluate("(import (tein nm))").expect("import");
    let r = ctx.evaluate("(nm-custom)").expect("eval");
    assert_eq!(r, Value::Integer(99));
}

#[test]
fn test_naming_method_q_bang() {
    let ctx = Context::builder().standard_env().build().expect("ctx");
    nm::register_module_nm(&ctx).expect("register");
    ctx.evaluate("(import (tein nm))").expect("import");

    let w = ctx.foreign_value(nm::Widget { val: 5 }).expect("foreign");
    let active = ctx.evaluate("widget-active?").expect("lookup");
    let result = ctx
        .call(&active, std::slice::from_ref(&w))
        .expect("call active?");
    assert_eq!(result, Value::Boolean(true));

    let clear = ctx.evaluate("widget-clear!").expect("lookup clear");
    let result2 = ctx
        .call(&clear, std::slice::from_ref(&w))
        .expect("call clear!");
    assert_eq!(result2, Value::Integer(0));
}
