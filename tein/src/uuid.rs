//! `(tein uuid)` — UUID generation via the `uuid` crate.
//!
//! provides:
//! - `make-uuid` — generate a random UUID v4 string
//! - `uuid?` — test whether a value is a valid UUID string
//! - `uuid-nil` — the nil UUID constant (all zeros)

use tein_macros::tein_module;

#[tein_module("uuid")]
mod uuid_impl {
    /// the nil UUID (all zeros)
    #[tein_const]
    pub const UUID_NIL: &str = "00000000-0000-0000-0000-000000000000";

    /// generate a random UUID v4 string
    #[tein_fn(name = "make-uuid")]
    pub fn make_uuid() -> String {
        ::uuid::Uuid::new_v4().to_string()
    }

    /// test whether a value is a valid UUID string
    #[tein_fn(name = "uuid?")]
    pub fn uuid_q(value: Value) -> bool {
        match value {
            Value::String(s) => ::uuid::Uuid::parse_str(&s).is_ok(),
            _ => false,
        }
    }
}
