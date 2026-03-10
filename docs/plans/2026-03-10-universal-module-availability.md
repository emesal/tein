# universal module availability implementation plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** make tein's override modules (scheme/process-context, scheme/file, etc.) and chibi compatibility modules (chibi/filesystem, chibi/process) available in all contexts — not just sandboxed ones — by converting them from VfsSource::Shadow to VfsSource::Embedded with real .sld files.

**Architecture:** override shadow modules become Embedded with .sld files committed to the chibi fork. new `(tein filesystem)` rust module provides filesystem ops. `(tein process)` is extended with process-spawning functions. `chibi/filesystem` and `chibi/process` become re-export SLDs. sandbox behaviour unchanged — trampolines check IS_SANDBOXED.

**Tech Stack:** rust (tein crate), scheme (.sld/.scm in chibi fork), chibi-scheme C patches (existing)

**Spec:** `docs/plans/2026-03-10-universal-module-availability-design.md`

---

## chunk 1: chibi fork SLD files + VFS registry conversion

### task 1: create `(tein filesystem)` SLD and SCM in chibi fork

the new module follows the `(tein time)` pattern — .sld declares exports, .scm is comment-only (plus constants), native fns registered by rust at context init.

**Files:**
- Create: `~/forks/chibi-scheme/lib/tein/filesystem.sld`
- Create: `~/forks/chibi-scheme/lib/tein/filesystem.scm`

- [ ] **step 1: create the .sld file**

the export list includes all functions from the SHADOW_STUBS `chibi/filesystem` entry in `vfs_registry.rs:4035-4069` plus all constants. every function and constant that `chibi/filesystem` exports, `(tein filesystem)` also exports.

```scheme
(define-library (tein filesystem)
  (import (scheme base))
  (export
    ;; real implementations (rust trampolines)
    file-exists? delete-file
    file-directory? file-regular? file-link?
    file-size directory-files
    create-directory delete-directory
    rename-file current-directory
    ;; deferred (raise "not implemented")
    duplicate-file-descriptor duplicate-file-descriptor-to
    close-file-descriptor renumber-file-descriptor
    open-input-file-descriptor open-output-file-descriptor
    link-file symbolic-link-file read-link
    directory-fold directory-fold-tree
    delete-file-hierarchy create-directory*
    change-directory with-directory
    open open-pipe make-fifo open-output-file/append
    file-status file-link-status
    file-device file-inode file-mode file-num-links
    file-owner file-group file-represented-device
    file-block-size file-num-blocks
    file-access-time file-change-time
    file-modification-time file-modification-time/safe
    file-character? file-block? file-fifo? file-socket?
    get-file-descriptor-flags set-file-descriptor-flags!
    get-file-descriptor-status set-file-descriptor-status!
    file-lock file-truncate
    file-is-readable? file-is-writable? file-is-executable?
    file-permissions set-file-permissions!
    chmod chown is-a-tty?
    ;; constants
    open/read open/write open/read-write
    open/create open/exclusive open/truncate
    open/append open/non-block
    lock/shared lock/exclusive lock/non-blocking lock/unlock)
  (include "filesystem.scm"))
```

note: `file-size` appears once (the real implementation). the SHADOW_STUBS entry's `file-size` and our real `file-size` are the same export.

- [ ] **step 2: create the .scm file**

```scheme
;; (tein filesystem) — native rust implementations are registered by the
;; tein rust layer (register_module_tein_filesystem) before this library
;; is first imported. chibi resolves the native fn exports via env
;; parent-chain lookup (localp=0). deferred functions raise "not
;; implemented" errors at call time.

;; constants — defined here since #[tein_const] emits scheme defines.
;; values match POSIX O_* / LOCK_* flags (informational only — the
;; low-level `open` function that uses these is deferred).
(define open/read 0)
(define open/write 1)
(define open/read-write 2)
(define open/create #x40)
(define open/exclusive #x80)
(define open/truncate #x200)
(define open/append #x400)
(define open/non-block #x800)
(define lock/shared 1)
(define lock/exclusive 2)
(define lock/non-blocking 4)
(define lock/unlock 8)
```

