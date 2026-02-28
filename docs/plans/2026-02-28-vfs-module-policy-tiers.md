# VfsSafe / VfsAll module policy tiers — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** replace the binary VfsOnly/Unrestricted module policy with a three-tier system: Allowlist (default safe set), VfsAll (all curated VFS), Unrestricted — with a builder API for customising the allowlist.

**Architecture:** C handles Unrestricted (policy 0) and VfsAll (policy 1) directly. for Allowlist (policy 2), C calls a rust `extern "C"` callback that checks the module path against a thread-local `Vec<String>` of allowed prefixes. the allowlist is populated by the builder and cleared on context drop via the existing RAII pattern.

**Tech Stack:** rust FFI, C thread-locals, chibi-scheme tein_shim.c

---

## task 1: update ModulePolicy enum and thread-locals in sandbox.rs

**files:**
- modify: `tein/src/sandbox.rs:82-168`

**step 1: replace ModulePolicy enum and thread-locals**

replace the entire `ModulePolicy` enum (lines 143–163) and `MODULE_POLICY` thread-local (lines 165–168) with:

```rust
/// module import policy for sandboxed standard-env contexts.
///
/// controls which modules can be loaded via `(import ...)`.
///
/// ## tiers
///
/// | policy | what passes | use case |
/// |--------|------------|----------|
/// | `Allowlist` | only listed module prefixes (must be in VFS) | tight LLM sandbox |
/// | `VfsAll` | all curated VFS modules | sandbox with full scheme ecosystem |
/// | `Unrestricted` | VFS + filesystem | unsandboxed |
///
/// ## VFS safety contract
///
/// VFS modules are safe by construction: tein curates the embedded virtual
/// filesystem to ensure no module can bypass the existing safety layers
/// (preset allowlists, FsPolicy, fuel/timeout). capabilities exposed by
/// VFS modules remain subject to these controls.
///
/// ## default behaviour
///
/// sandboxed contexts (standard_env + presets) default to
/// `Allowlist(SAFE_MODULES + IMPLICIT_DEPS)`. use [`.vfs_all()`](crate::ContextBuilder::vfs_all)
/// or [`.allow_module()`](crate::ContextBuilder::allow_module) to adjust.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModulePolicy {
    /// all modules allowed (VFS + filesystem). no gate.
    Unrestricted,
    /// all curated VFS modules allowed. filesystem blocked.
    VfsAll,
    /// only listed module prefixes allowed (must also be in VFS).
    /// entries are path prefixes matched against the module path after `/vfs/lib/`.
    Allowlist(Vec<String>),
}

/// numeric policy level for C interop and cheap thread-local checks.
/// mirrors `tein_module_policy` in tein_shim.c.
pub(crate) const POLICY_UNRESTRICTED: u8 = 0;
pub(crate) const POLICY_VFS_ALL: u8 = 1;
pub(crate) const POLICY_ALLOWLIST: u8 = 2;

impl ModulePolicy {
    /// numeric policy level for C interop.
    pub(crate) fn level(&self) -> u8 {
        match self {
            ModulePolicy::Unrestricted => POLICY_UNRESTRICTED,
            ModulePolicy::VfsAll => POLICY_VFS_ALL,
            ModulePolicy::Allowlist(_) => POLICY_ALLOWLIST,
        }
    }
}

thread_local! {
    /// numeric policy level (0/1/2). cheap to read for error checks in value.rs.
    pub(crate) static MODULE_POLICY: Cell<u8> = const { Cell::new(POLICY_UNRESTRICTED) };

    /// the actual allowlist, populated when policy is Allowlist.
    /// read by the C→rust callback during module resolution.
    pub(crate) static MODULE_ALLOWLIST: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}
```

**step 2: add SAFE_MODULES and IMPLICIT_DEPS consts**

after the `MODULE_ALLOWLIST` thread-local, add:

```rust
/// minimal safe set — tein modules + core r7rs pure-computation modules.
///
/// used as the default allowlist for sandboxed contexts. each entry is a
/// module path prefix matched against the path after `/vfs/lib/` — so
/// `"tein/"` matches all `(tein ...)` modules.
///
/// use [`ContextBuilder::allow_module()`](crate::ContextBuilder::allow_module)
/// to add entries, or [`ContextBuilder::vfs_all()`](crate::ContextBuilder::vfs_all)
/// to allow all VFS modules.
pub const SAFE_MODULES: &[&str] = &[
    "tein/",
    "scheme/base",
    "scheme/case-lambda",
    "scheme/char",
    "scheme/complex",
    "scheme/cxr",
    "scheme/inexact",
    "scheme/lazy",
    "scheme/read",
    "scheme/write",
    "scheme/bytevector",
    "scheme/sort",
];

/// transitive dependencies of safe modules. always included in any allowlist.
///
/// these are implementation plumbing — modules that safe user-facing modules
/// import internally. users shouldn't need to import these directly, but they
/// must pass the gate when the module system resolves transitive deps.
pub(crate) const IMPLICIT_DEPS: &[&str] = &[
    "srfi/9", "srfi/11", "srfi/16", "srfi/38", "srfi/39",
    "srfi/69", "srfi/151",
    "chibi/char-set/", "chibi/equiv", "chibi/string",
    "chibi/ast", "chibi/io", "chibi/iset/",
];

/// build the default allowlist from SAFE_MODULES + IMPLICIT_DEPS.
pub(crate) fn default_allowlist() -> Vec<String> {
    SAFE_MODULES.iter().chain(IMPLICIT_DEPS.iter())
        .map(|s| s.to_string())
        .collect()
}
```

**step 3: update module-level doc comment**

update the `# Module policy` section in the module doc (lines 75–81) to reflect three tiers:

```rust
//! # Module policy
//!
//! Module imports in sandboxed standard-env contexts are restricted by a
//! three-tier policy:
//!
//! - **Allowlist** (default) — only [`SAFE_MODULES`] + transitive deps.
//!   extend with [`.allow_module()`](crate::ContextBuilder::allow_module).
//! - **VfsAll** — all curated VFS modules. set with [`.vfs_all()`](crate::ContextBuilder::vfs_all).
//! - **Unrestricted** — VFS + filesystem (unsandboxed contexts).
```

**step 4: build to check compilation**

run: `cargo build 2>&1 | tail -5`
expected: compilation errors in context.rs and value.rs (they still reference old `ModulePolicy::VfsOnly` and `Cell<ModulePolicy>`)

**step 5: commit (WIP, will fix dependents in later tasks)**

```
feat(sandbox): rework ModulePolicy to three-tier Allowlist/VfsAll/Unrestricted (#86)

adds SAFE_MODULES, IMPLICIT_DEPS consts and default_allowlist() helper.
MODULE_POLICY thread-local is now Cell<u8> for cheap checks.
MODULE_ALLOWLIST thread-local holds the actual Vec<String>.

note: context.rs and value.rs not yet updated — will break until task 3.
```

---

## task 2: update C gate in tein_shim.c

**files:**
- modify: `target/chibi-scheme/tein_shim.c:248-260`

**step 1: add extern declaration and extend tein_module_allowed**

replace lines 248–260 (the module policy section) with:

```c
// --- module import policy ---
//
// three-tier policy for (import ...) restriction:
//   0 = unrestricted (all modules allowed)
//   1 = vfs-all (only /vfs/lib/ paths, but any of them)
//   2 = allowlist (only paths approved by rust callback)

TEIN_THREAD_LOCAL int tein_module_policy = 0;

// rust callback for allowlist checks (defined in ffi.rs)
extern int tein_module_allowlist_check(const char *path);

// check if a module path is allowed under the current policy.
// called from eval.c patch A (sexp_find_module_file_raw).
int tein_module_allowed(const char *path) {
    if (tein_module_policy == 0) return 1;                    /* unrestricted */
    if (strncmp(path, "/vfs/lib/", 9) != 0) return 0;        /* all non-unrestricted block filesystem */
    if (strstr(path, "..") != NULL) return 0;                 /* path traversal guard */
    if (tein_module_policy == 1) return 1;                    /* vfs-all */
    return tein_module_allowlist_check(path);                  /* allowlist — ask rust */
}

// set the module policy. called from rust ffi.
void tein_module_policy_set(int policy) {
    tein_module_policy = policy;
}
```

**step 2: verify C compiles**

run: `cargo build 2>&1 | tail -5`
expected: linker error for `tein_module_allowlist_check` (not yet defined in rust) — that's fine, C compiled. if we get a compile error in C itself, that needs fixing.

actually: the linker error will prevent the build from succeeding. that's ok — we'll add the rust callback in the next task and everything will link.

**step 3: commit**

```
feat(shim): extend tein_module_allowed with three-tier policy (#86)

policy 0 = unrestricted, 1 = vfs-all, 2 = allowlist (calls rust callback).
the extern tein_module_allowlist_check is defined in ffi.rs (next commit).
```

