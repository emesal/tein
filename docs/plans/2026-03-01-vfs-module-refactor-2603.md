# refactor: VfsGate — simplify module import policy

## context

the module import policy system (`ModulePolicy` enum, `SAFE_MODULES`, `IMPLICIT_DEPS`, three C-side policy levels) has grown organically and is harder to understand than the underlying concept warrants. the naming doesn't convey the VFS connection, the dependency tracking is a flat implicit list rather than per-module data, and there are more code paths (three policy tiers) than the two actual modes need.

the goal: a cleaner, more intuitive module gate with proper dependency tracking, fewer C-side code paths, and names that make the VFS relationship obvious.

## design

### data model

```rust
/// a module available in the VFS, with its transitive dependencies.
pub struct VfsModule {
    /// module path prefix, e.g. "scheme/char", "tein/json", "srfi/1"
    pub path: &'static str,
    /// paths of modules this one depends on (from vetting, not parsed at runtime)
    pub deps: &'static [&'static str],
}
```

two curated static lists:

- **`VFS_MODULES_SAFE: &[VfsModule]`** — conservative sandbox set (current `SAFE_MODULES`). the default when sandboxing is active. excludes `tein/process`, `scheme/file`, `scheme/process-context`, `scheme/load`, `scheme/r5rs`.
- **`VFS_MODULES_ALL: &[VfsModule]`** — every vetted module. superset of `VFS_MODULES_SAFE`. initially = `SAFE` + the 5 currently excluded modules (`tein/process`, `scheme/file`, `scheme/process-context`, `scheme/load`, `scheme/r5rs`). broader chibi/srfi vetting is tracked as a separate github issue.

### VfsGate enum

```rust
/// controls which VFS modules can be imported via (import ...).
pub enum VfsGate {
    /// no restriction — VFS + filesystem modules all pass. used for unsandboxed contexts.
    Off,
    /// only listed module prefixes (+ their transitive deps) pass.
    /// deps are resolved automatically from VfsModule data.
    Allow(Vec<String>),
}
```

- `Off` = current `Unrestricted` (policy 0)
- `Allow(vec)` = current `Allowlist` (policy 2)
- `VfsAll` (policy 1) disappears — it's just `Allow(all_vetted_modules())`

### C-side simplification

current: three policy levels (0=unrestricted, 1=vfs-all, 2=allowlist) with VFS prefix check, path traversal guard, and `.scm` passthrough in C.

proposed: two levels (0=off, 1=check-via-rust). all logic moves to the rust callback:

```c
TEIN_THREAD_LOCAL int tein_vfs_gate = 0;

int tein_module_allowed(const char *path) {
    if (tein_vfs_gate == 0) return 1;         /* off — allow everything */
    return tein_vfs_gate_check(path);          /* rust callback */
}
```

the rust callback (`tein_vfs_gate_check`, renamed from `tein_module_allowlist_check`) absorbs:
- VFS prefix check (`/vfs/lib/` requirement)
- path traversal guard (`..` rejection)
- `.scm` passthrough (already-allowed `.sld` guarantees safety)
- allowlist prefix matching

### builder API

```rust
// unsandboxed — no module gate
Context::builder().standard_env().build()

// sandboxed with safe modules (default)
Context::builder().standard_env().safe().allow(&["import"]).build()

// sandboxed with all vetted modules
Context::builder().standard_env().safe().allow(&["import"])
    .vfs_gate_all()  // or .modules_all()
    .build()

// sandboxed bare — no modules, add explicitly
Context::builder().standard_env().safe().allow(&["import"])
    .vfs_gate_none()  // starts from empty
    .allow_module("tein/json")  // adds tein/json + its deps
    .build()

// sandboxed with safe + one extra
Context::builder().standard_env().safe().allow(&["import"])
    .allow_module("tein/process")  // adds to default safe set + deps
    .build()
```

key change: `allow_module` automatically resolves transitive deps from the `VfsModule` registry. users never think about `chibi/iset/` or `srfi/38`.