- [ ] **step 3: push to chibi fork**

```bash
cd ~/forks/chibi-scheme
git add lib/tein/filesystem.sld lib/tein/filesystem.scm
git commit -m "feat: add (tein filesystem) module sld/scm for tein embedding"
git push origin emesal-tein
```

### task 2: overwrite r7rs SLD files in chibi fork

these replace chibi's originals that depend on dlopen-based C extensions.

**Files:**
- Modify: `~/forks/chibi-scheme/lib/scheme/process-context.sld`
- Modify: `~/forks/chibi-scheme/lib/scheme/eval.sld`
- Modify: `~/forks/chibi-scheme/lib/scheme/load.sld`
- Modify: `~/forks/chibi-scheme/lib/scheme/repl.sld`
- Modify: `~/forks/chibi-scheme/lib/scheme/file.sld`
- Modify: `~/forks/chibi-scheme/lib/scheme/time.sld`
- Modify: `~/forks/chibi-scheme/lib/srfi/98.sld`

- [ ] **step 1: overwrite scheme/process-context.sld**

```scheme
;; tein override: re-exports from (tein process) which provides
;; sandbox-aware trampolines for all r7rs process-context bindings.

(define-library (scheme process-context)
  (import (tein process))
  (export get-environment-variable get-environment-variables
          command-line exit emergency-exit))
```

- [ ] **step 2: overwrite scheme/eval.sld**

same content as current shadow SLD (`vfs_registry.rs:897-904`):

```scheme
(define-library (scheme eval)
  (import (chibi))
  (export eval environment)
  (begin
    (define (environment . specs)
      (apply tein-environment-internal specs))))
```

- [ ] **step 3: overwrite scheme/load.sld**

same content as current shadow SLD (`vfs_registry.rs:502-508`):

```scheme
(define-library (scheme load)
  (import (tein load) (chibi))
  (export load environment)
  (begin
    (define (environment . specs)
      (apply tein-environment-internal specs))))
```

- [ ] **step 4: overwrite scheme/repl.sld**

same content as current shadow SLD (`vfs_registry.rs:919-926`):

```scheme
(define-library (scheme repl)
  (import (chibi))
  (export interaction-environment)
  (begin
    (define (interaction-environment)
      (tein-interaction-environment-internal))))
```

- [ ] **step 5: overwrite scheme/file.sld**

updated to import from `(tein filesystem)` directly:

```scheme
;; tein override: file IO opcodes from (chibi) + delete-file/file-exists?
;; from (tein filesystem). FsPolicy enforcement at C opcode level.

(define-library (scheme file)
  (import (chibi) (only (tein filesystem) delete-file file-exists?))
  (export file-exists? delete-file
          open-input-file open-binary-input-file
          open-output-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file))
```

- [ ] **step 6: overwrite scheme/time.sld**

```scheme
;; tein override: re-exports from (tein time) which provides
;; rust implementations of r7rs time primitives.

(define-library (scheme time)
  (import (tein time))
  (export current-second current-jiffy jiffies-per-second))
```

- [ ] **step 7: overwrite srfi/98.sld**

```scheme
;; tein override: re-exports env var access from (tein process)
;; which provides sandbox-aware trampolines.

(define-library (srfi 98)
  (import (only (tein process)
                get-environment-variable
                get-environment-variables))
  (export get-environment-variable get-environment-variables))
```

- [ ] **step 8: commit and push**

```bash
cd ~/forks/chibi-scheme
git add lib/scheme/process-context.sld lib/scheme/eval.sld lib/scheme/load.sld \
        lib/scheme/repl.sld lib/scheme/file.sld lib/scheme/time.sld lib/srfi/98.sld
git commit -m "feat: overwrite r7rs sld files with tein re-export wrappers

scheme/process-context, scheme/eval, scheme/load, scheme/repl,
scheme/file, scheme/time, srfi/98 now re-export from tein modules
instead of depending on chibi's dlopen-based C extensions."
git push origin emesal-tein
```

