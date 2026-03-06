# sandboxed (scheme eval) + (scheme load) + (scheme repl) — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** enable `(scheme eval)`, `(scheme load)`, and `(scheme repl)` in sandboxed contexts with VFS allowlist enforcement, completing r7rs sandbox coverage.

**Architecture:** rust trampolines validate import specs against `VFS_ALLOWLIST`, then delegate to chibi's `mutable-environment` (from `meta-7.scm`) via `sexp_apply` into the meta env accessed through a thin C shim accessor. `(meta)` is never exposed to scheme-level sandbox code. `interaction-environment` returns a persistent mutable env per context, stored in a thread-local.

**Tech Stack:** rust (trampolines, thread-locals, FFI wrappers), C (tein_shim.c accessor), scheme (VFS shadow .sld modules)

**Design doc:** `docs/plans/2026-03-05-sandboxed-eval-design.md`

**Branch:** create with `just feature sandboxed-eval-2603`

---

## task 1: C shim — meta env accessor + `make-immutable!` FFI

**files:**
- modify: `~/forks/chibi-scheme/tein_shim.c` (append after line 626)

**step 1: add `tein_sexp_global_meta_env` accessor to tein_shim.c**

```c
/* --- sandboxed (scheme eval) support (#97) --- */

sexp tein_sexp_global_meta_env(sexp ctx) {
    return sexp_global(ctx, SEXP_G_META_ENV);
}

sexp tein_sexp_make_immutable(sexp ctx, sexp x) {
    return sexp_make_immutable_op(ctx, NULL, 1, x);
}
```

**step 2: push to chibi fork**

```bash
cd ~/forks/chibi-scheme
git add tein_shim.c
git commit -m "feat(shim): meta env accessor + make-immutable wrapper (#97)"
git push origin emesal-tein
```

**step 3: rebuild tein to pull fork changes**

```bash
cd ~/projects/tein
just clean && cargo build
```

verify the build succeeds and the new symbols are present:
```bash
nm target/debug/libtein.rlib 2>/dev/null | grep tein_sexp_global_meta_env || \
nm ~/.cache/cargo-target/debug/build/tein-*/out/libchibi_scheme.a 2>/dev/null | grep tein_sexp_global_meta_env
```

**step 4: commit**

```bash
git add -A && git commit -m "chore: rebuild against chibi fork with meta env accessor (#97)"
```

---

## task 2: FFI bindings — declare + wrap new C functions

**files:**
- modify: `tein/src/ffi.rs` — extern block (~line 204) and safe wrappers (~line 742)

**step 1: add extern declarations**

in the `extern "C"` block (after line 207, near the other `tein_*` declarations), add:

```rust
    // meta env accessor (for sandboxed scheme/eval #97)
    pub fn tein_sexp_global_meta_env(ctx: sexp) -> sexp;
    // make-immutable wrapper (chibi SEXP_API, for r7rs environment)
    pub fn tein_sexp_make_immutable(ctx: sexp, x: sexp) -> sexp;
```

**step 2: add safe wrappers**

after the `vfs_gate_set` wrapper (~line 744), add:

```rust
/// get the meta environment (`SEXP_G_META_ENV`) — contains `mutable-environment`,
/// `environment`, and other module-system internals from `meta-7.scm`.
///
/// # safety
/// `ctx` must be a valid chibi context with standard env loaded.
#[inline]
pub unsafe fn sexp_global_meta_env(ctx: sexp) -> sexp {
    unsafe { tein_sexp_global_meta_env(ctx) }
}

/// make a value immutable (wraps `sexp_make_immutable_op`).
/// used by `environment` trampoline to freeze the env after construction.
///
/// # safety
/// `ctx` must be a valid chibi context; `x` must be a valid sexp.
#[inline]
pub unsafe fn sexp_make_immutable(ctx: sexp, x: sexp) -> sexp {
    unsafe { tein_sexp_make_immutable(ctx, x) }
}
```

**step 3: run `just lint`**

**step 4: commit**

```bash
git add tein/src/ffi.rs
git commit -m "feat(ffi): meta env accessor + make-immutable bindings (#97)"
```

---

## task 3: `INTERACTION_ENV` thread-local + cleanup in `Context::drop`

