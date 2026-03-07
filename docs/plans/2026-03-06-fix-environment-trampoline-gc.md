# Fix environment_trampoline GC Rooting + Stale Comments

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix the GC rooting bug in `environment_trampoline` and update two stale comments in `sandbox.rs`.

**Architecture:** The cons-building loop in `environment_trampoline` (context.rs) must follow the established per-iteration root pattern used everywhere else in the file. Two comment blocks in sandbox.rs describe pre-refactor behaviour and must be updated to match what's actually there.

**Tech Stack:** Rust, unsafe FFI (`ffi::GcRoot`), no new dependencies.

---

### Task 1: Fix GC rooting in `environment_trampoline`

**Files:**
- Modify: `tein/src/context.rs` (the `environment_trampoline` function, ~lines 1285–1387)

**Background — the bug:**

The current loop:

```rust
let mut expr_parts = ffi::get_null();
let _parts_root = ffi::GcRoot::new(ctx, expr_parts);  // roots null — no-op in chibi

let mut arg_vec: Vec<ffi::sexp> = Vec::new();
// ... collect specs into arg_vec ...

for spec in arg_vec.iter().rev() {
    let quoted_inner = ffi::sexp_cons(ctx, *spec, ffi::get_null());  // unrooted result
    let quoted = ffi::sexp_cons(ctx, quote_sym, quoted_inner);        // quoted_inner unrooted
    expr_parts = ffi::sexp_cons(ctx, quoted, expr_parts);             // new head unrooted
}
```

Problems:
1. `_parts_root` roots the initial `get_null()` (an immediate — `sexp_preserve_object` on an immediate is a no-op). The growing list is never rooted.
2. `quoted_inner` is unrooted when the second `sexp_cons` runs.
3. `quoted` is unrooted when the third `sexp_cons` runs.
4. `arg_vec: Vec<ffi::sexp>` holds raw sexp pointers on the Rust heap — invisible to chibi GC across the cons calls. Each `spec` pointer from `arg_vec` must be rooted per iteration.
5. `sym` and `quote_sym` (interned symbols) are immortal in chibi's symbol table and safe — no root needed.

**The canonical pattern** (used at lines 209-211, 238-240, 1542-1563, etc.):

```rust
for item in collection {
    let _item_root = ffi::GcRoot::new(ctx, item);       // root the input
    let _tail_root = ffi::GcRoot::new(ctx, result);     // root the accumulator before cons
    let new_val = ffi::sexp_cons(ctx, item, result);
    if ffi::sexp_exceptionp(new_val) != 0 { return new_val; }
    result = new_val;
}
```

**Step 1: Write a failing test that exercises multi-spec environment calls**

Add this test inside the `mod tests` block in `tein/src/context.rs`, near the existing `test_sandboxed_environment_*` tests (~line 4787):

```rust
#[test]
fn test_environment_trampoline_multi_spec_gc() {
    // Multi-spec environment call — exercises the cons loop with 3+ specs.
    // Under the bug, the partial list could be GC'd mid-loop under heap pressure.
    // Use `cargo test --features debug-chibi` for GC instrumentation.
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(
            "(import (scheme eval))\
             (eval '(+ 1 2) (environment '(scheme base) '(scheme write) '(scheme cxr)))",
        )
        .expect("multi-spec environment should work");
    assert_eq!(result, Value::Integer(3));
}

#[test]
fn test_sandboxed_environment_trampoline_multi_spec_gc() {
    // Same but sandboxed — allowlist check + multi-spec cons loop both exercised.
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .build()
        .unwrap();
    let result = ctx
        .evaluate(
            "(import (scheme eval))\
             (eval '(+ 10 20) (environment '(scheme base) '(scheme write) '(scheme cxr)))",
        )
        .expect("sandboxed multi-spec environment should work");
    assert_eq!(result, Value::Integer(30));
}
```

**Step 2: Run tests (they may pass currently, but we want them to catch regressions):**

