# shadow module stubs — design

## goal

make OS-touching chibi modules importable in sandboxed contexts with clear
error-on-call behaviour. phase 1 stubs all exports; phase 2 (future) replaces
stubs with policy-gated rust trampolines progressively.

## module classification

### tier 1: shadow stubs (sandbox error on call)

shadow entries with build-time-generated `.sld` strings. each export is either
a function stub raising `[sandbox:path] name not available` or a constant
stub set to `0`.

| module | fn exports | const exports | C-backed |
|--------|-----------|---------------|----------|
| `chibi/filesystem` | ~55 | ~15 (open flags, lock modes) | yes |
| `chibi/process` | ~20 | ~20 (signal constants, wait flags) | yes |
| `chibi/system` | ~20 | 0 | yes |
| `chibi/net` | ~25 | ~20 (AF/socket/proto/flag constants) | yes |
| `chibi/shell` | ~20 | 0 | no (wraps process+filesystem) |
| `chibi/temp-file` | 2 | 0 | no (wraps filesystem) |
| `chibi/net/http` | ~9 | 0 | no (wraps net) |
| `chibi/net/server` | 2 | 0 | no (wraps net) |
| `chibi/net/http-server` | ~12 | 0 | no (wraps many) |
| `chibi/net/server-util` | ~5 | 0 | no (wraps net+fs) |
| `chibi/net/servlet` | ~28 | 0 | no (wraps net+fs) |

### tier 2: normal embedded (not OS-touching)

| module | notes |
|--------|-------|
| `chibi/channel` | pure scheme on srfi/18 threads — `VfsSource::Embedded` |

### tier 3: future gated (upgrade priority order)

when ready for policy-controlled access, create `tein/*` trampoline and update
shadow to re-export from it (same pattern as existing `tein/file`):

1. `chibi/filesystem` → `tein/filesystem` — extend existing `FsPolicy`
2. `chibi/system` → `tein/system` — neutered hostname/user-info
3. `chibi/net` → `tein/net` — network policy (host/port allowlists)
4. `chibi/process` → `tein/os-process` — most dangerous (exec/fork)

## architecture

### data source: `SHADOW_STUBS` in `vfs_registry.rs`

```rust
/// a shadow module whose exports are stubbed with sandbox-denial errors.
/// build.rs generates the `.sld` scheme source from this data.
struct ShadowStub {
    /// module path, e.g. "chibi/filesystem"
    path: &'static str,
    /// function exports — stubbed as `(define (name . args) (error ...))`
    fn_exports: &'static [&'static str],
    /// constant exports — stubbed as `(define name 0)`
    const_exports: &'static [&'static str],
}

const SHADOW_STUBS: &[ShadowStub] = &[
    ShadowStub {
        path: "chibi/filesystem",
        fn_exports: &["delete-file", "link-file", "create-directory", ...],
        const_exports: &["open/read", "open/write", ...],
    },
    // ...
];
```

### build.rs: generate `tein_shadow_stubs.rs`

build.rs already parses `vfs_registry.rs` for validation. extend it to:

1. parse `SHADOW_STUBS` entries
2. for each, generate a scheme `(define-library ...)` string:
   ```scheme
   (define-library (chibi filesystem)
     (import (scheme base))
     (export delete-file link-file open/read ...)
     (begin
       (define open/read 0)
       ...
       (define (delete-file . args)
         (error "[sandbox:chibi/filesystem] delete-file not available"))
       ...))
   ```
3. write `tein_shadow_stubs.rs` to `OUT_DIR`:
   ```rust
   const GENERATED_SHADOW_SLDS: &[(&str, &str)] = &[
       ("chibi/filesystem", "...generated sld..."),
       // ...
   ];
   ```

### sandbox.rs: register generated stubs

`register_vfs_shadows()` currently handles only hand-written `shadow_sld` fields.
extend it to also register generated stubs:

```rust
include!(concat!(env!("OUT_DIR"), "/tein_shadow_stubs.rs"));

pub(crate) fn register_vfs_shadows() {
    // existing: hand-written shadows from VFS_REGISTRY
    for entry in VFS_REGISTRY.iter() {
        if entry.source == VfsSource::Shadow {
            if let Some(sld) = entry.shadow_sld {
                register_one(entry.path, sld);
            }
        }
    }
    // new: generated stubs
    for &(path, sld) in GENERATED_SHADOW_SLDS.iter() {
        register_one(path, sld);
    }
}
```

### VfsEntry for stub modules

each stub module gets a `VfsEntry` so the allowlist machinery works:

```rust
VfsEntry {
    path: "chibi/filesystem",
    deps: &["scheme/base"],
    files: &[],
    clib: None,
    default_safe: true,
    source: VfsSource::Shadow,
    feature: None,
    shadow_sld: None, // generated from SHADOW_STUBS by build.rs
},
```

`shadow_sld: None` + `VfsSource::Shadow` = generated stub (looked up from
`GENERATED_SHADOW_SLDS`). `shadow_sld: Some(...)` = hand-written (existing
pattern, unchanged).

## generated sld format

```scheme
(define-library (<module path as list>)
  (import (scheme base))
  (export <all fn_exports> <all const_exports>)
  (begin
    ;; constants first
    (define <const> 0)
    ...
    ;; function stubs
    (define (<fn> . args)
      (error "[sandbox:<path>] <fn> not available"))
    ...))
```

`(import (scheme base))` provides `error` and `define`.

constants stub to `0` so code referencing flags/constants doesn't crash on
binding. function stubs accept any arity via `(. args)` and raise a clear
error identifying the module and function.

## chibi/channel

not OS-touching. add as normal `VfsSource::Embedded`:

```rust
VfsEntry {
    path: "chibi/channel",
    deps: &["srfi/9", "srfi/18"],
    files: &["lib/chibi/channel.sld", "lib/chibi/channel.scm"],
    clib: None,
    default_safe: true,
    source: VfsSource::Embedded,
    feature: None,
    shadow_sld: None,
},
```

## tracking: stub vs gated status

the handoff doc (`docs/handoff-module-inventory.md`) tracks per-module status:
- **stub**: shadow with error stubs (phase 1)
- **gated**: shadow re-exporting from `tein/*` trampoline with policy checks (phase 2)
- **native**: unsandboxed, using chibi's own implementation (always available)

## key decisions

- **build-time generation**: keeps `shadow_sld` as `&'static str`, no runtime allocation
- **constants stub to 0**: avoids crashing code that only references flag values
- **`deps: &["scheme/base"]`**: stubs need `error` from scheme/base
- **`default_safe: true`**: stubs are safe — they *deny* access, not *grant* it
- **no new rust code for phase 1**: all stubs are pure scheme error raisers