**files:**
- modify: `tein/src/context.rs` — thread-local block (~line 112) and `drop` impl (~line 3332)

**step 1: add thread-local**

near the other thread-locals (after line 141, after `SANDBOX_COMMAND_LINE`), add:

```rust
/// GC-rooted mutable env returned by `interaction-environment` in sandbox.
/// lazily created on first call; cleared on `Context::drop()`.
pub(crate) static INTERACTION_ENV: Cell<ffi::sexp> = const { Cell::new(std::ptr::null_mut()) };
```

**step 2: add cleanup in `Context::drop`**

in the `drop` impl (around line 3364, near the `EXIT_REQUESTED`/`EXIT_VALUE` cleanup), add:

```rust
// release interaction-environment if it was created (#97)
INTERACTION_ENV.with(|cell| {
    let env = cell.get();
    if !env.is_null() {
        unsafe { ffi::sexp_release_object(self.ctx, env) };
        cell.set(std::ptr::null_mut());
    }
});
```

**step 3: run `just lint`**

**step 4: commit**

```bash
git add tein/src/context.rs
git commit -m "feat(context): INTERACTION_ENV thread-local + cleanup (#97)"
```

---

## task 4: `environment` trampoline — test first

**files:**
- modify: `tein/src/context.rs` — tests section (bottom of file)

**step 1: write failing tests**

add tests near the other sandbox tests (~line 4536):

```rust
#[test]
fn test_sandboxed_environment_allowed_modules() {
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .build()
        .unwrap();
    // (scheme base) is in Safe allowlist — environment should succeed
    let result = ctx.evaluate(
        "(import (scheme eval)) (let ((e (environment '(scheme base)))) (eval '(+ 1 2) e))"
    );
    assert_eq!(result.unwrap(), Value::Integer(3));
}

#[test]
fn test_sandboxed_environment_disallowed_module() {
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .build()
        .unwrap();
    // (chibi ast) is NOT in Safe allowlist — should error
    let result = ctx.evaluate(
        "(import (scheme eval)) (environment '(chibi ast))"
    );
    assert!(result.is_err() || matches!(result, Ok(Value::String(ref s)) if s.contains("not in sandbox allowlist")));
}

#[test]
fn test_sandboxed_environment_empty() {
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .build()
        .unwrap();
    // (environment) with no args should return an empty env
    let result = ctx.evaluate(
        "(import (scheme eval)) (environment? (environment))"
    );
    // environment? may not exist — just check no error
    // alternatively: eval something trivial
    let result = ctx.evaluate(
        "(import (scheme eval)) (let ((e (environment))) (eval '42 e))"
    );
    assert_eq!(result.unwrap(), Value::Integer(42));
}

#[test]
fn test_sandboxed_environment_via_scheme_load() {
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .build()
        .unwrap();
    // environment should also be accessible from (scheme load)
    let result = ctx.evaluate(
        "(import (scheme load)) (let ((e (environment '(scheme base)))) (eval '(+ 10 20) e))"
    );
    // eval is not exported by (scheme load) — need to import (scheme eval) too
    let result = ctx.evaluate(
        "(import (scheme eval) (scheme load)) (let ((e (environment '(scheme base)))) (eval '(+ 10 20) e))"
    );
    assert_eq!(result.unwrap(), Value::Integer(30));
}
```

**step 2: run tests to verify they fail**

```bash
cargo test -p tein test_sandboxed_environment -- --nocapture 2>&1 | head -40
```

expected: fail (scheme/eval not importable in sandbox yet).

**step 3: commit failing tests**

```bash
git add tein/src/context.rs
git commit -m "test: failing tests for sandboxed environment (#97)"
```

---

## task 5: `environment` trampoline — implementation

**files:**
- modify: `tein/src/context.rs` — near `load_trampoline` (~line 1159)

**step 1: implement `environment_trampoline`**

add after `load_trampoline` (around line 1235):