### task 3: overwrite chibi/filesystem.sld and chibi/process.sld in fork

**Files:**
- Modify: `~/forks/chibi-scheme/lib/chibi/filesystem.sld`
- Modify: `~/forks/chibi-scheme/lib/chibi/process.sld`
- Modify: `~/forks/chibi-scheme/lib/tein/process.sld`
- Modify: `~/forks/chibi-scheme/lib/tein/process.scm`

- [ ] **step 1: overwrite chibi/filesystem.sld**

```scheme
;; tein override: re-exports from (tein filesystem) which provides
;; rust implementations of filesystem operations.

(define-library (chibi filesystem)
  (import (tein filesystem))
  (export
    file-exists? delete-file
    file-directory? file-regular? file-link?
    file-size directory-files
    create-directory delete-directory
    rename-file current-directory
    duplicate-file-descriptor duplicate-file-descriptor-to
    close-file-descriptor renumber-file-descriptor
    open-input-file-descriptor open-output-file-descriptor
    link-file symbolic-link-file read-link
    directory-fold directory-fold-tree
    delete-file-hierarchy create-directory*
    change-directory with-directory
    open open-pipe make-fifo open-output-file/append
    file-status file-link-status
    file-device file-inode file-mode file-num-links
    file-owner file-group file-represented-device
    file-block-size file-num-blocks
    file-access-time file-change-time
    file-modification-time file-modification-time/safe
    file-character? file-block? file-fifo? file-socket?
    get-file-descriptor-flags set-file-descriptor-flags!
    get-file-descriptor-status set-file-descriptor-status!
    file-lock file-truncate
    file-is-readable? file-is-writable? file-is-executable?
    file-permissions set-file-permissions!
    chmod chown is-a-tty?
    open/read open/write open/read-write
    open/create open/exclusive open/truncate
    open/append open/non-block
    lock/shared lock/exclusive lock/non-blocking lock/unlock))
```

- [ ] **step 2: overwrite chibi/process.sld**

```scheme
;; tein override: re-exports from (tein process) which provides
;; rust implementations of process operations.

(define-library (chibi process)
  (import (tein process))
  (export
    exit emergency-exit
    get-environment-variable get-environment-variables command-line
    current-process-id system call-with-process-io
    sleep alarm %fork fork kill execute
    waitpid system?
    process-command-line process-running?
    set-signal-action!
    make-signal-set signal-set? signal-set-contains?
    signal-set-fill! signal-set-add! signal-set-delete!
    current-signal-mask parent-process-id
    signal-mask-block! signal-mask-unblock! signal-mask-set!
    process->bytevector process->string process->sexp
    process->string-list
    process->output+error process->output+error+status
    signal/hang-up signal/interrupt signal/quit
    signal/illegal signal/abort signal/fpe
    signal/kill signal/segv signal/pipe
    signal/alarm signal/term
    signal/user1 signal/user2
    signal/child signal/continue signal/stop
    signal/tty-stop signal/tty-input signal/tty-output
    wait/no-hang))
```

- [ ] **step 3: update tein/process.sld exports**

read the current `lib/tein/process.sld` and add the new exports. the full export list must include everything that `chibi/process` expects to re-export.

- [ ] **step 4: update tein/process.scm with deferred stubs and constants**

read the current `lib/tein/process.scm`. add deferred stub definitions AFTER the existing exit/emergency-exit code. add signal constants. each deferred function is a variadic scheme procedure that raises an error with the function name:

```scheme
;; --- deferred: not implemented ---
(define (sleep . args) (error "not implemented" "sleep"))
(define (alarm . args) (error "not implemented" "alarm"))
;; ... (all deferred functions from spec)

;; signal constants (standard POSIX values)
(define signal/hang-up 1)
;; ... (all signal constants)
(define wait/no-hang 1)
```

the full list of deferred functions and constants comes from the `SHADOW_STUBS` entry for `chibi/process` in `vfs_registry.rs:4071-4099`.

- [ ] **step 5: commit and push**

