# universal module availability — handoff

## status

- **spec**: `docs/plans/2026-03-10-universal-module-availability-design.md` — committed to `dev`
- **implementation plan**: `docs/plans/2026-03-10-universal-module-availability.md` — committed to `dev`
- **plan review**: not yet completed (subagent quota hit). review chunks 1-3 before executing. key things to verify:
  - chunk 1 (tasks 1-4): chibi fork SLD files + VFS registry changes
  - chunk 2 (tasks 5-6): `(tein filesystem)` rust module + old trampoline removal
  - chunk 3 (tasks 7-9): extend `(tein process)` + integration tests + docs

## what to do next

1. **review the plan** — read the plan doc, cross-reference against the spec and codebase. the plan reviewers didn't get to run, so check for:
   - `file-exists?` trampoline is used by sandbox/VFS gate code independently — removing `register_file_module()` in task 6 may break things if `file-exists?` and `delete-file` are registered there for non-module purposes. check whether those trampolines are only accessed via `(import (tein filesystem))` or also via the top-level env directly.
   - the `#[tein_module]` pattern generates `register_vfs_module` calls for `.sld`/`.scm` — but for Embedded modules, the files are already in the static VFS. double-registration could be an issue (or a no-op if VFS overwrites). check how `(tein time)` handles this — it's Embedded AND calls `register_module_time()`.
   - POSIX constant values in `filesystem.scm` need verification against actual linux values (the plan uses common linux values but they're platform-dependent).

2. **create feature branch**: `just feature universal-module-availability-2603`

3. **execute the plan** using `superpowers:executing-plans` or `superpowers:subagent-driven-development`

4. **commit after each task**, lint after each batch

## key architectural decisions

- override modules (scheme/process-context, scheme/file, etc.) become `VfsSource::Embedded` with real `.sld` files in the chibi fork — they exist in the static VFS and work in ALL contexts
- safety stubs (chibi/stty, chibi/system, chibi/net/*) stay `VfsSource::Shadow` — sandbox-only
- `(tein filesystem)` new rust module provides real fs ops, `chibi/filesystem` re-exports from it
- `(tein process)` extended with `current-process-id` and `system`, `chibi/process` re-exports from it
- deferred functions (fork, signals, fd ops, etc.) defined as scheme stubs that raise "not implemented" errors
- `call-with-process-io` deferred to follow-up (needs custom port piping)

## files involved

### chibi fork (`~/forks/chibi-scheme`, branch `emesal-tein`)
- `lib/tein/filesystem.sld` + `.scm` — NEW
- `lib/tein/process.sld` + `.scm` — MODIFIED (add exports + deferred stubs)
- `lib/scheme/process-context.sld` — OVERWRITE
- `lib/scheme/eval.sld` — OVERWRITE
- `lib/scheme/load.sld` — OVERWRITE
- `lib/scheme/repl.sld` — OVERWRITE
- `lib/scheme/file.sld` — OVERWRITE
- `lib/scheme/time.sld` — OVERWRITE
- `lib/srfi/98.sld` — OVERWRITE
- `lib/chibi/filesystem.sld` — OVERWRITE
- `lib/chibi/process.sld` — OVERWRITE

### tein repo
- `tein/src/filesystem.rs` — NEW
- `tein/src/lib.rs` — add `mod filesystem;`
- `tein/src/context.rs` — register filesystem module, add process trampolines, remove old file trampolines, add tests
- `tein/src/vfs_registry.rs` — convert 9 Shadow->Embedded, remove broken entries, fix deps
- `tein/tests/vfs_module_tests.rs` — try un-ignoring chibi_diff
- `AGENTS.md` — update docs

## context notes

- the root cause is: `VfsSource::Shadow` modules have `files: &[]` so they're not in the static VFS. they only exist when dynamically registered by `register_vfs_shadows()` which only runs in sandbox mode.
- the VFS lookup (`tein_vfs_lookup` in eval.c patch A) works in ALL contexts. the gate (`tein_module_allowed`) allows everything when gate=0. modules just need to BE in the VFS.
- `(tein time)` is the template pattern: Embedded with `.sld`/`.scm` in fork, `#[tein_module]` in rust, registered before `load_standard_env`.
- existing `file-exists?`/`delete-file` trampolines are in `context.rs` at ~lines 1132-1189, registered via `register_file_module()` at ~line 2611.
- existing `(tein process)` trampolines are in `context.rs` at ~lines 1664-1881, registered into both primitive env (~line 2255) and top-level env (~line 2614).
- `SHADOW_STUBS` at `vfs_registry.rs:4033` has the canonical export lists for `chibi/filesystem` (42 fns + 12 consts) and `chibi/process` (25 fns + 20 consts).