---

## task 3: add rust callback and update context.rs / value.rs

**files:**
- modify: `tein/src/ffi.rs` (add callback + update module_policy_set doc)
- modify: `tein/src/context.rs` (imports, ContextBuilder, Context struct, build(), drop)
- modify: `tein/src/value.rs:464-467` (update sandbox violation check)

this is the largest task — it wires everything together. break it into steps.

**step 1: add the rust callback in ffi.rs**

after the existing `module_policy_set` wrapper, add:

```rust
/// called from C (`tein_shim.c`) when module policy is Allowlist (policy 2).
/// checks the module path against the thread-local allowlist.
///
/// the path arrives as e.g. `/vfs/lib/tein/json.sld` or `/vfs/lib/srfi/69/hash`.
/// we strip the `/vfs/lib/` prefix and check if any allowlist entry is a prefix
/// of the remainder.
#[unsafe(no_mangle)]
extern "C" fn tein_module_allowlist_check(path: *const c_char) -> c_int {
    use crate::sandbox::MODULE_ALLOWLIST;

    let path_str = unsafe { CStr::from_ptr(path) }.to_str().unwrap_or("");
    let suffix = path_str.strip_prefix("/vfs/lib/").unwrap_or(path_str);

    MODULE_ALLOWLIST.with(|cell| {
        let list = cell.borrow();
        if list.iter().any(|prefix| suffix.starts_with(prefix.as_str())) {
            1
        } else {
            0
        }
    })
}
```

also update the `module_policy_set` doc comment:

```rust
/// set the module import policy at C level.
///
/// 0 = unrestricted, 1 = vfs-all, 2 = allowlist (rust callback).
```

**step 2: update context.rs imports**

change the sandbox import (line 47) from:

```rust
    sandbox::{FS_POLICY, FsPolicy, MODULE_POLICY, ModulePolicy, Preset},
```

to:

```rust
    sandbox::{
        FS_POLICY, FsPolicy, MODULE_ALLOWLIST, MODULE_POLICY, ModulePolicy, Preset,
        POLICY_UNRESTRICTED, default_allowlist,
    },
```

**step 3: add module_policy field to ContextBuilder**

add to the `ContextBuilder` struct (after `file_write_prefixes`):

```rust
    module_policy: Option<ModulePolicy>,
```

and initialise it in the `Default` / `new` impl as `None`.

**step 4: add builder API methods**

after the `file_write` method, add:

```rust
    /// Set module policy to VfsAll — all curated VFS modules available.
    ///
    /// By default, sandboxed contexts use an allowlist ([`SAFE_MODULES`](crate::sandbox::SAFE_MODULES)
    /// + transitive deps). Call this to widen access to all VFS modules while
    /// still blocking filesystem module loading.
    pub fn vfs_all(mut self) -> Self {
        self.module_policy = Some(ModulePolicy::VfsAll);
        self
    }

    /// Add a module prefix to the import allowlist.
    ///
    /// If no policy has been explicitly set, starts from [`SAFE_MODULES`](crate::sandbox::SAFE_MODULES)
    /// + transitive deps. `prefix` is matched against module paths like
    /// `"chibi/regexp"`, `"srfi/1"`, `"scheme/eval"`.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::Context;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::builder()
    ///     .standard_env()
    ///     .safe()
    ///     .allow(&["import"])
    ///     .allow_module("chibi/regexp")
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn allow_module(mut self, prefix: &str) -> Self {
        self.ensure_allowlist();
        if let Some(ModulePolicy::Allowlist(ref mut list)) = self.module_policy {
            let s = prefix.to_string();
            if !list.contains(&s) {
                list.push(s);
            }
        }
        self
    }

    /// Replace the default safe set entirely. [`IMPLICIT_DEPS`](crate::sandbox::IMPLICIT_DEPS)
    /// are always included so transitive module loading works.
    ///
    /// For building minimal allowlists from scratch.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::Context;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::builder()
    ///     .standard_env()
    ///     .safe()
    ///     .allow(&["import"])
    ///     .allow_only_modules(&["tein/json"])
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn allow_only_modules(mut self, prefixes: &[&str]) -> Self {
        use crate::sandbox::IMPLICIT_DEPS;
        let mut list: Vec<String> = IMPLICIT_DEPS.iter().map(|s| s.to_string()).collect();
        for p in prefixes {
            let s = p.to_string();
            if !list.contains(&s) {
                list.push(s);
            }
        }
        self.module_policy = Some(ModulePolicy::Allowlist(list));
        self
    }

    fn ensure_allowlist(&mut self) {
        if !matches!(self.module_policy, Some(ModulePolicy::Allowlist(_))) {
            self.module_policy = Some(ModulePolicy::Allowlist(default_allowlist()));
        }
    }
```