```bash
cd ~/forks/chibi-scheme
git add lib/chibi/filesystem.sld lib/chibi/process.sld \
        lib/tein/process.sld lib/tein/process.scm
git commit -m "feat: chibi/filesystem + chibi/process re-export from tein modules

chibi/filesystem re-exports (tein filesystem), chibi/process re-exports
(tein process). deferred functions raise 'not implemented' errors.
signal constants defined with standard POSIX values."
git push origin emesal-tein
```

### task 4: update VFS registry

**Files:**
- Modify: `tein/src/vfs_registry.rs`

- [ ] **step 1: add `tein/filesystem` Embedded entry**

add near the other tein modules (after `tein/process` entry, around line 242):

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

- [ ] **step 2: convert scheme/process-context from Shadow to Embedded**

find the entry at ~line 937. change:
- `files: &[]` -> `files: &["lib/scheme/process-context.sld"]`
- `source: VfsSource::Shadow` -> `source: VfsSource::Embedded`
- `shadow_sld: Some(...)` -> `shadow_sld: None`
- `deps: &["scheme/base"]` -> `deps: &["tein/process"]`
- update/remove the comment above

- [ ] **step 3: convert scheme/eval from Shadow to Embedded**

find the entry at ~line 890. change:
- `files: &[]` -> `files: &["lib/scheme/eval.sld"]`
- `source: VfsSource::Shadow` -> `source: VfsSource::Embedded`
- `shadow_sld: Some(...)` -> `shadow_sld: None`
- keep deps as `&[]` (imports from (chibi) which is always available)

- [ ] **step 4: convert scheme/load from Shadow to Embedded**

find the entry at ~line 494. change:
- `files: &[]` -> `files: &["lib/scheme/load.sld"]`
- `source: VfsSource::Shadow` -> `source: VfsSource::Embedded`
- `shadow_sld: Some(...)` -> `shadow_sld: None`
- keep deps `&["tein/load"]`

- [ ] **step 5: convert scheme/repl from Shadow to Embedded**

find the entry at ~line 911. change:
- `files: &[]` -> `files: &["lib/scheme/repl.sld"]`
- `source: VfsSource::Shadow` -> `source: VfsSource::Embedded`
- `shadow_sld: Some(...)` -> `shadow_sld: None`

- [ ] **step 6: convert scheme/file from Shadow to Embedded**

find the entry at ~line 867. change:
- `files: &[]` -> `files: &["lib/scheme/file.sld"]`
- `source: VfsSource::Shadow` -> `source: VfsSource::Embedded`
- `shadow_sld: Some(...)` -> `shadow_sld: None`
- `deps: &["chibi/filesystem"]` -> `deps: &["tein/filesystem"]`

- [ ] **step 7: replace scheme/time — remove broken Embedded, convert Shadow to Embedded**

remove the Embedded entry at ~line 964 entirely (the one with `deps: &["scheme/time/tai", "scheme/time/tai-to-utc-offset"]` and `clib: Some(ClibEntry { source: "lib/scheme/time.c", ... })`).

find the Shadow entry at ~line 983. change:
- `files: &[]` -> `files: &["lib/scheme/time.sld"]`
- `source: VfsSource::Shadow` -> `source: VfsSource::Embedded`
- `shadow_sld: Some(...)` -> `shadow_sld: None`
- keep deps `&["tein/time"]` and feature `Some("time")`

- [ ] **step 8: replace srfi/98 — remove C clib Embedded, convert Shadow to Embedded**

remove the Embedded entry at ~line 1275 entirely (the one with `clib: Some(ClibEntry { source: "lib/srfi/98/env.c", ... })`).

find the Shadow entry at ~line 1293. change:
- `files: &[]` -> `files: &["lib/srfi/98.sld"]`
- `source: VfsSource::Shadow` -> `source: VfsSource::Embedded`
- `shadow_sld: Some(...)` -> `shadow_sld: None`
- `deps: &[]` -> `deps: &["tein/process"]`

- [ ] **step 9: convert chibi/filesystem from Shadow to Embedded**

