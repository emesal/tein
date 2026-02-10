# r7rs standard environment — implementation handoff

## status: phases 1–3 complete, phases 4–5 pending

all tests pass (108 lib + 12 scheme_fn + 8 doc-tests = 128 total).

## what's done

### phase 0: prerequisite — io.c pre-generation
- `tein/vendor/chibi-scheme/lib/chibi/io/io.c` generated from `io.stub` using a temporary local chibi build
- 766 lines, pre-generated and committed (chibi's `.gitignore` was edited to un-ignore it)
- this file is stable for chibi 0.11

### phase 1: VFS infrastructure
- **`tein/build.rs`**: `generate_vfs_data()` reads 48 `.sld`/`.scm` files from `vendor/chibi-scheme/lib/` and produces `tein_vfs_data.h` with `\xNN`-escaped C string constants + lookup table
- **`tein/build.rs`**: `generate_install_h()` now sets `sexp_default_module_path` to `"/vfs/lib"` (not `"vfs://..."` — colon is a path list separator in chibi)
- **`tein/vendor/chibi-scheme/tein_shim.c`**: added `tein_vfs_lookup(path, &out_length)` — linear scan of the VFS table
- **`tein/vendor/chibi-scheme/eval.c`**: 2 patches:
  - **patch A** (line ~2391): in `sexp_find_module_file_raw`, the existing `sexp_find_static_library || file_exists_p` check now also checks `tein_vfs_lookup`
  - **patch B** (line ~1530): in `sexp_load_op`, before `sexp_open_input_file`, calls `tein_vfs_lookup` and if found, uses `sexp_open_input_string` with embedded content instead
- **`tein/build.rs`**: added `-DSEXP_USE_STATIC_LIBS=1 -DSEXP_USE_STATIC_LIBS_NO_INCLUDE=1` flags

### phase 2: static C library table
- **`tein/build.rs`**: `generate_clibs()` produces `tein_clibs.c` using `#define sexp_init_library / #include / #undef` pattern for 5 C libraries: `ast.c`, `io/io.c`, `param.c`, `hash.c`, `bit.c`
- lookup table with `/vfs/lib/...` keys matches what `sexp_find_module_file_raw` constructs
- extra `-I` paths added for nested `#include` resolution
- **note**: forward declarations were REMOVED — the `#include` pattern already defines the functions, and the actual C signature uses `const char* version, const sexp_abi_identifier_t abi` (not `sexp version, sexp abi`)

### phase 3 partial: ffi.rs fixes
- **fixed**: `sexp_load_standard_env` signature — `version` param changed from `sexp_uint_t` to `sexp` (tagged fixnum)
- **added**: `tein_sexp_load_standard_ports` shim in `tein_shim.c` (wraps `sexp_load_standard_ports(ctx, env, stdin, stdout, stderr, 1)`)
- **added**: extern declaration + safe wrappers in `ffi.rs`: `load_standard_env()`, `load_standard_ports()`

### phase 3: rust API
- **`tein/src/context.rs`**: `ContextBuilder` gains `standard_env: bool` field + `.standard_env()` builder method
- **`tein/src/context.rs`**: `build()` loads standard env (init-7 + meta-7 via VFS) before sandbox restriction; sandbox copies from enriched env
- **`tein/src/context.rs`**: `Context::new_standard()` convenience method
- **`tein/src/ffi.rs`**: `env_copy_named()` wrapper — copies a named binding from one env to another, searching both direct and rename bindings (needed because the standard env stores many bindings as renames)
- **`tein/vendor/chibi-scheme/tein_shim.c`**: `tein_env_copy_named()` C function — walks env chain including rename bindings with synclo unwrapping
- **`tein/vendor/chibi-scheme/eval.c`**: patch C — `sexp_open_input_file_op` checks VFS before `fopen()`, sets port name for source tracking. enables module system to read `.sld` files from VFS.
- **`tein/vendor/chibi-scheme/eval.c`**: patch B enhanced — sets port name on VFS string ports for source tracking
- **7 new tests**: `new_standard`, `map`, `for_each`, `values`, `dynamic_wind`, `with_sandbox`, `with_step_limit`

## what remains

### ~~phase 3: rust API (the main deliverable)~~ done

implemented as described above. standard env loads ~200 direct bindings including `map`, `for-each`, `values`, `dynamic-wind`, `call-with-values`, etc. combined with sandbox presets, bindings from the standard env can be selectively allowed via `.allow(&["map", "for-each"])`.

**note on `import`**: `(import (scheme base))` partially works — VFS patch C enables the module system to find and read `.sld` files, and all 27+ VFS files load successfully. however, the module finalization step fails with an "invalid type, expected Input-Port" error, likely due to chibi's internal handling of string ports vs file ports during macro expansion (e.g., `include/aux` in `extras.scm` calls `read-sexps` which expects file-backed ports). fixing this is deferred to phase 4 (module allowlist) where a deeper investigation of the module system's port expectations is needed.

### phase 4: module allowlist
restrict which modules can be imported in sandboxed contexts.

**files to modify**: `tein/src/sandbox.rs`, `tein/src/context.rs`, `tein/vendor/chibi-scheme/tein_shim.c`

1. add `MODULE_ALLOWLIST` thread-local in `sandbox.rs` (same pattern as `FS_POLICY`)
2. add `tein_module_check` callback hook in `tein_shim.c` — a thread-local function pointer called from the VFS find path
3. add `.allow_modules(&[&str])` to `ContextBuilder`
4. **design choice**: the plan recommends option B — allowlist only filters user-facing `import`, not transitive module loads. this is cleaner but requires intercepting at the `repl-import` level rather than `find-module-file`. could be done by wrapping `repl-import` or adding a scheme-level check in the meta env.

### phase 5: docs + examples
- `examples/standard.rs` — demonstrates standard env, imports, standard_env + sandbox
- `AGENTS.md` — architecture section updated (VFS, eval.c patches, standard env flow). remaining: add VFS/clibs to "adding a new scheme type" checklist if relevant.
- update `DEVELOPMENT.md` (new build artifacts, new API surface)
- update `TODO.md` (mark r7rs standard environment complete once import works)

## key files (for quick orientation)

| file | role |
|---|---|
| `tein/build.rs` | generates `tein_vfs_data.h`, `tein_clibs.c`, `install.h`; compiles everything |
| `tein/vendor/chibi-scheme/eval.c` | patched in 3 places (A: find_module_file_raw ~2391, B: sexp_load_op ~1550, C: sexp_open_input_file_op ~1310) |
| `tein/vendor/chibi-scheme/tein_shim.c` | VFS lookup, standard ports wrapper, env_copy_named, fuel control, sandboxing |
| `tein/src/ffi.rs` | rust FFI declarations + safe wrappers |
| `tein/src/context.rs` | `ContextBuilder`, `Context`, all tests |
| `tein/src/sandbox.rs` | `Preset`, `FsPolicy`, thread-locals |

## important discoveries during implementation

1. **colon in VFS prefix**: `"vfs://..."` can't be used as a module path — chibi's `sexp_add_path` splits on `:`. using `/vfs/lib` instead.
2. **clib forward declarations**: don't add `extern` forward declarations for the init functions — the `#include` pattern already defines them, and the actual C signature uses `const char*` for version/abi, not `sexp`.
3. **threads feature**: on linux, `SEXP_USE_GREEN_THREADS` defaults to 1, so the `threads` cond-expand feature is active. `srfi/39.sld` includes `39/syntax.scm` (not `39/syntax-no-threads.scm`). both are in the VFS.
4. **full-unicode**: always enabled in chibi. `scheme/char.sld` uses the full unicode path.
5. **`sexp_load_standard_env` signature**: the version parameter is `sexp` (a tagged fixnum), NOT `sexp_uint_t`. the previous ffi declaration was wrong.
6. **standard env load takes ~80ms** — includes loading init-7.scm + meta-7.scm from VFS. acceptable for a one-time cost.
7. **rename bindings**: the standard env stores most bindings as *renames* (via `SEXP_USE_RENAME_BINDINGS`), not direct bindings. `sexp_env_ref` with a bare symbol won't find renamed bindings. the `tein_env_copy_named` helper handles this by scanning both direct bindings and renames with synclo unwrapping.
8. **`let` in sandboxed standard env**: closures from the standard env (e.g. `for-each`) reference the full env internally, but `let`-bound variables in user code live in the restricted null env. using `define` for top-level bindings works; `let` inside `for-each` callbacks does not. this is a scope chain issue specific to the null env sandbox.
9. **VFS and open-input-file**: the module system (`meta-7.scm`) calls `open-input-file` on VFS paths, not just `sexp_load_op`. patch C was added to intercept this. string ports need `sexp_port_name` set for source tracking.

## VFS file inventory (48 files)

```
lib/init-7.scm, lib/meta-7.scm
lib/scheme/{base,write,read,lazy,case-lambda,cxr,inexact,complex,char}.sld
lib/scheme/{define-values,extras,misc-macros,cxr,inexact,digit-value}.scm
lib/scheme/char/{full,special-casing,case-offsets}.scm
lib/chibi/{equiv,string,ast}.{sld,scm}
lib/chibi/io.sld, lib/chibi/io/io.scm
lib/chibi/char-set/{base,full}.sld, lib/chibi/char-set/full.scm
lib/chibi/iset/base.{sld,scm}
lib/srfi/{9,11,16,38,39,69,151}.sld
lib/srfi/9.scm, lib/srfi/38.scm
lib/srfi/39/{syntax,syntax-no-threads}.scm
lib/srfi/69/{type,interface}.scm
lib/srfi/151/bitwise.scm
```

## commands

```bash
cargo build                           # build (generates VFS + clibs at build time)
cargo test                            # all tests (108 lib + 12 scheme_fn + 8 doc-tests)
cargo test test_standard_env          # all standard env tests (7 tests)
cargo clean && cargo build            # nuclear option if generated files get weird
```
