# `#[tein_module]` Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

## Progress

- [x] Task 1: runtime VFS registration — C side (chibi fork commit 811d611, pushed)
- [x] Task 2: runtime VFS registration — rust side (tein commit 246bdb3)
- [x] Task 3: #[tein_fn] standalone mode (tein commit 652d5f4, tein_fn.rs created)
- [ ] Task 4: naming convention helpers
- [ ] Task 5: #[tein_module] parsing + structure
- [ ] Task 6: #[tein_module] ForeignType codegen
- [ ] Task 7: #[tein_module] extern C wrappers
- [ ] Task 8: #[tein_module] VFS content + register fn
- [ ] Task 9: naming edge cases + _q/_bang tests
- [ ] Task 10: scheme-side integration test
- [ ] Task 11: remove scheme_fn deprecation, update docs
- [ ] Task 12: final verification + design doc update

**Worktree:** `/home/fey/projects/tein/.worktrees/tein-module` on branch `feat/tein-module`
— all implementation work happens here, not in the main repo checkout.

**Note for resuming:** scheme_fn.rs is still in tests/ (deprecated, not yet removed — task 11).

**Goal:** Replace `#[scheme_fn]` with a unified `#[tein_module]` attribute macro system that generates ForeignType impls, extern "C" wrappers, VFS module content, and registration functions from annotated rust mod blocks.

**Architecture:** A single `#[tein_module("name")]` attribute macro parses a mod block, finds `#[tein_fn]`/`#[tein_type]`/`#[tein_methods]` items, and generates all scheme-side glue. Runtime VFS registration (`ctx.register_vfs_module()`) lets macro-generated modules participate in chibi's `(import ...)` system without build.rs coordination. `#[tein_fn]` also works standalone (replacing `#[scheme_fn]`).

**Tech Stack:** Rust proc macros (syn, quote, proc-macro2), chibi-scheme C FFI (tein_shim.c), tein's ForeignType protocol.

**Design doc:** `docs/plans/2026-02-26-tein-module-design.md`

---

## Task 1: runtime VFS registration — C side

Add a dynamic VFS extension to `tein_shim.c` so that rust can register VFS entries at runtime.

**Files:**
- Modify: `target/chibi-scheme/tein_shim.c`

**Step 1: Add the dynamic VFS table**

After the `#include "tein_vfs_data.h"` line and before `tein_vfs_lookup`, add a dynamic entry
table and registration function. The dynamic table is a simple linked list of heap-allocated
entries — small, no-dependency, searched after the static table.

```c
// --- dynamic VFS entries (registered at runtime from rust) ---

struct tein_vfs_dynamic_entry {
    char *key;
    char *content;
    unsigned int length;
    struct tein_vfs_dynamic_entry *next;
};

static __thread struct tein_vfs_dynamic_entry *tein_vfs_dynamic_head = NULL;

// register a VFS entry at runtime. key and content are copied.
// called from rust via ffi. not thread-safe across threads (but Context is !Send).
void tein_vfs_register(const char *key, const char *content, unsigned int length) {
    struct tein_vfs_dynamic_entry *entry = malloc(sizeof(struct tein_vfs_dynamic_entry));
    entry->key = malloc(strlen(key) + 1);
    strcpy(entry->key, key);
    entry->content = malloc(length);
    memcpy(entry->content, content, length);
    entry->length = length;
    entry->next = tein_vfs_dynamic_head;
    tein_vfs_dynamic_head = entry;
}

// clear all dynamic VFS entries (called on context drop from rust).
void tein_vfs_clear_dynamic(void) {
    struct tein_vfs_dynamic_entry *entry = tein_vfs_dynamic_head;
    while (entry) {
        struct tein_vfs_dynamic_entry *next = entry->next;
        free(entry->key);
        free(entry->content);
        free(entry);
        entry = next;
    }
    tein_vfs_dynamic_head = NULL;
}
```

**Step 2: Update `tein_vfs_lookup` to check dynamic entries**

Modify the existing `tein_vfs_lookup` to fall through to the dynamic table after the
static table miss:

```c
const char* tein_vfs_lookup(const char *full_path, unsigned int *out_length) {
    // check static table first (compile-time VFS)
    for (int i = 0; tein_vfs_table[i].key != NULL; i++) {
        if (strcmp(tein_vfs_table[i].key, full_path) == 0) {
            if (out_length) *out_length = tein_vfs_table[i].length;
            return tein_vfs_table[i].content;
        }
    }
    // check dynamic table (runtime VFS from rust)
    struct tein_vfs_dynamic_entry *entry = tein_vfs_dynamic_head;
    while (entry) {
        if (strcmp(entry->key, full_path) == 0) {
            if (out_length) *out_length = entry->length;
            return entry->content;
        }
        entry = entry->next;
    }
    return NULL;
}
```

**Step 3: Verify chibi fork still builds**

```bash
cd tein && cargo build
```

Expected: compiles. no test changes yet — just the C plumbing.

**Step 4: Commit to the chibi fork**

This change is in the chibi fork (`emesal/chibi-scheme`, branch `emesal-tein`).
Commit and push to the fork, then update the tein repo's build.rs fetch if needed.

```bash
cd target/chibi-scheme && git add tein_shim.c && git commit -m "feat: runtime VFS registration for tein module system"
git push origin emesal-tein
```

---

## Task 2: runtime VFS registration — rust side

Expose the C VFS functions to rust and add `Context::register_vfs_module()`.

**Files:**
- Modify: `tein/src/ffi.rs` — add extern declarations
- Modify: `tein/src/context.rs` — add `register_vfs_module()` method + cleanup in `drop()`

**Step 1: Add extern declarations in ffi.rs**

After the existing extern "C" declarations (find the block with `tein_shim.c` exports):

```rust
// --- runtime VFS registration (tein_shim.c) ---

extern "C" {
    /// register a VFS entry at runtime. key and content are copied by C.
    pub fn tein_vfs_register(key: *const c_char, content: *const c_char, length: c_uint);

    /// clear all dynamic VFS entries. called on Context::drop().
    pub fn tein_vfs_clear_dynamic();
}
```

