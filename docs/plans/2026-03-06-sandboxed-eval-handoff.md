# sandboxed eval handoff — continuation notes

issue: #97 | branch: `feature/sandboxed-eval-2603` | design: `2026-03-05-sandboxed-eval-design.md`

## status: tasks 1-3 complete, tasks 4-8 WIP

### completed (committed)

1. **C shim** — `tein_sexp_global_meta_env(ctx)` + `tein_sexp_make_immutable(ctx, x)` added to chibi fork (`~/forks/chibi-scheme`), pushed.
2. **FFI bindings** — extern declarations + safe wrappers in `ffi.rs`.
3. **INTERACTION_ENV thread-local** — `Cell<ffi::sexp>` in context.rs, cleanup in `Context::drop()`.

### WIP (committed as `43a1f1a`, NOT passing)

- **`environment_trampoline`** at `context.rs:1313` — validates specs against `VFS_ALLOWLIST`, builds `(mutable-environment '(spec) ...)` expression, evaluates in `SEXP_G_META_ENV`. working correctly.
- **`interaction_environment_trampoline`** at `context.rs:1420` — returns `sexp_context_env(ctx)` (the sandbox's current env). simple but r7rs-compliant for our embedding model.
- **`spec_to_path`** at `context.rs:1246` — scheme list → path string. working.
- **`path_to_spec`** at `context.rs:1281` — path string → scheme list. **currently unused** (was for old approach). can be removed.
- **`register_eval_module`** at `context.rs:3470` — registers both trampolines via `define_fn_variadic`.
- **VFS shadow updates** in `vfs_registry.rs` — all three shadows updated.
- **test updates** — old "scheme/eval blocked" tests updated, new tests added.
- **sandbox.rs** — `registry_safe_allowlist_contains_expected_modules` updated.

## THE BLOCKING BUG

**shadow modules reference trampolines that don't exist at shadow load time.**

the build order is:
1. `register_vfs_shadows()` — line 2052 in context.rs (sandbox build path)
2. `register_eval_module()` — line 2221 (standard_env registration)

shadows run at step 1, but the trampolines (`tein-environment-internal`, `tein-interaction-environment-internal`) are registered at step 2. so when the shadow `.sld` body is evaluated, the trampolines are undefined → `EvalError`.

### the fix

move the `register_eval_module()` call (or just the two `define_fn_variadic` calls) to run BEFORE `register_vfs_shadows()` in the sandbox build path.

look at context.rs around line 2050. add the registration before the shadows call:

```rust
// register eval/interaction-environment trampolines before shadows
// that reference them are loaded (#97)
context.register_eval_module()?;
crate::sandbox::register_vfs_shadows()?;
```

but also keep the call at line 2221 for non-sandboxed contexts (or guard it to avoid double registration). alternatively, move the `register_eval_module()` call out of the `if self.standard_env` block and into a position that runs for all standard_env contexts BEFORE shadows. the exact fix depends on whether `define_fn_variadic` is idempotent (calling it twice for the same name may overwrite or error — check).

### secondary issue: `(scheme/load)` shadow needs `(chibi)` import

the scheme/load shadow currently does `(import (tein load) (chibi))`. `(chibi)` is needed because the `(begin ...)` body uses `apply` and `tein-environment-internal` which are in the top-level env accessible via `(chibi)`. this should be fine since `(chibi)` is always available as a core module.

## tests that need attention

### already passing (after the ordering fix)
- `test_sandboxed_modules_safe_arithmetic` — verifies `(import (scheme eval))` works in Safe
- `test_sandbox_eval_contained` — verifies disallowed modules (`scheme/regex`) error
- `test_sandbox_eval_environment_disallowed_module` — same
- `test_sandboxed_environment_empty` — `(environment)` with no args
- `test_sandboxed_environment_via_scheme_load` — environment via `(scheme load)`
- `test_sandboxed_interaction_environment_mutable` — define+retrieve in interaction env
- `test_sandboxed_interaction_environment_persistent` — persistence across evals
- `test_sandboxed_interaction_environment_has_base_bindings` — needs `(import (scheme base))` first

### currently broken (will pass after ordering fix)
- `test_scheme_show_importable_in_sandbox` — `tein-interaction-environment-internal` undefined
- `test_srfi_166_base_importable_in_sandbox` — same root cause
- `test_sandboxed_modules_safe_eval_contained` — `tein-environment-internal` undefined

### already fixed
- `test_standard_env_with_sandbox` — uses `Modules::only(["scheme/base"])`, correctly blocks scheme/eval (NOT in that allowlist)
- `test_sandboxed_modules_safe_blocks_scheme_env_escape` — removed (subagent replaced it)
- old tests using `(chibi ast)` or `(chibi io)` as "disallowed" — fixed to use `(scheme regex)` which is actually `default_safe: false`

## important: test modules for allowlist checks

when testing that `environment` rejects a module, use one that is actually `default_safe: false`:
- `scheme/regex` — `default_safe: false` (backtracking ReDoS risk)
- `scheme/time` — `default_safe: false` (feature-gated)
- do NOT use `chibi/ast`, `chibi/io` — these are `default_safe: true`!

## remaining tasks after the fix

### task 9: cascading `default_safe` updates
- `srfi/64` → `default_safe: true` (already done in vfs_registry.rs)
- `scheme/small` — leave as `default_safe: false` (scheme/time dep is feature-gated)

### task 10: docs + AGENTS.md
- add sandboxed eval/environment flow to AGENTS.md
- add `interaction-environment` thread-local gotcha
- update docs/guide.md sandboxing section

### task 11: final verification
- `just test` — full suite
- `just lint` — format + clippy (will flag `path_to_spec` as unused — remove it)

## design refinement: `interaction-environment`

the original design planned a complex approach (build mutable env from VFS allowlist modules). this was abandoned because:
1. loading all modules from scratch triggered VFS gate errors in the meta env
2. SIGSEGV from GC issues with the list-building loop

the final approach is much simpler: return `sexp_context_env(ctx)` — the sandbox's own env. this works because:
- in sandbox, the context env is already set up with the correct module access
- definitions via `(eval '(define x 42) (interaction-environment))` go into this env and persist
- subsequent `(eval 'x (interaction-environment))` finds them
- the `INTERACTION_ENV` thread-local + GC rooting is still present but effectively unused (returns context env directly). **consider removing the thread-local** if keeping the simple approach.
