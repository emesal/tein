# universal module availability

## problem

tein's override modules (`scheme/process-context`, `scheme/file`, `scheme/eval`, `scheme/load`, `scheme/repl`, `scheme/time`, `srfi/98`) are `VfsSource::Shadow` — only injected into the VFS during sandboxed context builds. unsandboxed contexts can't import them because nobody calls `register_vfs_shadows()`.

additionally, `chibi/filesystem` and `chibi/process` are `.stub`-based C extensions that require chibi's `dlopen` machinery (`SEXP_USE_DL=1`), which tein disables for security. unsandboxed contexts have no implementation of these modules, breaking the dep chain for `chibi/term/ansi`, `chibi/diff`, `chibi/test`, and other pure-scheme chibi modules.

result: modules that work inside the sandbox don't work outside it.

## root cause

`VfsSource::Shadow` conflates two things:

1. **tein overrides** — tein's own implementations of standard modules (always needed)
2. **safety stubs** — sandbox-only error-raising shims for dangerous modules

the override SLDs have `files: &[]` so they're not baked into the static VFS by `build.rs`. they only exist when dynamically registered at runtime — which only happens in sandbox mode.

the VFS lookup (`tein_vfs_lookup` in eval.c patch A) is called for every module resolution in all contexts. the VFS gate (`tein_module_allowed`) returns 1 (allow everything) when gate=0 (unsandboxed). the modules are simply not *in* the VFS — that's the only reason they're unfindable.

## solution

### convert override shadows to embedded modules

7 tein override modules and 2 chibi compatibility modules (9 total) become `VfsSource::Embedded` with real `.sld` files committed to the chibi fork (`emesal/chibi-scheme`, branch `emesal-tein`). they're baked into the static VFS at build time and available in all contexts. the chibi fork's original `.sld` files for these modules are **overwritten** — the originals depend on dlopen-based C extensions that tein can't load.

| module | current state | new state | SLD body |
|--------|---------------|-----------|----------|
| `scheme/load` | shadow only | embedded | re-exports `(tein load)` + wraps `tein-environment-internal`. exports `environment` (tein extension — r7rs puts this in `scheme/eval` only) |
| `scheme/eval` | shadow only | embedded | re-exports `eval` from `(chibi)` + wraps `tein-environment-internal` |
| `scheme/repl` | shadow only | embedded | wraps `tein-interaction-environment-internal` |
| `scheme/process-context` | shadow only | embedded | re-exports from `(tein process)` |
| `scheme/file` | shadow only | embedded | `(chibi)` opcodes + `(tein filesystem)` for `delete-file`/`file-exists?` |
| `scheme/time` | shadow + broken embedded | embedded only | re-exports `(tein time)` |
| `srfi/98` | shadow + C clib embedded | embedded only | re-exports from `(tein process)` |
| `chibi/filesystem` | shadow (generated stub) | embedded | re-exports from `(tein filesystem)` |
| `chibi/process` | shadow (generated stub) | embedded | re-exports from `(tein process)` |

