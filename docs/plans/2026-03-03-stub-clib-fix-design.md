# Design: Fix include-shared stub modules (#103)

## problem

three VFS modules have `include-shared` in their `.sld` but no corresponding `.c` file
or `ClibEntry` in `vfs_registry.rs`. chibi-ffi was never run (bootstrapping problem:
chibi-ffi is a scheme program requiring a running chibi). result: these modules silently
load but their C-backed bindings are missing at runtime.

affected modules:
- `srfi/144` (`scheme/flonum` alias) — flonum constants and transcendentals, `default_safe: true`
- `scheme/bytevector` — endian-aware bytevector accessors, `default_safe: true`
- `chibi/time` — POSIX time structs, `default_safe: true`, only used by `srfi/18`

## solution

1. generate `.c` files via `chibi-ffi` from the fork, commit to `emesal-tein`
2. add `ClibEntry` to the three `VfsEntry` blocks in `vfs_registry.rs`
3. add a new `validate_include_shared` pass in `build.rs` to catch future drift
4. add regression tests

## architecture

### fork changes

run `chibi-ffi` on each stub from `~/forks/chibi-scheme`:

```
chibi-ffi lib/srfi/144/math.stub      → lib/srfi/144/math.c
chibi-ffi lib/scheme/bytevector.stub  → lib/scheme/bytevector.c
chibi-ffi lib/chibi/time.stub         → lib/chibi/time.c
```

notes:
- `lib/srfi/144/lgamma_r.c` is verbatim-included by `math.stub` via
  `(c-include-verbatim "lgamma_r.c")` — chibi-ffi inlines it into `math.c`, no
  separate compilation needed
- `lib/chibi/time.c` was already generated in an earlier test run; verify + keep
- commit all three to `emesal/chibi-scheme` branch `emesal-tein`; `cargo build` pulls them

### vfs_registry.rs changes

add `ClibEntry` to three entries, following the `scheme/time` pattern:

**`scheme/bytevector`**:
```rust
clib: Some(ClibEntry {
    source: "lib/scheme/bytevector.c",
    init_suffix: "scheme_bytevector",
    vfs_key: "/vfs/lib/scheme/bytevector",
    posix_only: false,
}),
```

**`srfi/144`**:
```rust
clib: Some(ClibEntry {
    source: "lib/srfi/144/math.c",
    init_suffix: "srfi_144_math",
    vfs_key: "/vfs/lib/srfi/144",
    posix_only: false,
}),
```

**`chibi/time`**:
```rust
clib: Some(ClibEntry {
    source: "lib/chibi/time.c",
    init_suffix: "chibi_time",
    vfs_key: "/vfs/lib/chibi/time",
    posix_only: true,  // uses <sys/time.h>, <sys/resource.h>
}),
```

note: `.c` files are referenced only via `clib.source` — they are NOT added to `files:`
(which is for VFS-embedded scheme source only).

### build.rs: validate_include_shared

new function, called at the same site as `validate_sld_includes` (line 435).
separate concern — checks C backing, not scheme file completeness.

algorithm:
1. for each `Embedded` entry in `VFS_REGISTRY`
2. find the `.sld` file, parse it for `(include-shared "stem")` directives
3. if any found and `entry.clib` is `None` → panic with actionable message
4. (no need to cross-check stem vs clib.source — presence of clib is sufficient)

the `include-shared` form uses a bare stem without extension: `(include-shared "144/math")`.
detection: walk the sexp tree looking for list heads `include-shared`, collect string args.

panic message format:
```
registry validation failed for 'srfi/144':
  .sld contains (include-shared "144/math") but clib is None.
  run chibi-ffi on lib/srfi/144/math.stub and add a ClibEntry to vfs_registry.rs.
```

### tests

three new tests in `context.rs`:

```rust
fn test_srfi_144_flonum_constants()
// (import (srfi 144)) fl-pi → Value::Real ≈ 3.14159

fn test_scheme_bytevector_endian()
// (import (scheme bytevector))
// (bytevector-u16-ref (bytevector 1 0) 0 'little) → Value::Integer(1)

fn test_chibi_time_import()
// (import (chibi time)) (procedure? get-time-of-day) → Value::Boolean(true)
```

## data flow / error path

before this fix: `(include-shared "144/math")` in `srfi/144.sld` → chibi tries
`sexp_find_static_library("/vfs/lib/srfi/144")` → not in table → silent failure
(missing bindings, not an import error).

after: `sexp_find_static_library` finds the entry → calls `sexp_init_lib_srfi_144_math`
→ registers all flonum constants and functions into the module env.

## notes

- `chibi/time` is `posix_only: true` — excluded from compilation on windows targets,
  same as other entries using POSIX time headers
- the `scheme/flonum` alias entry (`srfi/144` re-export) picks up the fix automatically
  via its dep on `srfi/144` — no separate change needed
- `validate_include_shared` runs at build time (cargo build panics) — no runtime cost
