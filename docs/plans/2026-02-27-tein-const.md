# `#[tein_const]` Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `#[tein_const]` attribute support to `#[tein_module]`, exposing rust constants as scheme bindings via pure VFS scheme emission.

**Architecture:** New `ConstInfo` struct + `Item::Const` arm in `parse_module_info` + scheme `(define ...)` emission in `generate_vfs_scm` + export in `generate_vfs_sld`. No extern C wrappers, no changes to `generate_register_fn`. Constants are entirely scheme-side.

**Tech Stack:** syn (proc macro parsing), quote (codegen), tein Context (integration tests)

**Design doc:** `docs/plans/2026-02-27-tein-const-design.md`

**Issue:** #59

---

### Task 1: Add `ConstInfo` and `screaming_to_kebab` helper

**Files:**
- Modify: `tein-macros/src/lib.rs:163-175` (data model section)
- Modify: `tein-macros/src/lib.rs:130-161` (naming helpers section)

**Step 1: Add `screaming_to_kebab` helper after `pascal_to_kebab`**

After `pascal_to_kebab` (line 161), add:

```rust
/// convert SCREAMING_SNAKE_CASE to kebab-case.
///
/// examples: `UUID_NIL` → `uuid-nil`, `MAX_SIZE` → `max-size`
fn screaming_to_kebab(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}
```

**Step 2: Add `ConstInfo` struct after `MethodInfo`**

After `MethodInfo` (line 203), add:

