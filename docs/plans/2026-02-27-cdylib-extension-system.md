# cdylib extension system implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> **Progress:** Tasks 1–3 complete. Resume from Task 4.
>
> **Implementation notes (deviations from original plan):**
> - `type_names()` returns `Vec<String>` (owned) rather than `Vec<&'static str>` — ext type names are owned `String`, not `'static`.
> - `sexp_string_size` and `sexp_bytes_length` return `sexp_uint_t` (unsigned), not `sexp_sint_t` — needed dedicated trampolines `ext_trampoline_string_size` / `ext_trampoline_bytes_length` / `ext_trampoline_make_bytes`.
> - `sexp_make_boolean` takes `bool` in the rust wrapper but `c_int` in C ABI — `ext_trampoline_make_boolean` handles the bridge.
> - `ext_trampoline_register_type` is split into a thin `extern "C"` shim + `ext_register_type_impl` (plain fn) for clarity.
> - `EXT_API` thread-local added to `context.rs` (pub(crate)) — set during `evaluate()`, `call()`, `evaluate_port()`, and `load_extension()` so ext method dispatch always has access to the vtable.
> - `Context` gained `ext_api: RefCell<Option<Box<tein_ext::TeinExtApi>>>` — stable Box pointer for the thread-local.
> - `ExtApiGuard` RAII added alongside `ForeignStoreGuard`.
> - `ForeignStore::find_method` kept (marked `#[allow(dead_code)]`) — superseded by `find_method_any` for dispatch, but still valid internal API.
> - `MethodLookup::Ext.is_mut` marked `#[allow(dead_code)]` — reserved for future read-only dispatch optimisation.
> - License: ISC (not MIT OR Apache-2.0 as originally written in plan).

**Goal:** Enable tein modules to be compiled as cdylib shared libraries and hot-loaded at runtime via a stable C ABI.

**Architecture:** Three new components — `tein-ext` (C ABI type definitions crate), `Context::load_extension()` (libloading-based loader in tein), and `ext = true` codegen path in `tein-macros`. Extensions interact with the host through a `TeinExtApi` vtable of `extern "C"` function pointers; no rust types cross the `.so` boundary.

**Tech Stack:** rust (edition 2024), `libloading` for dlopen, `tein-ext` new crate, proc-macro codegen in `tein-macros`.

**Design doc:** `docs/plans/2026-02-27-cdylib-extension-system-design.md`

---

### Task 1: Create `tein-ext` crate with C ABI types

**Files:**
- Create: `tein-ext/Cargo.toml`
- Create: `tein-ext/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

**Step 1: Add workspace member**

In `/home/fey/projects/tein/Cargo.toml`, change:
```toml
members = ["tein", "tein-macros", "tein-sexp"]
```
to:
```toml
members = ["tein", "tein-macros", "tein-sexp", "tein-ext"]
```

**Step 2: Create `tein-ext/Cargo.toml`**

```toml
[package]
name = "tein-ext"
version = "0.1.0"
edition = "2024"
authors = ["fey"]
license = "MIT OR Apache-2.0"
description = "C ABI type definitions for tein extension modules"
repository = "https://github.com/emesal/tein"
keywords = ["scheme", "tein", "extension", "ffi"]
```

No dependencies. No features.

**Step 3: Create `tein-ext/src/lib.rs`**

This file defines the stable C ABI surface. All types are `#[repr(C)]`.

```rust
//! C ABI type definitions for tein extension modules.
//!
//! This crate defines the stable interface between the tein host and
//! dynamically loaded cdylib extensions. Only C-compatible types cross
//! the boundary — no rust ABI fragility.
//!
//! Extension crates depend on this crate (and `tein-macros`), but never
//! on `tein` itself. The `#[tein_module("name", ext = true)]` macro
//! generates all the plumbing.

use std::ffi::{c_char, c_int, c_long, c_void};

// ── opaque pointer types ─────────────────────────────────────────────────────

/// Opaque chibi-scheme context pointer.
///
/// Never dereferenced by extension code — only received from the host
/// and passed back through API calls.
#[repr(C)]
pub struct OpaqueCtx {
    _private: [u8; 0],
}

/// Opaque chibi-scheme value pointer (sexp).
///
/// Never dereferenced by extension code — only received from the host
/// and passed back through API calls. In chibi's type system, both
/// context and value are `sexp`; we use separate types to prevent
/// accidental misuse.
#[repr(C)]
pub struct OpaqueVal {
    _private: [u8; 0],
}

// ── API version ──────────────────────────────────────────────────────────────

/// Current API version. Checked by extensions at init time.
/// Bump when adding fields to `TeinExtApi`.
pub const TEIN_EXT_API_VERSION: u32 = 1;

// ── error codes ──────────────────────────────────────────────────────────────

/// Extension initialised successfully.
pub const TEIN_EXT_OK: i32 = 0;

/// API version mismatch — extension requires a newer host.
pub const TEIN_EXT_ERR_VERSION: i32 = -1;

/// Extension-defined initialisation error.
pub const TEIN_EXT_ERR_INIT: i32 = -2;

// ── function type aliases ────────────────────────────────────────────────────

/// Variadic scheme function signature — same ABI as chibi's native fns.
///
/// `ctx`, `self_`, and `args` are all `sexp` in chibi's type system
/// (hence `*mut OpaqueVal`, not `*mut OpaqueCtx`).
pub type SexpFn = unsafe extern "C" fn(
    ctx: *mut OpaqueVal,
    self_: *mut OpaqueVal,
    n: c_long,
    args: *mut OpaqueVal,
) -> *mut OpaqueVal;

/// Foreign type method function pointer.
///
/// - `obj`: pointer to the rust object (`&mut T` as `*mut c_void`)
/// - `ctx`: opaque context for value construction
/// - `api`: API table for calling host primitives
/// - `n`: argument count
/// - `args`: scheme argument list (sexp)
pub type TeinMethodFn = unsafe extern "C" fn(
    obj: *mut c_void,
    ctx: *mut OpaqueCtx,
    api: *const TeinExtApi,
    n: c_long,
    args: *mut OpaqueVal,
) -> *mut OpaqueVal;

/// Extension init entry point. Every cdylib extension exports this symbol.
pub type TeinExtInitFn = unsafe extern "C" fn(
    ctx: *mut OpaqueCtx,
    api: *const TeinExtApi,
) -> i32;

// ── foreign type descriptors ─────────────────────────────────────────────────

