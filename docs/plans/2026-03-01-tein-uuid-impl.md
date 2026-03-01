# `(tein uuid)` Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** expose UUID v4 generation as `(tein uuid)` with `make-uuid`, `uuid?`, `uuid-nil` — the first internal use of `#[tein_module]`.

**Architecture:** single `src/uuid.rs` using `#[tein_module("uuid")]` macro. the macro generates VFS `.sld`/`.scm`, docs sub-library, trampolines, and `register_module_uuid()`. registration is feature-gated and called from `ContextBuilder::build()`. requires `extern crate self as tein;` in `lib.rs` since the macro generates `tein::*` paths.

**Tech Stack:** `uuid` crate (v4 + fast-rng), `#[tein_module]` proc macro, `tein-macros`.

**Issue:** https://github.com/emesal/tein/issues/39

**Design:** `docs/plans/2026-03-01-tein-uuid-design.md`

---

## Task 1: add `Value` arg support to `#[tein_fn]` free fn extraction

the macro's `gen_arg_extraction_internal` (tein-macros/src/lib.rs ~line 1660) supports `i64`, `f64`, `String`, `bool` for free fn args but not `Value`. `uuid?` needs to accept any value and return `#f` for non-strings. this task adds `Value` support.

**Files:**
- Modify: `tein-macros/src/lib.rs`
- Create: `tein/tests/tein_fn_value_arg.rs` (test)

**Step 1: write a failing test**

create `tein/tests/tein_fn_value_arg.rs`:

```rust
//! test that #[tein_fn] free fns can accept Value arguments.

use tein::{Context, Value, tein_module, tein_fn};

#[tein_module("valarg")]
mod valarg {
    /// return #t if the argument is a string, #f otherwise
    #[tein_fn(name = "string-value?")]
    pub fn string_value_q(v: Value) -> bool {
        matches!(v, Value::String(_))
    }
}

#[test]
fn test_value_arg_string() {
    let ctx = Context::new_standard().unwrap();
    valarg::register_module_valarg(&ctx).unwrap();
    ctx.evaluate("(import (tein valarg))").unwrap();
    assert_eq!(
        ctx.evaluate(r#"(string-value? "hello")"#).unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn test_value_arg_integer() {
    let ctx = Context::new_standard().unwrap();
    valarg::register_module_valarg(&ctx).unwrap();
    ctx.evaluate("(import (tein valarg))").unwrap();
    assert_eq!(
        ctx.evaluate("(string-value? 42)").unwrap(),
        Value::Boolean(false)
    );
}

#[test]
fn test_value_arg_boolean() {
    let ctx = Context::new_standard().unwrap();
    valarg::register_module_valarg(&ctx).unwrap();
    ctx.evaluate("(import (tein valarg))").unwrap();
    assert_eq!(
        ctx.evaluate("(string-value? #t)").unwrap(),
        Value::Boolean(false)
    );
}
```

**Step 2: run test to confirm it fails**

```bash
cargo test --test tein_fn_value_arg 2>&1 | tail -20
```

expected: compile error — `unsupported argument type: Value`

**Step 3: add `Value` extraction to `gen_arg_extraction_internal`**

in `tein-macros/src/lib.rs`, find the `gen_arg_extraction_internal` function (around line 1660). in the `match type_str.as_str()` block, before the `_ =>` fallthrough, add a `"Value"` arm:

```rust
        "Value" => quote! {
            let #arg_name: tein::Value = {
                let raw = tein::raw::sexp_car(__tein_current_args);
                tein::Value::from_raw(ctx, raw)
            };
            __tein_current_args = tein::raw::sexp_cdr(__tein_current_args);
        },
```

note: `Value::from_raw` is a pub fn on `Value` — verify it takes `(ctx: sexp, raw: sexp) -> Value`. check `tein/src/value.rs` for the exact signature.

**Step 4: run test to confirm it passes**

```bash
cargo test --test tein_fn_value_arg 2>&1 | tail -20
```

expected: 3 tests pass.

**Step 5: lint**

```bash
just lint
```

**Step 6: commit**

