# design: cdylib extension system (#62)

## summary

add hot-loadable cdylib module support to tein. extension crates compile to
`.so`/`.dylib` shared libraries, loaded at runtime via `Context::load_extension()`.
a stable C ABI (`TeinExtApi` vtable) decouples extensions from tein's internal
types — no rust ABI fragility, no chibi dependency in extension crates.

the `#[tein_module]` macro gains `ext = true`: same module syntax, different
codegen target. extension authors write identical rust code; only the crate-type
and one flag differ.

## architecture

three components:

1. **`tein-ext`** (new crate) — pure type definitions. `TeinExtApi` vtable,
   opaque pointer types, `TeinTypeDesc`, error codes. no dependencies on tein
   or chibi. this is what cdylib extension crates depend on.

2. **`tein`** — `Context::load_extension(path)` uses `libloading` to dlopen the
   `.so`, resolves `tein_ext_init`, calls it with a populated `TeinExtApi`.
   `Library` handle leaked (no unload). new dependency: `libloading`.

3. **`tein-macros`** — `#[tein_module("name", ext = true)]` generates
   `tein_ext_init` instead of `register_module_*`. wrapper codegen routes
   through api function pointers instead of direct `tein::raw::*` calls.

## `tein-ext` crate

zero dependencies. just C ABI type definitions.

### opaque pointers

```rust
#[repr(C)] pub struct OpaqueCtx { _private: [u8; 0] }
#[repr(C)] pub struct OpaqueVal { _private: [u8; 0] }
```

never dereferenced by extension code. zero-sized repr(C) prevents accidental
construction.

### `TeinExtApi`

```rust
#[repr(C)]
pub struct TeinExtApi {
    pub version: u32,

    // ── high-level registration ──
    pub register_vfs_module:   unsafe extern "C" fn(*mut OpaqueCtx, *const c_char, usize, *const c_char, usize) -> i32,
    pub define_fn_variadic:    unsafe extern "C" fn(*mut OpaqueCtx, *const c_char, usize, SexpFn) -> i32,
    pub register_foreign_type: unsafe extern "C" fn(*mut OpaqueCtx, *const TeinTypeDesc) -> i32,

    // ── type predicates ──
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

    // ── value extractors ──
    pub sexp_unbox_fixnum:    unsafe extern "C" fn(*mut OpaqueVal) -> c_long,
    pub sexp_flonum_value:    unsafe extern "C" fn(*mut OpaqueVal) -> f64,
    pub sexp_string_data:     unsafe extern "C" fn(*mut OpaqueVal) -> *const c_char,
    pub sexp_string_size:     unsafe extern "C" fn(*mut OpaqueVal) -> c_long,
    pub sexp_unbox_character: unsafe extern "C" fn(*mut OpaqueVal) -> c_long,
    pub sexp_bytes_data:      unsafe extern "C" fn(*mut OpaqueVal) -> *const u8,
    pub sexp_bytes_length:    unsafe extern "C" fn(*mut OpaqueVal) -> c_long,
    pub sexp_car:             unsafe extern "C" fn(*mut OpaqueVal) -> *mut OpaqueVal,
    pub sexp_cdr:             unsafe extern "C" fn(*mut OpaqueVal) -> *mut OpaqueVal,

    // ── value constructors ──
    pub sexp_make_fixnum:    unsafe extern "C" fn(c_long) -> *mut OpaqueVal,
    pub sexp_make_flonum:    unsafe extern "C" fn(*mut OpaqueCtx, f64) -> *mut OpaqueVal,
    pub sexp_make_boolean:   unsafe extern "C" fn(bool) -> *mut OpaqueVal,
    pub sexp_make_character: unsafe extern "C" fn(c_long) -> *mut OpaqueVal,
    pub sexp_c_str:          unsafe extern "C" fn(*mut OpaqueCtx, *const c_char, c_long) -> *mut OpaqueVal,
    pub sexp_cons:           unsafe extern "C" fn(*mut OpaqueCtx, *mut OpaqueVal, *mut OpaqueVal) -> *mut OpaqueVal,
    pub sexp_make_bytes:     unsafe extern "C" fn(*mut OpaqueCtx, *const u8, c_long) -> *mut OpaqueVal,

    // ── sentinels ──
    pub get_null:  unsafe extern "C" fn() -> *mut OpaqueVal,
    pub get_true:  unsafe extern "C" fn() -> *mut OpaqueVal,
    pub get_false: unsafe extern "C" fn() -> *mut OpaqueVal,
    pub get_void:  unsafe extern "C" fn() -> *mut OpaqueVal,
}
```