/// Describes a foreign type for registration across the C boundary.
///
/// Emitted as a `static` by the macro in cdylib extensions.
#[repr(C)]
pub struct TeinTypeDesc {
    /// Type name as UTF-8 (not null-terminated).
    pub type_name: *const c_char,
    /// Length of `type_name` in bytes.
    pub type_name_len: usize,
    /// Pointer to array of method descriptors.
    pub methods: *const TeinMethodDesc,
    /// Number of methods.
    pub method_count: usize,
}

/// Describes a single method on a foreign type.
#[repr(C)]
pub struct TeinMethodDesc {
    /// Method name as UTF-8 (not null-terminated).
    pub name: *const c_char,
    /// Length of `name` in bytes.
    pub name_len: usize,
    /// The method function pointer.
    pub func: TeinMethodFn,
    /// Whether the method requires mutable access to the object.
    pub is_mut: bool,
}

// ── the API vtable ───────────────────────────────────────────────────────────

/// Stable C ABI function pointer table, populated by the tein host and
/// passed to extensions at init time.
///
/// Versioned and append-only — new fields go at the end. Extensions
/// check `version` before accessing any field.
///
/// Function signatures mirror `tein::raw::*` but use opaque pointer
/// types. The host fills each slot with a trampoline that casts back
/// to `sexp` and calls the real chibi function.
#[repr(C)]
pub struct TeinExtApi {
    /// API version — must be >= `TEIN_EXT_API_VERSION`.
    pub version: u32,

    // ── high-level registration ──────────────────────────────────────

    /// Register a VFS module entry (path + content).
    ///
    /// Path and content are UTF-8, not null-terminated, with explicit
    /// lengths. Returns 0 on success, negative on error.
    pub register_vfs_module: unsafe extern "C" fn(
        ctx: *mut OpaqueCtx,
        path: *const c_char, path_len: usize,
        content: *const c_char, content_len: usize,
    ) -> i32,

    /// Register a variadic scheme function.
    ///
    /// Name is UTF-8, not null-terminated. Returns 0 on success.
    pub define_fn_variadic: unsafe extern "C" fn(
        ctx: *mut OpaqueCtx,
        name: *const c_char, name_len: usize,
        f: SexpFn,
    ) -> i32,

    /// Register a foreign type from a `TeinTypeDesc`.
    ///
    /// Returns 0 on success, negative on error.
    pub register_foreign_type: unsafe extern "C" fn(
        ctx: *mut OpaqueCtx,
        desc: *const TeinTypeDesc,
    ) -> i32,

    // ── type predicates ──────────────────────────────────────────────
    // Return nonzero if the value matches the type, 0 otherwise.

    pub sexp_integerp:   unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_flonump:    unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_stringp:    unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_booleanp:   unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_symbolp:    unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_pairp:      unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_nullp:      unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_charp:      unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_bytesp:     unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_vectorp:    unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_portp:      unsafe extern "C" fn(*mut OpaqueVal) -> c_int,
    pub sexp_exceptionp: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,

    // ── value extractors ─────────────────────────────────────────────

    /// Extract fixnum (integer) value. Caller must check `sexp_integerp` first.
    pub sexp_unbox_fixnum: unsafe extern "C" fn(*mut OpaqueVal) -> c_long,

    /// Extract flonum (f64) value. Caller must check `sexp_flonump` first.
    pub sexp_flonum_value: unsafe extern "C" fn(*mut OpaqueVal) -> f64,

    /// Get string data pointer. Caller must check `sexp_stringp` first.
    /// Returned pointer is valid for the lifetime of the sexp.
    pub sexp_string_data: unsafe extern "C" fn(*mut OpaqueVal) -> *const c_char,

    /// Get string byte length. Caller must check `sexp_stringp` first.
    pub sexp_string_size: unsafe extern "C" fn(*mut OpaqueVal) -> c_long,

    /// Extract character codepoint. Caller must check `sexp_charp` first.
    pub sexp_unbox_character: unsafe extern "C" fn(*mut OpaqueVal) -> c_int,

    /// Get bytevector data pointer. Caller must check `sexp_bytesp` first.
    pub sexp_bytes_data: unsafe extern "C" fn(*mut OpaqueVal) -> *const c_char,

    /// Get bytevector length. Caller must check `sexp_bytesp` first.
    pub sexp_bytes_length: unsafe extern "C" fn(*mut OpaqueVal) -> c_long,

    /// Get car of a pair. Caller must check `sexp_pairp` first.
    pub sexp_car: unsafe extern "C" fn(*mut OpaqueVal) -> *mut OpaqueVal,

    /// Get cdr of a pair. Caller must check `sexp_pairp` first.
    pub sexp_cdr: unsafe extern "C" fn(*mut OpaqueVal) -> *mut OpaqueVal,

    // ── value constructors ───────────────────────────────────────────

    /// Create a fixnum (integer). Does not allocate.
    pub sexp_make_fixnum: unsafe extern "C" fn(c_long) -> *mut OpaqueVal,

    /// Create a flonum (f64). Allocates on the scheme heap.
    pub sexp_make_flonum: unsafe extern "C" fn(*mut OpaqueCtx, f64) -> *mut OpaqueVal,

    /// Create a boolean. Does not allocate.
    pub sexp_make_boolean: unsafe extern "C" fn(c_int) -> *mut OpaqueVal,

    /// Create a character. Does not allocate.
    pub sexp_make_character: unsafe extern "C" fn(c_int) -> *mut OpaqueVal,

    /// Create a scheme string from UTF-8 data. Allocates.
    /// `len` is byte length; pass -1 for null-terminated C strings.
    pub sexp_c_str: unsafe extern "C" fn(
        *mut OpaqueCtx, *const c_char, c_long,
    ) -> *mut OpaqueVal,

    /// Construct a pair (cons cell). Allocates.
    pub sexp_cons: unsafe extern "C" fn(
        *mut OpaqueCtx, *mut OpaqueVal, *mut OpaqueVal,
    ) -> *mut OpaqueVal,

    /// Create a bytevector of given length, filled with `init`. Allocates.
    pub sexp_make_bytes: unsafe extern "C" fn(
        *mut OpaqueCtx, c_long, u8,
    ) -> *mut OpaqueVal,

    // ── sentinels ────────────────────────────────────────────────────
    // These return the canonical singleton values. Do not allocate.

