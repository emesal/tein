# feature-gate serde, serde_json, and format modules

closes #78

## summary

make serde-based format modules optional via cargo feature flags. currently `serde`
and `serde_json` are hard dependencies — they should only be pulled in when the
corresponding format feature is enabled.

## approach

compile-time VFS gating: gate json VFS entries in `build.rs` behind
`#[cfg(feature = "json")]`, so when json is disabled, the `.sld`/`.scm` files
aren't compiled into the binary. `(import (tein json))` gives "module not found".

## Cargo.toml

```toml
[features]
default = ["json"]
json = ["dep:serde_json", "dep:serde", "tein-sexp/serde"]
debug-chibi = []

[dependencies]
serde = { version = "1", features = ["derive"], optional = true }
serde_json = { version = "1", optional = true }
tein-sexp = { path = "../tein-sexp" }  # no hardcoded features
```

`tein-sexp/serde` is activated transitively by the `json` feature.

## source gating

- `lib.rs`: `#[cfg(feature = "json")] mod json;` and `#[cfg(feature = "json")] mod sexp_bridge;`
- `context.rs`: gate trampolines + `register_json_module()` + call site behind `#[cfg(feature = "json")]`
- `context.rs` tests: gate the 5 json tests behind `#[cfg(feature = "json")]`
- `json.rs` / `sexp_bridge.rs`: no changes — gated by module declaration

## build.rs VFS gating

json `.sld`/`.scm` entries move from static array into conditional append:

```rust
let mut vfs_files: Vec<&str> = VFS_FILES.to_vec();
#[cfg(feature = "json")]
vfs_files.extend_from_slice(&["lib/tein/json.sld", "lib/tein/json.scm"]);
```

## test gating

- `tests/scheme_tests.rs`: `#[cfg(feature = "json")]` on `test_scheme_json()`
- rust unit tests in `json.rs` auto-gated by module gate

## lib.rs docs

add `## feature flags` section documenting the `json` feature.

## verification

- `cargo build --no-default-features` compiles without serde/serde_json
- `cargo test` (default features) passes all existing tests
- `cargo test --no-default-features` passes all non-json tests

## implementation plan

1. create branch via `just chore feature-gate-format-modules-2602`
2. update `tein/Cargo.toml`: make serde + serde_json optional, add json feature, set default
3. gate `mod json` and `mod sexp_bridge` in `lib.rs`
4. gate json trampolines, `register_json_module()`, and call site in `context.rs`
5. gate json tests in `context.rs`
6. gate VFS entries in `build.rs`
7. gate `test_scheme_json` in `tests/scheme_tests.rs`
8. add feature flags docs to `lib.rs`
9. verify: `cargo build --no-default-features`, `cargo test`, `cargo test --no-default-features`
10. `just lint`
11. collect AGENTS.md notes if any