**step 5: update build() — policy activation**

find the block (around line 1316–1326) that does:

```rust
            let has_module_policy = self.standard_env && self.allowed_primitives.is_some();
            ...
            let prev_module_policy = MODULE_POLICY.with(|cell| cell.get());
            ...
            if has_module_policy {
                MODULE_POLICY.with(|cell| cell.set(ModulePolicy::VfsOnly));
                ffi::module_policy_set(ModulePolicy::VfsOnly as i32);
            }
```

replace with:

```rust
            // resolve module policy:
            // - explicit policy from builder takes precedence
            // - sandboxed standard-env defaults to Allowlist(SAFE_MODULES + IMPLICIT_DEPS)
            // - everything else is Unrestricted
            let has_sandbox = self.standard_env && self.allowed_primitives.is_some();
            let resolved_policy = self.module_policy.take().unwrap_or_else(|| {
                if has_sandbox {
                    ModulePolicy::Allowlist(default_allowlist())
                } else {
                    ModulePolicy::Unrestricted
                }
            });
            let has_module_policy = !matches!(resolved_policy, ModulePolicy::Unrestricted);

            // save current policy values before overwriting — restored on drop so that
            // a second context on the same thread (sequential or nested) is not affected.
            let prev_module_policy = MODULE_POLICY.with(|cell| cell.get());
            let prev_fs_policy = FS_POLICY.with(|cell| cell.borrow().clone());
            let prev_module_allowlist = MODULE_ALLOWLIST.with(|cell| cell.borrow().clone());

            if has_module_policy {
                let level = resolved_policy.level();
                MODULE_POLICY.with(|cell| cell.set(level));
                ffi::module_policy_set(level as i32);
                if let ModulePolicy::Allowlist(ref list) = resolved_policy {
                    MODULE_ALLOWLIST.with(|cell| {
                        *cell.borrow_mut() = list.clone();
                    });
                }
            }
```

**step 6: update Context struct**

change `prev_module_policy: ModulePolicy` to `prev_module_policy: u8` and add `prev_module_allowlist: Vec<String>`:

```rust
pub struct Context {
    ctx: ffi::sexp,
    step_limit: Option<u64>,
    has_io_wrappers: bool,
    has_module_policy: bool,
    /// previous MODULE_POLICY level, restored on drop
    prev_module_policy: u8,
    /// previous FS_POLICY value, restored on drop
    prev_fs_policy: Option<FsPolicy>,
    /// previous MODULE_ALLOWLIST, restored on drop
    prev_module_allowlist: Vec<String>,
    // ... rest unchanged ...
```

update the `Ok(Context { ... })` in build() to include `prev_module_allowlist`.

**step 7: update Drop impl**

find the block (around line 2577–2582) that does:

```rust
        if self.has_module_policy {
            MODULE_POLICY.with(|cell| cell.set(self.prev_module_policy));
            unsafe { ffi::module_policy_set(self.prev_module_policy as i32) };
        }
```

replace with:

```rust
        if self.has_module_policy {
            MODULE_POLICY.with(|cell| cell.set(self.prev_module_policy));
            unsafe { ffi::module_policy_set(self.prev_module_policy as i32) };
            MODULE_ALLOWLIST.with(|cell| {
                *cell.borrow_mut() = std::mem::take(&mut self.prev_module_allowlist);
            });
        }
```

**step 8: update value.rs sandbox violation check**

change (line 467):

```rust
                let is_vfs_only = MODULE_POLICY.with(|cell| cell.get() == ModulePolicy::VfsOnly);
                if is_vfs_only {
```

to:

```rust
                let is_sandboxed = MODULE_POLICY.with(|cell| cell.get() != crate::sandbox::POLICY_UNRESTRICTED);
                if is_sandboxed {
```

and remove the `use crate::sandbox::ModulePolicy;` import on line 466 (no longer needed; keep the `MODULE_POLICY` import).

**step 9: build and check**

run: `cargo build 2>&1 | tail -10`
expected: successful compilation

run: `just test 2>&1 | tail -15`
expected: existing tests will FAIL — the module policy tests still assert `ModulePolicy::VfsOnly` which no longer exists. we fix those in task 4.