    pub get_null:  unsafe extern "C" fn() -> *mut OpaqueVal,
    pub get_true:  unsafe extern "C" fn() -> *mut OpaqueVal,
    pub get_false: unsafe extern "C" fn() -> *mut OpaqueVal,
    pub get_void:  unsafe extern "C" fn() -> *mut OpaqueVal,
}
```

Note on `sexp_make_boolean` and `sexp_make_character`: the tein wrappers take
`bool` and `c_int` respectively but the underlying chibi C functions take
`c_int`. In the API table we use `c_int` (matching the C ABI), and the macro
codegen handles conversion.

Note on `sexp_bytes_data`: the ffi wrapper returns `*mut c_char`, but the API
table uses `*const c_char` since extensions only read bytevector data.

**Step 4: Verify it compiles**

Run: `cargo build -p tein-ext`
Expected: clean build, no warnings.

**Step 5: Commit**

```
feat(tein-ext): C ABI type definitions for cdylib extensions (#62)

TeinExtApi vtable, opaque pointer types, TeinTypeDesc/TeinMethodDesc,
error codes, and function type aliases.
```

---

### Task 2: Add `libloading` dependency and `Context::load_extension`

**Files:**
- Modify: `tein/Cargo.toml` (add `libloading` dependency)
- Modify: `tein/src/lib.rs` (add `tein-ext` re-export)
- Modify: `tein/src/context.rs` (add `load_extension` method)
- Modify: `tein/src/ffi.rs` (add `tein_vfs_register` to pub API if needed)

**Step 1: Add dependencies**

In `tein/Cargo.toml`, add:
```toml
[dependencies]
tein-macros = { path = "../tein-macros" }
tein-ext = { path = "../tein-ext" }
libloading = "0.8"
```

**Step 2: Write the API table builder and trampolines**

In `tein/src/context.rs`, add a private function that builds a `TeinExtApi`
populated with trampolines into `ffi::*`. Most trampolines are simple pointer
casts since `sexp = *mut c_void` and `OpaqueVal`/`OpaqueCtx` are zero-sized
`#[repr(C)]` structs. The type predicates, extractors, constructors, and
sentinels are direct transmutes (same ABI signature — `*mut c_void` ↔
`*mut OpaqueVal`).

The three high-level operations need actual trampoline functions because
they bridge from `(*const c_char, usize)` pairs to rust `&str` and call
`Context` methods:

```rust
/// Build a `TeinExtApi` populated with function pointers into tein's FFI.
fn build_ext_api() -> tein_ext::TeinExtApi {
    tein_ext::TeinExtApi {
        version: tein_ext::TEIN_EXT_API_VERSION,

        // high-level registration trampolines
        register_vfs_module: ext_trampoline_register_vfs,
        define_fn_variadic:  ext_trampoline_define_fn,
        register_foreign_type: ext_trampoline_register_type,

        // type predicates — direct transmutes (same C ABI)
        sexp_integerp:   unsafe { std::mem::transmute(ffi::sexp_integerp as unsafe fn(ffi::sexp) -> c_int) },
        // ... same pattern for all predicates, extractors, constructors, sentinels
    }
}
```

For the transmutes: `sexp` is `*mut c_void`, `*mut OpaqueVal` is also a
pointer to a ZST. Both are pointer-sized, same ABI. `transmute` between
`fn(*mut c_void) -> c_int` and `fn(*mut OpaqueVal) -> c_int` is sound.

The three trampoline functions:

```rust
unsafe extern "C" fn ext_trampoline_register_vfs(
    ctx: *mut tein_ext::OpaqueCtx,
    path: *const c_char, path_len: usize,
    content: *const c_char, content_len: usize,
) -> i32 {
    unsafe {
        let path_str = std::str::from_utf8(
            std::slice::from_raw_parts(path as *const u8, path_len)
        );
        let content_str = std::str::from_utf8(
            std::slice::from_raw_parts(content as *const u8, content_len)
        );
        match (path_str, content_str) {
            (Ok(p), Ok(c)) => {
                let full_path = format!("/vfs/{p}");
                let c_path = match std::ffi::CString::new(full_path) {
                    Ok(s) => s,
                    Err(_) => return -1,
                };
                ffi::tein_vfs_register(
                    c_path.as_ptr(),
                    c as *const c_char,
                    c.len() as std::ffi::c_uint,
                );
                0
            }
            _ => -1,
        }
    }
}

unsafe extern "C" fn ext_trampoline_define_fn(
    ctx: *mut tein_ext::OpaqueCtx,
    name: *const c_char, name_len: usize,
    f: tein_ext::SexpFn,
) -> i32 {
    unsafe {
        let name_str = match std::str::from_utf8(
            std::slice::from_raw_parts(name as *const u8, name_len)
        ) {
            Ok(s) => s,
            Err(_) => return -1,
        };
        let c_name = match std::ffi::CString::new(name_str) {
            Ok(s) => s,
            Err(_) => return -1,
        };
        let ctx_sexp = ctx as ffi::sexp;
        let env = ffi::sexp_context_env(ctx_sexp);
        let f_transmuted: Option<unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp) -> ffi::sexp>
            = Some(std::mem::transmute(f));
        let result = ffi::sexp_define_foreign_proc(
            ctx_sexp, env, c_name.as_ptr(),
            0, ffi::SEXP_PROC_VARIADIC,
            c_name.as_ptr(), f_transmuted,
        );
        if ffi::sexp_exceptionp(result) != 0 { -1 } else { 0 }
    }
}

unsafe extern "C" fn ext_trampoline_register_type(
    ctx: *mut tein_ext::OpaqueCtx,
    desc: *const tein_ext::TeinTypeDesc,
) -> i32 {
    // This is the most complex trampoline. It needs to:
    // 1. Extract type_name from TeinTypeDesc
    // 2. Build a MethodFn wrapper for each TeinMethodDesc
    // 3. Register in ForeignStore
    // 4. Generate scheme convenience procs (predicate + method wrappers)
    //
    // The challenge: ForeignStore expects `impl ForeignType` (static trait),
    // but we have dynamic TeinTypeDesc data. We need a dynamic registration
    // path that stores the method table and type name without a concrete
    // rust type.
    //
    // Solution: add `ForeignStore::register_ext_type(name, methods)` that
    // takes a name and a Vec of (method_name, extern_fn_ptr, is_mut) and
    // creates an entry without a concrete rust type. The dispatch path
    // for ext types calls the TeinMethodFn directly instead of going
    // through &mut dyn Any.
    //
    // Implementation details in task 3.
    0
}
```

**Step 3: Implement `Context::load_extension`**

```rust
impl Context {
    /// Load a cdylib extension from the given path.
    ///
    /// The extension's `tein_ext_init` function is called immediately with
    /// a populated API table. VFS entries, functions, and types registered
    /// by the extension become available to scheme code.
    ///
    /// The shared library remains loaded for the process lifetime.
    pub fn load_extension(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let path = path.as_ref();
        unsafe {
            let lib = libloading::Library::new(path).map_err(|e| {
                Error::InitError(format!("failed to load extension '{}': {}", path.display(), e))
            })?;

            let init: libloading::Symbol<tein_ext::TeinExtInitFn> =
                lib.get(b"tein_ext_init").map_err(|e| {
                    Error::InitError(format!(
                        "extension '{}' has no tein_ext_init symbol: {}",
                        path.display(), e
                    ))
                })?;

            let api = build_ext_api();
            let result = init(self.ctx as *mut tein_ext::OpaqueCtx, &api);

            match result {
                tein_ext::TEIN_EXT_OK => {}
                tein_ext::TEIN_EXT_ERR_VERSION => {
                    return Err(Error::InitError(format!(
                        "extension '{}': API version mismatch (host v{}, extension requires newer)",
                        path.display(), tein_ext::TEIN_EXT_API_VERSION
                    )));
                }
                code => {
                    return Err(Error::InitError(format!(
                        "extension '{}': init failed with code {}",
                        path.display(), code
                    )));
                }
            }

            // leak the library handle — no unload
            Box::leak(Box::new(lib));
        }
        Ok(())
    }
}
```

**Step 4: Verify it compiles**

Run: `cargo build -p tein`
Expected: clean build. (No tests yet — we can't test without an extension to load.)

**Step 5: Commit**

```
feat(tein): Context::load_extension via libloading (#62)

Adds TeinExtApi builder with trampolines into tein's FFI, and
load_extension() that dlopen's a cdylib, resolves tein_ext_init,
and calls it with the populated API table.
```

---

### Task 3: Extend ForeignStore for dynamic ext-type registration

**Files:**
- Modify: `tein/src/foreign.rs` (add ext type support)
- Modify: `tein/src/context.rs` (wire up trampoline)

The existing `ForeignStore` relies on `impl ForeignType` (a static trait with
`type_name()` and `methods()`). Ext types provide this data dynamically via
`TeinTypeDesc`. We need a registration path that doesn't require a concrete
rust type.

**Step 1: Add `ExtMethodEntry` and ext-type storage to ForeignStore**

In `tein/src/foreign.rs`, add:

```rust
/// A method entry for dynamically registered ext types.
///
/// Unlike `MethodFn` (which takes `&mut dyn Any`), ext methods take
/// `*mut c_void` and the API table — they operate at the C ABI level.
pub(crate) struct ExtMethodEntry {
    pub name: String,
    pub func: tein_ext::TeinMethodFn,
    pub is_mut: bool,
}

/// Entry for a dynamically registered ext type.
struct ExtTypeEntry {
    methods: Vec<ExtMethodEntry>,
}
```

Add to `ForeignStore`:
```rust
pub(crate) struct ForeignStore {
    types: HashMap<&'static str, TypeEntry>,
    ext_types: HashMap<String, ExtTypeEntry>,   // NEW
    instances: HashMap<u64, ForeignObject>,
}
```

**Step 2: Add `register_ext_type` method**

```rust
impl ForeignStore {
    pub(crate) fn register_ext_type(
        &mut self,
        type_name: String,
        methods: Vec<ExtMethodEntry>,
    ) -> Result<(), String> {
        if self.types.contains_key(type_name.as_str()) || self.ext_types.contains_key(&type_name) {
            return Err(format!("type '{}' already registered", type_name));
        }
        self.ext_types.insert(type_name, ExtTypeEntry { methods });
        Ok(())
    }

    /// Find a method by name, checking both static and ext types.
    pub(crate) fn find_method_any(&self, type_name: &str, method_name: &str)
        -> Option<MethodLookup>
    {
        // check static types first
        if let Some(entry) = self.types.get(type_name) {
            for (name, func) in entry.methods {
                if *name == method_name {
                    return Some(MethodLookup::Static(*func));
                }
            }
        }
        // check ext types
        if let Some(entry) = self.ext_types.get(type_name) {
            for m in &entry.methods {
                if m.name == method_name {
                    return Some(MethodLookup::Ext {
                        func: m.func,
                        is_mut: m.is_mut,
                    });
                }
            }
        }
        None
    }
}

pub(crate) enum MethodLookup {
    Static(MethodFn),
    Ext { func: tein_ext::TeinMethodFn, is_mut: bool },
}
```

**Step 3: Update dispatch_foreign_call to handle ext methods**

Modify `dispatch_foreign_call` to use `find_method_any` and handle the
`Ext` variant by calling the `TeinMethodFn` with `*mut c_void` pointing
to the object's `Box<dyn Any>` data, plus the API table and raw sexp args.

For ext dispatch, we skip `Value` conversion for args — the ext method
receives raw sexp args and uses the API table to extract them.

**Step 4: Complete the `ext_trampoline_register_type` implementation**

In `context.rs`, fill in the trampoline body:
1. Extract type_name from `TeinTypeDesc`
2. Build `Vec<ExtMethodEntry>` from `TeinMethodDesc` array
3. Call `self.foreign_store.borrow_mut().register_ext_type(name, methods)`
4. Generate scheme convenience procs (predicate + wrappers) via evaluate

The convenience proc generation is the same scheme code as
`register_foreign_type<T>` but called with the dynamic type name.

**Step 5: Verify it compiles**

Run: `cargo build -p tein`
Expected: clean build.

**Step 6: Commit**

```
feat(tein): dynamic ext-type registration in ForeignStore (#62)

Adds register_ext_type() for types registered across the cdylib
boundary via TeinTypeDesc. Extends dispatch_foreign_call to handle
both static MethodFn and dynamic TeinMethodFn dispatch.
```

---

### Task 4: Extend `tein-macros` — parse `ext = true` flag

**Files:**
- Modify: `tein-macros/Cargo.toml` (add `tein-ext` dependency)
- Modify: `tein-macros/src/lib.rs` (parsing changes)

**Step 1: Add dependency**

In `tein-macros/Cargo.toml`:
```toml
[dependencies]
syn = { version = "2", features = ["full"] }
quote = "1"
proc-macro2 = "1"
tein-ext = { path = "../tein-ext" }
```

Wait — proc macro crates can't depend on regular crates and re-export them at
compile time in the same way. Actually they can — `tein-ext` is just used for
the generated code to reference `tein_ext::*` types. The proc macro itself
only needs to emit `tein_ext::...` tokens as strings in `quote!` blocks.

Actually, the proc macro doesn't need `tein-ext` as a dependency at all. It
just emits token streams that reference `tein_ext::...` — the extension crate
that uses the macro will have `tein-ext` in its dependencies, and name
resolution happens there. Same as how `tein-macros` emits `tein::raw::...`
but doesn't depend on `tein`.

So: **no Cargo.toml change needed for tein-macros.**

**Step 2: Add `ext` field to `ModuleInfo`**

```rust
struct ModuleInfo {
    name: String,
    ext: bool,  // NEW: whether this is a cdylib extension module
    mod_item: ItemMod,
    free_fns: Vec<FreeFnInfo>,
    consts: Vec<ConstInfo>,
    types: Vec<TypeInfo>,
}
```

**Step 3: Parse `ext = true` from attribute**

Currently (line 99-101):
```rust
pub fn tein_module(attr: TokenStream, item: TokenStream) -> TokenStream {
    let module_name = parse_macro_input!(attr as syn::LitStr).value();
    let mod_item = parse_macro_input!(item as ItemMod);
```

Change to support both `#[tein_module("name")]` and
`#[tein_module("name", ext = true)]`:

```rust
pub fn tein_module(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mod_item = parse_macro_input!(item as ItemMod);
    let (module_name, ext) = match parse_module_attr(attr.into()) {
        Ok(v) => v,
        Err(err) => return err.to_compile_error().into(),
    };
    match parse_and_generate_module(module_name, ext, mod_item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Parse the `#[tein_module(...)]` attribute arguments.
///
/// Accepts either `"name"` or `"name", ext = true`.
fn parse_module_attr(tokens: proc_macro2::TokenStream) -> syn::Result<(String, bool)> {
    // Parse as a punctuated list of expressions
    let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    let args = parser.parse2(tokens)?;

    let mut iter = args.iter();

    // first arg: string literal (module name)
    let name_expr = iter.next().ok_or_else(|| {
        syn::Error::new(Span::call_site(), "expected module name string")
    })?;
    let name = match name_expr {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => s.value(),
        _ => return Err(syn::Error::new_spanned(name_expr, "expected string literal for module name")),
    };

    // optional second arg: ext = true
    let mut ext = false;
    if let Some(ext_expr) = iter.next() {
        match ext_expr {
            syn::Expr::Assign(assign) => {
                let key = assign.left.as_ref();
                if let syn::Expr::Path(p) = key
                    && p.path.is_ident("ext")
                {
                    if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Bool(b), .. }) = assign.right.as_ref() {
                        ext = b.value();
                    } else {
                        return Err(syn::Error::new_spanned(&assign.right, "expected `true` or `false`"));
                    }
                } else {
                    return Err(syn::Error::new_spanned(key, "expected `ext`"));
                }
            }
            _ => return Err(syn::Error::new_spanned(ext_expr, "expected `ext = true`")),
        }
    }

    if iter.next().is_some() {
        return Err(syn::Error::new(Span::call_site(), "unexpected extra arguments"));
    }

    Ok((name, ext))
}
```

Thread `ext` through `parse_and_generate_module` → `parse_module_info` →
`ModuleInfo`.

**Step 4: Add unit test for attribute parsing**

```rust
#[test]
fn test_parse_module_attr_basic() {
    let tokens: proc_macro2::TokenStream = quote! { "testmod" };
    let (name, ext) = parse_module_attr(tokens).unwrap();
    assert_eq!(name, "testmod");
    assert!(!ext);
}