```bash
cargo test test_environment_trampoline_multi_spec_gc
cargo test test_sandboxed_environment_trampoline_multi_spec_gc
```

Note: these tests may pass even with the bug because GC is not deterministically triggered on every allocation in non-debug builds. The fix is still required by the project's safety invariants. Run with `--features debug-chibi` for instrumented GC behaviour.

**Step 3: Fix `environment_trampoline` — rewrite the list-building loop**

Replace the current loop and surrounding code in `environment_trampoline` (from just after the `meta_env` root, through to the `sexp_cons(ctx, sym, expr_parts)` call). The fixed version:

```rust
// root the meta env
let meta_env = ffi::sexp_global_meta_env(ctx);
let _meta_root = ffi::GcRoot::new(ctx, meta_env);

let sym_name = c"mutable-environment";
let sym = ffi::sexp_intern(
    ctx,
    sym_name.as_ptr(),
    "mutable-environment".len() as ffi::sexp_sint_t,
);
// sym is an interned symbol — immortal in chibi's symbol table, no root needed.

// build quoted-spec list: (mutable-environment '(scheme base) '(scheme write) ...)
// each arg is already an evaluated list like (scheme base), so we quote them.
let quote_sym = ffi::sexp_intern(ctx, c"quote".as_ptr(), 5);
// quote_sym is an interned symbol — immortal, no root needed.

// walk args in reverse to build the list via cons
let mut arg_vec: Vec<ffi::sexp> = Vec::new();
let mut cursor = args;
while ffi::sexp_pairp(cursor) != 0 {
    arg_vec.push(ffi::sexp_car(cursor));
    cursor = ffi::sexp_cdr(cursor);
}

let mut expr_parts = ffi::get_null();
for spec in arg_vec.iter().rev() {
    // root spec (from arg_vec — Rust heap, invisible to chibi GC)
    let _spec_root = ffi::GcRoot::new(ctx, *spec);
    // root the accumulator before each allocation
    let _tail_root = ffi::GcRoot::new(ctx, expr_parts);

    // build (quote spec) = (quote_sym . (spec . ()))
    let quoted_inner = ffi::sexp_cons(ctx, *spec, ffi::get_null());
    if ffi::sexp_exceptionp(quoted_inner) != 0 {
        return quoted_inner;
    }
    let _inner_root = ffi::GcRoot::new(ctx, quoted_inner);

    let quoted = ffi::sexp_cons(ctx, quote_sym, quoted_inner);
    if ffi::sexp_exceptionp(quoted) != 0 {
        return quoted;
    }
    let _quoted_root = ffi::GcRoot::new(ctx, quoted);

    expr_parts = ffi::sexp_cons(ctx, quoted, expr_parts);
    if ffi::sexp_exceptionp(expr_parts) != 0 {
        return expr_parts;
    }
}

// prepend the mutable-environment symbol
let _parts_root = ffi::GcRoot::new(ctx, expr_parts);
let expr = ffi::sexp_cons(ctx, sym, expr_parts);
if ffi::sexp_exceptionp(expr) != 0 {
    return expr;
}
let _expr_root = ffi::GcRoot::new(ctx, expr);

// evaluate in meta env
let result = ffi::sexp_evaluate(ctx, expr, meta_env);
if ffi::sexp_exceptionp(result) != 0 {
    return result;
}

// make the resulting environment immutable (r7rs: environment returns immutable envs)
// note: sexp_make_immutable_op mutates in place and returns #t/#f,
// so we return `result` (the env), not the make_immutable return value.
let imm = ffi::sexp_make_immutable(ctx, result);
if ffi::sexp_exceptionp(imm) != 0 {
    return imm;
}
result
```

Key changes from the original:
- `_parts_root` moved to after the loop (before the final `sexp_cons`), where it actually guards a live pointer
- `_spec_root` per iteration roots each `spec` pointer from `arg_vec`
- `_tail_root` per iteration roots `expr_parts` before each allocation
- `_inner_root` roots `quoted_inner` before the second cons call
- `_quoted_root` roots `quoted` before the third cons call