```bash
git add tein-macros/src/lib.rs tein/tests/tein_fn_value_arg.rs
git commit -m "feat: support Value arg type in #[tein_fn] free fns

adds Value extraction to gen_arg_extraction_internal via
Value::from_raw. needed for predicates like uuid? that accept
any scheme value without type-checking."
```

---

## Task 2: add `extern crate self as tein` + uuid dependency

`#[tein_module]` generates `tein::*` paths. inside the tein crate itself, `tein::` doesn't resolve — we need `extern crate self as tein;`. this is the standard rust idiom for self-referencing proc macros.

**Files:**
- Modify: `tein/src/lib.rs`
- Modify: `tein/Cargo.toml`

**Step 1: add `extern crate self as tein` to lib.rs**

in `tein/src/lib.rs`, after the `#![warn(missing_docs)]` line (line 55), add:

```rust
extern crate self as tein;
```

**Step 2: add uuid dependency to Cargo.toml**

in `tein/Cargo.toml`, under `[dependencies]`, add:

```toml
uuid = { version = "1", features = ["v4", "fast-rng"], optional = true }
```

under `[features]`, add to the `default` list and add a new feature:

```toml
default = ["json", "toml", "uuid"]
## enables `(tein uuid)` module with `make-uuid`, `uuid?`, and `uuid-nil`.
## pulls in the uuid crate with v4 + fast-rng.
uuid = ["dep:uuid"]
```

**Step 3: verify it compiles**

```bash
cargo build 2>&1 | tail -5
```

expected: clean build.

**Step 4: commit**

```bash
git add tein/src/lib.rs tein/Cargo.toml Cargo.lock
git commit -m "chore: add extern crate self alias + uuid dependency

extern crate self as tein enables #[tein_module] inside the
tein crate itself (macro generates tein::* paths).
uuid crate added as optional dep, default-on."
```

---

## Task 3: implement `src/uuid.rs`

**Files:**
- Create: `tein/src/uuid.rs`
- Modify: `tein/src/lib.rs` (add `mod uuid;`)

**Step 1: create the module file**

create `tein/src/uuid.rs`:

```rust
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
```

**Step 2: add the module to lib.rs**

in `tein/src/lib.rs`, after the `#[cfg(feature = "toml")]` / `mod toml;` block (around line 71), add:

```rust
#[cfg(feature = "uuid")]
mod uuid;
```

**Step 3: verify it compiles**

```bash
cargo build 2>&1 | tail -10
```

expected: clean. if the macro emits references the compiler can't find (e.g. `Value::from_raw`), debug by expanding the macro:

```bash
cargo expand --lib uuid 2>&1 | head -80
```

**Step 4: commit**

```bash
git add tein/src/uuid.rs tein/src/lib.rs
git commit -m "feat: implement (tein uuid) module

#[tein_module] generates VFS, docs, and trampolines for
make-uuid, uuid?, uuid-nil. first internal use of the macro."
```

---

## Task 4: register the module + add to SAFE_MODULES

**Files:**
- Modify: `tein/src/context.rs`
- Modify: `tein/src/sandbox.rs`

**Step 1: add registration call in ContextBuilder::build()**

in `tein/src/context.rs`, find the toml registration block (around line 1969-1972):

```rust
            #[cfg(feature = "toml")]
            if self.standard_env {
                context.register_toml_module()?;
            }
```

immediately after, add:

```rust
            #[cfg(feature = "uuid")]
            if self.standard_env {
                crate::uuid::uuid_impl::register_module_uuid(&context)?;
            }
```

**Step 2: add `tein/uuid` to SAFE_MODULES**

in `tein/src/sandbox.rs`, find the `SAFE_MODULES` array (line 225). add `"tein/uuid"` after `"tein/toml"`:

```rust
    "tein/toml",
    "tein/uuid",
```

**Step 3: verify it compiles and existing tests pass**

```bash
cargo build 2>&1 | tail -5
cargo test --lib 2>&1 | tail -10
```

expected: clean build, all existing tests pass.

**Step 4: commit**

```bash
git add tein/src/context.rs tein/src/sandbox.rs
git commit -m "feat: register (tein uuid) in standard env, add to SAFE_MODULES

uuid module is auto-registered for standard-env contexts and
allowed in sandboxed presets (pure computation, no IO)."
```

---

