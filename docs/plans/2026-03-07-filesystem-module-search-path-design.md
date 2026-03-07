# Filesystem Module Search Path (#131)

**date**: 2026-03-07
**issue**: #131
**branch**: `feature/fs-module-search-path-YYMM` (to be created)

## Summary

Add user-configurable module search paths so `.sld`/`.scm` files on the
filesystem can be discovered via `(import ...)` without a Rust harness.
Three surfaces: `ContextBuilder::module_path()`, `TEIN_MODULE_PATH` env var,
and `-I`/`--include-path` CLI flag in `tein-bin`.

## Background

chibi-scheme resolves `(import (foo bar))` by searching `SEXP_G_MODULE_PATH`
— a per-context list of directory strings — for `foo/bar.sld`. tein currently
hardcodes this list to `["/vfs/lib"]` (via `sexp_default_module_path` in
`install.h`). `tein_vfs_gate_check` (the rust gate callback) rejects all
paths that don't start with `/vfs/lib/`, preventing any filesystem loading
even in unsandboxed contexts.

This feature extends both the path list and the gate to support user dirs.

## Non-Goals

- This is **not** about file IO policy (`FsPolicy`, `file_read`, issue #105).
  Module search paths are for module *discovery* only; they grant no
  `open-input-file` / `open-output-file` access.
- No eager VFS ingestion of filesystem modules (option A was considered and
  rejected: doesn't support `(include ...)`, requires reading all files
  eagerly).

## Design

### Approach: native chibi search path (option B)

Register user dirs into chibi's `SEXP_G_MODULE_PATH` via
`sexp_add_module_directory_op`. chibi's module loader already resolves
`(include "foo.scm")` relative to the `.sld` location — no extra work needed.

The VFS gate is extended to also permit paths rooted under user-configured
search dirs. This is the only C-level change required.

### New thread-local: `FS_MODULE_PATHS`

```rust
thread_local! {
    pub(crate) static FS_MODULE_PATHS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}
```

Populated during `build()` with canonicalised search dir paths. Cleared on
`Context::drop()` (same RAII pattern as `VFS_GATE`, `FS_GATE`, etc.).
Previous value saved and restored on `build()` so sequential contexts on the
same thread don't interfere.

### Gate change: `tein_vfs_gate_check`

Current logic (gate=1):
```
allowed = path.starts_with("/vfs/lib/") AND not ".." AND (scm passthrough OR in allowlist)
```

Extended logic (gate=1):
```
allowed = (path.starts_with("/vfs/lib/") AND not ".." AND (scm passthrough OR in allowlist))
       OR (path_under_any(FS_MODULE_PATHS) AND not "..")
```

`path_under_any` uses `canonicalize()` before prefix-checking to prevent
`..` traversal escaping a configured dir. The `.scm` passthrough for VFS
paths is not extended to filesystem paths — chibi passes the full resolved
path for `.scm` includes, so prefix matching is sufficient.

Gate=0 (unsandboxed, no module paths configured): no change — allows
everything as before.

### `ContextBuilder::module_path()`

```rust
/// Add a directory to the module search path.
///
/// When resolving `(import (foo bar))`, tein checks each path for
/// `foo/bar.sld` (and loads `(include ...)` files relative to the `.sld`).
/// Paths are checked in registration order (builder paths before
/// `TEIN_MODULE_PATH`).
///
/// Works in both sandboxed and unsandboxed contexts.
pub fn module_path(mut self, path: &str) -> Self { ... }
```

`module_path` can be called multiple times; paths accumulate. No interaction
with `sandboxed()` or `file_read()`.

During `build()`:
1. Read `TEIN_MODULE_PATH` env var (colon-separated); split into dirs.
2. Append builder-accumulated paths (these are checked first — prepended to
   chibi's path list so they shadow env-var paths).
3. For each dir: canonicalise, call `sexp_add_module_directory_op` (prepend
   mode), add canonicalised string to `FS_MODULE_PATHS`.

### `TEIN_MODULE_PATH` environment variable

Colon-separated list of directories, read in `build()` as a fallback.
Builder paths take precedence (prepended last, so searched first).
Consistent with `CHIBI_MODULE_PATH` convention.

### CLI: `-I` / `--include-path`

In `tein-bin`:

```
tein -I ./lib script.scm
tein -I ./lib -I /usr/share/tein/lib script.scm
tein --sandbox -I ./lib script.scm
```

`-I path` and `--include-path path` are equivalent. Can be repeated.
Maps directly to `.module_path(path)` on the builder.

`Args` gains:
```rust
module_paths: Vec<String>,
```

`parse_args` handles `-I <next-arg>` and `--include-path <next-arg>`.
Both `build_context_script` and `build_context_repl` thread `module_paths`
through to the builder.

### Sandboxed contexts

No special case. The gate extension allows the file to be *found*; transitive
imports within user modules go through the normal gate:

- VFS modules → must be in allowlist (unchanged)
- Other user filesystem modules → must be under a configured search path

If a user module imports a sandboxed-blocked module (e.g. `(tein http)` in
`Modules::Safe`), the gate rejects that import as normal. The filesystem
search path does not grant any allowlist entries.

## Interaction with issue #105

Orthogonal. `FsPolicy` controls `open-input-file` / `open-output-file` at
runtime. `FS_MODULE_PATHS` controls module *discovery*. Adding a module path
does not grant file-read access to that directory, and vice versa.

## Error handling

- Non-existent dir in `module_path()`: emit a warning at build time (don't
  error — the dir may be created later, and chibi silently skips missing
  dirs anyway).
- Dir that fails canonicalisation: skip with warning.
- `TEIN_MODULE_PATH` malformed entries: skip with warning.

## Testing

- Unit: `parse_args` with `-I`, `--include-path`, repeated flags
- Integration (`context.rs` tests):
  - unsandboxed: `module_path` → can import user `.sld`
  - sandboxed (`Modules::Safe`): `module_path` → can import user `.sld` that
    only uses `(scheme base)`
  - sandboxed: user `.sld` that imports blocked module → error
  - `(include ...)` in user `.sld` → works (file loaded relative to `.sld`)
  - `TEIN_MODULE_PATH` env var picked up
  - path traversal (`../evil`) → rejected by gate
- `tein-bin` integration: script that imports a local module via `-I`

## Files changed

- `tein/src/context.rs` — `ContextBuilder` field + `module_path()` method,
  `build()` path registration, `FS_MODULE_PATHS` thread-local, drop cleanup
- `tein/src/ffi.rs` — `tein_vfs_gate_check` extension, `sexp_add_module_directory` binding
- `tein/src/sandbox.rs` — `FS_MODULE_PATHS` thread-local declaration
- `tein-bin/src/main.rs` — `-I`/`--include-path` flag parsing, builder wiring
- `tein/tests/` — new integration tests (temp dir with `.sld` files)
