# handoff: module inventory work

## what has been done (across two sessions)

### session 1
1. created `docs/module-inventory.md` — comprehensive checklist of every chibi-scheme
   module (~553 files), status, and remaining work
2. added 47 pure-scheme modules to `tein/src/vfs_registry.rs` (commit `192fbe2`)
3. discovered stub collision bug with mega re-export bundles — excluded with note

### session 2
4. added `chibi/crypto/md5`, `chibi/crypto/sha2`, `chibi/crypto/rsa` (commit `d04061d`)
   - sha2 uses cond-expand; tein takes the pure-scheme `srfi/151 + chibi/bytevector` path
   - sha2-native.scm listed in files to satisfy validator (dead branch, harmless)
5. added `chibi/show/base` — thin alias for `srfi/166/base` (commit `d04061d`)
6. added `srfi/159` + sub-modules `base/color/columnar/unicode` (commit `d04061d`)
   - shares .scm files with srfi/166 via `../166/` relative includes
   - build.rs `collect_include_files` now normalises `../` (lexical, no fs access)
7. fixed `unexported_stubs()` dedup bug (commit `61ea8fc`)
   - now collects a `covered` set from allowed modules first; skips names in it
   - prevented stubs from clobbering real bindings when re-export bundles present
8. added `scheme/small` (`default_safe: false`) and `scheme/red` (`default_safe: false`)
   - both pull `scheme/eval + scheme/load` as transitive deps → would poison safe allowlist
   - users opt in explicitly via `.allow_module("scheme/red")`
9. hand-wrote `lib/srfi/160/uvprims.c` in the chibi fork (commit `945e3f6a` on fork)
   - equivalent of `uvprims.stub` — chibi-ffi can't run without a live interpreter
   - added to fork as `emesal-tein` branch (forced past .gitignore)
10. added full `srfi/160` family to VFS registry (commit `55c84dd`):
    - `srfi/160/prims` (C-backed via uvprims.c)
    - `srfi/160/base`, `srfi/160/uvector`, `srfi/160/mini`
    - 14 type-specific sub-modules: u8, s8, u16, s16, u32, s32, u64, s64,
      f8, f16, f32, f64, c64, c128
    - 2 integration tests added in `context.rs`

## current state

**724 tests pass. branch: `dev`.**

## what remains

### 1. ⚠️ shadow modules — OS-touching chibi libs

these modules touch the OS and need rust trampolines before they can be offered
in sandboxed contexts. in unsandboxed contexts they're already available natively.

**pattern to follow:** look at how `scheme/process-context` is shadowed in
`vfs_registry.rs` (the `VfsSource::Shadow` entry with inline `shadow_sld` that
re-exports via a `tein/` trampoline module).

modules needing this treatment:
- `chibi/filesystem` — stat, mkdir, readdir, rename, symlink, etc.
- `chibi/process` — exec/spawn (fork+exec, wait)
- `chibi/shell` — shell execution
- `chibi/system` — hostname, user info, getenv
- `chibi/channel` — pipes/sockets
- `chibi/net/*` — all network modules
- `chibi/temp-file` — temp file creation
- `chibi/tar` — file i/o via temp

each one needs:
1. a `tein/foo` rust module (`#[tein_module]`) that either stubs out unsafe fns
   or wraps them with policy checks
2. a `VfsSource::Shadow` entry in `vfs_registry.rs` with inline `shadow_sld`
   that imports from `tein/foo` and re-exports the safe subset

### 2. ➕ srfi/179 and srfi/231

both depend on `srfi/160` (now done). check if they have additional C deps:
```
grep 'import\|include-shared' /home/fey/forks/chibi-scheme/lib/srfi/179.sld
grep 'import\|include-shared' /home/fey/forks/chibi-scheme/lib/srfi/231.sld
```

### 3. ➕ `scheme/vector/*` sub-modules

all alias to `srfi/160` sub-modules. now that srfi/160 is done, check if they
just work as pure VfsSource::Embedded entries pointing at their `.sld` files:
```
ls /home/fey/forks/chibi-scheme/lib/scheme/vector/
grep 'import' /home/fey/forks/chibi-scheme/lib/scheme/vector/*.sld
```

### 4. ➕ remaining srfi pure-scheme modules

`docs/module-inventory.md` has the full checklist. look for entries marked `➕`
(ready to add) or re-evaluate any `❌` that might now have deps satisfied.

## key files

- `docs/module-inventory.md` — the checklist; update statuses as work progresses
- `tein/src/vfs_registry.rs` — the registry; all additions go here
- `tein/src/sandbox.rs` — includes vfs_registry.rs; has `unexported_stubs()`
- `tein/src/context.rs` — integration tests; clib tests ~line 7796
- `tein/build.rs` — validates registry; `normalise_path()` helper added ~line 170
- `/home/fey/forks/chibi-scheme/lib/` — upstream module sources

## gotchas from this work

**`default_safe` + transitive deps**: marking a module `default_safe: true` when
it has `scheme/eval` or `scheme/load` as a transitive dep will poison the safe
allowlist. `scheme/red` and `scheme/small` are `default_safe: false` for this reason.

**build.rs path normalisation**: `collect_include_files` now calls `normalise_path()`
so cross-directory `../` includes (like srfi/159 → srfi/166) resolve to canonical
paths matching VFS table keys. the `files` array should use canonical paths.

**cond-expand dead branches**: build.rs walks ALL cond-expand branches including
chibi-native ones. if a branch includes a file tein will never load (e.g.
`sha2-native.scm`), it still needs to be listed in the `files` array.

**chibi fork .gitignore**: `lib/**/*.c` is gitignored. `git add -f` is needed
to commit hand-written C files like `uvprims.c`.

**schema/red and scheme/small are in VFS** but not in the safe default set.
users must explicitly `.allow_module("scheme/red")` to use them in sandboxed contexts.
