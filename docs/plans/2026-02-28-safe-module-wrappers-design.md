# safe module wrappers: (tein file), (tein load), (tein process)

issue: #87

## motivation

`scheme/file`, `scheme/load`, and `scheme/process-context` are excluded from
`SAFE_MODULES` because they expose filesystem and process access. tein already
has the machinery to make safe versions — this design covers implementing them
as `(tein ...)` modules.

## approach

scheme-heavy: maximise scheme-level wrappers, minimise new rust trampolines.
scheme does what scheme does well (composition, dynamic-wind), rust handles
policy checks at the boundary.

## SAFE_MODULES change

replace the `"tein/"` blanket prefix with explicit entries for each safe tein
module. `(tein process)` is intentionally excluded because `command-line`
leaks host argv.

before: `"tein/"`

after:
```
"tein/foreign", "tein/reader", "tein/macro", "tein/test", "tein/docs",
"tein/json", "tein/toml", "tein/file", "tein/load"
```

## (tein file)

**exports**: `open-input-file`, `open-output-file`, `open-binary-input-file`,
`open-binary-output-file`, `call-with-input-file`, `call-with-output-file`,
`with-input-from-file`, `with-output-to-file`, `file-exists?`, `delete-file`

**implementation split**:

- **4 open-\*-file procs**: re-exported from env. already policy-wrapped by
  `IoOp` machinery when sandboxed, unwrapped chibi procs when not. no new
  rust code.
- **4 higher-order procs** (`call-with-*-file`, `with-*-from/to-file`): pure
  scheme in `file.scm`, composing over `open-*-file` + `dynamic-wind` +
  `close-*-port`. inherit policy safety from the underlying open procs.
- **`file-exists?`**: new rust trampoline, checks `file_read` policy. when no
  FsPolicy is set (unsandboxed), allows unconditionally via
  `std::path::Path::exists()`.
- **`delete-file`**: new rust trampoline, checks `file_write` policy. when no
  FsPolicy is set, allows unconditionally via `std::fs::remove_file()`.

**registration**: `register_file_module()` called during `build()` for
standard-env contexts. `file-exists?` and `delete-file` registered via
`define_fn_variadic`.

**VFS files**: `lib/tein/file.sld` + `lib/tein/file.scm`

## (tein load)

**exports**: `load`

**implementation**: single rust trampoline. takes a filename string, restricts
to VFS paths only.

**mechanism**: trampoline checks path starts with `/vfs/`. if so, calls
`tein_vfs_lookup` (newly exposed in ffi.rs) to get the embedded content string,
then `sexp_open_input_string` → read+eval loop (same pattern as `evaluate()`).
non-VFS paths return a sandbox violation error string.

**rationale**: `load` evaluates arbitrary code. even with FsPolicy, allowing
load on user-accessible paths lets sandboxed code execute anything readable.
VFS content is curated and trusted — same level as `(import ...)`.

**unsandboxed behaviour**: same VFS-only restriction. users wanting unrestricted
load can use `(import (scheme load))` in unsandboxed contexts.

**VFS files**: `lib/tein/load.sld` + `lib/tein/load.scm`

## (tein process)

**exports**: `get-environment-variable`, `get-environment-variables`,
`command-line`, `exit`

**NOT in SAFE_MODULES** — `command-line` leaks host argv. available via
`.vfs_all()` or `.allow_module("tein/process")`.

### standard bindings

3 rust trampolines, pure `std::env`:

- **`get-environment-variable`**: takes string, returns `std::env::var(name)`
  as scheme string or `#f` if unset (r7rs semantics).
- **`get-environment-variables`**: no args, returns `std::env::vars()` as
  alist of `(name . value)` pairs.
- **`command-line`**: no args, returns `std::env::args()` as list of strings.

### exit — eval escape hatch

`(exit)` / `(exit obj)` early-returns from the current `evaluate()` call,
returning the exit value to the rust caller.

**semantics**:
- `(exit)` → returns `Value::Integer(0)`
- `(exit obj)` → returns `obj`
- `(exit #t)` → returns `Value::Integer(0)` (r7rs: success)
- `(exit #f)` → returns `Value::Integer(1)` (r7rs: failure)

**mechanism** (exception + thread-local flag):

1. `exit` trampoline sets `EXIT_REQUESTED: Cell<bool>` +
   `EXIT_VALUE: Cell<sexp>` thread-locals, GC-roots the value via
   `sexp_preserve_object`
2. trampoline returns a scheme exception via `make_error` — this
   immediately stops the VM (no 500-instruction delay from fuel quantum)
3. in the eval loop (`evaluate`, `evaluate_port`, `call`), before
   converting exceptions to errors, check the exit flag — if set, clear
   it, release the GC root, convert the stashed value to `Value`, and
   return `Ok(value)` instead of propagating the exception

user-level `guard`/`with-exception-handler` cannot catch this because the
exception is returned from a foreign function call, not raised via scheme's
`raise` — chibi propagates it directly without invoking handlers.

**dynamic-wind**: not invoked. this is immediate-bail semantics (like
`emergency-exit`). correct for an embedded eval escape hatch — the rust
caller owns cleanup.

**VFS files**: `lib/tein/process.sld` + `lib/tein/process.scm`

## file inventory

**new rust code** (all in `context.rs`):
- `file_exists_trampoline` + `delete_file_trampoline`
- `load_trampoline`
- `get_env_var_trampoline` + `get_env_vars_trampoline` +
  `command_line_trampoline` + `exit_trampoline`
- `register_file_module()` + `register_load_module()` +
  `register_process_module()`
- `EXIT_REQUESTED` + `EXIT_VALUE` thread-locals, exit check in `check_fuel()`

**sandbox.rs**: replace `"tein/"` with explicit module entries in
`SAFE_MODULES`

**build.rs**: add 6 VFS files to `VFS_FILES` (not feature-gated)

**chibi fork** (6 new files):
- `lib/tein/file.sld`, `lib/tein/file.scm`
- `lib/tein/load.sld`, `lib/tein/load.scm`
- `lib/tein/process.sld`, `lib/tein/process.scm`

## tests

- `(tein file)`: with/without FsPolicy, policy violations, higher-order
  wrappers with dynamic-wind cleanup, file-exists? and delete-file
- `(tein load)`: VFS path allowed, non-VFS rejected
- `(tein process)`: env var access, command-line, exit early-return with
  various arg forms
- SAFE_MODULES: `(tein process)` blocked by default sandbox, allowed with
  `.allow_module("tein/process")`