```rust
/// trampoline for `tein-environment-internal`: validates import specs against
/// VFS allowlist, then delegates to chibi's `mutable-environment` in the meta env.
/// used by `(scheme eval)` and `(scheme load)` shadows.
///
/// scheme signature: `(tein-environment-internal spec ...)` → environment
///
/// each spec must be a proper list of symbols/numbers (e.g. `'(scheme base)`).
/// in sandboxed contexts, each spec is checked against the VFS allowlist.
/// returns an immutable environment containing bindings from all specified modules.
unsafe extern "C" fn environment_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    use crate::sandbox::{IS_SANDBOXED, VFS_ALLOWLIST};

    // collect specs into a scheme list (args is already a list from variadic)
    // but first validate each spec against the allowlist if sandboxed
    let is_sandboxed = IS_SANDBOXED.with(|c| c.get());

    if is_sandboxed {
        // walk the args list and validate each spec
        let mut cur = args;
        while unsafe { ffi::sexp_pairp(cur) } {
            let spec = unsafe { ffi::sexp_car(cur) };
            // convert spec (a scheme list like (scheme base)) to path string
            match spec_to_path(ctx, spec) {
                Ok(path) => {
                    let allowed = VFS_ALLOWLIST.with(|cell| {
                        let list = cell.borrow();
                        list.iter().any(|prefix| path.starts_with(prefix.as_str()))
                    });
                    if !allowed {
                        let msg = format!("module not in sandbox allowlist: {path}");
                        let c_msg = std::ffi::CString::new(msg).unwrap();
                        return unsafe {
                            ffi::tein_make_error(
                                ctx,
                                c_msg.as_ptr(),
                                c_msg.as_bytes().len() as ffi::sexp_sint_t,
                            )
                        };
                    }
                }
                Err(err_sexp) => return err_sexp,
            }
            cur = unsafe { ffi::sexp_cdr(cur) };
        }
    }

    // look up mutable-environment in meta env
    let meta_env = unsafe { ffi::sexp_global_meta_env(ctx) };
    if meta_env.is_null() || unsafe { ffi::sexp_exceptionp(meta_env) } {
        let msg = std::ffi::CString::new("meta environment not available").unwrap();
        return unsafe {
            ffi::tein_make_error(ctx, msg.as_ptr(), msg.as_bytes().len() as ffi::sexp_sint_t)
        };
    }

    let sym = unsafe {
        ffi::sexp_intern(
            ctx,
            c"mutable-environment".as_ptr(),
            "mutable-environment".len() as ffi::sexp_sint_t,
        )
    };
    let _sym_root = unsafe { ffi::GcRoot::new(ctx, sym) };

    let proc = unsafe { ffi::sexp_env_ref(ctx, meta_env, sym, ffi::SEXP_FALSE) };
    if proc == ffi::SEXP_FALSE || proc.is_null() {
        let msg = std::ffi::CString::new("mutable-environment not found in meta env").unwrap();
        return unsafe {
            ffi::tein_make_error(ctx, msg.as_ptr(), msg.as_bytes().len() as ffi::sexp_sint_t)
        };
    }
    let _proc_root = unsafe { ffi::GcRoot::new(ctx, proc) };

    // apply mutable-environment to the specs list
    let result = unsafe { ffi::sexp_apply_proc(ctx, proc, args) };
    if unsafe { ffi::sexp_exceptionp(result) } {
        return result;
    }

    // make immutable (r7rs environment returns immutable env)
    unsafe { ffi::sexp_make_immutable(ctx, result) }
}

