# VfsSafe / VfsAll module policy tiers — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** replace the binary VfsOnly/Unrestricted module policy with a three-tier system: Allowlist (default safe set), VfsAll (all curated VFS), Unrestricted — with a builder API for customising the allowlist.

**Architecture:** C handles Unrestricted (policy 0) and VfsAll (policy 1) directly. for Allowlist (policy 2), C calls a rust `extern "C"` callback that checks the module path against a thread-local `Vec<String>` of allowed prefixes. the allowlist is populated by the builder and cleared on context drop via the existing RAII pattern.

**Design doc:** `docs/designs/2026-02-27-vfs-module-policy-tiers.md`
**Branch:** `dev` (commits are going directly on dev per project convention)
**GitHub issue:** #86

---

## progress

- [x] task 1 — `sandbox.rs`: new enum, thread-locals, SAFE_MODULES, IMPLICIT_DEPS (commit `5a4a026`)
- [x] task 2 — `tein_shim.c`: three-tier C gate + extern declaration (commit in fork `f9a10fad`, `target/chibi-scheme` synced)
- [x] task 3 — `ffi.rs` / `context.rs` / `value.rs`: callback, builder API, RAII (commit `ef17ef9`)
- [ ] task 4 — update existing module policy tests ← **resume here**
- [ ] task 5 — add new tests for allowlist features
- [ ] task 6 — update public API surface and docs
- [ ] task 7 — final verification

**current state:** `cargo build` is clean. `just test` fails only because existing tests still compare `u8` against the removed `ModulePolicy::VfsOnly` — see task 4.

---

## what was done (summary for bootstrap)

### sandbox.rs (`tein/src/sandbox.rs`)

`ModulePolicy` is now a public `enum` with three variants:

```rust
pub enum ModulePolicy {
    Unrestricted,
    VfsAll,
    Allowlist(Vec<String>),
}
```

`MODULE_POLICY` is now `Cell<u8>` (not `Cell<ModulePolicy>`). constants:

```rust
pub(crate) const POLICY_UNRESTRICTED: u8 = 0;
pub(crate) const POLICY_VFS_ALL: u8 = 1;
pub(crate) const POLICY_ALLOWLIST: u8 = 2;
```

new thread-local:

```rust
pub(crate) static MODULE_ALLOWLIST: RefCell<Vec<String>>
```

new public items: `SAFE_MODULES: &[&str]`, `IMPLICIT_DEPS: &[&str]` (pub(crate)), `default_allowlist() -> Vec<String>` (pub(crate)).

### tein_shim.c (`target/chibi-scheme/tein_shim.c`, fork `~/forks/chibi-scheme`)

`tein_module_allowed()` now has three branches:

```c
if (tein_module_policy == 0) return 1;                    /* unrestricted */
if (strncmp(path, "/vfs/lib/", 9) != 0) return 0;
if (strstr(path, "..") != NULL) return 0;
if (tein_module_policy == 1) return 1;                    /* vfs-all */
return tein_module_allowlist_check(path);                  /* allowlist */
```

`extern int tein_module_allowlist_check(const char *path)` declared; defined in `ffi.rs`.

### ffi.rs (`tein/src/ffi.rs`)

- added `use std::ffi::CStr`
- added `tein_module_allowlist_check` (`#[unsafe(no_mangle)] extern "C"`) — strips `/vfs/lib/` prefix, checks against `MODULE_ALLOWLIST`
- updated `module_policy_set` doc: "0 = unrestricted, 1 = vfs-all, 2 = allowlist"

### context.rs (`tein/src/context.rs`)

`ContextBuilder` has new field `module_policy: Option<ModulePolicy>` (init `None`).

new builder methods (after `file_write`):
- `vfs_all(mut self) -> Self` — sets `ModulePolicy::VfsAll`
- `allow_module(mut self, prefix: &str) -> Self` — ensures allowlist, adds prefix
- `allow_only_modules(mut self, prefixes: &[&str]) -> Self` — replaces safe set, always includes IMPLICIT_DEPS
- `fn ensure_allowlist(&mut self)` — private, initialises to `default_allowlist()` if not already Allowlist

`build()` now resolves policy:

```rust
let has_sandbox = self.standard_env && self.allowed_primitives.is_some();
let resolved_policy = self.module_policy.take().unwrap_or_else(|| {
    if has_sandbox { ModulePolicy::Allowlist(default_allowlist()) }
    else { ModulePolicy::Unrestricted }
});
let has_module_policy = !matches!(resolved_policy, ModulePolicy::Unrestricted);
let prev_module_policy = MODULE_POLICY.with(|cell| cell.get());  // u8
let prev_module_allowlist = MODULE_ALLOWLIST.with(|cell| cell.borrow().clone());
if has_module_policy {
    let level = resolved_policy.level();
    MODULE_POLICY.with(|cell| cell.set(level));
    ffi::module_policy_set(level as i32);
    if let ModulePolicy::Allowlist(ref list) = resolved_policy {
        MODULE_ALLOWLIST.with(|cell| { *cell.borrow_mut() = list.clone(); });
    }
}
```