```rust
/// a `#[tein_const]` constant inside a module
struct ConstInfo {
    /// scheme name (derived from const name, or overridden via `name = "..."`)
    scheme_name: String,
    /// scheme literal representation (e.g. `"\"hello\""`, `42`, `#t`)
    scheme_literal: String,
}
```

**Step 3: Add `consts` field to `ModuleInfo`**

Add `consts: Vec<ConstInfo>` to `ModuleInfo` after `free_fns`:

```rust
struct ModuleInfo {
    name: String,
    mod_item: ItemMod,
    free_fns: Vec<FreeFnInfo>,
    /// constants annotated with `#[tein_const]`
    consts: Vec<ConstInfo>,
    types: Vec<TypeInfo>,
}
```

**Step 4: Update `parse_module_info` initialisation**

In `parse_module_info`, add `let mut consts = Vec::new();` alongside the existing vecs (line 220), and pass `consts` into the `ModuleInfo` return value at the end of the function.

**Step 5: Run `cargo build -p tein-macros` to verify compilation**

Run: `cargo build -p tein-macros`
Expected: compiles (consts field is empty vec, no breakage)

**Step 6: Commit**

```
feat(macros): add ConstInfo data model + screaming_to_kebab helper
```

---

### Task 2: Add `const_to_scheme_literal` and parsing arm

**Files:**
- Modify: `tein-macros/src/lib.rs` (helpers section + `parse_module_info` item loop)

**Step 1: Add `const_to_scheme_literal` helper**

Near the other helpers, add a function that extracts a scheme literal string from a `syn::Expr`:

```rust
/// extract a scheme literal representation from a const expression.
///
/// supported: string literals → `"..."`, integer → digits, float → digits,
/// bool → `#t`/`#f`. returns `Err` for unsupported expressions.
fn const_to_scheme_literal(expr: &syn::Expr) -> syn::Result<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit { lit, .. }) => match lit {
            syn::Lit::Str(s) => {
                // escape backslashes and double quotes for scheme string
                let escaped = s.value().replace('\\', "\\\\").replace('"', "\\\"");
                Ok(format!("\"{}\"", escaped))
            }
            syn::Lit::Int(i) => Ok(i.base10_digits().to_string()),
            syn::Lit::Float(f) => Ok(f.base10_digits().to_string()),
            syn::Lit::Bool(b) => Ok(if b.value { "#t" } else { "#f" }.to_string()),
            _ => Err(syn::Error::new_spanned(
                lit,
                "#[tein_const] supports string, integer, float, and bool literals",
            )),
        },
        // handle negative literals: `-42`, `-3.14`
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr: inner,
            ..
        }) => {
            let inner_lit = const_to_scheme_literal(inner)?;
            Ok(format!("-{}", inner_lit))
        }
        _ => Err(syn::Error::new_spanned(
            expr,
            "#[tein_const] requires a literal expression (string, integer, float, or bool)",
        )),
    }
}
```

**Step 2: Add `Item::Const` arm to `parse_module_info`**

In the item loop (after the `Item::Fn` arm, around line 242), add:

```rust
Item::Const(c) if has_tein_attr(&c.attrs, "tein_const") => {
    let override_name = extract_name_override(&c.attrs, "tein_const")?;
    let rust_name = c.ident.to_string();
    let scheme_name =
        override_name.unwrap_or_else(|| screaming_to_kebab(&rust_name));
    let scheme_literal = const_to_scheme_literal(&c.expr)?;
    consts.push(ConstInfo {
        scheme_name,
        scheme_literal,
    });
    let mut clean_const = c.clone();
    clean_const
        .attrs
        .retain(|a| !is_tein_attr_named(a, "tein_const"));
    clean_items.push(Item::Const(clean_const));
}
```

**Step 3: Run `cargo build -p tein-macros`**

Run: `cargo build -p tein-macros`
Expected: compiles (parsing works, but codegen doesn't use consts yet)

**Step 4: Commit**

```
feat(macros): parse #[tein_const] items in module item loop
```

---

### Task 3: Emit consts in VFS codegen

**Files:**
- Modify: `tein-macros/src/lib.rs:554-580` (`generate_vfs_sld` and `generate_vfs_scm`)

**Step 1: Add const exports to `generate_vfs_sld`**

In `generate_vfs_sld`, after the types loop (line 565), add:

```rust
for c in &info.consts {
    exports.push(c.scheme_name.clone());
}
```

**Step 2: Emit `(define ...)` forms in `generate_vfs_scm`**

Replace the current `generate_vfs_scm` body:

```rust
fn generate_vfs_scm(info: &ModuleInfo) -> String {
    let mut lines = vec![format!(
        ";; (tein {}) — generated by #[tein_module]",
        info.name
    )];
    for c in &info.consts {
        lines.push(format!("(define {} {})", c.scheme_name, c.scheme_literal));
    }
    lines.join("\n") + "\n"
}
```

**Step 3: Run `cargo build -p tein-macros`**

Run: `cargo build -p tein-macros`
Expected: compiles

**Step 4: Commit**

```
feat(macros): emit #[tein_const] definitions in VFS .scm/.sld
```

---

### Task 4: Write compile-time test

**Files:**
- Modify: `tein/tests/tein_module_parse.rs`

**Step 1: Add a const to the existing parse-test module**

Add a `#[tein_const]` inside the existing `parse_test` module:

```rust
#[tein_module("parse-test")]
mod parse_test {
    #[tein_fn]
    pub fn hello() -> i64 {
        42
    }

    #[tein_const]
    pub const MAX_ITEMS: i64 = 100;
}
```

This verifies codegen doesn't break when consts are present. The existing test (`test_module_generates_register_fn`) still passes unchanged.

**Step 2: Run the parse test**

Run: `cargo test -p tein --test tein_module_parse`
Expected: PASS — the module compiles with a const present

**Step 3: Commit**

```
test: add #[tein_const] to compile-time parse test
```

---

### Task 5: Write integration tests

**Files:**
- Create: `tein/tests/tein_module_const.rs`

**Step 1: Write the test file**

```rust
//! integration tests for `#[tein_const]` in `#[tein_module]`.
//!
//! exercises literal types (string, integer, float, bool), naming conventions
//! (SCREAMING_SNAKE → kebab-case), and `name = "..."` override.