/// convert a scheme import spec (list of symbols/numbers) to a path string
/// e.g. `(scheme base)` → `"scheme/base"`, `(srfi 1)` → `"srfi/1"`
unsafe fn spec_to_path(ctx: ffi::sexp, spec: ffi::sexp) -> Result<String, ffi::sexp> {
    let mut parts = Vec::new();
    let mut cur = spec;
    while unsafe { ffi::sexp_pairp(cur) } {
        let item = unsafe { ffi::sexp_car(cur) };
        if unsafe { ffi::sexp_symbolp(item) } {
            let s = unsafe { ffi::sexp_symbol_to_str(item) };
            parts.push(s.to_string());
        } else if unsafe { ffi::sexp_integerp(item) } {
            let n = unsafe { ffi::sexp_unbox_fixnum(item) };
            parts.push(n.to_string());
        } else {
            let msg = std::ffi::CString::new("invalid import spec element").unwrap();
            return Err(unsafe {
                ffi::tein_make_error(
                    ctx,
                    msg.as_ptr(),
                    msg.as_bytes().len() as ffi::sexp_sint_t,
                )
            });
        }
        cur = unsafe { ffi::sexp_cdr(cur) };
    }
    Ok(parts.join("/"))
}
```

**notes for implementor:**
- check `ffi.rs` for exact function signatures — `sexp_symbol_to_str`, `sexp_integerp`, `sexp_unbox_fixnum`, `sexp_symbolp`, `sexp_pairp`, `sexp_car`, `sexp_cdr` all need to exist. search ffi.rs for each; add wrappers if missing.
- `SEXP_FALSE` needs to be accessible — check `ffi.rs` for its definition.
- the `c"..."` literal requires rust edition 2021+. if not available, use `CString::new(...).unwrap().as_ptr()`.

**step 2: register the trampoline**

in the standard env setup, near where `tein-load-vfs-internal` is registered (~line 3231), add:

```rust
self.define_fn_variadic("tein-environment-internal", environment_trampoline)?;
```

**step 3: run tests**

```bash
cargo test -p tein test_sandboxed_environment -- --nocapture 2>&1 | head -60
```

tests won't pass yet — VFS shadow not updated. but the trampoline should compile.

**step 4: commit**

```bash
git add tein/src/context.rs
git commit -m "feat(context): environment trampoline with VFS allowlist validation (#97)"
```

---

## task 6: `interaction-environment` trampoline — test first

**files:**
- modify: `tein/src/context.rs` — tests section

**step 1: write failing tests**

```rust
#[test]
fn test_sandboxed_interaction_environment_mutable() {
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .build()
        .unwrap();
    // define a binding in interaction-environment, then retrieve it
    let result = ctx.evaluate(
        "(import (scheme eval) (scheme repl))
         (eval '(define x 42) (interaction-environment))
         (eval 'x (interaction-environment))"
    );
    assert_eq!(result.unwrap(), Value::Integer(42));
}

#[test]
fn test_sandboxed_interaction_environment_persistent() {
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .build()
        .unwrap();
    // first eval: define
    ctx.evaluate(
        "(import (scheme eval) (scheme repl))
         (eval '(define y 99) (interaction-environment))"
    ).unwrap();
    // second eval: retrieve — should still be there
    let result = ctx.evaluate(
        "(import (scheme eval) (scheme repl))
         (eval 'y (interaction-environment))"
    );
    assert_eq!(result.unwrap(), Value::Integer(99));
}

#[test]
fn test_sandboxed_interaction_environment_has_base_bindings() {
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .build()
        .unwrap();
    // interaction-environment should have (scheme base) bindings
    let result = ctx.evaluate(
        "(import (scheme eval) (scheme repl))
         (eval '(+ 1 2) (interaction-environment))"
    );
    assert_eq!(result.unwrap(), Value::Integer(3));
}
```

**step 2: run to verify failure**

```bash
cargo test -p tein test_sandboxed_interaction_environment -- --nocapture 2>&1 | head -40
```

**step 3: commit**

```bash
git add tein/src/context.rs
git commit -m "test: failing tests for sandboxed interaction-environment (#97)"
```

---

## task 7: `interaction-environment` trampoline — implementation

**files:**
- modify: `tein/src/context.rs` — near the `environment_trampoline`

**step 1: implement `interaction_environment_trampoline`**

add after `environment_trampoline`:

```rust
/// trampoline for `tein-interaction-environment-internal`: returns a persistent
/// mutable environment for REPL-style interaction in sandbox.
///
/// first call: creates a mutable env from the current VFS allowlist modules,
/// GC-roots it, stores in `INTERACTION_ENV` thread-local.
/// subsequent calls: returns the stored env.
///
/// scheme signature: `(tein-interaction-environment-internal)` → environment
unsafe extern "C" fn interaction_environment_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    _args: ffi::sexp,
) -> ffi::sexp {
    use crate::sandbox::{IS_SANDBOXED, VFS_ALLOWLIST};

    INTERACTION_ENV.with(|cell| {
        let existing = cell.get();
        if !existing.is_null() {
            return existing;
        }

        // build specs list from VFS allowlist
        // we need to reconstruct scheme import specs from the allowlist paths.
        // the allowlist contains strings like "scheme/base", "scheme/write", etc.
        // convert each to a scheme list: "scheme/base" → (scheme base)
        let is_sandboxed = IS_SANDBOXED.with(|c| c.get());

        let specs = if is_sandboxed {
            VFS_ALLOWLIST.with(|al| {
                let list = al.borrow();
                let mut specs = ffi::SEXP_NULL;
                let _specs_root = unsafe { ffi::GcRoot::new(ctx, specs) };
                for path in list.iter().rev() {
                    match path_to_spec(ctx, path) {
                        Ok(spec) => {
                            specs = unsafe { ffi::sexp_cons(ctx, spec, specs) };
                        }
                        Err(_) => continue, // skip malformed paths
                    }
                }
                specs
            })
        } else {
            // non-sandboxed: just use (scheme base)
            let base_spec = path_to_spec(ctx, "scheme/base").unwrap_or(ffi::SEXP_NULL);
            unsafe { ffi::sexp_cons(ctx, base_spec, ffi::SEXP_NULL) }
        };

        // look up mutable-environment in meta env
        let meta_env = unsafe { ffi::sexp_global_meta_env(ctx) };
        if meta_env.is_null() {
            let msg = std::ffi::CString::new("meta environment not available").unwrap();
            return unsafe {
                ffi::tein_make_error(ctx, msg.as_ptr(), msg.as_bytes().len() as ffi::sexp_sint_t)
            };
        }

        let sym = unsafe {
            ffi::sexp_intern(
                ctx,
                c"mutable-environment".as_ptr(),
                "mutable-environment".len() as ffi::sexp_sint_t,
            )
        };
        let _sym_root = unsafe { ffi::GcRoot::new(ctx, sym) };

        let proc = unsafe { ffi::sexp_env_ref(ctx, meta_env, sym, ffi::SEXP_FALSE) };
        if proc == ffi::SEXP_FALSE || proc.is_null() {
            let msg =
                std::ffi::CString::new("mutable-environment not found in meta env").unwrap();
            return unsafe {
                ffi::tein_make_error(ctx, msg.as_ptr(), msg.as_bytes().len() as ffi::sexp_sint_t)
            };
        }
        let _proc_root = unsafe { ffi::GcRoot::new(ctx, proc) };

        // apply mutable-environment to specs — do NOT make immutable
        let env = unsafe { ffi::sexp_apply_proc(ctx, proc, specs) };
        if unsafe { ffi::sexp_exceptionp(env) } {
            return env;
        }

        // GC-root and store
        unsafe { ffi::sexp_preserve_object(ctx, env) };
        cell.set(env);
        env
    })
}

/// convert a path string like "scheme/base" to a scheme list `(scheme base)`.
/// number segments (e.g. "srfi/1") become fixnums.
unsafe fn path_to_spec(ctx: ffi::sexp, path: &str) -> Result<ffi::sexp, ()> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.is_empty() {
        return Err(());
    }
    let mut result = ffi::SEXP_NULL;
    for part in parts.iter().rev() {
        let elem = if let Ok(n) = part.parse::<i64>() {
            unsafe { ffi::sexp_make_fixnum(n) }
        } else {
            let c_str = std::ffi::CString::new(*part).map_err(|_| ())?;
            unsafe { ffi::sexp_intern(ctx, c_str.as_ptr(), part.len() as ffi::sexp_sint_t) }
        };
        result = unsafe { ffi::sexp_cons(ctx, elem, result) };
    }
    Ok(result)
}
```

**notes for implementor:**
- `SEXP_NULL` — check ffi.rs for its definition. it's the scheme empty list.
- `sexp_make_fixnum` — check ffi.rs. if it's a macro in C, there may be a `tein_sexp_make_fixnum` wrapper.
- the `path_to_spec` GC rooting may need attention — `sexp_intern` and `sexp_cons` allocate. the list is built tail-first (rev iter + cons), so intermediates need rooting. however, since these are short lists (typically 2 elements), the conservative approach of building without explicit rooting per step may work if chibi's nursery is large enough. add GcRoot if tests show GC issues.

**step 2: register the trampoline**

near where `tein-environment-internal` was registered, add:

```rust
self.define_fn_variadic(
    "tein-interaction-environment-internal",
    interaction_environment_trampoline,
)?;
```

**step 3: run `just lint`**

**step 4: commit**

```bash
git add tein/src/context.rs
git commit -m "feat(context): interaction-environment trampoline with persistent mutable env (#97)"
```

---

## task 8: VFS registry — update shadows

**files:**
- modify: `tein/src/vfs_registry.rs` — entries for scheme/eval (~line 373), scheme/load (~line 464), scheme/repl (~line 855)

**step 1: update `scheme/eval` entry**

change from `Embedded, default_safe: false` to `Shadow, default_safe: true`:

```rust
    // scheme/eval: VFS shadow — sandboxed environment validated against VFS allowlist.
    // eval is re-exported from (chibi); environment delegates to
    // tein-environment-internal trampoline which checks the allowlist.
    // closes #97.
    VfsEntry {
        path: "scheme/eval",
        deps: &[],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: Some("\
(define-library (scheme eval)
  (import (chibi))
  (export eval environment)
  (begin
    (define (environment . specs)
      (apply tein-environment-internal specs))))
"),
    },
```

**step 2: update `scheme/load` shadow**

update the existing shadow (~line 464) to also export `environment`:

```rust
    // scheme/load: VFS shadow — re-exports VFS-restricted load from (tein load)
    // and sandboxed environment from tein-environment-internal trampoline.
    // see also: tein/load.sld exports load as (rename tein-load-vfs-internal load).
    VfsEntry {
        path: "scheme/load",
        deps: &["tein/load"],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: Some("\
(define-library (scheme load)
  (import (tein load) (chibi))
  (export load environment)
  (begin
    (define (environment . specs)
      (apply tein-environment-internal specs))))
"),
    },
```

note: added `(chibi)` import since `apply` and `tein-environment-internal` come from the top-level env.

**step 3: update `scheme/repl` shadow**

update the existing shadow (~line 855) to use the new trampoline:

```rust
    // scheme/repl: VFS shadow — sandboxed interaction-environment returns
    // a persistent mutable env that accumulates definitions across evals.
    // r7rs compliant: interaction-environment is mutable per context.
    // closes #97.
    VfsEntry {
        path: "scheme/repl",
        deps: &[],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: Some("\
(define-library (scheme repl)
  (import (chibi))
  (export interaction-environment)
  (begin
    (define (interaction-environment)
      (tein-interaction-environment-internal))))
"),
    },
```

**step 4: run tests**

```bash
cargo test -p tein test_sandboxed_environment -- --nocapture
cargo test -p tein test_sandboxed_interaction_environment -- --nocapture
```

all tests from tasks 4 and 6 should now pass.

**step 5: run full test suite**

```bash
just test
```

check for regressions — especially existing sandbox tests and VFS module tests.

**step 6: commit**

```bash
git add tein/src/vfs_registry.rs
git commit -m "feat(sandbox): scheme/eval, scheme/load, scheme/repl shadows with allowlist enforcement (#97)"
```

---

## task 9: cascading `default_safe` updates

**files:**
- modify: `tein/src/vfs_registry.rs` — `scheme/small` (~line 757), `srfi/64` (~line 2365)

**step 1: evaluate scheme/small deps**

check that ALL deps of `scheme/small` are now `default_safe: true`:
- scheme/base ✓, scheme/char ✓, scheme/complex ✓, scheme/cxr ✓
- scheme/eval ✓ (just changed), scheme/file ✓, scheme/inexact ✓, scheme/lazy ✓
- scheme/load ✓ (already was), scheme/process-context ✓, scheme/read ✓
- scheme/repl ✓ (already was), scheme/time (feature-gated on "time")
- scheme/write ✓

**scheme/time** is feature-gated — if "time" feature is off, it's still embedded with `default_safe: false`. this means `scheme/small` cannot be unconditionally `default_safe: true`. leave it as-is for now, or make it conditional. **decision: leave `scheme/small` as `default_safe: false`** — document why. same reasoning applies to `scheme/red`.

**step 2: update srfi/64**

`srfi/64` depends only on `scheme/base`, `scheme/write`, `scheme/eval`. all now safe:

```rust
    VfsEntry {
        path: "srfi/64",
        deps: &["scheme/base", "scheme/write", "scheme/eval"],
        files: &["lib/srfi/64.sld", "lib/srfi/64.scm"],
        clib: None,
        default_safe: true, // scheme/eval now safe (#97)
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
```

**step 3: add a comment to scheme/small explaining why it stays unsafe**

```rust
    VfsEntry {
        // r7rs "small" standard — the 14-library bundle. default_safe: false
        // because scheme/time is feature-gated ("time") and falls back to the
        // embedded chibi version (which is default_safe: false) when the
        // feature is off. use .allow_module("scheme/small") to enable explicitly.
        path: "scheme/small",
        ...
```

**step 4: run tests**

```bash
cargo test -p tein --test vfs_module_tests -- --nocapture 2>&1 | tail -20
just test
```

**step 5: commit**

```bash
git add tein/src/vfs_registry.rs
git commit -m "feat(vfs): srfi/64 now default_safe, scheme/small stays gated on scheme/time (#97)"
```

---

## task 10: docs + AGENTS.md update

**files:**
- modify: `AGENTS.md` — sandboxing flow, architecture section
- modify: `docs/guide.md` or relevant docs file — sandboxing section

**step 1: update AGENTS.md**

add to the sandboxing flow section:

> **sandboxed eval/environment flow**: `(import (scheme eval))` in sandbox → shadow re-exports `eval` from `(chibi)` + defines `environment` as `(apply tein-environment-internal specs)` → trampoline validates each spec against `VFS_ALLOWLIST` → delegates to `mutable-environment` from `SEXP_G_META_ENV` via `sexp_apply` → `sexp_make_immutable` → returns frozen env. `(import (scheme load))` re-exports same `environment` + `load` from `(tein load)`. `(import (scheme repl))` → `interaction-environment` returns a persistent mutable env (thread-local `INTERACTION_ENV`, GC-rooted, cleared on `Context::drop()`).

add to critical gotchas:

> **`interaction-environment` is per-context thread-local**: the mutable env is lazily created on first call and persists for the context's lifetime. `Context::drop()` releases it. two contexts on different threads each get their own interaction env.

update the architecture table to include the new trampolines.

**step 2: update docs**

add a note in the sandboxing section of `docs/guide.md` (or equivalent) about `(scheme eval)`, `(scheme load)`, and `(scheme repl)` now being available in sandbox.

**step 3: run `just lint`**

**step 4: commit**

```bash
git add AGENTS.md docs/
git commit -m "docs: sandboxed eval/load/repl documentation (#97), closes #97"
```

---

## task 11: final verification

**step 1: run full test suite**

```bash
just test
```

**step 2: run lint**

```bash
just lint
```

**step 3: review all changes**

```bash
git log --oneline dev..HEAD
git diff --stat dev..HEAD
```

**step 4: update implementation plan with notes**

note any caveats, AGENTS.md additions discovered during implementation.

---

## implementation notes for the executing agent

- **branch**: `just feature sandboxed-eval-2603` (creates `feature/sandboxed-eval-2603` from `dev`)
- **chibi fork work** (task 1) must happen in `~/forks/chibi-scheme`, NOT in `target/chibi-scheme`
- **GC rooting**: be careful with intermediates in `path_to_spec` and the interaction env builder — `sexp_intern` and `sexp_cons` allocate and can trigger GC
- **`ffi.rs` gaps**: the plan assumes certain wrappers exist (`sexp_symbolp`, `sexp_make_fixnum`, `SEXP_NULL`, `SEXP_FALSE`). check ffi.rs and add any missing wrappers — follow existing patterns
- **`c"..."` literals**: require edition 2021. check `Cargo.toml` — if edition is older, use `CString::new().unwrap()`
- **`scheme/load` shadow**: needs `(import (chibi))` in addition to `(tein load)` for access to `apply` and the globally-registered trampoline
- **test design**: the failing tests in tasks 4 and 6 test the *interface* — they should all pass after task 8 (shadow updates). if any fail earlier (tasks 5/7), that's expected
- **`scheme/small` cascade**: intentionally NOT flipped to `default_safe: true` due to scheme/time feature gate dependency
