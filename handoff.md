# universal module availability — handoff

## status

- **branch**: `feature/universal-module-availability-2603`
- **spec**: `docs/plans/2026-03-10-universal-module-availability-design.md`
- **implementation plan**: `docs/plans/2026-03-10-universal-module-availability.md`
- **plan review**: completed. all 3 concerns verified clean.
- **execution mode**: direct (no subagents) — easier to resume after rate limits

## progress

- [x] task 1: create `(tein filesystem)` SLD/SCM in chibi fork — pushed to emesal-tein
- [x] task 2: overwrite r7rs SLD files in chibi fork — pushed to emesal-tein
- [x] task 3: overwrite chibi/filesystem + chibi/process SLDs, update tein/process — pushed to emesal-tein
- [ ] **task 4: update VFS registry** ← NEXT (reading done, edits not started)
- [ ] task 5: implement src/filesystem.rs
- [ ] task 6: remove old trampolines + update (tein file)
- [ ] task 7: add current-process-id and system to (tein process)
- [ ] task 8: integration tests
- [ ] task 9: update AGENTS.md and docs

## task 4 notes (ready to execute)

all the VFS registry sections have been read. here's exactly what to do:

### add new entry — `tein/filesystem`
insert after `tein/file` entry (~line 218), before `tein/load`:
```rust
VfsEntry {
    path: "tein/filesystem",
    deps: &["scheme/base"],
    files: &["lib/tein/filesystem.sld", "lib/tein/filesystem.scm"],
    clib: None,
    default_safe: true,
    source: VfsSource::Embedded,
    feature: None,
    shadow_sld: None,
},
```

### convert 7 shadow entries to embedded
for each: change `files: &[]` → `files: &["lib/<path>.sld"]`, `source: Shadow` → `Embedded`, `shadow_sld: Some(...)` → `None`, update deps and comments.

| entry | ~line | new deps | new files |
|-------|-------|----------|-----------|
| scheme/load | 494 | keep `&["tein/load"]` | `&["lib/scheme/load.sld"]` |
| scheme/file | 867 | `&["tein/filesystem"]` | `&["lib/scheme/file.sld"]` |
| scheme/eval | 890 | keep `&[]` | `&["lib/scheme/eval.sld"]` |
| scheme/repl | 911 | keep `&[]` | `&["lib/scheme/repl.sld"]` |
| scheme/process-context | 937 | `&["tein/process"]` | `&["lib/scheme/process-context.sld"]` |
| scheme/time (shadow at ~983) | convert; keep `&["tein/time"]` + `feature: Some("time")` | `&["lib/scheme/time.sld"]` |
| srfi/98 (shadow at ~1293) | `&["tein/process"]` | `&["lib/srfi/98.sld"]` |

### convert 2 shadow stubs to embedded
| entry | ~line | new deps | new files |
|-------|-------|----------|-----------|
| chibi/filesystem | ~2133 | `&["tein/filesystem"]` | `&["lib/chibi/filesystem.sld"]` |
| chibi/process | ~2143 | `&["tein/process"]` | `&["lib/chibi/process.sld"]` |

### remove 2 broken entries
- scheme/time Embedded entry at ~964 (depends on tai-to-utc-offset, has ClibEntry for scheme/time.c) — DELETE entire entry
- srfi/98 Embedded entry at ~1275 (has ClibEntry for srfi/98/env.c) — DELETE entire entry

### remove from SHADOW_STUBS (~4033)
remove the `ShadowStub` entries for `"chibi/filesystem"` and `"chibi/process"`. keep `chibi/system`, `chibi/shell`, etc.

### fix chibi/term/ansi deps (~3032)
change `deps: &["scheme/base"]` → `deps: &["scheme/base", "scheme/write", "scheme/process-context"]`

### verify
`just clean && cargo build` — must succeed (chibi fork re-fetched with new SLDs)

## key architectural decisions

- override modules become `VfsSource::Embedded` with real `.sld` files in chibi fork
- safety stubs (chibi/stty, chibi/system, chibi/net/*) stay `VfsSource::Shadow`
- `(tein filesystem)` new rust module provides real fs ops
- `(tein process)` extended with `current-process-id` and `system`
- deferred functions raise "not implemented" errors at call time
- `call-with-process-io` deferred to follow-up

## context notes

- `VfsSource::Shadow` has `files: &[]` → not in static VFS → only exists when `register_vfs_shadows()` runs (sandbox only)
- `(tein time)` is the template: Embedded + `#[tein_module]` + registered before `load_standard_env`
- `#[tein_module]` double-registration with Embedded is harmless (dynamic VFS shadows static, first-match wins)
- existing `file-exists?`/`delete-file` trampolines: `context.rs` ~lines 1132-1189, registered via `register_file_module()` ~line 2611
- existing `(tein process)` trampolines: `context.rs` ~lines 1664-1881, registered into primitive env (~2255) and top-level env (~2614)
- `(tein file)` must be updated (task 6) to import from `(tein filesystem)` before removing old trampolines