#[test]
fn test_parse_module_attr_ext() {
    let tokens: proc_macro2::TokenStream = quote! { "testmod", ext = true };
    let (name, ext) = parse_module_attr(tokens).unwrap();
    assert_eq!(name, "testmod");
    assert!(ext);
}
```

**Step 5: Verify tests pass**

Run: `cargo test -p tein-macros`
Expected: all existing tests pass + 2 new tests pass.

**Step 6: Commit**

```
feat(tein-macros): parse ext = true flag in #[tein_module] (#62)

Adds ModuleInfo.ext field and parse_module_attr() to handle both
#[tein_module("name")] and #[tein_module("name", ext = true)].
No codegen changes yet — ext flag is parsed and stored but not acted on.
```

---

### Task 5: Ext-mode codegen — `generate_ext_init_fn`

**Files:**
- Modify: `tein-macros/src/lib.rs`

This is the core macro change. When `ext = true`, `generate_module` emits
different code.

**Step 1: Branch in `generate_module`**

```rust
fn generate_module(info: ModuleInfo) -> syn::Result<proc_macro2::TokenStream> {
    if info.ext {
        generate_module_ext(info)
    } else {
        generate_module_internal(info)
    }
}
```

Rename the existing `generate_module` body to `generate_module_internal`.

**Step 2: Write `generate_module_ext`**

```rust
fn generate_module_ext(info: ModuleInfo) -> syn::Result<proc_macro2::TokenStream> {
    let mod_name = &info.mod_item.ident;
    let mod_vis = &info.mod_item.vis;
    let mod_attrs = &info.mod_item.attrs;
    let (_, items) = info.mod_item.content.as_ref().unwrap();

    // generate extern "C" wrappers for free functions (ext-mode variants)
    let fn_wrappers: Vec<proc_macro2::TokenStream> = info
        .free_fns
        .iter()
        .map(|f| generate_scheme_fn_ext(f.func.clone()))
        .collect::<syn::Result<_>>()?;

    // generate foreign type descriptors (not ForeignType trait impls)
    let type_descs: Vec<proc_macro2::TokenStream> = info
        .types
        .iter()
        .map(generate_ext_type_desc)
        .collect::<syn::Result<_>>()?;

    let init_fn = generate_ext_init_fn(&info);

    // thread-local for API pointer
    let api_tls = quote! {
        std::thread_local! {
            static __TEIN_API: std::cell::Cell<*const tein_ext::TeinExtApi> =
                const { std::cell::Cell::new(std::ptr::null()) };
        }
    };

    Ok(quote! {
        #(#mod_attrs)*
        #mod_vis mod #mod_name {
            #(#items)*
            #api_tls
            #(#type_descs)*
            #(#fn_wrappers)*
            #init_fn
        }
    })
}
```

**Step 3: Write `generate_ext_init_fn`**

```rust
fn generate_ext_init_fn(info: &ModuleInfo) -> proc_macro2::TokenStream {
    let sld_content = generate_vfs_sld(info);
    let scm_content = generate_vfs_scm(info);
    let sld_path = format!("lib/tein/{}.sld", info.name);
    let scm_path = format!("lib/tein/{}.scm", info.name);
    let docs_sld_content = generate_vfs_docs_sld(info);
    let docs_scm_content = generate_vfs_docs_scm(info);
    let docs_sld_path = format!("lib/tein/{}/docs.sld", info.name);
    let docs_scm_path = format!("lib/tein/{}/docs.scm", info.name);

    // VFS registration calls
    let vfs_entries = [
        (&sld_path, &sld_content),
        (&scm_path, &scm_content),
        (&docs_sld_path, &docs_sld_content),
        (&docs_scm_path, &docs_scm_content),
    ];
    let vfs_registrations: Vec<proc_macro2::TokenStream> = vfs_entries
        .iter()
        .map(|(path, content)| {
            let path_bytes = path.as_bytes();
            let path_len = path_bytes.len();
            let content_bytes = content.as_bytes();
            let content_len = content_bytes.len();
            quote! {
                {
                    static PATH: &[u8] = #path.as_bytes();
                    static CONTENT: &[u8] = #content.as_bytes();
                    let rc = ((*api).register_vfs_module)(
                        ctx,
                        PATH.as_ptr() as *const ::std::ffi::c_char, PATH.len(),
                        CONTENT.as_ptr() as *const ::std::ffi::c_char, CONTENT.len(),
                    );
                    if rc != 0 { return tein_ext::TEIN_EXT_ERR_INIT; }
                }
            }
        })
        .collect();

    // function registration
    let fn_registrations: Vec<proc_macro2::TokenStream> = info
        .free_fns
        .iter()
        .map(|f| {
            let wrapper_ident = syn::Ident::new(
                &format!("__tein_{}", f.func.sig.ident),
                f.func.sig.ident.span(),
            );
            let scheme_name = &f.scheme_name;
            quote! {
                {
                    static NAME: &[u8] = #scheme_name.as_bytes();
                    let rc = ((*api).define_fn_variadic)(
                        ctx,
                        NAME.as_ptr() as *const ::std::ffi::c_char, NAME.len(),
                        #wrapper_ident,
                    );
                    if rc != 0 { return tein_ext::TEIN_EXT_ERR_INIT; }
                }
            }
        })
        .collect();

    // type registrations (via TeinTypeDesc statics generated by generate_ext_type_desc)
    let type_registrations: Vec<proc_macro2::TokenStream> = info
        .types
        .iter()
        .map(|t| {
            let desc_ident = syn::Ident::new(
                &format!("__TEIN_TYPE_DESC_{}", t.struct_item.ident.to_string().to_uppercase()),
                t.struct_item.ident.span(),
            );
            quote! {
                {
                    let rc = ((*api).register_foreign_type)(ctx, &#desc_ident);
                    if rc != 0 { return tein_ext::TEIN_EXT_ERR_INIT; }
                }
            }
        })
        .collect();

    quote! {
        /// Extension entry point — called by the tein host at load time.
        ///
        /// # Safety
        ///
        /// Must be called with a valid context pointer and API table.
        /// Only called by `Context::load_extension`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn tein_ext_init(
            ctx: *mut tein_ext::OpaqueCtx,
            api: *const tein_ext::TeinExtApi,
        ) -> i32 {
            unsafe {
                // version check
                if (*api).version < tein_ext::TEIN_EXT_API_VERSION {
                    return tein_ext::TEIN_EXT_ERR_VERSION;
                }

                // store API pointer for wrappers to use
                __TEIN_API.with(|cell| cell.set(api));

                // register VFS entries
                #(#vfs_registrations)*

                // register foreign types
                #(#type_registrations)*

                // register free functions
                #(#fn_registrations)*

                tein_ext::TEIN_EXT_OK
            }
        }
    }
}
```

**Step 4: Verify it compiles (macro crate)**

Run: `cargo build -p tein-macros`
Expected: clean build. The generated `tein_ext_init` references `tein_ext::`
types that will be resolved when an extension crate uses the macro.

**Step 5: Commit**

```
feat(tein-macros): ext-mode init function codegen (#62)

generate_ext_init_fn emits tein_ext_init extern "C" entry point with
VFS registration, function registration, and type registration through
the TeinExtApi vtable.
```

---

### Task 6: Ext-mode codegen — API-routed wrapper functions

**Files:**
- Modify: `tein-macros/src/lib.rs`

**Step 1: Write `generate_scheme_fn_ext`**

Same structure as `generate_scheme_fn` but arg extraction and return conversion
route through the API table instead of `tein::raw::*`.

```rust
fn generate_scheme_fn_ext(input: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    // same arg/return parsing as generate_scheme_fn...
    // but calls gen_arg_extraction_ext / gen_return_conversion_ext
    // wrapper body starts with:
    //   let __tein_api = __TEIN_API.with(|cell| cell.get());
    // then uses ((*__tein_api).sexp_integerp)(raw) etc.
}
```

**Step 2: Write `gen_arg_extraction_ext` and `gen_return_conversion_ext`**

These mirror `gen_arg_extraction` / `gen_return_conversion` but replace every
`tein::raw::foo(x)` with `((*__tein_api).foo)(x as *mut tein_ext::OpaqueVal)`.

For example, the i64 extraction becomes:
```rust
"i64" => quote! {
    let #arg_name: i64 = {
        let raw = ((*__tein_api).sexp_car)(__tein_current_args);
        if ((*__tein_api).sexp_integerp)(raw) == 0 {
            let msg = #err_msg;
            let c_msg = ::std::ffi::CString::new(msg).unwrap();
            return ((*__tein_api).sexp_c_str)(
                ctx, c_msg.as_ptr(), c_msg.as_bytes().len() as ::std::ffi::c_long,
            );
        }
        ((*__tein_api).sexp_unbox_fixnum)(raw) as i64
    };
    __tein_current_args = ((*__tein_api).sexp_cdr)(__tein_current_args);
},
```

To avoid code duplication, consider a helper approach: factor the common
structure into a shared function that takes a "call style" enum, or generate
the token stream with a closure that wraps function calls. However, since the
token streams are quite different (qualified paths vs pointer dereferences),
the cleanest approach is separate `_ext` variants that mirror the originals.

**Step 3: Write `generate_ext_type_desc`**

For each `TypeInfo`, generate:
- `extern "C"` method wrapper functions (like `TeinMethodFn`)
- static `TeinMethodDesc` array
- static `TeinTypeDesc`

```rust
fn generate_ext_type_desc(type_info: &TypeInfo) -> syn::Result<proc_macro2::TokenStream> {
    let struct_name = &type_info.struct_item.ident;
    let scheme_type_name = &type_info.scheme_type_name;
    let desc_ident = syn::Ident::new(
        &format!("__TEIN_TYPE_DESC_{}", struct_name.to_string().to_uppercase()),
        struct_name.span(),
    );
    let methods_ident = syn::Ident::new(
        &format!("__TEIN_METHODS_{}", struct_name.to_string().to_uppercase()),
        struct_name.span(),
    );

    let method_wrappers: Vec<proc_macro2::TokenStream> = type_info
        .methods
        .iter()
        .map(|m| generate_ext_method_wrapper(m, struct_name, scheme_type_name))
        .collect::<syn::Result<_>>()?;

    let method_descs: Vec<proc_macro2::TokenStream> = type_info
        .methods
        .iter()
        .map(|m| {
            let method_ident = &m.method.sig.ident;
            let wrapper_ident = syn::Ident::new(
                &format!("__tein_ext_method_{}_{}", struct_name, method_ident),
                method_ident.span(),
            );
            let scheme_name = &m.scheme_name;
            let is_mut = m.is_mut;
            quote! {
                tein_ext::TeinMethodDesc {
                    name: #scheme_name.as_ptr() as *const ::std::ffi::c_char,
                    name_len: #scheme_name.len(),
                    func: #wrapper_ident,
                    is_mut: #is_mut,
                }
            }
        })
        .collect();

    let method_count = method_descs.len();

    Ok(quote! {
        #(#method_wrappers)*

        static #methods_ident: [tein_ext::TeinMethodDesc; #method_count] = [
            #(#method_descs),*
        ];

        static #desc_ident: tein_ext::TeinTypeDesc = tein_ext::TeinTypeDesc {
            type_name: #scheme_type_name.as_ptr() as *const ::std::ffi::c_char,
            type_name_len: #scheme_type_name.len(),
            methods: #methods_ident.as_ptr(),
            method_count: #method_count,
        };
    })
}
```

**Step 4: Write `generate_ext_method_wrapper`**

Each method becomes an `extern "C"` function matching `TeinMethodFn`:

```rust
fn generate_ext_method_wrapper(
    m: &MethodInfo,
    struct_name: &syn::Ident,
    scheme_type_name: &str,
) -> syn::Result<proc_macro2::TokenStream> {
    let method_ident = &m.method.sig.ident;
    let wrapper_ident = syn::Ident::new(
        &format!("__tein_ext_method_{}_{}", struct_name, method_ident),
        method_ident.span(),
    );

    // arg extraction from sexp list using api table
    // similar to gen_arg_extraction_ext but for method args
    // (skip first two sexp args — obj and method name are already extracted
    //  by the dispatch path)

    let cast = if m.is_mut {
        quote! { let __tein_this = &mut *(obj as *mut #struct_name); }
    } else {
        quote! { let __tein_this = &*(obj as *const #struct_name); }
    };

    // ... arg extraction, call, return conversion using api table ...

    Ok(quote! {
        unsafe extern "C" fn #wrapper_ident(
            obj: *mut ::std::ffi::c_void,
            ctx: *mut tein_ext::OpaqueCtx,
            api: *const tein_ext::TeinExtApi,
            _n: ::std::ffi::c_long,
            args: *mut tein_ext::OpaqueVal,
        ) -> *mut tein_ext::OpaqueVal {
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                unsafe {
                    #cast
                    let mut __tein_current_args = args;
                    // ... extractions and call ...
                }
            }));
            match result {
                Ok(val) => val,
                Err(_) => unsafe {
                    let msg = concat!("rust panic in method ", stringify!(#method_ident));
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    ((*api).sexp_c_str)(
                        ctx, c_msg.as_ptr(), msg.len() as ::std::ffi::c_long,
                    )
                }
            }
        }
    })
}
```

**Step 5: Verify it compiles**

Run: `cargo build -p tein-macros`
Expected: clean build.

**Step 6: Commit**

```
feat(tein-macros): API-routed wrapper codegen for ext mode (#62)

generate_scheme_fn_ext emits extern "C" wrappers that call through
the TeinExtApi vtable. generate_ext_type_desc emits static TeinTypeDesc
arrays with extern "C" method wrappers.
```

---

### Task 7: Create in-tree test extension cdylib

**Files:**
- Create: `tein-test-ext/Cargo.toml`
- Create: `tein-test-ext/src/lib.rs`
- Modify: `Cargo.toml` (add workspace member)

**Step 1: Add workspace member**

In root `Cargo.toml`:
```toml
members = ["tein", "tein-macros", "tein-sexp", "tein-ext", "tein-test-ext"]
```

**Step 2: Create `tein-test-ext/Cargo.toml`**

```toml
[package]
name = "tein-test-ext"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
tein-ext = { path = "../tein-ext" }
tein-macros = { path = "../tein-macros" }
```

**Step 3: Create `tein-test-ext/src/lib.rs`**

```rust
//! Test extension for verifying the cdylib extension system.
//!
//! This crate compiles to a `.so` / `.dylib` and is loaded at test time
//! by `tein/tests/ext_loading.rs`.

use tein_macros::{tein_const, tein_fn, tein_methods, tein_module, tein_type};

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

    /// fallible function
    #[tein_fn]
    pub fn safe_div(a: i64, b: i64) -> Result<i64, String> {
        if b == 0 {
            Err("division by zero".to_string())
        } else {
            Ok(a / b)
        }
    }

    /// a test constant
    #[tein_const]
    pub const GREETING: &str = "hello from testext";

    /// the answer
    #[tein_const]
    pub const ANSWER: i64 = 42;

    /// a counter type
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

        /// increment by one, return new value
        pub fn increment(&mut self) -> i64 {
            self.n += 1;
            self.n
        }

        /// add a value
        pub fn add(&mut self, amount: i64) -> i64 {
            self.n += amount;
            self.n
        }
    }
}
```

**Step 4: Build the test extension**

Run: `cargo build -p tein-test-ext`
Expected: produces `target/debug/libtein_test_ext.so` (linux) or
`libtein_test_ext.dylib` (mac).

**Step 5: Commit**

```
feat(tein-test-ext): in-tree test cdylib extension (#62)

Minimal extension with free functions (all arg/return types), constants,
and a foreign type — used by integration tests.
```

---

### Task 8: Integration tests

**Files:**
- Create: `tein/tests/ext_loading.rs`
- Create: `tein/tests/scheme/ext_module.scm`

**Step 1: Write the integration test**

```rust
//! Integration tests for the cdylib extension system.
//!
//! These tests load `tein-test-ext` as a shared library and exercise
//! the full extension lifecycle: loading, VFS registration, function
//! calls, foreign types, and documentation.

use tein::{Context, Value};

/// find the test extension library path.
fn ext_lib_path() -> std::path::PathBuf {
    // cargo puts cdylib output in target/{profile}/
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // up from tein/ to workspace root
    path.push("target");
    path.push(if cfg!(debug_assertions) { "debug" } else { "release" });

    #[cfg(target_os = "linux")]
    path.push("libtein_test_ext.so");
    #[cfg(target_os = "macos")]
    path.push("libtein_test_ext.dylib");
    #[cfg(target_os = "windows")]
    path.push("tein_test_ext.dll");

    path
}

#[test]
fn test_ext_load_and_import() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load extension");
    ctx.evaluate("(import (tein testext))").expect("import");
}

#[test]
fn test_ext_free_fn_integer() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx.evaluate("(testext-add 20 22)").expect("eval");
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn test_ext_free_fn_float() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx.evaluate("(testext-multiply 2.5 4.0)").expect("eval");
    assert_eq!(result, Value::Float(10.0));
}

