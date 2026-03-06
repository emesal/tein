# sandboxed eval handoff — continuation notes

issue: #97 | branch: `feature/sandboxed-eval-2603` | design: `2026-03-05-sandboxed-eval-design.md`

## status: COMPLETE — all tests pass, lint clean

### completed (committed)

1. **C shim** — `tein_sexp_global_meta_env(ctx)` + `tein_sexp_make_immutable(ctx, x)` added to chibi fork (`~/forks/chibi-scheme`), pushed.
2. **FFI bindings** — extern declarations + safe wrappers in `ffi.rs`.
3. **INTERACTION_ENV thread-local** — `Cell<ffi::sexp>` in context.rs, cleanup in `Context::drop()`.
4. **eval trampolines** — `register_eval_trampolines` free fn, VFS shadow SLDs for `scheme/eval`, `scheme/load`, `scheme/repl`, all tests passing.
5. **chibi fork debug cleanup** — debug `fprintf` removed from `eval.c`, pushed to emesal-tein.

### the fix: primitive-env registration

The root problem was that VFS shadow SLD bodies reference `tein-environment-internal` as a free
variable, but that trampoline wasn't reachable during library body evaluation.

**Fix**: register the trampolines into the primitive env BEFORE `load_standard_env` runs.
`init-7.scm` builds `*chibi-env*` by importing ALL bindings from the primitive env (the interaction
env at that point). Any name present in the primitive env propagates into `*chibi-env*`, making it
available to any library body that `(import (chibi))` — which is exactly what the shadow SLDs do.

```rust
// in build(), BEFORE load_standard_env:
let prim_env = ffi::sexp_context_env(ctx);
register_eval_trampolines(ctx, prim_env)?;
```

### cleanup performed this session

- removed `register_eval_trampolines(ctx, null_env)` from sandbox block (wrong approach)
- removed debug test `test_debug_tein_interaction_env_trampoline_direct`
- removed dead fn `path_to_spec`
- fixed clippy `manual_contains` lint
- updated vfs_registry.rs comments to reflect primitive-env approach
- removed debug fprintf from chibi fork eval.c, pushed

## remaining tasks (from original plan)

- task 10: docs + AGENTS.md (sandboxed eval flow)
- task 11: PR + close #97

## tests passing

All 7 previously-failing tests now pass:
- `test_scheme_show_importable_in_sandbox`
- `test_srfi_166_base_importable_in_sandbox`
- `test_sandboxed_modules_safe_eval_contained`
- `test_sandboxed_interaction_environment_mutable`
- `test_sandboxed_interaction_environment_persistent`
- `test_sandboxed_interaction_environment_has_base_bindings`
- `test_sandboxed_environment_via_scheme_load`

Full suite: 942/942 pass.

## chibi fork state

emesal-tein branch commits (relevant):
- `feat(shim)`: meta env accessor + make-immutable wrapper
- `feat(tein/eval)`: add (tein eval) library — present in fork but unused by tein
- `fix(eval)`: remove debug trace from patch H

The `tein/eval.sld` + `tein/eval.scm` in the fork are unused; can be removed in a cleanup PR.