## Task 5: rust integration tests

**Files:**
- Create: `tein/tests/tein_uuid.rs`

**Step 1: write the test file**

create `tein/tests/tein_uuid.rs`:

```rust
//! integration tests for `(tein uuid)`.

use tein::{Context, Value};

fn ctx() -> Context {
    Context::new_standard().expect("context")
}

#[test]
fn test_make_uuid_returns_string() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    let val = ctx.evaluate("(make-uuid)").expect("make-uuid");
    assert!(matches!(val, Value::String(_)), "expected string, got {:?}", val);
}

#[test]
fn test_make_uuid_format() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    let val = ctx.evaluate("(make-uuid)").expect("make-uuid");
    if let Value::String(s) = val {
        assert_eq!(s.len(), 36, "uuid wrong length: {}", s);
        let parts: Vec<&str> = s.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!((parts[0].len(), parts[1].len(), parts[2].len(), parts[3].len(), parts[4].len()),
                   (8, 4, 4, 4, 12));
        assert!(parts[2].starts_with('4'), "not v4: {}", s);
    } else {
        panic!("expected string");
    }
}

#[test]
fn test_make_uuid_unique() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    let a = ctx.evaluate("(make-uuid)").expect("uuid a");
    let b = ctx.evaluate("(make-uuid)").expect("uuid b");
    assert_ne!(a, b);
}

#[test]
fn test_uuid_predicate_valid() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    assert_eq!(ctx.evaluate("(uuid? (make-uuid))").unwrap(), Value::Boolean(true));
}

#[test]
fn test_uuid_predicate_invalid_string() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    assert_eq!(ctx.evaluate(r#"(uuid? "nope")"#).unwrap(), Value::Boolean(false));
}

#[test]
fn test_uuid_predicate_non_string() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    assert_eq!(ctx.evaluate("(uuid? 42)").unwrap(), Value::Boolean(false));
    assert_eq!(ctx.evaluate("(uuid? #t)").unwrap(), Value::Boolean(false));
    assert_eq!(ctx.evaluate("(uuid? '())").unwrap(), Value::Boolean(false));
}

#[test]
fn test_uuid_nil_value() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    assert_eq!(
        ctx.evaluate("uuid-nil").unwrap(),
        Value::String("00000000-0000-0000-0000-000000000000".to_string())
    );
}

#[test]
fn test_uuid_nil_is_valid() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import");
    assert_eq!(ctx.evaluate("(uuid? uuid-nil)").unwrap(), Value::Boolean(true));
}

#[test]
fn test_uuid_docs() {
    let ctx = ctx();
    ctx.evaluate("(import (tein uuid))").expect("import uuid");
    ctx.evaluate("(import (tein uuid docs))").expect("import uuid docs");
    ctx.evaluate("(import (tein docs))").expect("import docs");
    let desc = ctx.evaluate("(describe uuid-docs)").expect("describe");
    if let Value::String(s) = desc {
        assert!(s.contains("make-uuid"), "docs missing make-uuid: {}", s);
        assert!(s.contains("uuid?"), "docs missing uuid?: {}", s);
        assert!(s.contains("uuid-nil"), "docs missing uuid-nil: {}", s);
    } else {
        panic!("describe returned non-string: {:?}", desc);
    }
}
```

**Step 2: run tests**

```bash
cargo test --test tein_uuid 2>&1 | tail -20
```

expected: all 9 tests pass.

**Step 3: commit**

```bash
git add tein/tests/tein_uuid.rs
git commit -m "test: rust integration tests for (tein uuid)"
```

---

## Task 6: scheme integration tests

**Files:**
- Create: `tein/tests/scheme/tein_uuid.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: write the scheme test file**

create `tein/tests/scheme/tein_uuid.scm`:

```scheme
;;; (tein uuid) scheme-level tests

(import (tein uuid))

;; make-uuid returns a string
(test-true "uuid/make-uuid-string?" (string? (make-uuid)))

;; make-uuid returns a valid uuid
(test-true "uuid/make-uuid-valid" (uuid? (make-uuid)))

;; two calls return different values
(test-false "uuid/make-uuid-unique"
  (equal? (make-uuid) (make-uuid)))