### dependency resolution

```rust
/// resolve a module path to itself + all transitive deps.
/// looks up in VFS_MODULES_ALL (the full registry).
fn resolve_module_deps(path: &str) -> Vec<String> {
    // BFS/DFS through VfsModule.deps, deduplicating
}
```

called by `allow_module()` at builder time. the resolved flat list is what gets stored in `VfsGate::Allow(vec)` and passed to the thread-local for the rust callback.

### naming summary

| current | new | notes |
|---------|-----|-------|
| `ModulePolicy` | `VfsGate` | |
| `ModulePolicy::Unrestricted` | `VfsGate::Off` | |
| `ModulePolicy::VfsAll` | removed | `= Allow(all)` |
| `ModulePolicy::Allowlist(Vec)` | `VfsGate::Allow(Vec)` | |
| `SAFE_MODULES` | `VFS_MODULES_SAFE` | now `&[VfsModule]` with deps |
| `IMPLICIT_DEPS` | removed | deps are per-module in `VfsModule` |
| `default_allowlist()` | `vfs_safe_allowlist()` | resolves `VFS_MODULES_SAFE` + deps to flat `Vec<String>` |
| `ensure_allowlist()` | removed | init logic simplified |
| `MODULE_POLICY` thread-local | `VFS_GATE` thread-local | `u8`, 0=off 1=check |
| `MODULE_ALLOWLIST` thread-local | `VFS_ALLOWLIST` thread-local | |
| `POLICY_UNRESTRICTED/VFS_ALL/ALLOWLIST` | `GATE_OFF` / `GATE_CHECK` | two constants |
| `.vfs_all()` | `.vfs_gate_all()` | sets `Allow(all_vetted)` |
| `.allow_only_modules(&[..])` | `.vfs_gate_none().allow_module(..)` | composable |
| `tein_module_policy` (C) | `tein_vfs_gate` (C) | |
| `tein_module_allowed` (C) | `tein_module_allowed` (C) | same name, simpler body |
| `tein_module_allowlist_check` (rust callback) | `tein_vfs_gate_check` (rust callback) | absorbs VFS/traversal/scm logic |
| `tein_module_policy_set` (C) | `tein_vfs_gate_set` (C) | |
| `module_policy_set` (ffi.rs) | `vfs_gate_set` (ffi.rs) | |

### default behaviour

when `standard_env + presets` (sandboxed): `VfsGate::Allow(vfs_safe_allowlist())` — same as today but with deps resolved from registry.

when no presets (unsandboxed): `VfsGate::Off` — same as today.

explicit `.vfs_gate_none()` or `.vfs_gate_all()` overrides the default.

## files to modify

| file | changes |
|------|---------|
| `tein/src/sandbox.rs` | `VfsGate` enum, `VfsModule` struct, `VFS_MODULES_SAFE`, `VFS_MODULES_ALL`, dep resolution, thread-locals renamed, remove `ModulePolicy`/`SAFE_MODULES`/`IMPLICIT_DEPS`/`default_allowlist`/`ensure_allowlist` |
| `tein/src/context.rs` | builder fields + methods renamed, `build()` policy resolution simplified, drop cleanup updated |
| `tein/src/ffi.rs` | rename `module_policy_set` → `vfs_gate_set`, rename + expand callback `tein_vfs_gate_check` |
| `target/chibi-scheme/tein_shim.c` | simplify to 2-level gate, rename symbols |
| `target/chibi-scheme/eval.c` | no change (calls `tein_module_allowed` which keeps its name) |
| `tein/src/lib.rs` | update re-exports if `ModulePolicy` was public |

## execution plan

