# sandboxing

Reference for tein's sandbox model: module restriction, VFS gate, file IO policy, step limits, and wall-clock timeouts.

---

## the four layers

tein's sandbox is a composable trust boundary made of four independent layers. All four can be combined freely.

| layer | what it controls | configured via |
|---|---|---|
| module restriction | which R7RS libraries Scheme can `import` | `.sandboxed(Modules::...)` |
| VFS gate | enforces module restriction at the C level | set automatically by `.sandboxed()` |
| file IO policy | which filesystem paths can be opened | `.file_read()`, `.file_write()` |
| step limit / timeout | resource exhaustion | `.step_limit(n)`, `.build_timeout(d)` |

Each layer operates independently. You can set a step limit without sandboxing modules, or restrict file paths without restricting modules — though the most common combination is all four together.

---

## module restriction — `Modules` variants

`.sandboxed(modules)` activates the module sandbox. It builds a null environment containing only `import` syntax, arms the VFS gate to enforce an allowlist, and registers UX stubs for all excluded module exports.

`.sandboxed()` requires `.standard_env()` — the full R7RS environment must be loaded before restriction is applied (the null env copies bindings out of it).

```rust
use tein::{Context, sandbox::Modules};

// conservative safe set (default)
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .build()?;

// all vetted VFS modules — superset of Safe, includes scheme/eval
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::All)
    .build()?;

// syntax only — define, if, lambda, begin, quote; all imports rejected
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::None)
    .build()?;

// explicit list — transitive deps resolved automatically
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::only(&["scheme/base", "scheme/write"]))
    .build()?;
```

### `Modules::Safe`

The default for sandboxed contexts. Includes all modules marked `default_safe` in the registry with transitive dependencies resolved.

Included in Safe:

- `scheme/base`, `scheme/char`, `scheme/write`, `scheme/read`
- `scheme/complex`, `scheme/inexact`, `scheme/lazy`, `scheme/case-lambda`, `scheme/cxr`
- `scheme/fixnum`, `scheme/flonum`, `scheme/bitwise`, `scheme/rlist`
- `scheme/file` — via shadow module that re-exports from `(tein file)`, enforcing FsPolicy
- `scheme/repl` — via shadow module returning `(current-environment)` (neutered, no raw eval)
- `scheme/process-context` — via shadow re-exporting from `(tein process)` with neutered env/argv
- all `srfi/*` modules in the registry
- all `tein/*` modules (including `tein/process` — env vars and command-line are neutered by trampolines in sandboxed contexts)
- feature-gated modules when enabled: `tein/json`, `tein/toml`, `tein/uuid`, `tein/time`

Excluded from Safe:

- `scheme/eval` — exports `eval` and `environment`; use `Modules::All` to enable explicitly
- `scheme/load` — loads arbitrary files from the filesystem; use `(tein load)` instead
- `scheme/r5rs` — re-exports `scheme/file`, `scheme/load`, `scheme/process-context`
- `scheme/time` — depends on `scheme/process-context` and `scheme/file` at the Embedded level

### `Modules::All`

Superset of `Modules::Safe`. Includes all vetted modules in the VFS registry, filtered by active cargo features. Adds `scheme/eval` and any other modules not in the safe tier.

### `Modules::None`

Syntax only. The `import` form is available (so Scheme code can attempt imports), but the VFS gate rejects every module. UX stubs are registered for all known module exports.

### `Modules::only(&[...])`

Custom explicit allowlist. Transitive dependencies are resolved automatically from the registry — you only need to list the modules you want, not their entire dependency graphs.

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::only(&["scheme/base", "scheme/write"]))
    .build()?;
```

---

## `allow_module()` — extending a preset

`.allow_module(path)` adds a single module (and its transitive deps) to whatever `Modules` preset was set. Call it after `.sandboxed()`.

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .allow_module("scheme/eval")
    .build()?;
```

Transitive deps are resolved automatically against the VFS registry. You do not need to enumerate each dependency manually.

---

## UX stubs — informative errors in restricted envs