versioned, append-only. new fields go at the end. version check at init time.

### function types

```rust
/// variadic scheme function — same ABI as chibi's native fns.
/// ctx/self/args are all sexp (OpaqueVal) in chibi's type system.
pub type SexpFn = unsafe extern "C" fn(
    ctx: *mut OpaqueVal, self_: *mut OpaqueVal, n: c_long, args: *mut OpaqueVal,
) -> *mut OpaqueVal;

/// method function — receives opaque self + raw args + api table.
pub type TeinMethodFn = unsafe extern "C" fn(
    obj: *mut c_void, ctx: *mut OpaqueCtx, api: *const TeinExtApi,
    n: c_long, args: *mut OpaqueVal,
) -> *mut OpaqueVal;

/// extension init entry point.
pub type TeinExtInitFn = unsafe extern "C" fn(
    ctx: *mut OpaqueCtx, api: *const TeinExtApi,
) -> i32;
```

### foreign type descriptor

```rust
#[repr(C)]
pub struct TeinTypeDesc {
    pub type_name: *const c_char,
    pub type_name_len: usize,
    pub methods: *const TeinMethodDesc,
    pub method_count: usize,
}

#[repr(C)]
pub struct TeinMethodDesc {
    pub name: *const c_char,
    pub name_len: usize,
    pub func: TeinMethodFn,
    pub is_mut: bool,
}
```

emitted as `static` arrays in the cdylib by the macro.

### error codes

```rust
pub const TEIN_EXT_OK: i32 = 0;
pub const TEIN_EXT_ERR_VERSION: i32 = -1;
pub const TEIN_EXT_ERR_INIT: i32 = -2;
pub const TEIN_EXT_API_VERSION: u32 = 1;
```

## `tein` crate changes

### api table population

a function builds `TeinExtApi` with function pointers into tein's ffi module.
since `sexp = *mut c_void` and our opaque types are zero-sized repr(C), the
casts are pointer casts with no runtime cost. most entries are direct
transmutes (identical ABI). the three high-level operations
(`register_vfs_module`, `define_fn_variadic`, `register_foreign_type`) are thin
trampolines that bridge C strings to rust `&str` and call `Context` methods.

### `Context::load_extension`

```rust
impl Context {
    /// load a cdylib extension from the given path.
    ///
    /// the extension's `tein_ext_init` function is called immediately with
    /// a populated API table. VFS entries, functions, and types registered
    /// by the extension become available after loading.
    ///
    /// the shared library remains loaded for the process lifetime.
    pub fn load_extension(&self, path: impl AsRef<Path>) -> Result<()>
}
```

internally:

1. `libloading::Library::new(path)` — dlopen
2. resolve symbol `tein_ext_init` as `TeinExtInitFn`
3. build `TeinExtApi` with trampolines pointing into tein's ffi
4. call `tein_ext_init(ctx_ptr, &api)`
5. check return code (0 = ok, negative = error)
6. `Box::leak(Box::new(library))` — no unload

called on `&Context` (post-build), matching `register_module_*` pattern.

```rust
let ctx = ContextBuilder::new().standard_env().build()?;
ctx.load_extension("/path/to/libtein_uuid.so")?;
ctx.evaluate("(import (tein uuid))")?;
```

## `tein-macros` changes

### `ext = true` flag

`#[tein_module("name", ext = true)]` parsed in `parse_module_info()`.
`ModuleInfo` gains `ext: bool` field.

### `generate_ext_init_fn`

when `ext = true`, `generate_module()` calls `generate_ext_init_fn()` instead
of `generate_register_fn()`. emits:

```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tein_ext_init(
    ctx: *mut tein_ext::OpaqueCtx,
    api: *const tein_ext::TeinExtApi,
) -> i32 { ... }
```

body: version check, VFS registration (4 entries), foreign type registration
via `TeinTypeDesc`, free function registration via `define_fn_variadic`. VFS
content strings emitted as `static` byte arrays.

### api-routed wrapper codegen

a thread-local stores the api pointer, set once in `tein_ext_init`:

```rust
std::thread_local! {
    static TEIN_API: std::cell::Cell<*const tein_ext::TeinExtApi> =
        const { std::cell::Cell::new(std::ptr::null()) };
}
```

`gen_arg_extraction` and `gen_return_conversion` gain ext-mode variants that
emit `((*__tein_api).sexp_integerp)(raw)` instead of
`tein::raw::sexp_integerp(raw)`. the wrapper signature is unchanged —
`extern "C" fn(sexp, sexp, sexp_sint_t, sexp) -> sexp` — since chibi calls
these directly.

