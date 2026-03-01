# `(tein uuid)` design

**issue:** #39
**date:** 2026-03-01

## summary

UUID v4 generation exposed as `(tein uuid)` via the `uuid` crate and `#[tein_module]` proc macro. three exports: `make-uuid`, `uuid?`, `uuid-nil`.

## API

```scheme
(import (tein uuid))

(make-uuid)              ; → "f47ac10b-58cc-4372-a567-0e02b2c3d479"
(uuid? (make-uuid))      ; → #t
(uuid? "not-a-uuid")     ; → #f
(uuid? 42)               ; → #f (non-string → #f, no error)
uuid-nil                 ; → "00000000-0000-0000-0000-000000000000"

;; auto-generated docs sub-library
(import (tein uuid docs))
(import (tein docs))
(describe uuid-docs)
```

## rust implementation

single file `tein/src/uuid.rs` using `#[tein_module("uuid")]`:

```rust
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
```

naming rationale:
- `make_uuid` would auto-generate `uuid-make-uuid` (module prefix + fn name). override to `make-uuid`.
- `uuid_q` would auto-generate `uuid-uuid?`. override to `uuid?`.
- `UUID_NIL` → `uuid-nil` (constants have no module prefix, screaming→kebab). no override needed.

## decisions

- **feature-gated**: `uuid` cargo feature, default-on (pure computation, no IO — same as json/toml)
- **sandbox-safe**: add `"tein/uuid"` to `SAFE_MODULES` (stateless, no filesystem/network)
- **uuid? takes Value**: returns `#f` for non-strings instead of type error. more scheme-idiomatic.
- **uuid? validates via crate**: `Uuid::parse_str` — lenient (mixed case, optional hyphens)
- **uuid-nil is a constant**: not a function. `#[tein_const]` embeds the literal in generated `.scm`
- **no modules/ directory**: follows existing pattern (json.rs, toml.rs are top-level in src/)

## registration

`register_module_uuid(&context)?` called from `ContextBuilder::build()`, gated behind `#[cfg(feature = "uuid")]` + `if self.standard_env`.

## tests

- rust integration tests in `tein/tests/tein_uuid.rs`
- scheme integration tests in `tein/tests/scheme/tein_uuid.scm` via `(tein test)`
- docs sub-library test (verify `uuid-docs` alist)
