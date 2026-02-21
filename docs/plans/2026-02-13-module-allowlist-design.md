# phase 4: module allowlist — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** restrict module imports to VFS-only in sandboxed standard-env contexts, preventing LLM-generated scheme code from loading dangerous filesystem-based modules.

**Architecture:** C-level interception in `sexp_find_module_file_raw` using a thread-local policy integer. rust side manages policy lifecycle via `ContextBuilder::build()` and `Context::drop()`. no new public API — policy is implicit when sandbox + standard_env are combined.

**Tech Stack:** rust FFI, C thread-locals, chibi-scheme eval.c patching

---

# design

## problem

LLM-generated scheme code is adversarial-adjacent: not deliberately malicious, but
unpredictable. a sandboxed context with the standard environment needs to prevent
`import` from loading dangerous modules (e.g. `(chibi process)`, `(chibi filesystem)`)
while still allowing the standard r7rs libraries that ship in the VFS.

## design: VFS-only module restriction

### threat model

an LLM writes scheme code that calls `(import ...)` with arbitrary module names.
the interception must be **C-level** (in `sexp_find_module_file_raw`) because
scheme-level wrapping can be bypassed via `eval` + meta env.

### VFS safety contract

VFS modules are safe by construction: tein curates the embedded virtual filesystem
to ensure no module can bypass the existing safety layers (preset allowlists,
FsPolicy, fuel/timeout). capabilities exposed by VFS modules remain subject to
these controls — e.g. IO operations are gated by preset availability and filesystem
path policies. when a sandboxed context uses the standard environment, module imports
are automatically restricted to VFS-only, blocking filesystem-based module loading.

### security layers (independent, composable)

| layer                    | gates                                      |
|--------------------------|--------------------------------------------|
| module allowlist (new)   | which libraries can be `import`ed           |
| preset allowlist         | which primitives/bindings are in scope      |
| FsPolicy                 | which filesystem paths can be opened        |
| fuel/timeout             | resource exhaustion                         |

### data model

```rust
// sandbox.rs
pub(crate) enum ModulePolicy {
    /// no restriction — all modules allowed (unsandboxed context)
    Unrestricted,
    /// only VFS modules allowed (sandboxed context)
    VfsOnly,
}

thread_local! {
    pub(crate) static MODULE_POLICY: Cell<ModulePolicy> = const { Cell::new(ModulePolicy::Unrestricted) };
}
```

no `.allow_modules()` builder API needed — the policy is implicit:
- `standard_env()` + any preset → VfsOnly (automatic)
- `standard_env()` alone → Unrestricted
- no `standard_env()` → Unrestricted (no module system)

### C-level interception

**`tein_shim.c`**: new thread-local + two functions:

```c
TEIN_THREAD_LOCAL int tein_module_policy = 0;  /* 0 = unrestricted, 1 = vfs-only */

int tein_module_allowed(const char *path) {
    if (tein_module_policy == 0) return 1;           /* unrestricted */
    return strncmp(path, "/vfs/lib/", 9) == 0;       /* vfs-only */
}

void tein_module_policy_set(int policy) {
    tein_module_policy = policy;
}
```

**`eval.c`** patch A enhancement — in `sexp_find_module_file_raw` (~line 2411):

```c
// before (current):
if (tein_vfs_lookup(path, &tein_vfs_dummy) || sexp_find_static_library(path) || file_exists_p(path, buf))
    return path;

// after:
if (tein_vfs_lookup(path, &tein_vfs_dummy) || sexp_find_static_library(path) || file_exists_p(path, buf)) {
    if (tein_module_allowed(path))
        return path;
    free(path);
}
```

disallowed modules simply "aren't found" — clean failure, no info leak.

### rust integration

**`ffi.rs`**: extern declaration + safe wrapper for `tein_module_policy_set`.

**`context.rs` `build()` flow**:

```
1. create context
2. if standard_env + sandbox → set MODULE_POLICY = VfsOnly
   if standard_env only    → leave Unrestricted
3. load standard env (init-7, meta-7 via VFS — allowed under VfsOnly)
4. apply sandbox restrictions (presets, IO wrappers)
5. return Context
```

**`Context::drop()`**: clear `MODULE_POLICY` back to `Unrestricted` (alongside existing `FS_POLICY` cleanup).

### tests

1. `test_standard_env_sandbox_blocks_filesystem_import` — standard_env + sandbox,
   non-VFS module → fails (module not found)