;; uuid? on a known valid uuid
(test-true "uuid/predicate-valid"
  (uuid? "f47ac10b-58cc-4372-a567-0e02b2c3d479"))

;; uuid? returns #f for non-uuids
(test-false "uuid/predicate-int"    (uuid? 42))
(test-false "uuid/predicate-bool"   (uuid? #t))
(test-false "uuid/predicate-list"   (uuid? '()))
(test-false "uuid/predicate-empty"  (uuid? ""))
(test-false "uuid/predicate-junk"   (uuid? "not-a-uuid"))

;; uuid-nil is the nil uuid string
(test-equal "uuid/nil-value"
  "00000000-0000-0000-0000-000000000000"
  uuid-nil)

;; uuid-nil passes uuid?
(test-true "uuid/nil-valid" (uuid? uuid-nil))
```

**Step 2: add test runner to scheme_tests.rs**

in `tein/tests/scheme_tests.rs`, after the toml test block (around line 179), add:

```rust
#[cfg(feature = "uuid")]
#[test]
fn test_scheme_tein_uuid() {
    run_scheme_test(include_str!("scheme/tein_uuid.scm"));
}
```

note: `run_scheme_test` suffices (no `run_scheme_test_with_module` needed) because the uuid module is auto-registered in standard-env contexts now (task 4).

**Step 3: run the scheme test**

```bash
cargo test test_scheme_tein_uuid -- --nocapture 2>&1 | tail -20
```

expected: passes.

**Step 4: run all tests**

```bash
just test 2>&1 | tail -20
```

expected: all tests pass.

**Step 5: commit**

```bash
git add tein/tests/scheme/tein_uuid.scm tein/tests/scheme_tests.rs
git commit -m "test: scheme-level (tein uuid) integration tests"
```

---

## Task 7: update docs and AGENTS.md

**Files:**
- Modify: `tein/src/lib.rs` (feature flags table)
- Modify: `AGENTS.md` (architecture, test counts, safe modules list)

**Step 1: update feature flags table in lib.rs**

in `tein/src/lib.rs`, find the feature flags table (around line 40-43). add a `uuid` row:

```
//! | `uuid`  | yes     | Enables `(tein uuid)` module with `make-uuid`, `uuid?`, and `uuid-nil`. Pulls in `uuid` crate. |
```

**Step 2: update AGENTS.md**

- in the architecture section, after `toml.rs` entry, add:
  ```
  uuid.rs      — uuid_impl #[tein_module]: make-uuid (v4 generation), uuid? (predicate via
                 Uuid::parse_str), uuid-nil (constant). feature-gated behind `uuid` cargo feature
  ```
- update test counts in the `## commands` section (add uuid test counts to the total)
- in the `SAFE_MODULES` mention, note that `tein/uuid` is included
- add `(tein uuid)` to the VFS module list in the architecture section:
  ```
  lib/tein/uuid.sld  — (tein uuid) library definition (generated by #[tein_module])
  lib/tein/uuid.scm  — module documentation + uuid-nil constant (generated by #[tein_module])
  ```

**Step 3: get actual test counts**

```bash
just test 2>&1 | grep "test result"
```

use these numbers to update AGENTS.md.

**Step 4: lint**

```bash
just lint
```

**Step 5: commit**

```bash
git add tein/src/lib.rs AGENTS.md
git commit -m "docs: update feature flags and AGENTS.md for (tein uuid)

closes #39"
```

---

## Task 8: final verification + cleanup

**Step 1: full test suite**

```bash
just test
```

expected: all tests pass.

**Step 2: lint**

```bash
just lint
```

expected: clean.

**Step 3: verify the old plan is superseded**

the old plan at `docs/plans/2026-02-26-tein-uuid.md` is now obsolete. delete it:

```bash
git rm docs/plans/2026-02-26-tein-uuid.md
git commit -m "chore: remove superseded uuid plan (replaced by 2026-03-01)"
```

**Step 4: collect AGENTS.md notes**

review this session for any new gotchas or patterns worth documenting:
- `extern crate self as tein;` pattern for internal `#[tein_module]` use
- `Value` arg support in `#[tein_fn]` free fns (added in task 1)

if these are already captured in AGENTS.md from task 7, no further action needed.
