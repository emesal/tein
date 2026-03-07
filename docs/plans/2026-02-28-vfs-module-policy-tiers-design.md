# VfsSafe / VfsAll module policy tiers — design

> **issue:** #86
> **supersedes:** the binary VfsOnly/Unrestricted model from `2026-02-13-module-allowlist-design.md`

## problem

the current module policy is binary: `VfsOnly` (all VFS modules) or `Unrestricted` (anything).
this is insufficient for constructing LLM-facing sandboxes where you want `(tein safe-regexp)`
and `(tein json)` but not `(chibi regexp)` (ReDoS risk) or `(scheme eval)` (sandbox escape).

the VFS is curated — everything in it has been reviewed and upholds the safety contract. but
"all VFS modules" is still more than some sandboxes should expose.

## design: three-tier policy with allowlist

### tiers

| policy | what passes | use case |
|--------|------------|----------|
| `Allowlist(Vec<String>)` | only listed module prefixes (must be in VFS) | tight LLM sandbox, cherry-pick modules |
| `VfsAll` | everything in the VFS | sandbox with full scheme ecosystem |
| `Unrestricted` | VFS + filesystem | unsandboxed |

### data model

```rust
// sandbox.rs

/// module import policy for sandboxed contexts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModulePolicy {
    /// all modules allowed (VFS + filesystem). no gate.
    Unrestricted,
    /// all curated VFS modules allowed. filesystem blocked.
    VfsAll,
    /// only listed module prefixes allowed (must also be in VFS).
    Allowlist(Vec<String>),
}

/// minimal safe set — tein modules + core r7rs pure-computation modules.
/// used as the default allowlist for sandboxed contexts.
pub const SAFE_MODULES: &[&str] = &[
    "tein/",          // all tein modules
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

/// transitive dependencies of safe modules. included implicitly in every
/// allowlist — these are plumbing, not user-facing APIs.
pub(crate) const IMPLICIT_DEPS: &[&str] = &[
    "srfi/9", "srfi/11", "srfi/16", "srfi/38", "srfi/39",
    "srfi/69", "srfi/151",
    "chibi/char-set/", "chibi/equiv", "chibi/string",
    "chibi/ast", "chibi/io", "chibi/iset/",
];
```

### thread-locals

`ModulePolicy` contains a `Vec` and can't be `Copy`, so we split the thread-local
storage into two parts:

```rust
thread_local! {
    /// numeric policy level: 0 = Unrestricted, 1 = VfsAll, 2 = Allowlist.
    /// mirrors the C-level tein_module_policy int. cheap to read for error checks.
    pub(crate) static MODULE_POLICY: Cell<u8> = const { Cell::new(0) };

    /// the actual allowlist, populated when policy is Allowlist.
    /// only read by the C→rust callback during module resolution.
    static MODULE_ALLOWLIST: RefCell<Vec<String>> = RefCell::new(Vec::new());
}
```

### C-level gate (approach C: hybrid)

C handles the simple cases directly; only the allowlist case calls into rust:

```c
// tein_shim.c
TEIN_THREAD_LOCAL int tein_module_policy = 0;  // 0=unrestricted, 1=vfs-all, 2=allowlist

extern int tein_module_allowlist_check(const char *path);  // rust callback

int tein_module_allowed(const char *path) {
    if (tein_module_policy == 0) return 1;                    // Unrestricted
    if (strncmp(path, "/vfs/lib/", 9) != 0) return 0;        // non-Unrestricted blocks filesystem
    if (strstr(path, "..") != NULL) return 0;                 // path traversal guard
    if (tein_module_policy == 1) return 1;                    // VfsAll
    return tein_module_allowlist_check(path);                  // Allowlist → ask rust
}
```

rust callback in `ffi.rs`:

```rust
/// called from C when policy is Allowlist. checks path against thread-local allowlist.
#[unsafe(no_mangle)]
extern "C" fn tein_module_allowlist_check(path: *const c_char) -> c_int {
    let path_str = unsafe { CStr::from_ptr(path) }.to_str().unwrap_or("");
    let suffix = path_str.strip_prefix("/vfs/lib/").unwrap_or(path_str);

    MODULE_ALLOWLIST.with(|cell| {
        let list = cell.borrow();
        if list.iter().any(|prefix| suffix.starts_with(prefix)) { 1 } else { 0 }
    })
}
```

### builder API

```rust
impl ContextBuilder {
    /// set module policy to VfsAll — all curated VFS modules available.
    pub fn vfs_all(mut self) -> Self { ... }

    /// add a module prefix to the allowlist. starts from SAFE_MODULES + IMPLICIT_DEPS
    /// if no allowlist has been set yet. prefix matches against module paths
    /// like "chibi/regexp", "srfi/1", "scheme/eval".
    pub fn allow_module(mut self, prefix: &str) -> Self { ... }

    /// replace the default safe set entirely. IMPLICIT_DEPS are always included.
    /// for building minimal allowlists from scratch.
    pub fn allow_only_modules(mut self, prefixes: &[&str]) -> Self { ... }
}
```

**default behaviour**: sandboxed contexts (standard_env + presets) auto-get
`Allowlist(SAFE_MODULES + IMPLICIT_DEPS)`. this is a **breaking change** from the
current `VfsOnly`. existing sandboxed code that imports `(chibi ...)` or `(srfi ...)`
directly will need `.vfs_all()` or `.allow_module(...)`.

non-sandboxed contexts remain `Unrestricted`.

### context lifecycle (RAII)

`ContextGuard` saves/restores both `MODULE_POLICY` (the u8) and `MODULE_ALLOWLIST`:

```rust
struct ContextGuard {
    // ... existing fields ...
    prev_module_policy: u8,
    prev_fs_policy: Option<FsPolicy>,
    prev_module_allowlist: Vec<String>,  // new
}
```

on build: save → set policy + populate allowlist → sync to C.
on drop: restore allowlist → restore policy → sync to C.

### error messages

`value.rs` check for `SandboxViolation` on import failure triggers for any
non-Unrestricted policy:

```rust
let is_sandboxed = MODULE_POLICY.with(|cell| cell.get() != 0);
```

### testing

all rust-side tests (sandboxed contexts need custom builder, not `run_scheme_test`):

- `test_module_policy_allowlist_default` — sandboxed context imports `(tein test)` ✓, cannot import a module outside safe set
- `test_module_policy_allowlist_extend` — `.allow_module("chibi/string")` enables import
- `test_module_policy_vfs_all` — `.vfs_all()` allows all VFS modules
- `test_module_policy_allow_only` — `.allow_only_modules(&["tein/test"])` blocks `(scheme base)` direct import but allows `(tein test)` + implicit deps
- `test_module_policy_unrestricted_unchanged` — non-sandboxed contexts work as before
- `test_module_policy_raii` — sequential contexts don't leak state
- `test_module_policy_error_message` — blocked import → `SandboxViolation`
- `test_module_policy_transitive_deps` — `(tein test)` succeeds (srfi/9 passes via IMPLICIT_DEPS)

### files touched

| file | changes |
|---|---|
| `target/chibi-scheme/tein_shim.c` | extend `tein_module_allowed` with policy 2 + extern callback decl |
| `tein/src/sandbox.rs` | `ModulePolicy` enum rework, `SAFE_MODULES`, `IMPLICIT_DEPS`, `MODULE_ALLOWLIST` thread-local, policy level const values |
| `tein/src/ffi.rs` | `tein_module_allowlist_check` extern "C" callback, update `module_policy_set` |
| `tein/src/context.rs` | builder API (`vfs_all`, `allow_module`, `allow_only_modules`), allowlist population in `build()`, RAII save/restore, update tests |
| `tein/src/value.rs` | update sandbox violation check from `VfsOnly` to `!= 0` |
| `tein/src/lib.rs` | re-export `SAFE_MODULES`, `ModulePolicy` if making public |
| `AGENTS.md` | update module policy flow, add to architecture |