**Step 4: Run tests:**

```bash
cargo test test_environment_trampoline_multi_spec_gc
cargo test test_sandboxed_environment_trampoline_multi_spec_gc
cargo test --features debug-chibi -- environment 2>&1 | tail -20
```

Expected: all pass.

**Step 5: Run full test suite and lint:**

```bash
just test
just lint
```

Expected: all pass, no warnings about unused variables.

**Step 6: Commit:**

```bash
git add tein/src/context.rs
git commit -m "fix(ffi): GC rooting in environment_trampoline cons loop (#97)

root spec, quoted_inner, quoted, and expr_parts per-iteration following
the established pattern (get_env_vars_trampoline, foreign_types_wrapper).
_parts_root moved to after the loop where it guards a live pointer."
```

---

### Task 2: Fix stale comments in `sandbox.rs`

**Files:**
- Modify: `tein/src/sandbox.rs` (~lines 120–141)

**Background:**

Two comments in the block starting at line 120 describe the pre-refactor state:

Line 127:
```
// - `scheme/repl` — neutered interaction-environment via (current-environment)
```
Actual current state: the shadow delegates to `tein-interaction-environment-internal` (a persistent mutable env cached in `INTERACTION_ENV`, not `(current-environment)`).

Line 140:
```
// - `scheme/eval` — eval + environment. tracked for future shadow (GH issue #97).
```
Actual current state: the shadow is now live (merged in #97). `scheme/eval` should move to the "hand-written shadows (functional)" section.

Line 141:
```
// - `scheme/r5rs` — re-exports scheme/eval; tracked in #106 (blocked on #97).
```
`scheme/r5rs` is no longer blocked on #97 since #97 is complete. Update accordingly.

**Step 1: Update the comment block**

Replace lines 120–141 in `tein/src/sandbox.rs` with:

```rust
// the following modules have `VfsSource::Shadow` entries in the registry.
// in sandboxed contexts, `register_vfs_shadows()` injects replacement `.sld`
// files that re-export from safe tein counterparts or provide neutered stubs.
// unsandboxed contexts use chibi's native versions (no shadow registered).
//
// hand-written shadows (functional):
// - `scheme/eval` — exports eval (from chibi) + environment (tein-environment-internal
//   trampoline, validates specs against VFS_ALLOWLIST). closes #97.
// - `scheme/load` — re-exports (tein load) VFS-restricted load + environment trampoline.
// - `scheme/repl` — interaction-environment delegates to tein-interaction-environment-internal:
//   a persistent mutable env (INTERACTION_ENV) that accumulates definitions across evals.
// - `scheme/file` — re-exports (tein file), providing FsPolicy enforcement
// - `scheme/process-context` — re-exports (tein process) with neutered env/argv
// - `srfi/98` — neutered get-environment-variable (always #f)
//
// generated shadow stubs (error-on-call): chibi/filesystem, chibi/process,
// chibi/system, chibi/shell, chibi/temp-file, chibi/stty, chibi/term/edit-line,
// chibi/app, chibi/config, chibi/log, chibi/tar, chibi/apropos, srfi/193,
// chibi/net, chibi/net/http, chibi/net/server, chibi/net/http-server,
// chibi/net/server-util, chibi/net/servlet.
//
// modules NOT shadowed and intentionally blocked:
//
// - `scheme/r5rs` — re-exports scheme/eval; tracked in #106.
```

**Step 2: Run lint to confirm no format issues:**

```bash
just lint
```

**Step 3: Commit:**

```bash
git add tein/src/sandbox.rs
git commit -m "docs(sandbox): update shadow module comment block (#97)

scheme/eval and scheme/repl shadows are now live; remove stale 'future
shadow' and '(current-environment)' notes. scheme/r5rs no longer blocked
on #97."
```

---

### Task 3: Collect AGENTS.md notes

No AGENTS.md updates needed — the fix follows the existing documented GC rooting pattern and the comment corrections are self-contained.

Final check:

```bash
just test
```

Expected: all tests pass.
