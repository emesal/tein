# cdylib extensions

load compiled Rust code into a tein context at runtime via a stable C ABI.

## when to use extensions vs. inline #[tein_module]

| | inline `#[tein_module]` | cdylib extension |
|--|----------------------|-----------------|
| compiled into host binary | yes | no |
| distributable separately | no | yes |
| can use private crates | yes | no |
| stable ABI | n/a | yes |
| unloadable | n/a | no |

use extensions when you want to ship Scheme modules as standalone `.so` files —
plugins, optional capabilities, or code that updates independently of the host binary.

## extension crate structure

Extension crates depend on `tein-ext` and `tein-macros` — never on `tein` itself.
This keeps the dependency footprint minimal and avoids ABI coupling.

```toml
[dependencies]
tein-ext = { git = "https://github.com/emesal/tein" }
tein-macros = { git = "https://github.com/emesal/tein" }

[lib]
crate-type = ["cdylib"]
```

## writing an extension

`#[tein_module("name", ext = true)]` generates everything needed:

```rust
use tein_macros::{tein_module, tein_fn, tein_const, tein_type, tein_methods};

#[tein_module("myext", ext = true)]
mod myext_impl {
    /// greet someone
    #[tein_fn]
    pub fn hello(name: String) -> String {
        format!("hello from extension, {name}!")
    }

    /// divide a by b safely
    #[tein_fn]
    pub fn safe_div(a: i64, b: i64) -> Result<i64, String> {
        if b == 0 { Err("division by zero".into()) } else { Ok(a / b) }
    }

    /// greeting constant
    #[tein_const]
    pub const GREETING: &str = "hello from myext";

    /// a simple widget type
    #[tein_type]
    pub struct Widget { pub id: i64 }

    #[tein_methods]
    impl Widget {
        pub fn get_id(&self) -> i64 { self.id }
    }
}
```

The macro generates:
- `tein_ext_init` — the C entry point resolved by `load_extension()`
- API version check at init time
- VFS module registration via the host vtable
- Foreign type registration via the host vtable

The same naming conventions as inline `#[tein_module]` apply — see
[rust-scheme-bridge.md](rust-scheme-bridge.md).

## loading an extension from rust

```rust
let ctx = Context::new_standard()?;
ctx.load_extension("./libmyext.so")?;

// scheme can now:
ctx.evaluate("(import (tein myext))")?;
ctx.evaluate(r#"(myext-hello "world")"#)?;   // => "hello from extension, world!"
ctx.evaluate("(make-widget 42)")?;            // => #<widget:1>
ctx.evaluate("(widget-get-id w)")?;           // => 42
```

The library is loaded once per `load_extension()` call and the shared library handle
is leaked — no unload mechanism exists. The extension stays live for the process lifetime.

## stable C ABI

`tein-ext` defines the `TeinExtApi` vtable — a C-compatible struct of function pointers
that the host populates and passes to `tein_ext_init`. Extensions call host capabilities
through this vtable, never via direct function calls into `tein`.

The `TEIN_EXT_API_VERSION` constant is checked at init time. A version mismatch returns
`TEIN_EXT_ERR_VERSION` and the extension is rejected with an error.

When adding fields to `TeinExtApi`: bump `TEIN_EXT_API_VERSION`.

## caveats

- **no unload** — `libloading::Library::new()` is leaked; `dlclose()` is never called.
  the extension's code stays mapped for the process lifetime.
- **linux only today** — `.dylib` (macOS) and `.dll` (Windows) loading not yet
  implemented. tracked in issue #66.
- **panic safety** — panics in extension code unwind across the FFI boundary, which
  is undefined behaviour. extension methods should catch panics or avoid them.
- **dependency graph** — extension crates depend on `tein-ext` + `tein-macros`, never
  on `tein`. the macro emits `tein_ext::*` references resolved at extension compile time.