2. `test_standard_env_sandbox_allows_vfs_import` — standard_env + sandbox,
   VFS module like `(scheme write)` → succeeds.
   **blocked by**: import finalization port type bug (see handoff.md).
   add when import works; track in TODO.md.
3. `test_standard_env_unsandboxed_unrestricted` — standard_env without sandbox,
   module policy stays unrestricted
4. `test_module_policy_cleared_on_drop` — create sandboxed standard context,
   drop it, verify policy resets

### files touched

| file | changes |
|---|---|
| `tein/src/sandbox.rs` | `ModulePolicy` enum + `MODULE_POLICY` thread-local |
| `tein/vendor/chibi-scheme/tein_shim.c` | `tein_module_policy` thread-local + `tein_module_allowed()` + `tein_module_policy_set()` |
| `tein/vendor/chibi-scheme/eval.c` | patch A enhancement: `tein_module_allowed()` gate |
| `tein/src/ffi.rs` | extern decl + safe wrapper for `tein_module_policy_set` |
| `tein/src/context.rs` | set policy in `build()`, clear in `Drop`, add tests |
| `DEVELOPMENT.md` | VFS safety contract documentation |
| `AGENTS.md` | module policy flow in architecture section |

---

# implementation tasks

### task 1: C-level — module policy thread-local and check function

**files:**
- modify: `tein/vendor/chibi-scheme/tein_shim.c` (after line 119, fuel thread-locals section)

**step 1: add module policy thread-local and functions to tein_shim.c**

after the fuel thread-locals (`tein_fuel_budget`, `tein_fuel_exhausted_flag`) and before `tein_fuel_arm`, add:

```c
// --- module import policy ---
//
// controls which modules can be loaded via sexp_find_module_file_raw.
// 0 = unrestricted (all modules allowed), 1 = vfs-only (only /vfs/lib/ paths).
// set from rust before loading the standard env in sandboxed contexts.

TEIN_THREAD_LOCAL int tein_module_policy = 0;

// check if a module path is allowed under the current policy.
// called from eval.c patch A (sexp_find_module_file_raw).
int tein_module_allowed(const char *path) {
    if (tein_module_policy == 0) return 1;
    return strncmp(path, "/vfs/lib/", 9) == 0;
}

// set the module policy. called from rust ffi.
void tein_module_policy_set(int policy) {
    tein_module_policy = policy;
}
```

**step 2: build and verify compilation**

run: `cargo build 2>&1 | tail -5`
expected: successful compilation (new symbols not yet referenced)

**step 3: commit**

```
feat: add module policy thread-local and check function to tein_shim.c
```

---

### task 2: C-level — patch A enhancement in eval.c

**files:**
- modify: `tein/vendor/chibi-scheme/eval.c:2410-2413`

**step 1: add forward declaration for tein_module_allowed**

at line 12 (after the `tein_vfs_lookup` forward declaration), add:

```c
extern int tein_module_allowed(const char *path);
```

**step 2: gate module resolution with tein_module_allowed**

replace lines 2410-2413:

```c
    /* tein VFS: check embedded files alongside static libs and filesystem (patch A) */
    if (tein_vfs_lookup(path, &tein_vfs_dummy) || sexp_find_static_library(path) || file_exists_p(path, buf))
      return path;
    free(path);
```

with:

```c
    /* tein VFS: check embedded files alongside static libs and filesystem (patch A) */
    if (tein_vfs_lookup(path, &tein_vfs_dummy) || sexp_find_static_library(path) || file_exists_p(path, buf)) {
      /* tein module policy: reject paths not allowed by current policy */
      if (tein_module_allowed(path))
        return path;
      free(path);
    } else {
      free(path);
    }
```

note: the `free(path)` must happen in both branches — when the file exists but is disallowed, and when it doesn't exist. the original code only freed in the not-found case (fall-through to `free(path)` at end of loop). now the found-but-disallowed case also needs explicit free.

**step 3: build and verify**

run: `cargo build 2>&1 | tail -5`
expected: successful compilation

**step 4: run existing tests to verify no regressions**

run: `cargo test 2>&1 | tail -10`
expected: all 128 tests pass (policy defaults to 0 = unrestricted, so behaviour unchanged)

**step 5: commit**

```
feat: gate module resolution with tein_module_allowed in eval.c patch A
```

---

### task 3: rust FFI — extern declaration and safe wrapper

**files:**
- modify: `tein/src/ffi.rs:155-166` (extern block, after `tein_make_error`)
- modify: `tein/src/ffi.rs:467-487` (safe wrappers, after `load_standard_ports`)