find the entry at ~line 2133. change:
- `files: &[]` -> `files: &["lib/chibi/filesystem.sld"]`
- `source: VfsSource::Shadow` -> `source: VfsSource::Embedded`
- `deps: &["scheme/base"]` -> `deps: &["tein/filesystem"]`

- [ ] **step 10: convert chibi/process from Shadow to Embedded**

find the entry at ~line 2143. change:
- `files: &[]` -> `files: &["lib/chibi/process.sld"]`
- `source: VfsSource::Shadow` -> `source: VfsSource::Embedded`
- `deps: &["scheme/base"]` -> `deps: &["tein/process"]`

- [ ] **step 11: remove chibi/filesystem and chibi/process from SHADOW_STUBS**

find the `SHADOW_STUBS` array at ~line 4033. remove the `ShadowStub` entries for `"chibi/filesystem"` and `"chibi/process"`. the remaining stubs (`chibi/system`, `chibi/shell`, etc.) stay.

- [ ] **step 12: fix chibi/term/ansi deps**

find the `chibi/term/ansi` entry at ~line 3032. change:
- `deps: &["scheme/base"]` -> `deps: &["scheme/base", "scheme/write", "scheme/process-context"]`

- [ ] **step 13: verify build compiles**

```bash
cd ~/projects/tein
just clean && cargo build
```

expected: build succeeds. the chibi fork is re-fetched with the new SLD files. the VFS registry changes compile.

- [ ] **step 14: commit**

```bash
git add tein/src/vfs_registry.rs
git commit -m "refactor: convert 9 shadow modules to embedded, fix deps

- scheme/process-context, scheme/eval, scheme/load, scheme/repl,
  scheme/file, scheme/time, srfi/98: Shadow -> Embedded
- chibi/filesystem, chibi/process: Shadow stub -> Embedded re-export
- remove broken scheme/time Embedded + ClibEntry
- remove srfi/98 C clib entry
- remove chibi/filesystem + chibi/process from SHADOW_STUBS
- fix chibi/term/ansi deps (add scheme/write, scheme/process-context)"
```

## chunk 2: `(tein filesystem)` rust module

### task 5: implement `src/filesystem.rs` — real functions

**Files:**
- Create: `tein/src/filesystem.rs`
- Modify: `tein/src/lib.rs`
- Modify: `tein/src/context.rs`

- [ ] **step 1: add mod declaration to lib.rs**

add after the existing `mod time;` line (~line 80-81):

```rust
mod filesystem;
```

no feature gate — filesystem is always available.

- [ ] **step 2: write filesystem.rs — module structure and real implementations**

follow the `(tein time)` pattern. use `#[tein_module("tein/filesystem")]` attribute. the module inner name should be `filesystem_impl`.

key patterns:
- use `crate::context::{FsAccess, check_fs_access}` for sandbox policy checks
- `#[tein_fn(name = "...")]` for each function
- `Result<T, String>` return type for error propagation
- `Value` return for functions returning lists (like `directory-files`)

see spec for the full list of 11 real functions + their implementations.

helper functions `check_read(path)` and `check_write(path)` wrap `check_fs_access` and return `Option<String>` (None = allowed, Some(msg) = denied).

- [ ] **step 3: register the module in context.rs**

find the module registration block at ~line 2610. add BEFORE the existing `register_file_module()` call:

```rust
if self.standard_env {
    crate::filesystem::filesystem_impl::register_module_tein_filesystem(&context)?;
}
```

- [ ] **step 4: verify build**

```bash
cargo build -p tein
```

- [ ] **step 5: write tests for real filesystem functions**

add tests in `tein/src/context.rs` (test module). test each real function: `file-exists?`, `file-directory?`, `directory-files`, `current-directory`, `create-directory`+`delete-directory` roundtrip.

- [ ] **step 6: run tests**

```bash
cargo test -p tein test_tein_filesystem -- --nocapture
```

- [ ] **step 7: commit**