When a module is excluded from the allowlist, tein registers a stub for every binding that module exports. Calling a stub raises an informative error rather than an opaque "undefined variable":

```scheme
(map (lambda (x) x) '(1 2))
;; => sandbox violation: sandbox: 'map' requires (import (scheme base))
```

This surfaces as `Error::SandboxViolation` in Rust. The message names the exact import path needed, which is useful when sandboxed code is LLM-generated — the model can read the error and self-correct.

Stubs are registered for all excluded modules, including `scheme/base` bindings under `Modules::None`. Alias modules with no direct top-level exports (like `scheme/bitwise`) are silently skipped.

---

## file IO policy

`.file_read()` and `.file_write()` restrict which filesystem paths Scheme code can open. Both take a slice of absolute path prefixes.

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .file_read(&["/data/config/"])
    .file_write(&["/tmp/output/"])
    .build()?;
```

Calling `.file_read()` or `.file_write()` without a preceding `.sandboxed()` call auto-activates `sandboxed(Modules::Safe)`.

### how path matching works

Paths are canonicalised before prefix matching:

- for reads: `Path::canonicalize()` is called on the full path. The file must exist for this to succeed; a missing file is denied.
- for writes: the parent directory is canonicalised (must exist), the filename is appended. The file itself does not need to exist yet — `open-output-file` creates it per R7RS semantics.

Symlink traversals are resolved by `canonicalize()`, so a symlink that exits the allowed prefix is denied. `..` components are collapsed before matching.

### what is gated

All four file operations in the Scheme environment are checked against FsPolicy in sandboxed contexts:

- `open-input-file` — gated at the C opcode level (`eval.c` patch F)
- `open-output-file` — gated at the C opcode level (`eval.c` patch G)
- `file-exists?` — gated via Rust trampoline
- `delete-file` — gated via Rust trampoline

A denied access raises `Error::SandboxViolation`.

### the `scheme/file` shadow

In sandboxed contexts, `(scheme file)` is a shadow module that re-exports from `(tein file)`. The `tein/file` trampolines apply FsPolicy before any filesystem call. This means `(import (scheme file))` works in sandboxed contexts as long as `scheme/file` is in the allowlist — the FsPolicy gate is enforced regardless.

---

## step limits

`.step_limit(n)` caps the number of VM instructions per `evaluate()` or `call()` invocation. Fuel resets before each call.

```rust
let ctx = Context::builder()
    .step_limit(50_000)
    .build()?;

match ctx.evaluate("((lambda () (define (f) (f)) (f)))") {
    Err(tein::Error::StepLimitExceeded) => { /* expected */ }
    other => panic!("unexpected: {:?}", other),
}
```

Fuel is consumed at VM timeslice boundaries (a two-line patch in `vm.c`). The limit is not per-instruction-exact but is bounded within one timeslice granularity.

Step limits can be combined with any sandbox configuration. `step_limit` is required when using `TimeoutContext` — without it, the context thread cannot terminate after a timeout fires.

---

## wall-clock timeouts

`build_timeout(duration)` wraps the context on a dedicated thread and enforces a wall-clock deadline on each `evaluate()` call. Returns `TimeoutContext`.

```rust
use std::time::Duration;
use tein::{Context, sandbox::Modules};

let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .step_limit(10_000_000)
    .build_timeout(Duration::from_secs(2))?;

match ctx.evaluate("(define (f) (f)) (f)") {
    Err(tein::Error::Timeout) => { /* expected */ }
    other => panic!("unexpected: {:?}", other),
}
```

`build_timeout` returns `Error::InitError` if no `step_limit` is set.

State persists across calls — unlike a fresh context, `TimeoutContext` accumulates definitions between evaluations. See [embedding.md](embedding.md) for the full context type comparison and managed context patterns.

---

## where the sandbox is heading

The current sandbox controls what code can access at build time: a fixed allowlist resolved before the first evaluation. Planned work includes host callbacks to intercept specific operations at runtime — environment variable reads (GH #99), per-call file IO interception. The goal is a fully configurable permission system where the host can observe, modify, or deny any privileged operation without rebuilding the context.