`Context` struct: `prev_module_policy: u8` (was `ModulePolicy`), `prev_module_allowlist: Vec<String>` (new).

`Drop` restores both:

```rust
if self.has_module_policy {
    MODULE_POLICY.with(|cell| cell.set(self.prev_module_policy));
    unsafe { ffi::module_policy_set(self.prev_module_policy as i32) };
    MODULE_ALLOWLIST.with(|cell| {
        *cell.borrow_mut() = std::mem::take(&mut self.prev_module_allowlist);
    });
}
```

### value.rs (`tein/src/value.rs`)

sandbox violation check:

```rust
// was: cell.get() == ModulePolicy::VfsOnly
let is_sandboxed = MODULE_POLICY.with(|cell| cell.get() != crate::sandbox::POLICY_UNRESTRICTED);
```

---

## task 4: update existing tests

**files:**
- modify: `tein/src/context.rs` (test module, around lines 4488–4575)

find the five tests below by name (use grep) and replace as shown.

**test_module_policy_blocks_non_vfs**

```rust
#[test]
fn test_module_policy_blocks_non_vfs() {
    use crate::sandbox::*;
    let ctx = Context::builder()
        .standard_env()
        .preset(&ARITHMETIC)
        .build()
        .expect("standard + sandbox");

    MODULE_POLICY.with(|cell| {
        assert_eq!(
            cell.get(),
            POLICY_ALLOWLIST,
            "sandboxed standard env should activate allowlist policy"
        );
    });

    drop(ctx);
}
```

**test_module_policy_unrestricted_without_sandbox**

```rust
#[test]
fn test_module_policy_unrestricted_without_sandbox() {
    let ctx = Context::new_standard().expect("new_standard");

    MODULE_POLICY.with(|cell| {
        assert_eq!(
            cell.get(),
            crate::sandbox::POLICY_UNRESTRICTED,
            "unsandboxed standard env should be unrestricted"
        );
    });

    drop(ctx);
}
```

**test_module_policy_cleared_on_drop**

```rust
#[test]
fn test_module_policy_cleared_on_drop() {
    use crate::sandbox::*;
    {
        let _ctx = Context::builder()
            .standard_env()
            .preset(&ARITHMETIC)
            .build()
            .expect("standard + sandbox");

        MODULE_POLICY.with(|cell| {
            assert_eq!(cell.get(), POLICY_ALLOWLIST);
        });
    }
    MODULE_POLICY.with(|cell| {
        assert_eq!(
            cell.get(),
            POLICY_UNRESTRICTED,
            "module policy should reset to unrestricted after context drop"
        );
    });
}
```

**test_module_policy_not_set_without_standard_env**

```rust
#[test]
fn test_module_policy_not_set_without_standard_env() {
    use crate::sandbox::*;
    let ctx = Context::builder()
        .preset(&ARITHMETIC)
        .build()
        .expect("sandbox without standard env");

    MODULE_POLICY.with(|cell| {
        assert_eq!(
            cell.get(),
            POLICY_UNRESTRICTED,
            "non-standard-env sandbox should not set module policy"
        );
    });

    drop(ctx);
}
```

**test_sequential_context_policy_isolation** — find the assert that checks `ModulePolicy::VfsOnly` and replace with `POLICY_ALLOWLIST`. the structure of the test stays the same.

**after updating all five tests:**