1. ~~**vet dependency tree**~~ ✅ — full transitive dep tree traced for all VFS modules. discovered `scheme/time` and `scheme/show` depend on unvetted modules (`scheme/process-context`, `scheme/file`). created github issues #90 (tein time) and #91 (tein show) to track safe alternatives. decision: remove both from `VFS_MODULES_SAFE`, keep in `VFS_MODULES_ALL`.
2. ~~**define `VfsModule` struct and static data**~~ ✅ — `VfsModule` struct, `VfsGate` enum (`Off`/`Allow`), `VFS_MODULES_SAFE` and `VFS_MODULES_ALL` with per-module deps, `resolve_module_deps()` (BFS + dedup), `vfs_safe_allowlist()`, `vfs_all_allowlist()`. replaced `ModulePolicy`, `SAFE_MODULES`, `IMPLICIT_DEPS`, `default_allowlist()`, `ensure_allowlist()`.
3. ~~**simplify C side**~~ ✅ — `tein_shim.c` reduced to 2-level gate (0=off, 1=check-via-rust). renamed `tein_module_policy` → `tein_vfs_gate`, `tein_module_policy_set` → `tein_vfs_gate_set`. pushed to chibi-scheme fork (emesal-tein branch, commit 182d79f7). `tein_module_allowed()` keeps its name (eval.c unchanged).
4. ~~**refactor rust side**~~ ✅ — `ffi.rs`: `tein_module_policy_set` → `tein_vfs_gate_set`, callback `tein_module_allowlist_check` → `tein_vfs_gate_check` absorbing VFS prefix/traversal/.scm logic. `context.rs`: imports, builder fields/methods (`vfs_gate_all`, `vfs_gate_none`, `allow_module` with transitive dep resolution), `build()` simplified to 2-level gate, Context struct fields renamed, Drop updated. `value.rs`: `MODULE_POLICY` → `VFS_GATE`. `lib.rs` unchanged (no direct re-exports). all 669 tests pass, lint clean.
   - builder API change: `allow_only_modules()` removed in favour of composable `vfs_gate_none().allow_module(...)` pattern. `allow_module()` now resolves transitive deps automatically via `resolve_module_deps()`.
5. ~~**update tests**~~ ✅ — all module policy tests renamed to VFS gate tests, assertions updated. existing sandbox tests confirm gate behaviour unchanged. done as part of step 4.
6. ~~**update docs**~~ ✅ — `sandbox.rs` module doc updated (three-tier → VfsGate enum), `value.rs` docstring, `build.rs` comment, `docs/guide.md`, AGENTS.md (architecture, VFS gate flow, gotchas) — all references migrated from ModulePolicy/SAFE_MODULES/IMPLICIT_DEPS to VfsGate/VFS_MODULES_SAFE.
7. ~~**create github issue**~~ ✅ — #92 "vet chibi/* and srfi/* VFS modules for sandbox safety"
8. ~~**collect AGENTS.md notes**~~ ✅ — no new gotchas discovered. AGENTS.md already updated in step 6. key API change: `allow_only_modules()` removed → `vfs_gate_none().allow_module(...)` composable pattern.

## corrections from original plan

- `VFS_MODULES_ALL` does NOT include `scheme/file`, `scheme/process-context`, `scheme/load`, `scheme/r5rs` — these are unvetted and blocked by any active gate. tein counterparts (`tein/file`, `tein/load`) are in `VFS_MODULES_SAFE`; `tein/process` is in `VFS_MODULES_ALL`.
- `scheme/time` and `scheme/show` removed from `VFS_MODULES_SAFE` (transitive unsafe deps). kept in `VFS_MODULES_ALL`. tracked as #90 and #91.
- no `VfsAll` variant — replaced by `Allow(vfs_all_allowlist())`.

## verification

- `just test` — all existing tests pass
- `just lint` — clean
- new unit tests for `resolve_module_deps()` (cycles, missing deps, transitive chains)
- existing sandbox tests confirm gate behaviour unchanged
- test bare mode: `vfs_gate_none()` blocks all imports
- test all mode: `vfs_gate_all()` allows everything vetted
- test dep resolution: `allow_module("scheme/char")` automatically includes `chibi/char-set/` etc