**step 10: commit**

```
feat: wire up three-tier module policy across C/rust boundary (#86)

- ffi.rs: tein_module_allowlist_check callback for C→rust allowlist checks
- context.rs: vfs_all(), allow_module(), allow_only_modules() builder API;
  build() resolves policy with default_allowlist() for sandboxed contexts;
  RAII save/restore of MODULE_ALLOWLIST thread-local
- value.rs: sandbox violation check uses numeric policy level

closes #86
```

---

## task 4: update existing tests

**files:**
- modify: `tein/src/context.rs` (test module, lines ~4388-4540)

**step 1: update test_module_policy_blocks_non_vfs**

this test asserts `ModulePolicy::VfsOnly`. update to check the numeric level:

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

**step 2: update test_module_policy_unrestricted_without_sandbox**

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

**step 3: update test_module_policy_cleared_on_drop**

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

**step 4: update test_module_policy_not_set_without_standard_env**

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

**step 5: update test_sequential_context_policy_isolation**

find where it checks `ModulePolicy::VfsOnly` and replace with `POLICY_ALLOWLIST`.

**step 6: test_module_policy_blocks_filesystem_import — should work as-is**

this test checks that `(import (chibi process))` returns `SandboxViolation`. the default allowlist doesn't include `chibi/process`, so it should still fail. verify it passes unchanged.

**step 7: test_standard_env_sandbox_allows_vfs_import — may need attention**

this test imports `(scheme write)` and `(scheme base)`. both are in `SAFE_MODULES`. should pass unchanged. verify.

**step 8: run all tests**

run: `just test 2>&1 | tail -20`
expected: all tests pass

**step 9: commit**

```
test: update module policy tests for three-tier model (#86)
```

---

## task 5: add new tests for allowlist features

**files:**
- modify: `tein/src/context.rs` (test module)

**step 1: test vfs_all allows chibi modules**

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
```

**step 2: test allow_module extends the default safe set**

```rust
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
```

**step 3: test allow_only_modules for minimal allowlist**

```rust
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
```

**step 4: test RAII restore of allowlist**

```rust
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
```

**step 5: test transitive deps pass the gate**

```rust
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

**step 6: run all tests**

run: `just test 2>&1 | tail -20`
expected: all tests pass including new ones

**step 7: commit**

```
test: add tests for allowlist, vfs_all, allow_module, allow_only_modules (#86)
```

---

## task 6: update public API surface and docs

**files:**
- modify: `tein/src/lib.rs` (re-exports)
- modify: `tein/src/sandbox.rs` (module doc table)
- modify: `AGENTS.md` (architecture section)

**step 1: add re-exports in lib.rs**

`ModulePolicy` and `SAFE_MODULES` should be publicly accessible since `sandbox` is already `pub mod`. verify they're reachable as `tein::sandbox::ModulePolicy` and `tein::sandbox::SAFE_MODULES`. no changes needed if the types are already `pub` — just verify.

**step 2: update the preset reference table in sandbox.rs module doc**

add a row to the table for the module policy section, or update the existing module policy doc to reference the new tiers.

**step 3: update AGENTS.md**

update the `module policy flow` in the architecture section:

```
**module policy flow**: ContextBuilder with standard_env + presets → resolve policy (default: Allowlist with SAFE_MODULES + IMPLICIT_DEPS) → set MODULE_POLICY level + MODULE_ALLOWLIST thread-locals + C-level tein_module_policy → sexp_find_module_file_raw calls tein_module_allowed() → policy 0: allow all, policy 1: VFS prefix check, policy 2: rust callback checks allowlist → policy + allowlist cleared on Context::drop()
```

also update the security layers table to mention three tiers.

**step 4: run lint**

run: `just lint 2>&1 | tail -10`
expected: clean

**step 5: commit**

```
docs: update module policy docs for three-tier model (#86)
```

---

## task 7: final verification

**step 1: run full test suite**

run: `just test 2>&1 | tail -20`
expected: all tests pass

**step 2: run lint**

run: `just lint 2>&1 | tail -10`
expected: clean

**step 3: verify the existing sandboxed test in scheme_tests.rs**

run: `cargo test --test scheme_tests -- reader_macro_sandbox --nocapture 2>&1`
expected: passes (imports `(tein test)`, `(tein reader)`, `(tein macro)` — all under `tein/` prefix)

**step 4: closing commit if any cleanup needed**

if all green, no commit needed. update the implementation plan with completion notes.