- `test_module_policy_blocks_filesystem_import` — verify it still passes unchanged (default allowlist doesn't include `chibi/process`)
- `test_standard_env_sandbox_allows_vfs_import` — verify it still passes unchanged (`scheme/write` and `scheme/base` are in SAFE_MODULES)

run: `just test 2>&1 | tail -20`
expected: all existing tests pass

**commit:**

```
test: update module policy tests for three-tier model (#86)
```

---

## task 5: add new tests for allowlist features

**files:**
- modify: `tein/src/context.rs` (test module — add after the existing module policy tests)

add these five tests:

```rust
#[test]
fn test_module_policy_vfs_all() {
    use crate::sandbox::*;
    let ctx = Context::builder()
        .standard_env()
        .preset(&ARITHMETIC)
        .allow(&["import"])
        .vfs_all()
        .build()
        .expect("standard + sandbox + vfs_all");

    MODULE_POLICY.with(|cell| {
        assert_eq!(cell.get(), POLICY_VFS_ALL);
    });

    // (chibi string) is in VFS but not in SAFE_MODULES — should work under VfsAll
    let r = ctx.evaluate("(import (chibi string))");
    assert!(r.is_ok(), "(import (chibi string)) should succeed under VfsAll: {:?}", r.err());

    // filesystem module should still fail
    let err = ctx.evaluate("(import (chibi process))").unwrap_err();
    assert!(matches!(err, Error::SandboxViolation(_)),
        "filesystem import should fail under VfsAll: {:?}", err);

    drop(ctx);
}

#[test]
fn test_module_policy_allow_module() {
    use crate::sandbox::*;
    let ctx = Context::builder()
        .standard_env()
        .preset(&ARITHMETIC)
        .allow(&["import"])
        .allow_module("chibi/string")
        .build()
        .expect("standard + sandbox + allow_module");

    MODULE_POLICY.with(|cell| {
        assert_eq!(cell.get(), POLICY_ALLOWLIST);
    });

    // chibi/string was explicitly allowed
    let r = ctx.evaluate("(import (chibi string))");
    assert!(r.is_ok(), "(import (chibi string)) should succeed: {:?}", r.err());

    drop(ctx);
}

#[test]
fn test_module_policy_allow_only_modules() {
    use crate::sandbox::*;
    let ctx = Context::builder()
        .standard_env()
        .preset(&ARITHMETIC)
        .allow(&["import"])
        .allow_only_modules(&["tein/test"])
        .build()
        .expect("standard + sandbox + allow_only");

    // tein/test should work (explicitly listed)
    let r = ctx.evaluate("(import (tein test))");
    assert!(r.is_ok(), "(import (tein test)) should succeed: {:?}", r.err());

    // scheme/write is NOT in the custom list (only in SAFE_MODULES default)
    let err = ctx.evaluate("(import (scheme write))").unwrap_err();
    assert!(matches!(err, Error::SandboxViolation(_)),
        "(import (scheme write)) should fail with allow_only: {:?}", err);

    drop(ctx);
}

#[test]
fn test_module_policy_allowlist_raii() {
    use crate::sandbox::*;
    // verify allowlist is restored, not just the policy level
    {
        let _ctx = Context::builder()
            .standard_env()
            .preset(&ARITHMETIC)
            .allow(&["import"])
            .allow_module("chibi/string")
            .build()
            .expect("context with extended allowlist");
    }
    // after drop, allowlist should be empty (previous was empty)
    MODULE_ALLOWLIST.with(|cell| {
        assert!(cell.borrow().is_empty(),
            "allowlist should be restored to empty after drop");
    });
}

#[test]
fn test_module_policy_transitive_deps() {
    use crate::sandbox::*;
    // (tein test) depends on (srfi 9) transitively.
    // IMPLICIT_DEPS includes srfi/9, so this should work even with allow_only.
    let ctx = Context::builder()
        .standard_env()
        .preset(&ARITHMETIC)
        .allow(&["import"])
        .allow_only_modules(&["tein/test"])
        .build()
        .expect("minimal allowlist");

    let r = ctx.evaluate("(import (tein test))");
    assert!(r.is_ok(),
        "tein/test should load despite minimal allowlist (implicit deps): {:?}", r.err());

    drop(ctx);
}
```

run: `just test 2>&1 | tail -20`
expected: all tests pass including new ones

**commit:**

```
test: add tests for allowlist, vfs_all, allow_module, allow_only_modules (#86)
```

---

## task 6: update public API surface and docs

**files:**
- check: `tein/src/lib.rs` (re-exports)
- modify: `tein/src/sandbox.rs` (module doc)
- modify: `AGENTS.md`

**step 1: verify re-exports**

`sandbox` is already `pub mod` in lib.rs. `ModulePolicy` is `pub` and `SAFE_MODULES` is `pub` — both reachable as `tein::sandbox::ModulePolicy` and `tein::sandbox::SAFE_MODULES`. no changes needed; just confirm.

**step 2: update sandbox.rs module doc**

the line:

```
//! 4. **Module policy** — restrict `(import ...)` to safe modules (three-tier: Allowlist / VfsAll / Unrestricted)
```

is already updated. verify the `# Module policy` section at the bottom of the module doc block also reads correctly (three tiers listed).

**step 3: update AGENTS.md**

find the `**module policy flow**:` line and replace with:

```
**module policy flow**: ContextBuilder with standard_env + presets → resolve policy (explicit builder policy, or default Allowlist(SAFE_MODULES + IMPLICIT_DEPS) for sandboxed, Unrestricted otherwise) → set MODULE_POLICY level (u8) + MODULE_ALLOWLIST (Vec<String>) thread-locals + C-level tein_module_policy → sexp_find_module_file_raw calls tein_module_allowed() → policy 0: allow all, policy 1: VFS prefix check only, policy 2: rust callback (tein_module_allowlist_check) strips /vfs/lib/ prefix and checks against MODULE_ALLOWLIST → policy + allowlist cleared on Context::drop() via RAII
```

**step 4: run lint**

```
just lint 2>&1 | tail -10
```

expected: clean

**commit:**

```
docs: update module policy docs for three-tier model (#86)
```

---

## task 7: final verification

**step 1:** `just test 2>&1 | tail -20` — all pass

**step 2:** `just lint 2>&1 | tail -10` — clean

**step 3:** `cargo test --test scheme_tests -- reader_macro_sandbox --nocapture 2>&1` — passes (imports `(tein test)`, `(tein reader)`, `(tein macro)` — all under `tein/` prefix which is in SAFE_MODULES)

**step 4:** if all green, use `superpowers:finishing-a-development-branch` to wrap up. collect any AGENTS.md notes.