### foreign type in ext mode

- `generate_foreign_type_impl` is not emitted (no `impl ForeignType for T`)
- instead, a `static TeinTypeDesc` + `static [TeinMethodDesc; N]` are emitted
- method bodies become `extern "C"` functions matching `TeinMethodFn`
- arg extraction routes through api table
- downcast via `*mut c_void` → `&mut T`

### unchanged codegen

- `parse_module_info()` — same data model (+ `ext` field)
- VFS content generation — `generate_vfs_sld`, `generate_vfs_scm`,
  `generate_vfs_docs_sld`, `generate_vfs_docs_scm` — identical
- naming helpers, doc extraction — unchanged

### extension crate dependency graph

```
tein-uuid (cdylib)
  ├─ tein-ext   (types only, no chibi)
  ├─ tein-macros (proc macro)
  └─ uuid       (actual functionality)
```

extension crates do **not** depend on `tein`. the macro generates code
referencing only `tein_ext::*` types in ext mode.

## error handling and safety

### error propagation

- `tein_ext_init` returns `i32` error codes
- runtime errors in `#[tein_fn]` wrappers: same as current codegen — return
  scheme error string via `((*api).sexp_c_str)(...)`
- panics caught at `extern "C"` boundary with `catch_unwind`

### safety invariants

- **thread-local api pointer** — set once, never mutated, valid for process
  lifetime (no unload)
- **no `Library` unload** — function pointers remain valid forever
- **version check** — mismatched versions return `TEIN_EXT_ERR_VERSION`
  immediately, before dereferencing any function pointers
- **opaque pointers** — zero-sized repr(C) types prevent construction by
  extension code
- **foreign object lifetime** — objects live in host's `ForeignStore`, cdylib
  only provides method implementations

## testing

### in-tree test cdylib

workspace member `tein-test-ext/`:

```
tein-test-ext/
  Cargo.toml    # [lib] crate-type = ["cdylib"]
  src/lib.rs    # #[tein_module("testext", ext = true)]
```

module with free functions (all supported arg/return types), a foreign type
with methods, and constants with docs.

### integration tests

in `tein/tests/`:

1. build test cdylib (cargo invocation in test setup or build script)
2. `Context::new_standard()` + `ctx.load_extension("path/to/libtein_test_ext.so")`
3. `(import (tein testext))` + exercise free fns, foreign types, docs
4. version mismatch test (tampered version → `TEIN_EXT_ERR_VERSION`)

### scheme-level tests

```scheme
(import (tein testext))
(import (tein test))
(test-equal 42 (testext-add 20 22))
(test-true (testext-is-loaded))
```

## extension authoring guide

to be shipped as documentation (extracted from this design).

### crate setup

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
tein-ext = "0.1"
tein-macros = "0.1"
```

### module definition

```rust
use tein_macros::{tein_module, tein_fn, tein_type, tein_methods, tein_const};

#[tein_module("mymod", ext = true)]
mod mymod_impl {
    /// add two integers
    #[tein_fn]
    fn add(a: i64, b: i64) -> i64 { a + b }

    /// a greeting constant
    #[tein_const]
    const GREETING: &str = "hello from mymod";

    /// a counter type
    #[tein_type]
    struct Counter { val: i64 }

    #[tein_methods]
    impl Counter {
        /// get the current value
        fn get(&self) -> i64 { self.val }
        /// increment by one
        fn increment(&mut self) { self.val += 1; }
    }
}
```

identical to compiled-in modules except `ext = true` and `crate-type`.

### building and loading

```bash
cargo build --release  # produces target/release/libmymod.so
```

```rust
let ctx = ContextBuilder::new().standard_env().build()?;
ctx.load_extension("target/release/libmymod.so")?;
ctx.evaluate("(import (tein mymod))")?;
```

### supported types

- arguments: `i64`, `f64`, `String`, `bool`
- returns: `i64`, `f64`, `String`, `bool`, `()`, `Result<T>`
- method args additionally: `Value`

### versioning

`tein-ext` version must match the host tein version. mismatch is detected at
load time via `TeinExtApi::version` and returns `Error::InitError`.

## out of scope

- unloading extensions (no unload, process-lifetime)
- cross-platform `.dylib`/`.dll` testing (#66)
- name-based search path (`TEIN_EXT_PATH`) — deferred until standalone REPL
- scheme-level `(load-extension ...)` — host-only for now
