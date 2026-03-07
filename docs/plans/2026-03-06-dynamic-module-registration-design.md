# dynamic module registration

**issue**: #132
**date**: 2026-03-06
**status**: design

## summary

add the ability to register scheme modules at runtime from both rust and scheme code. enables LLMs to create, store, and import their own tools inside sandboxed environments without filesystem access.

## motivation

tein's VFS is static-by-default: modules are embedded at compile time or registered via `register_vfs_module` before first import. there's no way for scheme code running inside the sandbox to define new importable modules. for LLM agent harnesses (e.g. chibi~), this is a key capability — agents need to create tools as scheme libraries, then import and compose them across evaluations.

the embedder (chibi~) has its own VFS for persistent storage. it reads module source strings from its storage and feeds them into tein. tein needs to accept these cleanly and make them importable, including in sandboxed contexts.

## design

### layer 1: allowlist mutation (internal)

`VFS_ALLOWLIST` is currently set once during `Context::build()` and restored on drop. a new internal method mutates the live allowlist:

```rust
// pub(crate) — building block, not public API
fn allow_module_runtime(&self, path: &str)
```

appends to the `VFS_ALLOWLIST` thread-local. only meaningful when `VFS_GATE` is `GATE_CHECK` (sandboxed contexts); no-op otherwise.

### layer 2: `Context::register_module` (public rust API)

```rust
/// register a scheme module from a `define-library` source string.
///
/// parses the library name from the source, registers it into the dynamic
/// VFS, and (if sandboxed) appends it to the live import allowlist.
///
/// the source must use `(begin ...)` for definitions — `(include ...)`
/// is not supported in dynamically registered modules.
pub fn register_module(&self, source: &str) -> Result<()>
```

**steps**:
1. `sexp_read` the source to get the `define-library` form
2. extract library name list: `(define-library (my tool) ...)` -> `["my", "tool"]`
3. validate: must be `define-library`, name must not be empty
4. derive VFS path: `["my", "tool"]` -> `"my/tool"` -> `/vfs/lib/my/tool.sld`
5. **collision check**: `tein_vfs_lookup("/vfs/lib/my/tool.sld")` — if it already exists in the *static* VFS table, return error. dynamic entries (from previous `register_module` calls) are allowed to be overwritten (update semantics).
6. **`(include ...)` rejection**: walk the parsed sexp for `include`, `include-ci`, `include-library-declarations` — return error if found
7. register the source string as `/vfs/lib/my/tool.sld` via `tein_vfs_register`
8. call layer 1 to append `"my/tool"` to the live allowlist

**collision semantics**: only *static* VFS entries (compiled-in modules) are protected. re-registering a dynamic module overwrites it (the linked list prepend in `tein_vfs_register` naturally shadows the previous entry). this enables the "edit and reload" workflow for LLM-authored tools.

**note on chibi module caching**: chibi caches module environments after first import. re-registering a module's VFS entry does NOT invalidate the cache — a subsequent `(import (my tool))` may return the old version. this is a known limitation; fresh imports require a fresh context (or `ManagedContext::reset()`). document this clearly.

### layer 3: `(tein modules)` (scheme API)

a new `VfsSource::Dynamic` module with two exports:

- `(register-module source-string)` — trampoline to `Context::register_module`. returns `#t` on success, raises scheme error on failure.
- `(module-registered? '(my tool))` — takes a quoted list, converts to VFS path, checks `tein_vfs_lookup`. returns `#t` or `#f`.

**gating**: `(tein modules)` is `default_safe: false` in the VFS registry. to make it available in sandboxed contexts, the embedder calls:

```rust
.allow_dynamic_modules()  // sugar for .allow_module("tein/modules")
```

without this, sandboxed code cannot import `(tein modules)` — the VFS gate blocks it.

**trampoline registration**: registered into the primitive env before `load_standard_env` (same pattern as `tein-environment-internal`), so it's available to any library body via `(import (chibi))`.

### scheme usage

```scheme
(import (tein modules))

(register-module
  "(define-library (my tool)
     (import (scheme base))
     (export greet)
     (begin
       (define (greet name)
         (string-append \"hello \" name))))")

(import (my tool))
(greet "world")  ;; => "hello world"
```

### rust embedder usage

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .allow_dynamic_modules()
    .build()?;

// register from rust (e.g. after reading from chibi~'s VFS)
ctx.register_module(r#"
    (define-library (my tool)
      (import (scheme base))
      (export greet)
      (begin (define (greet x) (string-append "hi " x))))
"#)?;

let result = ctx.evaluate("(import (my tool)) (greet \"world\")")?;
assert_eq!(result, Value::String("hi world".into()));
```

## collision check detail

`register_module` rejects registration when a module already exists in the **static** VFS table (compile-time embedded modules). this prevents accidental shadowing of `scheme/base`, `tein/json`, `srfi/1`, etc.

dynamic-over-dynamic shadowing is allowed — this is the update/reload path.

implementation: `tein_vfs_lookup` checks dynamic entries first, then static. we need a way to distinguish "found in static" from "found in dynamic" from "not found." options:

- **option A**: add `tein_vfs_lookup_static(path)` that only checks the static table
- **option B**: add a flag parameter to `tein_vfs_lookup`

option A is cleaner — a separate function with a clear name.

## `(include ...)` rejection

dynamically registered modules must be self-contained — all code in `(begin ...)`. `(include "foo.scm")` requires a separate VFS entry for the included file, which complicates the single-string API and creates opportunities for path confusion.

rejection is implemented by walking the parsed sexp for `include`, `include-ci`, and `include-library-declarations` symbols at the top level of the `define-library` form.

embedders who need multi-file modules can still use the low-level `register_vfs_module` API directly.

## build.rs changes

- add `tein/modules` to `DYNAMIC_MODULE_EXPORTS`: `["register-module", "module-registered?"]`
- add VFS registry entry: `path: "tein/modules"`, `default_safe: false`, `source: VfsSource::Dynamic`, `deps: &["scheme/base"]`

## tein_shim.c changes

- add `tein_vfs_lookup_static(path)` — same as `tein_vfs_lookup` but only searches the static `tein_vfs_table`, skipping the dynamic linked list

## testing

- **rust unit**: `register_module` valid source, collision rejection, `(include)` rejection, allowlist mutation
- **rust integration (unsandboxed)**: register + import + call
- **rust integration (sandboxed)**: register + import with `allow_dynamic_modules()`, verify blocked without
- **scheme integration**: `(register-module ...)` then `(import ...)` in same eval and across evals
- **collision**: register `(define-library (scheme base) ...)` — error
- **update**: register `(my tool)` twice — second shadows first
- **module-registered?**: before and after registration
- **chibi cache**: register, import, re-register, import again — document that cached version persists

## non-goals

- filesystem `-I` path scanning — tracked in #131, builds on this foundation
- `(include ...)` support in dynamic modules
- module *unregistration* (the dynamic VFS has no delete; cleared on context drop)
- cross-context module sharing (each context has its own dynamic VFS)

## future

- #131: filesystem module paths (`-I`, `TEIN_MODULE_PATH`) — reads files from disk, calls `register_module` for each
- module versioning / cache invalidation — if needed, likely via `ManagedContext::reset()`
