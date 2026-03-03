# design: module inventory completion

**date**: 2026-03-03
**branch**: `feature/module-inventory-completion-2603`
**closes**: #92 (vet VFS modules for sandbox safety)
**creates**: #106 (scheme/r5rs deferred, blocked on #97)
**references**: #105 (writable VFS compartment — future progressive gating)

## goal

close out the module inventory. after this work, every chibi-scheme module is either:
- in the VFS (safe, unsafe, or shadow)
- intentionally excluded with documented rationale
- tracked in a github issue for future work

## module disposition

### A. pure VFS additions (3 modules)

embed `.sld` + `.scm`, `default_safe: true`.

| module | deps (all already in VFS) | files |
|--------|---------------------------|-------|
| `chibi/mime` | `scheme/base`, `scheme/char`, `scheme/write`, `chibi/base64`, `chibi/quoted-printable`, `chibi/string` | `mime.sld`, `mime.scm` |
| `chibi/binary-record` | `scheme/base`, `srfi/1`, `srfi/151`, `srfi/130` | `binary-record.sld`, `binary-types.scm`, `binary-record.scm` |
| `chibi/memoize` | `chibi/optional`, `chibi/pathname`, `chibi/string`, `srfi/9`, `srfi/38`, `srfi/69`, `srfi/98`, `chibi/ast`, `chibi/system`, `chibi/filesystem` | `memoize.sld`, `memoize.scm` |

`chibi/memoize` note: chibi cond-expand branch pulls `chibi/system` + `chibi/filesystem`
(both already shadowed). in-memory LRU cache works; file-backed `memoize-to-file` errors
via shadowed deps. #105 upgrades this automatically.

### B. new shadow stubs (8 modules)

generated error-on-call stubs via `SHADOW_STUBS` in `vfs_registry.rs`.

| module | fn | const | macro | rationale |
|--------|----|-------|-------|-----------|
| `chibi/stty` | 12 (incl record types) | 3 | 0 | terminal ioctl, C-backed |
| `chibi/term/edit-line` | 19 | 0 | 0 | line editor, depends on stty |
| `chibi/log` | 35 | 0 | 3 | OS-coupled logging (file lock, PIDs) |
| `chibi/app` | 6 | 0 | 0 | CLI framework, depends on config + argv |
| `chibi/config` | 21 | 0 | 0 | config file reader, filesystem access |
| `chibi/tar` | 32 | 0 | 0 | tar archives, hard-wired to filesystem |
| `srfi/193` | 5 | 0 | 0 | leaks argv, script path |
| `chibi/apropos` | 2 | 0 | 0 | env introspection, info leak |

record type exports from `chibi/stty` (`winsize?`, `make-winsize`, `winsize-row`, etc.)
are listed as `fn_exports` — in the stub context they're just error-raising functions.

### C. hand-written shadow (1 module)

`scheme/load` — functional VFS-restricted load, delegates to `tein/load`.

```scheme
(define-library (scheme load)
  (import (tein load))
  (export load))
```

hand-written `.sld` in `lib/tein/` (chibi fork), registered as `VfsSource::Shadow`
with `shadow_sld: Some(...)`. same pattern as `scheme/file` and `scheme/process-context`.

### D. deferred (1 module)

`scheme/r5rs` — mega-bundle blocked on #97 (sandboxed eval). tracked in #106.

### E. intentionally excluded (23 modules)

documented in `docs/module-inventory.md` appendix B with per-module rationale.
no code changes needed.

## design constraints

- record type exports stubbed as fn_exports (no new stub generator category)
- `chibi/memoize` designed to upgrade transparently when #105 lands
- `scheme/load` shadow uses real `tein/load` — forward-compatible with #105
- no changes to chibi fork needed (shadow stubs are generated; `scheme/load` shadow
  is a tein-side `.sld` registered via VFS, not placed in the fork)

## testing

each new module gets at least one integration test:
- pure VFS entries: import + basic usage in sandbox
- shadow stubs: import succeeds, calling fn raises `[sandbox:path]` error
- `scheme/load`: `(import (scheme load))` + `(load "/vfs/lib/...")` works in sandbox

## documentation updates

- `docs/module-inventory.md`: update status markers, summary table, priority queue
- `docs/handoff-module-inventory.md`: new session entry, update "what remains"
- implementation plan: self-updating after each batch
