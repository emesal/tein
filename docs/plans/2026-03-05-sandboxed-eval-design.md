# sandboxed (scheme eval) + (scheme load) + (scheme repl)

issue: #97

## problem

`(scheme eval)` is blocked from sandbox (`default_safe: false`) because it exports
`environment`, which can create envs from arbitrary module names — bypassing the
VFS allowlist. `(scheme load)` also exports `environment`. `(scheme repl)`'s
`interaction-environment` returns a frozen env instead of a mutable one.

this blocks r7rs completeness in sandbox: no `eval`, no `environment`, no proper
REPL interaction env.

## approach: A' — rust trampoline + C shim delegation

`environment` is implemented as a rust trampoline that validates import specs
against the VFS allowlist, then delegates to chibi's `mutable-environment` (from
`meta-7.scm`) via a thin C shim that accesses `SEXP_G_META_ENV`. `(meta)` is
never exposed to scheme-level sandbox code.

### why not expose `(meta)` directly?

`(meta)` exports the entire module system internals: `add-module!`,
`delete-module!`, `module-env-set!`, `*modules*` (mutable global list). exposing
it to sandbox would allow corrupting the module registry.

### why not pure rust/C reimplementation?

`find-module`, `load-module`, `resolve-import` are scheme procedures in
`meta-7.scm`, not C primitives. reimplementing them in rust would duplicate
complex logic and diverge from upstream.

## design

### `(scheme eval)` shadow

two exports: `eval` and `environment`.

**`eval`**: chibi primitive from `(chibi)`. evaluates an expression in a given
env. in sandbox the current env is already restricted, so `eval` is safe as-is.
re-exported directly.

**`environment`**: new rust trampoline `tein-environment-internal` registered
globally via `define_fn_variadic`:

1. accepts variadic import specs (e.g. `'(scheme base)`, `'(scheme write)`)
2. validates each spec is a proper list of symbols/numbers
3. converts each to a path string and checks against `VFS_ALLOWLIST`
4. rejects if any module is disallowed (raises scheme error)
5. calls C shim `tein_make_environment(ctx, specs_list)` which:
   - looks up `mutable-environment` in `SEXP_G_META_ENV`
   - applies it to the specs list
   - calls `make-immutable!` on the result
   - returns the env (or exception)

shadow .sld:
```scheme
(define-library (scheme eval)
  (import (chibi))
  (export eval environment)
  (begin
    (define (environment . specs)
      (apply tein-environment-internal specs))))
```

### `(scheme load)` shadow

update existing shadow to also export `environment`:
```scheme
(define-library (scheme load)
  (import (tein load))
  (export load environment)
  (begin
    (define (environment . specs)
      (apply tein-environment-internal specs))))
```

the one-liner `define` delegates to the same trampoline — single source of truth
is the rust trampoline.

### `(scheme repl)` shadow

one export: `interaction-environment`.

r7rs requires this to return a mutable env that accumulates definitions across
evals. new rust trampoline `tein-interaction-environment-internal`:

- **first call**: creates a mutable env from the current allowlist modules via C
  shim `tein_make_mutable_environment(ctx, specs_list)`, GC-roots it, stores in
  thread-local `INTERACTION_ENV`, returns it.
- **subsequent calls**: returns the stored env.

the interaction env is populated with the same modules as the sandbox allowlist,
so `eval` into it has access to the same bindings plus any user-defined ones.

thread-local cleared on `Context::drop()`.

shadow .sld:
```scheme
(define-library (scheme repl)
  (import (chibi))
  (export interaction-environment)
  (begin
    (define (interaction-environment)
      (tein-interaction-environment-internal))))
```

## C shim additions (tein_shim.c, fork)

two new functions:

- **`tein_make_environment(ctx, specs_list)`** — looks up `mutable-environment`
  in `SEXP_G_META_ENV`, applies it to specs, then `make-immutable!`. returns env
  or exception.
- **`tein_make_mutable_environment(ctx, specs_list)`** — same, skips
  immutability. used for `interaction-environment`.

module resolution flows through chibi's existing `mutable-environment` →
`find-module` → `find-module-file` → VFS gate. allowlist enforced at two levels:
rust trampoline pre-validates, VFS gate blocks at load time.

## VFS registry changes

| module | change | new `default_safe` |
|--------|--------|--------------------|
| `scheme/eval` | `Embedded` → `Shadow` | `true` |
| `scheme/load` | update shadow to also export `environment` | `true` (no change) |
| `scheme/repl` | update shadow to use new trampoline | `true` (no change) |

### cascading effects

once `scheme/eval` + `scheme/load` are safe:
- `scheme/small` can become `default_safe: true` (check full dep chain)
- `scheme/red` — likely still blocked by feature-gated deps, evaluate separately
- `srfi/64` can become `default_safe: true` (dep on `scheme/eval` resolved)

## new rust/C surface

| component | type | location |
|-----------|------|----------|
| `tein-environment-internal` | rust trampoline (`define_fn_variadic`) | context.rs |
| `tein-interaction-environment-internal` | rust trampoline (`define_fn_variadic`) | context.rs |
| `tein_make_environment` | C shim | tein_shim.c (fork) |
| `tein_make_mutable_environment` | C shim | tein_shim.c (fork) |
| `INTERACTION_ENV` | thread-local `Cell<sexp>` | context.rs |
| 3 shadow .sld updates | VFS registry | vfs_registry.rs |

## thread-local lifecycle

`INTERACTION_ENV` follows the same pattern as `EXIT_REQUESTED`, `EXIT_VALUE`,
`FOREIGN_STORE_PTR`:
- set lazily on first `interaction-environment` call
- GC-rooted via `sexp_preserve_object`
- cleared + `sexp_release_object` on `Context::drop()`

## testing

- `environment` with allowed modules succeeds
- `environment` with disallowed module raises error
- empty `(environment)` returns empty env
- `interaction-environment` persistence: define binding via `eval`, retrieve in
  subsequent eval
- `(scheme load)` exports `environment`
- `scheme/eval` importable in sandbox
- sandbox gating: `environment` cannot load disallowed modules

## r7rs deviations

none introduced — this design achieves full r7rs compliance for `(scheme eval)`,
`(scheme load)`, and `(scheme repl)` in sandboxed contexts.