**step 1: add extern declaration**

in the `unsafe extern "C"` block, after line 158 (`tein_make_error`), add:

```rust
    // module import policy (for sandboxed standard env)
    pub fn tein_module_policy_set(policy: c_int);
```

**step 2: add safe wrapper**

after the `load_standard_ports` wrapper (after line 474), add:

```rust
/// set the module import policy at C level.
/// 0 = unrestricted (all modules), 1 = vfs-only.
#[inline]
pub unsafe fn module_policy_set(policy: i32) {
    unsafe { tein_module_policy_set(policy as c_int) }
}
```

**step 3: build and verify**

run: `cargo build 2>&1 | tail -5`
expected: successful compilation

**step 4: commit**

```
feat: add module_policy_set FFI wrapper
```

---

### task 4: rust data model — ModulePolicy enum and thread-local

**files:**
- modify: `tein/src/sandbox.rs` (after `FS_POLICY` thread-local, line 69)

**step 1: add ModulePolicy enum and thread-local**

after the `FS_POLICY` thread-local (line 69), add:

```rust
/// module import policy for sandboxed standard-env contexts
///
/// controls which modules can be loaded via `(import ...)`.
/// when a sandboxed context uses the standard environment, this is
/// automatically set to `VfsOnly` to prevent loading filesystem-based
/// modules (e.g. `(chibi process)`, `(chibi filesystem)`).
///
/// ## VFS safety contract
///
/// VFS modules are safe by construction: tein curates the embedded virtual
/// filesystem to ensure no module can bypass the existing safety layers
/// (preset allowlists, FsPolicy, fuel/timeout). capabilities exposed by
/// VFS modules remain subject to these controls — e.g. IO operations are
/// gated by preset availability and filesystem path policies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ModulePolicy {
    /// all modules allowed (unsandboxed or non-standard-env context)
    Unrestricted = 0,
    /// only VFS modules allowed (sandboxed standard-env context)
    VfsOnly = 1,
}

thread_local! {
    /// active module import policy (set during build, cleared on drop)
    pub(crate) static MODULE_POLICY: Cell<ModulePolicy> = const { Cell::new(ModulePolicy::Unrestricted) };
}
```

also add `use std::cell::Cell;` to the imports at the top of `sandbox.rs` (line 12, alongside `RefCell`).

**step 2: build and verify**

run: `cargo build 2>&1 | tail -5`
expected: successful compilation (enum not yet used, but no dead_code warning since pub(crate))

**step 3: commit**

```
feat: add ModulePolicy enum and MODULE_POLICY thread-local
```

---

### task 5: rust integration — set policy in build(), clear in drop()

**files:**
- modify: `tein/src/context.rs:7` (imports)
- modify: `tein/src/context.rs:339-483` (build method)
- modify: `tein/src/context.rs:503-507` (Context struct)
- modify: `tein/src/context.rs:809-829` (Drop impl)

**step 1: add import**

at line 7 (the `sandbox` import line), change:

```rust
    sandbox::{FS_POLICY, FsPolicy, Preset},
```

to:

```rust
    sandbox::{FS_POLICY, FsPolicy, MODULE_POLICY, ModulePolicy, Preset},
```

**step 2: add `has_module_policy` field to Context struct**

change the struct at line 503:

```rust
pub struct Context {
    ctx: ffi::sexp,
    step_limit: Option<u64>,
    has_io_wrappers: bool,
    has_module_policy: bool,
}
```

**step 3: set module policy in build()**

in `build()`, after the standard env loading block (after line 376, `}`), and before the IO prefix extraction (line 379), add:

```rust
            // activate VFS-only module policy if both standard_env and
            // sandbox (presets) are configured. this restricts (import ...)
            // to only load modules from the embedded VFS, blocking
            // filesystem-based modules like (chibi process).
            // set early so it's active during sandbox setup (which may
            // trigger transitive module loads).
            let has_module_policy = self.standard_env && self.allowed_primitives.is_some();
            if has_module_policy {
                MODULE_POLICY.with(|cell| cell.set(ModulePolicy::VfsOnly));
                unsafe { ffi::module_policy_set(ModulePolicy::VfsOnly as i32) };
            }
```

update the `Ok(Context { ... })` at line 477 to include the new field:

```rust
            Ok(Context {
                ctx,
                step_limit: self.step_limit,
                has_io_wrappers: has_io,
                has_module_policy,
            })
```

**step 4: clear module policy in Drop**