**removals**: the broken `scheme/time` embedded entry (chibi's original depending on `scheme/time/tai-to-utc-offset`) and the `srfi/98` C clib entry (`lib/srfi/98/env.c`) are removed.

**behavioural change for unsandboxed contexts**: `exit`/`emergency-exit` go from "unavailable" to "working" (exit escape hatch with dynamic-wind cleanup). `get-environment-variable` goes from "unavailable" to returning real env vars. this is the desired behaviour.

sandbox behaviour is unchanged — the VFS gate controls which modules are importable. the trampolines behind these modules check `IS_SANDBOXED` and neuter behaviour accordingly.

**note on `scheme/file`**: the current shadow imports `(only (chibi filesystem) delete-file file-exists?)`. the new embedded version imports `(tein filesystem)` directly instead of going through `(chibi filesystem)`, avoiding unnecessary indirection through the re-export SLD.

### new `(tein filesystem)` module

rust implementations of the `chibi/filesystem` functions needed by the real-world dep chain.

follows the `(tein time)` pattern: `VfsSource::Embedded` with `.sld`/`.scm` files in the chibi fork. the `.scm` is comment-only; all functions are native trampolines registered via `#[tein_module("tein/filesystem")]` in `src/filesystem.rs`, which overwrites the scheme-level stubs at module load time.

**real implementations** (via `std::fs` / `std::path`):

- `file-exists?` — already exists as trampoline
- `delete-file` — already exists as trampoline
- `file-directory?` — `Path::is_dir()`
- `file-regular?` — `Path::is_file()`
- `file-link?` — `Path::is_symlink()` (chibi name: `file-link?`)
- `file-size` — `metadata().len()`
- `directory-files` — `read_dir()`
- `create-directory` — `create_dir()`
- `delete-directory` — `remove_dir()`
- `rename-file` — `rename()`
- `current-directory` — `current_dir()`

**deferred** (raise "not implemented" error with informative message):

all remaining `chibi/filesystem` exports not in the real list above. the canonical export list is the `SHADOW_STUBS` entry for `chibi/filesystem` in `vfs_registry.rs` — every function, constant, and macro export listed there that isn't implemented as a real function becomes a deferred stub. this includes fd operations, low-level `open` with POSIX flags, stat accessors, permission ops, lock ops, pipe/fifo, link/symlink management, and all associated constants.

the `chibi/filesystem` re-export SLD exports everything from `(tein filesystem)` — both real and deferred functions. consumers that call deferred functions get a clear error at call time, not at import time.

**sandbox integration**: all real functions check `IS_SANDBOXED` + `FsPolicy`. read operations (`file-exists?`, `file-directory?`, `directory-files`, etc.) check read policy. write operations (`delete-file`, `create-directory`, `rename-file`, etc.) check write policy.

### extend `(tein process)` with process-spawning functions

new functions added alongside existing `exit`, `emergency-exit`, `get-environment-variable`, `get-environment-variables`, `command-line`. registered as additional `define_fn_variadic` calls in the existing registration block in `context.rs`. the `.sld` exports list in the chibi fork is updated.

**real implementations**:

- `current-process-id` — `std::process::id()`
- `system` — `std::process::Command` with shell (`/bin/sh -c` on unix, `cmd /C` on windows), returns exit status as integer
- `call-with-process-io` — `std::process::Command` with piped stdin/stdout/stderr, calls scheme proc with 4 args (pid, input-port, output-port, error-port)

**deferred** (raise "not implemented"): all remaining `chibi/process` exports not in the real list above. the canonical export list is the `SHADOW_STUBS` entry for `chibi/process` in `vfs_registry.rs`. includes `fork`, `execute` (exec-replaces host process — incompatible with embedded interpreter), `waitpid`, `process-id`, `open-pipe`, `sleep`, `alarm`, `parent-process-id`, `process->string`, `process->bytevector`, `process->sexp`, `process->string-list`, `process->output+error`, `process->output+error+status`, signal functions (`make-signal-handler`, `set-signal-handler!`, `set-signal-action!`, `kill`, signal constants, signal set ops), `process-group-id`, `set-process-group-id!`, and all signal constants.

**sandbox behaviour** (precise):
- `current-process-id` — returns `0` when `IS_SANDBOXED`
- `system` — raises error when `IS_SANDBOXED`
- `call-with-process-io` — raises error when `IS_SANDBOXED`

the `chibi/process` re-export SLD exports everything from `(tein process)` — both real and deferred functions.

### dep chain resolution

after these changes, the following pure-scheme modules work without modification in both sandboxed and unsandboxed contexts:

```
scheme/process-context  ← re-exports (tein process) ✓
scheme/file             ← (chibi) opcodes + (tein filesystem) ✓
scheme/eval             ← (chibi) + tein trampoline ✓
scheme/load             ← (tein load) + tein trampoline ✓
scheme/repl             ← tein trampoline ✓
scheme/time             ← (tein time) ✓
srfi/98                 ← (tein process) ✓
chibi/filesystem        ← (tein filesystem) ✓
chibi/process           ← (tein process) ✓
chibi/term/ansi         ← (scheme process-context) ✓
chibi/diff              ← (chibi term ansi) ✓
```

**out of scope** (stay as sandbox-only safety stubs or remain broken in unsandboxed):
- `chibi/shell` — needs `fork`, `open-pipe`, `duplicate-file-descriptor-to` (all deferred)
- `chibi/temp-file` — needs `open` with POSIX flag constants (deferred)
- `chibi/config` — needs `(meta)` module which is not in the VFS registry
- `chibi/log` — needs `chibi/system` (no tein implementation)
- `chibi/system`, `chibi/stty`, `chibi/net/*` — no tein implementation

### registry fix: `chibi/term/ansi` deps

pre-existing bug: `chibi/term/ansi`'s VFS registry entry lists deps as `["scheme/base"]` only, but the actual SLD also imports `(scheme write)` and `(scheme process-context)`. fix as part of this work to ensure transitive dep resolution is correct.

### what gets removed

- 7 shadow SLD entries for tein overrides: `scheme/process-context`, `scheme/eval`, `scheme/repl`, `scheme/load`, `scheme/file`, `scheme/time`, `srfi/98`
- 2 shadow stub entries promoted to embedded re-exports: `chibi/filesystem`, `chibi/process`
- broken embedded `scheme/time` entry (chibi's original) including its `ClibEntry` (`lib/scheme/time.c`)
- embedded `srfi/98` C clib entry (`lib/srfi/98/env.c`)
- corresponding `SHADOW_STUBS` entries for `chibi/filesystem` and `chibi/process` in `vfs_registry.rs`

### what stays

- `register_vfs_shadows()` — still needed for remaining safety stubs (`chibi/stty`, `chibi/system`, `chibi/shell`, `chibi/temp-file`, `chibi/net/*`, etc.)
- `GENERATED_SHADOW_SLDS` — still generated for those stubs
- `with_vfs_shadows()` builder method — still useful for test harness contexts that want safety stubs available

## implementation structure

### new rust files
- `src/filesystem.rs` — `#[tein_module("tein/filesystem")]`, follows `(tein time)` pattern (Embedded + native overwrite)

### modified rust files
- `src/lib.rs` — add `mod filesystem;`
- `src/context.rs` — register `tein/filesystem` module, add new `(tein process)` trampolines via `define_fn_variadic`
- `src/vfs_registry.rs` — convert 9 entries from Shadow to Embedded, remove broken `scheme/time` Embedded, remove `srfi/98` C clib, remove `chibi/filesystem` and `chibi/process` from `SHADOW_STUBS`, fix `chibi/term/ansi` deps, update `chibi/filesystem` and `chibi/process` deps to point at `tein/filesystem` and `tein/process`

### chibi fork files (pushed to `emesal/chibi-scheme`, branch `emesal-tein`)

all are overwrites of existing chibi files:

- `lib/tein/filesystem.sld` + `lib/tein/filesystem.scm` — new `(tein filesystem)` module
- `lib/tein/process.sld` — updated exports list (existing file)
- `lib/scheme/process-context.sld` — overwrite: re-export from `(tein process)`
- `lib/scheme/file.sld` — overwrite: `(chibi)` opcodes + `(tein filesystem)`
- `lib/scheme/eval.sld` — overwrite: `(chibi)` eval + `tein-environment-internal`
- `lib/scheme/load.sld` — overwrite: `(tein load)` + `tein-environment-internal`
- `lib/scheme/repl.sld` — overwrite: `tein-interaction-environment-internal`
- `lib/scheme/time.sld` — overwrite: re-export from `(tein time)`
- `lib/srfi/98.sld` — overwrite: re-export from `(tein process)`
- `lib/chibi/filesystem.sld` — overwrite: re-export from `(tein filesystem)`
- `lib/chibi/process.sld` — overwrite: re-export from `(tein process)`

## testing

1. **unsandboxed availability** — import all 9 converted modules in a `standard_env()` context, verify they load and return real values (real env vars, real file ops, real time, real PID)
2. **sandboxed behaviour** — same imports in `sandboxed(Modules::Safe)`, verify env vars neutered, file ops gated, process spawning blocked, `current-process-id` returns 0
3. **dep chain** — `(import (chibi diff))` in an unsandboxed context (the original failure case)
4. **`(tein filesystem)` functions** — unit tests for each real function, both sandboxed (FsPolicy) and unsandboxed
5. **deferred function errors** — calling a deferred function raises a clear "not implemented" error
6. **`(tein process)` new functions** — `current-process-id` returns positive integer, `system` runs a command, `call-with-process-io` captures output
7. **existing `vfs_module_tests`** — full chibi/srfi suite still passes
8. **`chibi_diff` un-ignore** — try un-ignoring; re-ignore if CI env lacks `TERM`
9. **`with_vfs_shadows()`** — remaining safety stubs still work for test harness contexts
10. **`scheme/process-context` exit** — verify `exit`/`emergency-exit` work correctly in unsandboxed mode (returns `Value::Exit`)