```bash
git add tein/src/filesystem.rs tein/src/lib.rs tein/src/context.rs
git commit -m "feat: add (tein filesystem) module with real fs implementations

file-exists?, delete-file, file-directory?, file-regular?, file-link?,
file-size, directory-files, create-directory, delete-directory,
rename-file, current-directory. sandbox-aware via FsPolicy."
```

### task 6: remove old file-exists?/delete-file trampolines + update `(tein file)`

the old hand-written trampolines in `context.rs` (`file_exists_trampoline`, `delete_file_trampoline`, `register_file_module`) are superseded by `(tein filesystem)`.

**critical**: `(tein file)` currently gets `file-exists?` and `delete-file` via env parent-chain lookup — the old trampolines are registered into the top-level env by `register_file_module()`, and `(tein file)` picks them up through `(import (chibi))`. removing the trampolines breaks `(tein file)` unless we update it to import from `(tein filesystem)` directly.

**Files:**
- Modify: `tein/src/context.rs`
- Modify: `~/forks/chibi-scheme/lib/tein/file.sld`
- Modify: `~/forks/chibi-scheme/lib/tein/file.scm`
- Modify: `tein/src/vfs_registry.rs` (tein/file deps)

- [ ] **step 1: update `lib/tein/file.sld` in chibi fork**

```scheme
(define-library (tein file)
  (import (chibi) (only (tein filesystem) file-exists? delete-file))
  (export file-exists? delete-file
          open-input-file open-binary-input-file
          open-output-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file)
  (include "file.scm"))
```

- [ ] **step 2: update `lib/tein/file.scm` comments in chibi fork**

update the comment block at the top to reflect that `file-exists?` and `delete-file` now come from `(tein filesystem)` instead of `register_file_module()` trampolines.

- [ ] **step 3: push chibi fork changes**

```bash
cd ~/forks/chibi-scheme
git add lib/tein/file.sld lib/tein/file.scm
git commit -m "refactor: (tein file) imports file-exists?/delete-file from (tein filesystem)

previously relied on env parent-chain lookup from top-level trampolines
registered by register_file_module(). now imports directly from the
new (tein filesystem) module."
git push origin emesal-tein
```

- [ ] **step 4: update `tein/file` VFS registry deps**

in `tein/src/vfs_registry.rs`, find the `tein/file` entry (~line 210). change:
- `deps: &["scheme/base"]` -> `deps: &["scheme/base", "tein/filesystem"]`

- [ ] **step 5: remove trampoline functions and registration in context.rs**

remove `file_exists_trampoline` (~lines 1132-1156), `delete_file_trampoline` (~lines 1162-1189), `register_file_module` (~lines 4075-4079), and the call `context.register_file_module()?;` at ~line 2611.

- [ ] **step 6: verify build and full test suite**

```bash
just clean && cargo build -p tein && just test
```

note: `just clean` needed because chibi fork changes require re-fetch.

- [ ] **step 7: commit**

```bash
git add tein/src/context.rs tein/src/vfs_registry.rs
git commit -m "refactor: remove old file-exists?/delete-file trampolines

superseded by (tein filesystem) module. (tein file) now imports from
(tein filesystem) directly. file-exists? and delete-file are now
#[tein_fn] functions in src/filesystem.rs."
```

## chunk 3: extend `(tein process)` + integration tests

### task 7: add new process functions to `(tein process)`

**Files:**
- Modify: `tein/src/context.rs`

- [ ] **step 1: add `current_process_id_trampoline`**

add near the existing process trampolines (~after line 1831). returns `std::process::id()` as fixnum, or 0 when `IS_SANDBOXED`.

- [ ] **step 2: add `system_trampoline`**

takes one string arg, runs via `std::process::Command::new("/bin/sh").arg("-c").arg(&cmd).status()`, returns exit code as fixnum. raises error when `IS_SANDBOXED`.

note: `call-with-process-io` is complex (needs custom ports for stdin/stdout/stderr piping). defer to a follow-up task — `system` and `current-process-id` cover the immediate needs. the scheme-level stub from `tein/process.scm` will handle the "not implemented" error for now.

- [ ] **step 3: register new trampolines**