#[test]
fn test_ext_free_fn_string() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx.evaluate("(testext-greet \"world\")").expect("eval");
    assert_eq!(result, Value::String("hello, world!".to_string()));
}

#[test]
fn test_ext_free_fn_bool() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    assert_eq!(ctx.evaluate("(testext-positive? 5)").expect("eval"), Value::Boolean(true));
    assert_eq!(ctx.evaluate("(testext-positive? -3)").expect("eval"), Value::Boolean(false));
}

#[test]
fn test_ext_free_fn_void() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx.evaluate("(testext-noop)").expect("eval");
    assert_eq!(result, Value::Unspecified);
}

#[test]
fn test_ext_free_fn_result_ok() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx.evaluate("(testext-safe-div 10 2)").expect("eval");
    assert_eq!(result, Value::Integer(5));
}

#[test]
fn test_ext_constants() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    assert_eq!(
        ctx.evaluate("testext-greeting").expect("eval"),
        Value::String("hello from testext".to_string())
    );
    assert_eq!(
        ctx.evaluate("testext-answer").expect("eval"),
        Value::Integer(42)
    );
}

#[test]
fn test_ext_load_nonexistent() {
    let ctx = Context::new_standard().expect("context");
    let result = ctx.load_extension("/nonexistent/path.so");
    assert!(result.is_err());
}