use tein::{Context, Value, tein_module};

#[tein_module("tc")]
mod tc {
    #[tein_const]
    pub const GREETING: &str = "hello";

    #[tein_const]
    pub const MAX_SIZE: i64 = 256;

    #[tein_const]
    pub const PI_APPROX: f64 = 3.14;

    #[tein_const]
    pub const ENABLED: bool = true;

    #[tein_const]
    pub const DISABLED: bool = false;

    #[tein_const]
    pub const NEGATIVE: i64 = -42;

    #[tein_const(name = "custom-name")]
    pub const OVERRIDDEN: &str = "custom";

    #[tein_fn]
    pub fn dummy() -> i64 {
        0
    }
}

fn setup() -> Context {
    let ctx = Context::builder().standard_env().build().expect("ctx");
    tc::register_module_tc(&ctx).expect("register");
    ctx.evaluate("(import (tein tc))").expect("import");
    ctx
}

#[test]
fn test_const_string() {
    let ctx = setup();
    let r = ctx.evaluate("greeting").expect("eval");
    assert_eq!(r, Value::String("hello".into()));
}

#[test]
fn test_const_integer() {
    let ctx = setup();
    let r = ctx.evaluate("max-size").expect("eval");
    assert_eq!(r, Value::Integer(256));
}

#[test]
fn test_const_float() {
    let ctx = setup();
    let r = ctx.evaluate("pi-approx").expect("eval");
    assert_eq!(r, Value::Float(3.14));
}

#[test]
fn test_const_bool_true() {
    let ctx = setup();
    let r = ctx.evaluate("enabled").expect("eval");
    assert_eq!(r, Value::Boolean(true));
}

#[test]
fn test_const_bool_false() {
    let ctx = setup();
    let r = ctx.evaluate("disabled").expect("eval");
    assert_eq!(r, Value::Boolean(false));
}

#[test]
fn test_const_negative() {
    let ctx = setup();
    let r = ctx.evaluate("negative").expect("eval");
    assert_eq!(r, Value::Integer(-42));
}

#[test]
fn test_const_name_override() {
    let ctx = setup();
    let r = ctx.evaluate("custom-name").expect("eval");
    assert_eq!(r, Value::String("custom".into()));
}

#[test]
fn test_const_coexists_with_fn() {
    let ctx = setup();
    // const works
    let c = ctx.evaluate("greeting").expect("const");
    assert_eq!(c, Value::String("hello".into()));
    // fn works alongside
    let f = ctx.evaluate("(tc-dummy)").expect("fn");
    assert_eq!(f, Value::Integer(0));
}
```

**Step 2: Run the integration tests**

Run: `cargo test -p tein --test tein_module_const`
Expected: all 8 tests PASS

**Step 3: Commit**

```
test: integration tests for #[tein_const] — types, naming, override
```

---

### Task 6: Update docs and docstrings

**Files:**
- Modify: `tein-macros/src/lib.rs` (module-level doc comment, line 1-5)
- Modify: `AGENTS.md` (architecture section)

**Step 1: Update the tein-macros crate-level doc**

Line 4 currently reads:
```
//! provides `#[tein_fn]`, `#[tein_module]`, `#[tein_type]`, and `#[tein_methods]`
```

Change to:
```
//! provides `#[tein_fn]`, `#[tein_module]`, `#[tein_type]`, `#[tein_methods]`,
//! and `#[tein_const]` for ergonomic foreign function and module definition.
```

**Step 2: Update AGENTS.md command comment**

Update the test count comment in the commands section to reflect the new test count (run `cargo test` and count).

**Step 3: Run all tests**

Run: `cargo test`
Expected: all tests PASS (existing + 8 new const tests)

Run: `cargo clippy`
Expected: no warnings

**Step 4: Commit**

```
docs: update crate docs and AGENTS.md for #[tein_const]
```