add to `register_process_module()`:
```rust
self.define_fn_variadic("current-process-id", current_process_id_trampoline)?;
self.define_fn_variadic("system", system_trampoline)?;
```

- [ ] **step 4: write tests**

test `current-process-id` returns positive integer unsandboxed, 0 sandboxed. test `system "true"` returns 0 unsandboxed, error sandboxed.

- [ ] **step 5: run tests**

```bash
cargo test -p tein test_current_process_id test_system -- --nocapture
```

- [ ] **step 6: commit**

```bash
git add tein/src/context.rs
git commit -m "feat: add current-process-id and system to (tein process)

current-process-id returns real PID unsandboxed, 0 sandboxed.
system runs shell commands via /bin/sh -c, blocked in sandbox."
```

### task 8: integration tests — unsandboxed module availability

**Files:**
- Modify: `tein/src/context.rs` (test section)
- Modify: `tein/tests/vfs_module_tests.rs`

- [ ] **step 1: test r7rs modules importable unsandboxed**

tests for: `scheme/process-context` (real env vars), `scheme/eval` (eval works), `scheme/time` (positive current-second), `srfi/98` (real env vars), `chibi/filesystem` (file-exists? works), `chibi/process` (current-process-id > 0).

- [ ] **step 2: test the original failure case — `(chibi diff)` unsandboxed**

```rust
#[test]
fn test_unsandboxed_chibi_diff() {
    // this was the original failure: (chibi diff) -> (chibi term ansi) ->
    // (scheme process-context) which didn't exist unsandboxed
    let ctx = Context::builder()
        .standard_env()
        .step_limit(10_000_000)
        .build()
        .expect("build");
    let r = ctx.evaluate_to_string("(import (chibi diff)) (diff \"hello\" \"world\")");
    assert!(r.is_ok(), "chibi/diff should import and run: {:?}", r);
}
```

- [ ] **step 3: test deferred function errors**

verify importing `(chibi filesystem)` works but calling `(make-fifo "/tmp/x")` raises "not implemented".

- [ ] **step 4: run all tests**

```bash
cargo test -p tein test_unsandboxed test_deferred -- --nocapture
```

- [ ] **step 5: run full test suite**

```bash
just test
```

- [ ] **step 6: try un-ignoring chibi_diff test**

in `tests/vfs_module_tests.rs`, remove `#[ignore]` from `test_chibi_diff`. run:

```bash
cargo test -p tein --test vfs_module_tests test_chibi_diff -- --nocapture
```

if it passes, keep un-ignored. if it fails (CI may lack `TERM`), re-add `#[ignore]`.

- [ ] **step 7: lint**

```bash
just lint
```

- [ ] **step 8: commit**

```bash
git add tein/src/context.rs tein/tests/vfs_module_tests.rs
git commit -m "test: unsandboxed module availability + chibi/diff integration

verify scheme/process-context, scheme/eval, scheme/time, srfi/98,
chibi/filesystem, chibi/process importable in unsandboxed contexts.
verify (chibi diff) imports and runs (original failure case).
verify deferred functions raise 'not implemented' errors."
```

### task 9: update AGENTS.md and docs

**Files:**
- Modify: `AGENTS.md`

- [ ] **step 1: update AGENTS.md**

update the sandboxing flow description to note that override modules are now Embedded. update `VfsSource::Shadow` references. add `(tein filesystem)` to the architecture section. update `chibi/filesystem` and `chibi/process` descriptions.

- [ ] **step 2: update comments referencing old shadow behaviour**

grep for references to `scheme/process-context` being shadow-only. update comments in `context.rs`, `sandbox.rs` that mention the old pattern.

- [ ] **step 3: lint and final test run**

```bash
just lint && just test
```

- [ ] **step 4: commit**

```bash
git add AGENTS.md tein/src/context.rs tein/src/sandbox.rs
git commit -m "docs: update AGENTS.md for universal module availability

override modules are now Embedded, not Shadow. (tein filesystem)
added to architecture section. chibi/filesystem and chibi/process
are re-export SLDs."
```
