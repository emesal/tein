# VFS shadow: (scheme file) + (scheme show) — design

issue: #91

## problem

`(srfi 166 columnar)` imports `(scheme file)` for `open-input-file`, used by `from-file`.
this blocks the entire `(scheme show)` / `(srfi 166)` tree from `VFS_MODULES_SAFE`.

additionally, `(tein file)` was supposed to provide the full `(scheme file)` surface but
currently only exports `file-exists?` and `delete-file` — missing the 4 `open-*` variants
and 4 higher-order wrappers.

## approach: dynamic VFS shadowing

when building a sandboxed context, register replacement `.sld` files into the dynamic VFS
so chibi's module resolver finds ours instead of the native versions. the shadow modules
re-export from the corresponding `(tein ...)` module.

this pattern is reusable for future shadows (`scheme/load`, `scheme/process-context`, etc.).

### why dynamic, not static?

future shadowed modules may provide only a subset of the original surface. always-on
shadowing would require maintaining full reimplementations for unsandboxed contexts where
chibi's native version works fine. dynamic shadows only activate when sandboxed.

## design

### 1. expanded (tein file)

`(tein file)` becomes the full safe replacement for `(scheme file)`. 10 exports:

**rust trampolines** (6 functions, policy-checked via `IS_SANDBOXED` + `FsPolicy`):
- `file-exists?` — existing
- `delete-file` — existing
- `open-input-file` — new
- `open-binary-input-file` — new
- `open-output-file` — new
- `open-binary-output-file` — new

each trampoline follows the `file_exists_trampoline` pattern:
1. unsandboxed (`IS_SANDBOXED` = false) → allow, delegate to captured chibi original
2. sandboxed + policy set → check policy, delegate to chibi original
3. sandboxed + no policy → deny with sandbox violation error

the 4 `open-*` trampolines delegate to chibi's original primitives (captured from the
source env during `register_file_module()`). they're a policy gate, not a reimplementation.

**scheme definitions** (4 higher-order wrappers in `file.scm`):
- `call-with-input-file` — opens, calls proc, closes via `dynamic-wind`
- `call-with-output-file` — same for output
- `with-input-from-file` — opens, parameterizes `current-input-port`
- `with-output-to-file` — opens, parameterizes `current-output-port`

policy enforcement happens at the `open-*` layer only (single point of check).

### 2. VFS shadow infrastructure

reusable system in `src/shadow/`:

```rust
struct VfsShadow {
    path: &'static str,          // e.g. "scheme/file"
    sld: &'static str,           // .sld content
    scm: Option<&'static str>,   // .scm content, if any
}

const VFS_SHADOWS: &[VfsShadow] = &[
    VfsShadow {
        path: "scheme/file",
        sld: include_str!("shadow/scheme_file.sld"),
        scm: None,  // pure re-export, no .scm needed
    },
];
```

`register_vfs_shadows()` iterates the registry and calls `tein_vfs_register()` for each
entry. called during sandboxed context build, after `IS_SANDBOXED` is set.

shadow `.sld` for `scheme/file`:
```scheme
(define-library (scheme file)
  (import (tein file))
  (export open-input-file open-output-file
          open-binary-input-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file
          file-exists? delete-file))
```

### 3. unify IO wrapper system

the existing `check_and_delegate` + `wrapper_open_input_file` etc. are replaced by the
`(tein file)` trampolines. `file_read()`/`file_write()` on the builder just configures
`FsPolicy` — the trampolines are already in place.

**removed:**
- `check_and_delegate()`
- `wrapper_open_input_file` / `wrapper_open_binary_input_file` / `wrapper_open_output_file` / `wrapper_open_binary_output_file`
- `wrapper_fn_for()`
- the IO wrapper capture + registration block in `build()` (`has_io` / `for op in IoOp::ALL`)

**kept:** `IoOp` enum (for indexing `ORIGINAL_PROCS`), `ORIGINAL_PROCS` thread-local.

### 4. VFS_MODULES_SAFE updates

add to safe registry:
- `scheme/file` (dep: `tein/file`)
- `scheme/show` (dep: `srfi/166`)
- `srfi/166` and sub-modules (`base`, `pretty`, `columnar`, `unicode`, `color`)
- transitive deps: `srfi/1`, `srfi/117`, `srfi/130`, `chibi/optional`

### 5. build flow

**sandboxed context:**
1. set `IS_SANDBOXED = true`
2. `register_file_module(source_env)` — capture originals, register 6 trampolines
3. `register_vfs_shadows()` — inject shadow `.sld` into dynamic VFS
4. set `FsPolicy` if `file_read()`/`file_write()` configured
5. copy allowed bindings into restricted env
6. `(scheme file)` resolves to shadow → re-exports `(tein file)` → policy-checked

**unsandboxed context:**
1. `IS_SANDBOXED = false`
2. `register_file_module(source_env)` — trampolines allow everything
3. no shadows registered — `(scheme file)` resolves to chibi's native version

## no chibi fork changes

all changes are on the rust/VFS side. the existing dynamic VFS registration
(`tein_vfs_register`) and VFS gate (`tein_module_allowed`) handle everything.

## tests

- sandboxed: `(import (scheme file))` succeeds, gets policy-checked versions
- sandboxed without `file_read()`: `open-input-file` returns sandbox violation
- sandboxed with `file_read()`: `open-input-file` works for permitted paths
- sandboxed: `(import (scheme show))` works
- sandboxed: `(srfi 166 columnar)` `from-file` works with read policy
- unsandboxed: `(scheme file)` still uses chibi's native implementation
- existing `(tein file)` tests still pass
- higher-order wrappers: `call-with-input-file`, `with-input-from-file` etc.