**Step 2: Add `register_vfs_module` to Context**

In `context.rs`, add a public method (near the other registration methods like
`register_foreign_type` and `define_fn_variadic`):

```rust
/// Register a virtual filesystem entry at runtime.
///
/// `path` is relative to the VFS root, e.g. `"lib/tein/json.sld"`.
/// The entry becomes available to chibi's module resolver (`(import ...)`)
/// immediately. Must be called before any scheme code imports the module.
///
/// Entries registered this way are cleared on `Context::drop()`, so
/// each context has its own set of runtime VFS modules.
///
/// # errors
///
/// Returns `Error::EvalError` if path contains null bytes.
pub fn register_vfs_module(&self, path: &str, content: &str) -> Result<()> {
    let full_path = format!("/vfs/{}", path);
    let c_path = CString::new(full_path)
        .map_err(|_| Error::EvalError("VFS path contains null bytes".to_string()))?;
    unsafe {
        ffi::tein_vfs_register(
            c_path.as_ptr(),
            content.as_ptr() as *const std::ffi::c_char,
            content.len() as std::ffi::c_uint,
        );
    }
    Ok(())
}
```

**Step 3: Add cleanup in `Context::drop()`**

Find the `Drop` impl for `Context` and add `tein_vfs_clear_dynamic()`:

```rust
// in impl Drop for Context:
unsafe { ffi::tein_vfs_clear_dynamic(); }
```

**Step 4: Write a test**

In `context.rs` tests (after the existing VFS/import tests):

```rust
#[test]
fn test_register_vfs_module() {
    let ctx = Context::new_standard().expect("standard context");

    // register a trivial runtime VFS module
    ctx.register_vfs_module(
        "lib/tein/test-runtime.sld",
        "(define-library (tein test-runtime) (export test-rt-val) (include \"test-runtime.scm\"))",
    ).expect("register sld");
    ctx.register_vfs_module(
        "lib/tein/test-runtime.scm",
        "(define test-rt-val 42)",
    ).expect("register scm");

    // import and use it
    let result = ctx.evaluate("(import (tein test-runtime)) test-rt-val").expect("eval");
    assert_eq!(result, Value::Integer(42));
}
```

**Step 5: Run test**

```bash
cd tein && cargo test test_register_vfs_module -- --nocapture
```

Expected: PASS.

**Step 6: Commit**

```bash
git add tein/src/ffi.rs tein/src/context.rs
git commit -m "feat: Context::register_vfs_module — runtime VFS registration from rust"
```

---

## Task 3: `#[tein_fn]` standalone mode (replaces `#[scheme_fn]`)

Implement `#[tein_fn]` as a standalone attribute macro that generates the same extern "C"
wrapper as `#[scheme_fn]`, then migrate all uses.

**Files:**
- Modify: `tein-macros/src/lib.rs` — add `tein_fn` attribute, refactor shared codegen
- Modify: `tein/src/lib.rs` — re-export `tein_fn` instead of `scheme_fn`
- Modify: `tein/tests/scheme_fn.rs` — rename to `tein_fn.rs`, migrate all uses

**Step 1: Add `#[tein_fn]` entry point**

In `tein-macros/src/lib.rs`, add a new proc macro attribute that delegates to the same
codegen as `#[scheme_fn]`:

```rust
/// attribute macro for defining scheme-callable foreign functions.
///
/// generates an `unsafe extern "C"` wrapper function named `__tein_{fn_name}`
/// that handles argument extraction, type conversion, and panic safety.
///
/// works standalone (caller registers via `ctx.define_fn_variadic`) or
/// inside a `#[tein_module]` block (module macro handles registration).
///
/// # supported argument types
///
/// - `i64` — scheme integer
/// - `f64` — scheme float
/// - `String` — scheme string
/// - `bool` — scheme boolean
///
/// <!-- extensibility: to add new arg types (e.g. Value, Vec<Value>, &[u8], char),
///      add a branch in gen_arg_extraction() matching the type name string.
///      each branch emits a proc_macro2::TokenStream declaring a local variable
///      with the extracted value. follow the i64/f64/String/bool patterns. -->
///
/// # supported return types
///
/// - `i64`, `f64`, `String`, `bool` — auto-converted to scheme
/// - `Result<T, E>` where T is a supported type — Err becomes scheme exception
/// - `()` — returns scheme void
///
/// <!-- extensibility: to add new return types (e.g. Value, Vec<Value>),
///      add a branch in gen_return_conversion(). -->
///
/// # examples
///
/// ```ignore
/// #[tein_fn]
/// fn add(a: i64, b: i64) -> i64 { a + b }
///
/// // register manually:
/// ctx.define_fn_variadic("add", __tein_add)?;
/// ```
#[proc_macro_attribute]
pub fn tein_fn(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    match generate_scheme_fn(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
```

This re-uses the existing `generate_scheme_fn` codegen — same output, new name.

**Step 2: Keep `#[scheme_fn]` as a deprecated alias**

Mark `scheme_fn` as deprecated to give a migration signal:

```rust
#[deprecated(note = "use #[tein_fn] instead")]
#[proc_macro_attribute]
pub fn scheme_fn(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    match generate_scheme_fn(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
```

**Step 3: Update lib.rs re-export**

In `tein/src/lib.rs`, add the new re-export:

```rust
/// Re-export the `#[tein_fn]` proc macro for defining scheme-callable functions.
pub use tein_macros::tein_fn;

/// Deprecated: use [`tein_fn`] instead.
#[deprecated(note = "use #[tein_fn] instead")]
pub use tein_macros::scheme_fn;
```

**Step 4: Migrate tests**

Rename `tein/tests/scheme_fn.rs` to `tein/tests/tein_fn.rs`. Replace all `#[scheme_fn]`
with `#[tein_fn]` and update the import:

```rust
use tein::{Context, Value, tein_fn};

#[tein_fn]
fn add(a: i64, b: i64) -> i64 { a + b }
// ... etc (12 test functions, all mechanical replacement)
```

**Step 5: Run all tests**

```bash
cd tein && cargo test
```

Expected: all pass. the `scheme_fn` import triggers deprecation warnings but still compiles.

**Step 6: Commit**

```bash
git add tein-macros/src/lib.rs tein/src/lib.rs tein/tests/tein_fn.rs
git rm tein/tests/scheme_fn.rs
git commit -m "feat: #[tein_fn] replaces #[scheme_fn] — same codegen, unified naming"
```

---

## Task 4: naming convention helpers

Add the rust→scheme name transformation functions to `tein-macros`. These are pure string
transforms used by both `#[tein_fn]` (inside modules) and `#[tein_module]`.

**Files:**
- Modify: `tein-macros/src/lib.rs` — add naming functions + tests

**Step 1: Add the naming functions**

```rust
/// convert a rust identifier to a scheme name.
///
/// - `snake_case` → `kebab-case`
/// - trailing `_q` → `?`
/// - trailing `_bang` → `!`
///
/// examples: `is_match` → `is-match`, `object_q` → `object?`, `set_bang` → `set!`
fn rust_to_scheme_name(rust_name: &str) -> String {
    let name = if let Some(stem) = rust_name.strip_suffix("_q") {
        format!("{}?", stem.replace('_', "-"))
    } else if let Some(stem) = rust_name.strip_suffix("_bang") {
        format!("{}!", stem.replace('_', "-"))
    } else {
        rust_name.replace('_', "-")
    };
    name
}

/// convert a PascalCase type name to kebab-case.
///
/// examples: `JsonValue` → `json-value`, `HttpClient` → `http-client`
fn pascal_to_kebab(name: &str) -> String {
    let mut result = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}
```

**Step 2: Add unit tests**

At the bottom of `tein-macros/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_to_scheme_name() {
        assert_eq!(rust_to_scheme_name("parse"), "parse");
        assert_eq!(rust_to_scheme_name("is_match"), "is-match");
        assert_eq!(rust_to_scheme_name("object_q"), "object?");
        assert_eq!(rust_to_scheme_name("set_value_bang"), "set-value!");
        assert_eq!(rust_to_scheme_name("a_b_c"), "a-b-c");
    }

    #[test]
    fn test_pascal_to_kebab() {
        assert_eq!(pascal_to_kebab("JsonValue"), "json-value");
        assert_eq!(pascal_to_kebab("HttpClient"), "http-client");
        assert_eq!(pascal_to_kebab("Regex"), "regex");
        assert_eq!(pascal_to_kebab("URL"), "u-r-l");  // edge case, use name override
    }
}
```

**Step 3: Run tests**

```bash
cd tein && cargo test -p tein-macros
```

Expected: PASS.

**Step 4: Commit**

```bash
git add tein-macros/src/lib.rs
git commit -m "feat: rust→scheme naming convention transforms (snake→kebab, _q→?, _bang→!)"
```

---

## Task 5: `#[tein_module]` — parsing and structure

Implement the `#[tein_module]` attribute macro's parsing phase: extract `#[tein_fn]`,
`#[tein_type]`, and `#[tein_methods]` items from the mod block. No codegen yet — just
the data model and parsing.

**Files:**
- Modify: `tein-macros/src/lib.rs` — add module parsing + data structures

**Step 1: Add the module data model**

```rust
/// parsed representation of a `#[tein_module("name")]` block.
struct ModuleInfo {
    /// module name as it appears in scheme, e.g. "json"
    name: String,
    /// the original mod item (preserved in output)
    mod_item: syn::ItemMod,
    /// free functions annotated with #[tein_fn]
    free_fns: Vec<FreeFnInfo>,
    /// types annotated with #[tein_type]
    types: Vec<TypeInfo>,
}

/// a #[tein_fn] free function
struct FreeFnInfo {
    /// the original function
    func: syn::ItemFn,
    /// scheme name (derived or overridden)
    scheme_name: String,
}

/// a #[tein_type] struct + its #[tein_methods] impl block
struct TypeInfo {
    /// the original struct
    struct_item: syn::ItemStruct,
    /// scheme type name (derived from struct name or name="..." override)
    scheme_type_name: String,
    /// methods from #[tein_methods] impl block, if any
    methods: Vec<MethodInfo>,
}

/// a single method from a #[tein_methods] impl block
struct MethodInfo {
    /// the original method
    method: syn::ImplItemFn,
    /// scheme name (module-type-method, e.g. "regex-is-match")
    scheme_name: String,
    /// whether the method takes &mut self (vs &self)
    is_mut: bool,
}
```

**Step 2: Add the parsing entry point**

```rust
#[proc_macro_attribute]
pub fn tein_module(attr: TokenStream, item: TokenStream) -> TokenStream {
    let module_name = parse_macro_input!(attr as syn::LitStr).value();
    let mod_item = parse_macro_input!(item as syn::ItemMod);
    match parse_and_generate_module(module_name, mod_item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn parse_and_generate_module(
    module_name: String,
    mod_item: syn::ItemMod,
) -> syn::Result<proc_macro2::TokenStream> {
    let info = parse_module_info(module_name, mod_item)?;
    generate_module(info)
}
```

**Step 3: Implement `parse_module_info`**

This function walks the mod's items, identifies annotated items, strips the tein
attributes, and builds `ModuleInfo`. Key logic:

- walk `mod_item.content` (the `Some((brace, items))` variant)
- for each item, check for `#[tein_fn]`, `#[tein_type]`, `#[tein_methods]` attributes
- strip those attributes from the original items (they're consumed by the macro)
- error on `#[tein_module]` on a mod without a body (mod file references)
- error on `#[tein_methods]` impl for a type not annotated with `#[tein_type]`
- parse `name = "..."` overrides from attribute arguments

```rust
fn parse_module_info(module_name: String, mod_item: syn::ItemMod) -> syn::Result<ModuleInfo> {
    let (brace, items) = mod_item.content.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(&mod_item, "#[tein_module] requires an inline mod body")
    })?;

    let mut free_fns = Vec::new();
    let mut types = Vec::new();
    let mut type_names: HashMap<String, usize> = HashMap::new(); // rust name → types index
    let mut clean_items = Vec::new(); // items with tein attrs stripped

    for item in items {
        match item {
            syn::Item::Fn(func) if has_attr(&func.attrs, "tein_fn") => {
                let override_name = extract_name_attr(&func.attrs, "tein_fn")?;
                let rust_name = func.sig.ident.to_string();
                let scheme_name = override_name.unwrap_or_else(|| {
                    format!("{}-{}", module_name, rust_to_scheme_name(&rust_name))
                });
                let mut clean_func = func.clone();
                clean_func.attrs.retain(|a| !is_tein_attr(a, "tein_fn"));
                free_fns.push(FreeFnInfo { func: clean_func.clone(), scheme_name });
                clean_items.push(syn::Item::Fn(clean_func));
            }
            syn::Item::Struct(s) if has_attr(&s.attrs, "tein_type") => {
                let override_name = extract_name_attr(&s.attrs, "tein_type")?;
                let rust_name = s.ident.to_string();
                let scheme_type_name = override_name
                    .unwrap_or_else(|| pascal_to_kebab(&rust_name));
                let idx = types.len();
                type_names.insert(rust_name.clone(), idx);
                let mut clean_struct = s.clone();
                clean_struct.attrs.retain(|a| !is_tein_attr(a, "tein_type"));
                types.push(TypeInfo {
                    struct_item: clean_struct.clone(),
                    scheme_type_name,
                    methods: Vec::new(),
                });
                clean_items.push(syn::Item::Struct(clean_struct));
            }
            syn::Item::Impl(imp) if has_attr(&imp.attrs, "tein_methods") => {
                // find which type this impl is for
                let type_name = impl_type_name(imp)?;
                let type_idx = type_names.get(&type_name).ok_or_else(|| {
                    syn::Error::new_spanned(
                        imp,
                        format!("#[tein_methods] on impl for '{}' but no #[tein_type] struct found — \
                                 add #[tein_type] to the struct definition", type_name),
                    )
                })?;
                let scheme_type_name = types[*type_idx].scheme_type_name.clone();

                for item in &imp.items {
                    if let syn::ImplItem::Fn(method) = item {
                        let rust_name = method.sig.ident.to_string();
                        let scheme_name = format!(
                            "{}-{}",
                            scheme_type_name,
                            rust_to_scheme_name(&rust_name)
                        );
                        let is_mut = method.sig.inputs.first().map_or(false, |arg| {
                            matches!(arg, syn::FnArg::Receiver(r) if r.mutability.is_some())
                        });
                        types[*type_idx].methods.push(MethodInfo {
                            method: method.clone(),
                            scheme_name,
                            is_mut,
                        });
                    }
                }
                let mut clean_impl = imp.clone();
                clean_impl.attrs.retain(|a| !is_tein_attr(a, "tein_methods"));
                clean_items.push(syn::Item::Impl(clean_impl));
            }
            other => {
                clean_items.push(other.clone());
            }
        }
    }

    let mut clean_mod = mod_item.clone();
    if let Some((brace, _)) = &mut clean_mod.content {
        // replace with cleaned items
    }
    clean_mod.content = Some((*brace, clean_items));
    clean_mod.attrs.retain(|a| !is_tein_attr(a, "tein_module"));

    Ok(ModuleInfo { name: module_name, mod_item: clean_mod, free_fns, types })
}
```

Also add the helper functions `has_attr`, `is_tein_attr`, `extract_name_attr`, `impl_type_name`.

**Step 4: Stub out `generate_module` to just emit the original mod**

For now, just pass through the cleaned mod with no generated code:

```rust
fn generate_module(info: ModuleInfo) -> syn::Result<proc_macro2::TokenStream> {
    let mod_item = &info.mod_item;
    Ok(quote! { #mod_item })
}
```

**Step 5: Add `#[tein_type]` and `#[tein_methods]` as no-op attributes**

These need to exist as proc macro attributes so the compiler doesn't reject them
inside the mod before `#[tein_module]` processes them. However, since `#[tein_module]`
strips them during parsing, they only need to exist as passthrough markers:

```rust
/// marks a struct as a foreign type within a `#[tein_module]`.
/// outside a module, this attribute has no effect.
#[proc_macro_attribute]
pub fn tein_type(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item // passthrough — actual processing done by #[tein_module]
}

/// marks an impl block's methods for scheme exposure within a `#[tein_module]`.
/// outside a module, this attribute has no effect.
#[proc_macro_attribute]
pub fn tein_methods(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item // passthrough — actual processing done by #[tein_module]
}
```

**Step 6: Write a compile test**

Create `tein/tests/tein_module_parse.rs`:

```rust
//! test that #[tein_module] parses without errors (no codegen yet)

use tein::tein_module;

#[tein_module("test-parse")]
mod test_parse {
    use tein::tein_fn;

    #[tein_fn]
    fn hello() -> i64 { 42 }
}

#[test]
fn test_module_parses() {
    // if this compiles, parsing succeeded
    assert!(true);
}
```

Add re-exports in `tein/src/lib.rs`:

```rust
pub use tein_macros::{tein_fn, tein_module, tein_type, tein_methods};
```

**Step 7: Run**

```bash
cd tein && cargo test test_module_parses -- --nocapture
```

Expected: PASS (module parses, emits the original mod, no generated code yet).

**Step 8: Commit**

```bash
git add tein-macros/src/lib.rs tein/src/lib.rs tein/tests/tein_module_parse.rs
git commit -m "feat: #[tein_module] parsing phase — extracts tein_fn, tein_type, tein_methods items"
```

---

## Task 6: `#[tein_module]` — ForeignType codegen

Generate the `ForeignType` impl for `#[tein_type]` structs based on their
`#[tein_methods]` impl block.

**Files:**
- Modify: `tein-macros/src/lib.rs` — add `generate_foreign_type_impl`

**Step 1: Add method argument extraction codegen for `&[Value]`**

Methods receive `&[Value]` (already inside the ForeignType dispatch chain), not raw sexp.
Add a function that generates extraction from `Value`:

```rust
/// generate extraction code for a method argument from &[Value].
///
/// <!-- extensibility: to add new types (e.g. Vec<Value>, &[u8], char),
///      add a branch here matching the type name string. each branch
///      should emit code that extracts from args[index] using Value's
///      accessor methods. follow the i64/String/bool patterns. -->
fn gen_method_arg_extraction(
    arg_name: &syn::Ident,
    ty: &Type,
    index: usize,
    type_name: &str,
    method_name: &str,
) -> syn::Result<proc_macro2::TokenStream> {
    let type_str = type_name_str(ty).unwrap_or_default();
    let err_msg = format!(
        "{}-{}: argument {} ({}): expected {}",
        type_name, method_name, index + 1, arg_name, type_str
    );

    let extraction = match type_str.as_str() {
        "i64" => quote! {
            let #arg_name: i64 = __tein_args[#index].as_integer()
                .ok_or_else(|| tein::Error::TypeError(#err_msg.to_string()))?;
        },
        "f64" => quote! {
            let #arg_name: f64 = __tein_args[#index].as_float()
                .or_else(|| __tein_args[#index].as_integer().map(|i| i as f64))
                .ok_or_else(|| tein::Error::TypeError(#err_msg.to_string()))?;
        },
        "String" => quote! {
            let #arg_name: String = __tein_args[#index].as_string()
                .ok_or_else(|| tein::Error::TypeError(#err_msg.to_string()))?
                .to_string();
        },
        "bool" => quote! {
            let #arg_name: bool = __tein_args[#index].as_boolean()
                .ok_or_else(|| tein::Error::TypeError(#err_msg.to_string()))?;
        },
        "Value" => quote! {
            let #arg_name: tein::Value = __tein_args[#index].clone();
        },
        _ => {
            return Err(syn::Error::new_spanned(
                ty,
                format!("unsupported method argument type: '{}'. supported: i64, f64, String, bool, Value", type_str),
            ));
        }
    };
    Ok(extraction)
}
```

**Step 2: Generate the `ForeignType` impl**

```rust
fn generate_foreign_type_impl(type_info: &TypeInfo) -> syn::Result<proc_macro2::TokenStream> {
    let struct_name = &type_info.struct_item.ident;
    let scheme_type_name = &type_info.scheme_type_name;

    let method_entries: Vec<proc_macro2::TokenStream> = type_info.methods.iter().map(|m| {
        let method_name = &m.method.sig.ident;
        let scheme_method_name = rust_to_scheme_name(&method_name.to_string());

        // parse method args (skip &self/&mut self)
        let args: Vec<_> = m.method.sig.inputs.iter()
            .skip(1) // skip self
            .enumerate()
            .collect();

        let extractions: Vec<_> = args.iter().map(|(i, arg)| {
            if let syn::FnArg::Typed(pat_type) = arg {
                if let syn::Pat::Ident(pat) = pat_type.pat.as_ref() {
                    return gen_method_arg_extraction(
                        &pat.ident, &pat_type.ty, *i,
                        scheme_type_name, &scheme_method_name,
                    );
                }
            }
            Err(syn::Error::new_spanned(arg, "expected named argument"))
        }).collect::<syn::Result<Vec<_>>>()?;

        let arg_names: Vec<_> = args.iter().filter_map(|(_, arg)| {
            if let syn::FnArg::Typed(pat_type) = arg {
                if let syn::Pat::Ident(pat) = pat_type.pat.as_ref() {
                    return Some(&pat.ident);
                }
            }
            None
        }).collect();

        let cast = if m.is_mut {
            quote! { let __tein_this = __tein_obj.downcast_mut::<#struct_name>().unwrap(); }
        } else {
            quote! { let __tein_this = __tein_obj.downcast_ref::<#struct_name>().unwrap(); }
        };

        let call = quote! { #struct_name::#method_name(__tein_this, #(#arg_names),*) };

        // handle return type conversion
        let return_conv = gen_method_return_conversion(&m.method.sig.output, call)?;

        let scheme_name_lit = syn::LitStr::new(&scheme_method_name, method_name.span());

        Ok(quote! {
            (#scheme_name_lit, |__tein_obj: &mut dyn ::std::any::Any,
                                _ctx: &tein::MethodContext,
                                __tein_args: &[tein::Value]| -> tein::Result<tein::Value> {
                #(#extractions)*
                #cast
                #return_conv
            } as tein::MethodFn)
        })
    }).collect::<syn::Result<Vec<_>>>()?;

    Ok(quote! {
        impl tein::ForeignType for #struct_name {
            fn type_name() -> &'static str { #scheme_type_name }
            fn methods() -> &'static [(&'static str, tein::MethodFn)] {
                &[#(#method_entries),*]
            }
        }
    })
}
```

Also add `gen_method_return_conversion` handling `Result<T, Error>`, primitive types,
`()`, and `Value`.

**Step 3: Wire into `generate_module`**

```rust
fn generate_module(info: ModuleInfo) -> syn::Result<proc_macro2::TokenStream> {
    let mod_item = &info.mod_item;
    let foreign_impls: Vec<_> = info.types.iter()
        .map(|t| generate_foreign_type_impl(t))
        .collect::<syn::Result<Vec<_>>>()?;

    // inject generated code into the mod
    let mod_name = &mod_item.ident;
    let mod_vis = &mod_item.vis;
    let mod_attrs = &mod_item.attrs;
    let (_, items) = mod_item.content.as_ref().unwrap();

    Ok(quote! {
        #(#mod_attrs)*
        #mod_vis mod #mod_name {
            #(#items)*
            #(#foreign_impls)*
        }
    })
}
```

**Step 4: Write integration test**

Create `tein/tests/tein_module_foreign.rs`:

```rust
use tein::{Context, Value, tein_module, tein_fn, tein_type, tein_methods};

#[tein_module("counter")]
mod counter {
    #[tein_type(name = "counter")]
    struct Counter { n: i64 }

    #[tein_methods]
    impl Counter {
        fn get(&self) -> i64 { self.n }
        fn increment(&mut self) -> i64 { self.n += 1; self.n }
    }
}

#[test]
fn test_module_foreign_type_generated() {
    use tein::ForeignType;
    // ForeignType impl should exist and be correct
    assert_eq!(counter::Counter::type_name(), "counter");
    assert_eq!(counter::Counter::methods().len(), 2);
    assert_eq!(counter::Counter::methods()[0].0, "get");
    assert_eq!(counter::Counter::methods()[1].0, "increment");
}
```

**Step 5: Run**

```bash
cd tein && cargo test test_module_foreign_type_generated -- --nocapture
```

Expected: PASS.

**Step 6: Commit**

```bash
git add tein-macros/src/lib.rs tein/tests/tein_module_foreign.rs
git commit -m "feat: #[tein_module] generates ForeignType impls from #[tein_type] + #[tein_methods]"
```

---

## Task 7: `#[tein_module]` — extern "C" wrappers for `#[tein_fn]`

Generate extern "C" wrapper functions for free functions inside the module.

**Files:**
- Modify: `tein-macros/src/lib.rs` — add `generate_module_fn_wrapper`

**Step 1: Add wrapper generation for module-level fns**

Re-use the existing `gen_arg_extraction` and `gen_return_conversion` from the standalone
`#[tein_fn]` codegen. The wrapper follows the exact same pattern — the difference is that
it's emitted inside the module's generated block, and fns returning a `ForeignType` need
to call `foreign_value()` (which requires a Context reference).

Note: for M8's initial version, `#[tein_fn]` inside modules use the same raw-sexp extraction
as standalone `#[tein_fn]`. ForeignType args in `#[tein_fn]` are out of scope for this task
(the type mapping section lists them, but they require ForeignStore access from within the
extern "C" wrapper, which means reading from the thread-local — same pattern as
`foreign_call_wrapper` in context.rs).

```rust
fn generate_module_fn_wrapper(
    fn_info: &FreeFnInfo,
) -> syn::Result<proc_macro2::TokenStream> {
    // delegate to the same codegen as standalone #[tein_fn]
    generate_scheme_fn(fn_info.func.clone())
}
```

**Step 2: Wire into `generate_module`**

Add the wrapper functions to the module output alongside the ForeignType impls.

**Step 3: Test**

Extend `tein/tests/tein_module_foreign.rs`:

```rust
#[tein_module("mathmod")]
mod mathmod {
    #[tein_fn]
    fn double(x: i64) -> i64 { x * 2 }
}

#[test]
fn test_module_fn_wrapper() {
    let ctx = Context::new().expect("ctx");
    ctx.define_fn_variadic("mathmod-double", mathmod::__tein_double).expect("define");
    let result = ctx.evaluate("(mathmod-double 21)").expect("eval");
    assert_eq!(result, Value::Integer(42));
}
```

**Step 4: Run**

```bash
cd tein && cargo test test_module_fn_wrapper -- --nocapture
```

**Step 5: Commit**

```bash
git add tein-macros/src/lib.rs tein/tests/tein_module_foreign.rs
git commit -m "feat: #[tein_module] generates extern C wrappers for #[tein_fn] free functions"
```

---

## Task 8: `#[tein_module]` — VFS content + registration function

Generate the `.sld`/`.scm` content and the `register_module_*` function.

**Files:**
- Modify: `tein-macros/src/lib.rs` — add `generate_vfs_content` and `generate_register_fn`

**Step 1: Generate `.sld` content**

Build the export list from all free fn names, type predicates, and method wrapper names:

```rust
fn generate_vfs_sld(info: &ModuleInfo) -> String {
    let mut exports = Vec::new();

    for f in &info.free_fns {
        exports.push(f.scheme_name.clone());
    }
    for t in &info.types {
        // type predicate
        exports.push(format!("{}?", t.scheme_type_name));
        // method wrappers
        for m in &t.methods {
            exports.push(m.scheme_name.clone());
        }
    }

    let exports_str = exports.join("\n          ");
    format!(
        "(define-library (tein {name})\n  \
           (import (scheme base))\n  \
           (export {exports})\n  \
           (include \"{name}.scm\"))",
        name = info.name,
        exports = exports_str,
    )
}
```

**Step 2: Generate `.scm` content**

The `.scm` file is minimal — predicates and method wrappers are registered from rust via
`register_foreign_type` + `define_fn_variadic`. The scheme file just needs to exist for the
`.sld`'s `(include ...)` to succeed.

```rust
fn generate_vfs_scm(info: &ModuleInfo) -> String {
    format!(";; (tein {}) — generated by #[tein_module]\n", info.name)
}
```

**Step 3: Generate the registration function**

```rust
fn generate_register_fn(info: &ModuleInfo) -> proc_macro2::TokenStream {
    let fn_name = syn::Ident::new(
        &format!("register_module_{}", info.name.replace('-', "_")),
        proc_macro2::Span::call_site(),
    );

    let sld_content = generate_vfs_sld(info);
    let scm_content = generate_vfs_scm(info);
    let sld_path = format!("lib/tein/{}.sld", info.name);
    let scm_path = format!("lib/tein/{}.scm", info.name);

    let type_registrations: Vec<_> = info.types.iter().map(|t| {
        let struct_name = &t.struct_item.ident;
        quote! { __tein_ctx.register_foreign_type::<#struct_name>()?; }
    }).collect();

    let fn_registrations: Vec<_> = info.free_fns.iter().map(|f| {
        let wrapper_name = syn::Ident::new(
            &format!("__tein_{}", f.func.sig.ident),
            f.func.sig.ident.span(),
        );
        let scheme_name = &f.scheme_name;
        quote! { __tein_ctx.define_fn_variadic(#scheme_name, #wrapper_name)?; }
    }).collect();

    quote! {
        /// Register this module's types, functions, and VFS entries with a context.
        ///
        /// Must be called before any scheme code does `(import (tein ...))`.
        pub fn #fn_name(__tein_ctx: &tein::Context) -> tein::Result<()> {
            __tein_ctx.register_vfs_module(#sld_path, #sld_content)?;
            __tein_ctx.register_vfs_module(#scm_path, #scm_content)?;
            #(#type_registrations)*
            #(#fn_registrations)*
            Ok(())
        }
    }
}
```

**Step 4: Wire everything into `generate_module`**

Update `generate_module` to emit the VFS constants and register function alongside the
existing foreign type impls and wrapper functions.

**Step 5: Write the full integration test**

Create `tein/tests/tein_module_full.rs`:

```rust
use tein::{Context, ContextBuilder, Value, tein_module, tein_fn, tein_type, tein_methods};

#[tein_module("testmod")]
mod testmod {
    #[tein_fn]
    fn greet(name: String) -> String {
        format!("hello, {}!", name)
    }

    #[tein_fn]
    fn add(a: i64, b: i64) -> i64 {
        a + b
    }

    #[tein_type(name = "counter")]
    struct Counter { n: i64 }

    #[tein_methods]
    impl Counter {
        fn get(&self) -> i64 { self.n }
        fn increment(&mut self) -> i64 { self.n += 1; self.n }
    }
}

#[test]
fn test_full_module_registration() {
    let ctx = ContextBuilder::new().standard_env().build().expect("ctx");
    testmod::register_module_testmod(&ctx).expect("register");

    // free functions
    let r = ctx.evaluate(r#"(testmod-greet "world")"#).expect("eval");
    assert_eq!(r, Value::String("hello, world!".to_string()));

    let r = ctx.evaluate("(testmod-add 3 4)").expect("eval");
    assert_eq!(r, Value::Integer(7));
}

#[test]
fn test_full_module_foreign_type() {
    let ctx = ContextBuilder::new().standard_env().build().expect("ctx");
    testmod::register_module_testmod(&ctx).expect("register");

    // create foreign value and use method wrappers
    let c = ctx.foreign_value(testmod::Counter { n: 0 }).expect("foreign");
    let inc = ctx.evaluate("counter-increment").expect("lookup");
    let get = ctx.evaluate("counter-get").expect("lookup");

    ctx.call(&inc, &[c.clone()]).expect("inc");
    ctx.call(&inc, &[c.clone()]).expect("inc");
    let result = ctx.call(&get, &[c]).expect("get");
    assert_eq!(result, Value::Integer(2));
}

#[test]
fn test_full_module_import() {
    let ctx = ContextBuilder::new().standard_env().build().expect("ctx");
    testmod::register_module_testmod(&ctx).expect("register");

    // (import (tein testmod)) should work via runtime VFS
    let r = ctx.evaluate("(import (tein testmod)) (testmod-add 10 20)").expect("eval");
    assert_eq!(r, Value::Integer(30));
}
```

**Step 6: Run**

```bash
cd tein && cargo test tein_module_full -- --nocapture
```

Expected: all 3 tests PASS.

**Step 7: Commit**

```bash
git add tein-macros/src/lib.rs tein/tests/tein_module_full.rs
git commit -m "feat: #[tein_module] generates VFS content + register_module_* function"
```

---

## Task 9: naming convention edge cases and `_q`/`_bang` tests

Test the naming transform edge cases with a dedicated module.

**Files:**
- Create: `tein/tests/tein_module_naming.rs`

**Step 1: Write tests**

```rust
use tein::{Context, ContextBuilder, Value, tein_module, tein_fn, tein_type, tein_methods};

#[tein_module("nm")]
mod nm {
    #[tein_fn]
    fn is_valid_q(x: i64) -> bool { x > 0 }

    #[tein_fn]
    fn reset_bang() -> i64 { 0 }

    #[tein_fn(name = "nm-custom")]
    fn custom_override() -> i64 { 99 }

    #[tein_type(name = "widget")]
    struct Widget { val: i64 }

    #[tein_methods]
    impl Widget {
        fn active_q(&self) -> bool { self.val > 0 }
        fn clear_bang(&mut self) -> i64 { self.val = 0; 0 }
    }
}

#[test]
fn test_naming_q_suffix() {
    let ctx = ContextBuilder::new().standard_env().build().expect("ctx");
    nm::register_module_nm(&ctx).expect("register");
    let r = ctx.evaluate("(nm-is-valid? 5)").expect("eval");
    assert_eq!(r, Value::Boolean(true));
}

#[test]
fn test_naming_bang_suffix() {
    let ctx = ContextBuilder::new().standard_env().build().expect("ctx");
    nm::register_module_nm(&ctx).expect("register");
    let r = ctx.evaluate("(nm-reset!)").expect("eval");
    assert_eq!(r, Value::Integer(0));
}

#[test]
fn test_naming_override() {
    let ctx = ContextBuilder::new().standard_env().build().expect("ctx");
    nm::register_module_nm(&ctx).expect("register");
    let r = ctx.evaluate("(nm-custom)").expect("eval");
    assert_eq!(r, Value::Integer(99));
}

#[test]
fn test_naming_method_q_bang() {
    let ctx = ContextBuilder::new().standard_env().build().expect("ctx");
    nm::register_module_nm(&ctx).expect("register");

    let w = ctx.foreign_value(nm::Widget { val: 5 }).expect("foreign");
    let active = ctx.evaluate("widget-active?").expect("lookup");
    let result = ctx.call(&active, &[w]).expect("call");
    assert_eq!(result, Value::Boolean(true));
}
```

**Step 2: Run**

```bash
cd tein && cargo test tein_module_naming -- --nocapture
```

**Step 3: Commit**

```bash
git add tein/tests/tein_module_naming.rs
git commit -m "test: #[tein_module] naming conventions — _q→?, _bang→!, name override"
```

---

## Task 10: scheme-side integration test

Test the generated module from pure scheme code via `(import ...)`.

**Files:**
- Create: `tein/tests/scheme/tein_module.scm`
- Modify: `tein/tests/scheme_tests.rs` — add `run_scheme_test_with_module` helper + test fn

**Step 1: Write the scheme test file**

```scheme
;;; tein module integration — tests generated module from scheme side

(import (tein testmod))

;; free functions
(test-equal "module/greet" "hello, world!" (testmod-greet "world"))
(test-equal "module/add" 7 (testmod-add 3 4))

;; type predicate
(test-false "module/counter?-int" (counter? 42))
```

**Step 2: Add a test runner that registers a module before evaluating**

In `scheme_tests.rs`, add a helper that sets up the testmod before running scheme code.
This needs the `testmod` module from `tein_module_full.rs` — either extract it to a shared
location, or define a second copy for scheme tests. The cleanest approach: define the module
inline in scheme_tests.rs as well (duplication is acceptable for test infrastructure):

```rust
// in scheme_tests.rs
mod testmod_for_scheme {
    use tein::{tein_module, tein_fn, tein_type, tein_methods};

    #[tein_module("testmod")]
    pub mod testmod {
        #[tein_fn]
        pub fn greet(name: String) -> String {
            format!("hello, {}!", name)
        }

        #[tein_fn]
        pub fn add(a: i64, b: i64) -> i64 {
            a + b
        }

        #[tein_type(name = "counter")]
        pub struct Counter { pub n: i64 }

        #[tein_methods]
        impl Counter {
            pub fn get(&self) -> i64 { self.n }
            pub fn increment(&mut self) -> i64 { self.n += 1; self.n }
        }
    }
}

fn run_scheme_test_with_module(code: &str) {
    let ctx = tein::ContextBuilder::new().standard_env().build()
        .expect("standard context");
    testmod_for_scheme::testmod::register_module_testmod(&ctx)
        .expect("register testmod");
    // pre-import (tein test) as run_scheme_test does
    ctx.evaluate("(import (tein test))").expect("import tein test");
    let result = ctx.evaluate(code).expect("evaluate scheme test");
    // ... same assertion checking as run_scheme_test
}

#[test]
fn test_scheme_tein_module() {
    run_scheme_test_with_module(include_str!("scheme/tein_module.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_tein_module -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/tein_module.scm tein/tests/scheme_tests.rs
git commit -m "test: scheme-side integration test for #[tein_module] generated modules"
```

---

## Task 11: remove `#[scheme_fn]` deprecation, update docs

Clean up: remove the deprecated `#[scheme_fn]`, update all documentation.

**Files:**
- Modify: `tein-macros/src/lib.rs` — remove `scheme_fn` entry point
- Modify: `tein/src/lib.rs` — remove `scheme_fn` re-export, update module docs
- Modify: `ARCHITECTURE.md` — update macro documentation
- Modify: `AGENTS.md` — update conventions section

**Step 1: Remove `#[scheme_fn]`**

Delete the `scheme_fn` function from `tein-macros/src/lib.rs` and its re-export from
`tein/src/lib.rs`.

**Step 2: Update ARCHITECTURE.md**

Replace the "Registering Rust functions in Scheme" section with `#[tein_fn]` examples.
Add a "Module system" section documenting `#[tein_module]`.

**Step 3: Update AGENTS.md**

Update the test count and any references to `#[scheme_fn]`.

**Step 4: Run full test suite**

```bash
cd tein && cargo test
```

Expected: all pass, no references to `scheme_fn` remain.

**Step 5: Commit**

```bash
git add tein-macros/src/lib.rs tein/src/lib.rs ARCHITECTURE.md AGENTS.md
git commit -m "chore: remove deprecated #[scheme_fn], update docs for #[tein_module] system"
```

---

## Task 12: final verification + design doc update

**Step 1: Run full test suite**

```bash
cd tein && cargo test 2>&1 | grep "test result"
```

Expected: all test binaries pass.

**Step 2: Run clippy**

```bash
cd tein && cargo clippy
```

Expected: no warnings (or only pre-existing ones).

**Step 3: Update the design doc progress**

Mark all tasks as done in `docs/plans/2026-02-26-tein-module-design.md`.

**Step 4: Update the roadmap**

In `docs/plans/original-roadmap.md`, mark `#[tein_module]` as complete.

**Step 5: Commit**

```bash
git add docs/plans/2026-02-26-tein-module-design.md docs/plans/original-roadmap.md
git commit -m "docs: mark #[tein_module] implementation complete"
```

---

## Notes for the implementor

- **Proc macro crate limitation:** proc macro crates can only export proc macros. All
  helper functions (`rust_to_scheme_name`, `pascal_to_kebab`, etc.) must live in
  `tein-macros/src/lib.rs` — they can't be in a separate module within the proc macro crate
  (well, they can be in submodules, but the crate itself can only export macros).

- **`#[tein_type]` and `#[tein_methods]` as standalone attributes:** these are registered
  as proc macro attributes that pass through their input unchanged. `#[tein_module]` strips
  them during parsing. This avoids "unknown attribute" errors from the compiler.

- **Task ordering:** tasks 1-2 (runtime VFS) are independent from tasks 3-4 (naming +
  tein_fn migration). Tasks 5-8 build sequentially. Tasks 9-10 are independent test tasks.
  Task 11 is cleanup. Task 12 is verification.

- **The chibi fork change (task 1)** is the only change outside the tein repo. It needs to
  be pushed to `emesal/chibi-scheme` branch `emesal-tein` and then fetched by tein's
  build.rs on the next `cargo build`.

- **ForeignType args in `#[tein_fn]`** (e.g. `fn stringify(v: JsonValue) -> String`) are
  noted in the design but deferred — they require reading from `FOREIGN_STORE_PTR` inside
  the extern "C" wrapper. The infrastructure exists (see `foreign_call_wrapper` in
  context.rs), but wiring it into the macro codegen is a separate task. For M8 initial
  modules, constructors return the type and methods receive `&self`, so this isn't blocking.

- **Error handling in VFS registration:** if `register_vfs_module` is called after scheme
  code has already tried (and failed) to import the module, chibi may have cached the
  failure. Call `register_module_*` before any `evaluate()` that might trigger imports.