in `impl Drop for Context` (line 809), add cleanup before the unsafe block (line 823):

```rust
        // clean up module policy if active
        if self.has_module_policy {
            MODULE_POLICY.with(|cell| cell.set(ModulePolicy::Unrestricted));
            unsafe { ffi::module_policy_set(ModulePolicy::Unrestricted as i32) };
        }
```

**step 5: build and run all tests**

run: `cargo build 2>&1 | tail -5`
run: `cargo test 2>&1 | tail -10`
expected: all existing tests pass. sandboxed standard env tests now activate VfsOnly policy but since all module loads during standard env init are VFS-based, behaviour unchanged.

**step 6: commit**

```
feat: integrate module policy into ContextBuilder and Context lifecycle
```

---

### task 6: tests

**files:**
- modify: `tein/src/context.rs` (test module, after `test_standard_env_with_step_limit`)

**step 1: write test — sandbox blocks non-VFS module**

```rust
    #[test]
    fn test_module_policy_blocks_non_vfs() {
        // a sandboxed standard-env context should block attempts to
        // import filesystem-based modules. we can't directly test
        // (import (chibi process)) because the import mechanism has
        // a known port finalization bug, but we CAN verify the policy
        // is set correctly by checking the thread-local.
        use crate::sandbox::*;
        let ctx = Context::builder()
            .standard_env()
            .preset(&ARITHMETIC)
            .build()
            .expect("standard + sandbox");

        MODULE_POLICY.with(|cell| {
            assert_eq!(cell.get(), ModulePolicy::VfsOnly,
                "sandboxed standard env should activate VfsOnly policy");
        });

        drop(ctx);
    }
```

**step 2: write test — unsandboxed is unrestricted**

```rust
    #[test]
    fn test_module_policy_unrestricted_without_sandbox() {
        // standard env without sandbox should leave module policy unrestricted
        let ctx = Context::new_standard().expect("new_standard");

        MODULE_POLICY.with(|cell| {
            assert_eq!(cell.get(), ModulePolicy::Unrestricted,
                "unsandboxed standard env should be unrestricted");
        });

        drop(ctx);
    }
```

**step 3: write test — policy cleared on drop**

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
                assert_eq!(cell.get(), ModulePolicy::VfsOnly);
            });
        }
        // after drop, policy should reset
        MODULE_POLICY.with(|cell| {
            assert_eq!(cell.get(), ModulePolicy::Unrestricted,
                "module policy should reset to unrestricted after context drop");
        });
    }
```

**step 4: write test — no standard env means no policy**

```rust
    #[test]
    fn test_module_policy_not_set_without_standard_env() {
        // sandbox without standard_env should NOT activate module policy
        // (there's no module system to restrict)
        use crate::sandbox::*;
        let ctx = Context::builder()
            .preset(&ARITHMETIC)
            .build()
            .expect("sandbox without standard env");

        MODULE_POLICY.with(|cell| {
            assert_eq!(cell.get(), ModulePolicy::Unrestricted,
                "non-standard-env sandbox should not set module policy");
        });

        drop(ctx);
    }
```

**step 5: add TODO comment for import-based test**

```rust
    // TODO: add test_module_policy_blocks_filesystem_import once the import
    // finalization port type bug is resolved (see handoff.md). this test
    // should verify that (import (chibi process)) fails in a sandboxed
    // standard-env context while (import (scheme write)) succeeds.
```

**step 6: run tests**

run: `cargo test 2>&1 | tail -15`
expected: all tests pass including the 4 new ones

**step 7: commit**

```
test: add module policy tests for sandboxed standard env
```

---

### task 7: documentation

**files:**
- modify: `DEVELOPMENT.md`
- modify: `AGENTS.md`
- modify: `TODO.md`

**step 1: update AGENTS.md architecture section**

add to the data flow section:

```
**module policy flow**: ContextBuilder with standard_env + presets → set MODULE_POLICY = VfsOnly (thread-local + C-level) → sexp_find_module_file_raw checks tein_module_allowed() → VFS paths pass, filesystem paths blocked → policy cleared on Context::drop()
```

**step 2: update DEVELOPMENT.md**

add a "module import policy" section documenting:
- the VFS safety contract
- the security layers table
- how the C-level interception works

**step 3: update TODO.md**

mark the r7rs standard environment item partially complete, note that module allowlist (VFS-only restriction) is done, and add a sub-item tracking the import finalization bug and the blocked test.

**step 4: commit**

```
docs: document module policy and VFS safety contract
```