#[test]
fn test_ext_docs_sublibrary() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    // import the docs module for the hand-written (tein docs) if available,
    // otherwise just verify the docs sub-library imports
    ctx.evaluate("(import (tein testext))").expect("import testext");
    ctx.evaluate("(import (tein testext docs))").expect("import docs");
    // the docs alist should be defined
    let result = ctx.evaluate("testext-docs").expect("eval docs");
    match result {
        Value::List(_) => {} // alist is a list of pairs
        other => panic!("expected list for docs alist, got {:?}", other),
    }
}
```

**Step 2: Write the scheme-level test**

`tein/tests/scheme/ext_module.scm`:
```scheme
(import (tein testext))
(import (tein test))

;; free functions
(test-equal 42 (testext-add 20 22))
(test-equal 10.0 (testext-multiply 2.5 4.0))
(test-equal "hello, world!" (testext-greet "world"))
(test-true (testext-positive? 5))
(test-false (testext-positive? -3))
(test-equal 5 (testext-safe-div 10 2))

;; constants
(test-equal "hello from testext" testext-greeting)
(test-equal 42 testext-answer)
```

Add to `scheme_tests.rs` a test function that loads the extension first:
```rust
#[test]
fn test_scheme_ext_module() {
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein test))").expect("import test");
    let source = include_str!("scheme/ext_module.scm");
    ctx.evaluate(source).expect("scheme ext module test");
}
```

**Step 3: Build the extension, then run tests**

Run: `cargo build -p tein-test-ext && cargo test -p tein`
Expected: all tests pass including the new ext_loading tests.

**Step 4: Commit**

```
test: integration tests for cdylib extension system (#62)

Rust-level and scheme-level tests verifying extension loading, function
calls (all arg/return types), constants, foreign types, and docs
sub-library.
```

---

### Task 9: Documentation and cleanup

**Files:**
- Modify: `tein/src/lib.rs` (re-export `load_extension` in public API docs)
- Modify: `tein/src/context.rs` (ensure docstrings are complete)
- Modify: `tein-ext/src/lib.rs` (ensure all items documented)
- Modify: `AGENTS.md` (update architecture section)

**Step 1: Update `lib.rs` re-exports**

Add `tein_ext` to public re-exports if needed for extension authors.
Actually, extension crates import `tein_ext` directly — tein doesn't
need to re-export it. But add a doc comment pointing to it.

**Step 2: Update AGENTS.md architecture**

Add the extension system to the architecture section:
- `tein-ext/` crate description
- `load_extension` in data flow
- extension crate dependency graph
- `tein-test-ext/` purpose

Add to commands section:
```bash
cargo build -p tein-test-ext        # build test extension cdylib
cargo test -p tein -- ext           # run extension tests
```

Update test count.

**Step 3: Verify everything is clean**

Run: `cargo clippy --workspace && cargo fmt --check && cargo test --workspace`
Expected: no warnings, properly formatted, all tests pass.

**Step 4: Commit**

```
docs: update architecture for cdylib extension system (#62)

Updates AGENTS.md with extension system architecture, tein-ext crate,
and new test commands.
```

---

### Task 10: Final verification and PR

**Step 1: Run the full test suite**

Run: `cargo build -p tein-test-ext && cargo test --workspace`
Expected: all tests pass.

**Step 2: Run clippy and fmt**

Run: `cargo clippy --workspace && cargo fmt --check`
Expected: clean.

**Step 3: Create PR**

PR title: `feat: tein extension system — hot-loadable cdylib modules via stable C ABI (#62)`

Body should summarize:
- New `tein-ext` crate with C ABI types
- `Context::load_extension()` with libloading
- `#[tein_module("name", ext = true)]` macro codegen
- In-tree test extension with integration tests
- Extension authoring guide in design doc
