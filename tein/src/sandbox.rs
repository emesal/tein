//! Sandboxing and filesystem policy for restricted Scheme environments.
//!
//! tein's sandboxing has four independent layers:
//!
//! 1. **Module restriction** — importable modules via [`Modules`] + [`ContextBuilder::sandboxed()`](crate::ContextBuilder::sandboxed)
//! 2. **Step limits** — cap VM instructions per evaluation
//! 3. **File IO policy** — allowlist filesystem paths for reading/writing
//! 4. **VFS gate** — restrict `(import ...)` to vetted VFS modules; automatic when using `sandboxed()`
//!
//! # Module sets
//!
//! [`Modules`] controls which VFS modules sandboxed code can import. The registry
//! (`VFS_REGISTRY` in `vfs_registry.rs`) declares all vetted modules with their
//! dependencies, files, and safety tier.
//!
//! ```
//! use tein::{Context, sandbox::Modules};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ctx = Context::builder()
//!     .standard_env()
//!     .sandboxed(Modules::Safe)
//!     .build()?;
//!
//! // import and use scheme/base
//! let result = ctx.evaluate("(import (scheme base)) (+ 1 2)")?;
//! assert_eq!(result, tein::Value::Integer(3));
//! # Ok(())
//! # }
//! ```
//!
//! # File IO policy
//!
//! `FsPolicy` controls which filesystem paths Scheme code can access.
//! Registered internally via
//! [`ContextBuilder::file_read()`](crate::ContextBuilder::file_read) and
//! [`ContextBuilder::file_write()`](crate::ContextBuilder::file_write).
//! Paths are canonicalised before prefix-checking, so symlink and `..`
//! traversals are resolved.
//!
//! Enforcement is at the C opcode level: `eval.c` patches F and G call
//! `tein_fs_check_access()` before `fopen()` in `sexp_open_input_file_op`
//! and `sexp_open_output_file_op`. The C dispatcher checks `tein_fs_policy_gate`
//! (thread-local, 0=off, 1=check) and calls the rust callback
//! `tein_fs_policy_check` which delegates to [`FsPolicy`] prefix matching.
//! `file-exists?` and `delete-file` remain rust trampolines (no opcode equivalents).
//!
//! # VFS gate
//!
//! Module imports in sandboxed contexts are restricted automatically:
//!
//! - unsandboxed contexts — no restriction; VFS + filesystem modules all pass.
//! - sandboxed contexts — only modules in the resolved `Modules` allowlist pass.
//!   extend with [`.allow_module()`](crate::ContextBuilder::allow_module).

use std::cell::{Cell, RefCell};
use std::path::Path;

include!("vfs_registry.rs");

/// Filesystem access policy for sandboxed IO.
///
/// Controls which paths Scheme code can read from and write to.
/// Uses prefix matching against canonicalised paths.
#[derive(Clone)]
pub(crate) struct FsPolicy {
    /// allowed path prefixes for reading
    pub read_prefixes: Vec<String>,
    /// allowed path prefixes for writing
    pub write_prefixes: Vec<String>,
}

impl FsPolicy {
    /// Check if a path is allowed for reading.
    ///
    /// Canonicalises the full path (file must exist for reads).
    /// Returns false if path is invalid or canonicalisation fails.
    pub fn check_read(&self, path: &str) -> bool {
        Path::new(path)
            .canonicalize()
            .ok()
            .map(|canon| {
                let canon_str = canon.to_string_lossy();
                self.read_prefixes
                    .iter()
                    .any(|prefix| canon_str.starts_with(prefix))
            })
            .unwrap_or(false)
    }

    /// Check if a path is allowed for writing.
    ///
    /// Canonicalises the parent directory (must exist), appends filename.
    /// The file itself doesn't need to exist (R7RS: open-output-file creates it).
    pub fn check_write(&self, path: &str) -> bool {
        let p = Path::new(path);
        let parent = match p.parent().and_then(|d| d.canonicalize().ok()) {
            Some(d) => d,
            None => return false,
        };
        let filename = match p.file_name() {
            Some(f) => f,
            None => return false,
        };
        let full = parent.join(filename);
        let full_str = full.to_string_lossy();
        self.write_prefixes
            .iter()
            .any(|prefix| full_str.starts_with(prefix))
    }
}

thread_local! {
    /// Active filesystem policy for the current context (set during build, cleared on drop).
    pub(crate) static FS_POLICY: RefCell<Option<FsPolicy>> = const { RefCell::new(None) };
}

// VFS shadow modules:
//
// the following modules have `VfsSource::Shadow` entries in the registry.
// in sandboxed contexts, `register_vfs_shadows()` injects replacement `.sld`
// files that re-export from safe tein counterparts or provide neutered stubs.
// unsandboxed contexts use chibi's native versions (no shadow registered).
//
// - `scheme/file` — re-exports (tein file), providing FsPolicy enforcement
// - `scheme/repl` — neutered interaction-environment via (current-environment)
// - `scheme/process-context` — re-exports (tein process) with neutered env/argv
// - `srfi/98` — neutered get-environment-variable (always #f)
//
// modules NOT shadowed and intentionally blocked:
//
// - `scheme/load` — loads arbitrary files from filesystem. use (tein load) instead.
// - `scheme/eval` — eval + environment. tracked for future shadow (GH issue #97).
// - `scheme/r5rs` — re-exports scheme/file, scheme/load, scheme/process-context.

/// numeric gate level for C interop. mirrors `tein_vfs_gate` in `tein_shim.c`.
pub(crate) const GATE_OFF: u8 = 0;
/// numeric gate level for C interop — rust callback checks the allowlist.
pub(crate) const GATE_CHECK: u8 = 1;

/// numeric FS policy gate level for C interop. mirrors `tein_fs_policy_gate` in `tein_shim.c`.
pub(crate) const FS_GATE_OFF: u8 = 0;
/// numeric FS policy gate level — rust callback checks IS_SANDBOXED + FsPolicy.
pub(crate) const FS_GATE_CHECK: u8 = 1;

thread_local! {
    /// numeric gate level (0=off, 1=check). set during Context::build(), cleared on drop.
    pub(crate) static VFS_GATE: Cell<u8> = const { Cell::new(GATE_OFF) };

    /// the resolved allowlist, populated when gate is `Allow`.
    /// read by the C→rust callback (`tein_vfs_gate_check`) during module resolution.
    pub(crate) static VFS_ALLOWLIST: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };

    /// FS policy gate level (0=off, 1=check). set during Context::build(), cleared on drop.
    /// when armed, C-level `open-*-file` opcodes call `tein_fs_policy_check` (rust callback).
    pub(crate) static FS_GATE: Cell<u8> = const { Cell::new(FS_GATE_OFF) };
}

/// resolve transitive deps from `VFS_REGISTRY`.
///
/// follows `deps` recursively for each entry, returns a deduplicated flat list
/// of all module path strings (including the inputs). unknown paths are included
/// as-is (not expanded).
pub fn registry_resolve_deps(paths: &[&str]) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut stack: Vec<&str> = paths.to_vec();

    while let Some(path) = stack.pop() {
        if !seen.insert(path) {
            continue;
        }
        result.push(path.to_string());

        // union deps from all entries with this path (handles Embedded + Shadow pairs)
        for entry in VFS_REGISTRY.iter().filter(|e| e.path == path) {
            for dep in entry.deps {
                if !seen.contains(dep) {
                    stack.push(dep);
                }
            }
        }
    }

    result
}

/// build the default safe allowlist from `VFS_REGISTRY` (`default_safe: true` entries
/// with their transitive deps resolved, filtered by active cargo features).
pub(crate) fn registry_safe_allowlist() -> Vec<String> {
    let paths: Vec<&str> = VFS_REGISTRY
        .iter()
        .filter(|e| e.default_safe && feature_enabled(e.feature))
        .map(|e| e.path)
        .collect();
    registry_resolve_deps(&paths)
}

/// build the full allowlist from `VFS_REGISTRY` (all entries with deps resolved,
/// filtered by active cargo features).
pub(crate) fn registry_all_allowlist() -> Vec<String> {
    let paths: Vec<&str> = VFS_REGISTRY
        .iter()
        .filter(|e| feature_enabled(e.feature))
        .map(|e| e.path)
        .collect();
    registry_resolve_deps(&paths)
}

/// get all VFS files to embed from `VFS_REGISTRY` (embedded + feature-gated).
#[allow(dead_code)] // used in build.rs via include!
fn registry_vfs_files() -> Vec<&'static str> {
    VFS_REGISTRY
        .iter()
        .filter(|e| e.source == VfsSource::Embedded && feature_enabled(e.feature))
        .flat_map(|e| e.files.iter().copied())
        .collect()
}

/// get all clib entries from `VFS_REGISTRY`.
#[allow(dead_code)] // used in build.rs via include!
fn registry_clib_entries() -> Vec<&'static ClibEntry> {
    VFS_REGISTRY
        .iter()
        .filter_map(|e| e.clib.as_ref())
        .collect()
}

/// Inject VFS shadow modules for sandboxed contexts.
///
/// Iterates `VFS_REGISTRY` for `VfsSource::Shadow` entries and registers
/// their `.sld` content into the dynamic VFS under canonical `/vfs/lib/`
/// paths. Hand-written shadows use `shadow_sld`; generated stubs (from
/// `SHADOW_STUBS` via build.rs) are looked up in `GENERATED_SHADOW_SLDS`.
///
/// Must be called before the VFS gate is armed (before `VFS_GATE` is set
/// to `GATE_CHECK`).
pub(crate) fn register_vfs_shadows() {
    use std::ffi::CString;

    let register_one = |path: &str, sld: &str| {
        let vfs_path = format!("/vfs/lib/{}.sld", path);
        let c_path = CString::new(vfs_path).expect("valid VFS path");
        unsafe {
            crate::ffi::tein_vfs_register(
                c_path.as_ptr(),
                sld.as_ptr() as *const std::ffi::c_char,
                sld.len() as std::ffi::c_uint,
            );
        }
    };

    for entry in VFS_REGISTRY.iter() {
        if entry.source != VfsSource::Shadow {
            continue;
        }
        if let Some(sld) = entry.shadow_sld {
            // hand-written shadow (scheme/file, scheme/process-context, etc.)
            register_one(entry.path, sld);
        }
        // generated stubs have shadow_sld: None — handled below
    }

    // generated stubs from SHADOW_STUBS (via build.rs)
    for &(path, sld) in GENERATED_SHADOW_SLDS.iter() {
        register_one(path, sld);
    }
}

// generated by build.rs — shadow stub .sld strings for OS-touching modules
include!(concat!(env!("OUT_DIR"), "/tein_shadow_stubs.rs"));

// generated by build.rs — module path → exported binding names
include!(concat!(env!("OUT_DIR"), "/tein_exports.rs"));

/// look up the exported binding names for a VFS module by path.
///
/// returns `Some(&[...])` if the module is in the generated exports table,
/// or `None` for paths not in the registry (e.g. `chibi/*` internal modules).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn module_exports(path: &str) -> Option<&'static [&'static str]> {
    MODULE_EXPORTS
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, exports)| *exports)
}

/// collect all exports from modules NOT in the given allowlist.
///
/// returns `(binding_name, module_path)` pairs for registering UX stubs —
/// informative errors that tell sandbox users which module to import.
/// bindings from modules with empty export lists (alias modules like `scheme/bitwise`)
/// are silently skipped since they have no top-level names to stub.
pub(crate) fn unexported_stubs(allowed_modules: &[String]) -> Vec<(&'static str, &'static str)> {
    // collect every binding name already provided by an allowed module.
    // stubs must never be generated for these — doing so clobbers real bindings.
    // this is especially important for mega re-export bundles like scheme/red and
    // scheme/small that duplicate hundreds of names from other modules.
    let covered: std::collections::HashSet<&str> = MODULE_EXPORTS
        .iter()
        .filter(|(p, _)| allowed_modules.iter().any(|a| a == p))
        .flat_map(|(_, exports)| exports.iter().copied())
        .collect();

    let mut stubs = Vec::new();
    for (path, exports) in MODULE_EXPORTS.iter() {
        if !allowed_modules.iter().any(|a| a == path) {
            for name in exports.iter() {
                if !covered.contains(name) {
                    stubs.push((*name, *path));
                }
            }
        }
    }
    stubs
}

/// Module set configuration for sandboxed contexts.
///
/// Controls which VFS modules are importable when using [`ContextBuilder::sandboxed()`].
/// Dependencies are always resolved automatically from the registry.
///
/// # examples
///
/// ```
/// use tein::{Context, sandbox::Modules};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // allow only scheme/base
/// let ctx = Context::builder()
///     .standard_env()
///     .sandboxed(Modules::only(&["scheme/base"]))
///     .build()?;
/// let result = ctx.evaluate("(import (scheme base)) (+ 1 2)")?;
/// assert_eq!(result, tein::Value::Integer(3));
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Default)]
pub enum Modules {
    /// conservative safe set — default for sandboxed contexts.
    ///
    /// includes all modules marked `default_safe: true` in the registry,
    /// with transitive deps resolved. excludes `scheme/eval`, `scheme/load`,
    /// `scheme/r5rs`, and `scheme/time`. `scheme/repl`, `scheme/file`,
    /// `scheme/process-context`, and `tein/process` are included via
    /// shadow modules or neutered trampolines.
    #[default]
    Safe,
    /// all vetted modules in the registry (superset of `Safe`).
    All,
    /// syntax only — no modules, not even `scheme/base`.
    ///
    /// `import` is still available as syntax (so code can attempt imports),
    /// but all module imports will be rejected by the VFS gate.
    None,
    /// custom explicit module list; transitive deps resolved automatically.
    Only(Vec<String>),
}

impl Modules {
    /// construct a custom module list from module path strings.
    ///
    /// transitive dependencies are resolved automatically at build time.
    pub fn only(modules: &[&str]) -> Self {
        Modules::Only(modules.iter().map(|s| s.to_string()).collect())
    }
}

/// check whether a cargo feature gate is satisfied at runtime.
///
/// in sandbox.rs this is a compile-time check. build.rs uses the same check.
#[inline]
fn feature_enabled(feature: Option<&str>) -> bool {
    match feature {
        None => true,
        Some("json") => cfg!(feature = "json"),
        Some("toml") => cfg!(feature = "toml"),
        Some("uuid") => cfg!(feature = "uuid"),
        Some("time") => cfg!(feature = "time"),
        Some(f) => {
            // unknown feature name — conservatively include (build.rs handles gating)
            eprintln!("warning: unknown feature gate in VFS_REGISTRY: {f}");
            true
        }
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn registry_safe_allowlist_contains_expected_modules() {
        let safe = registry_safe_allowlist();
        // core r7rs modules expected in safe set
        assert!(
            safe.iter().any(|m| m == "scheme/base"),
            "scheme/base missing"
        );
        assert!(
            safe.iter().any(|m| m == "scheme/write"),
            "scheme/write missing"
        );
        assert!(safe.iter().any(|m| m == "srfi/1"), "srfi/1 missing");
        // shadow modules — present in safe set (shadow replaces native)
        assert!(
            safe.iter().any(|m| m == "scheme/file"),
            "scheme/file missing from safe (shadow)"
        );
        assert!(
            safe.iter().any(|m| m == "scheme/repl"),
            "scheme/repl missing from safe (shadow)"
        );
        // tein/process — safe (trampolines neuter env/argv in sandbox)
        assert!(
            safe.iter().any(|m| m == "tein/process"),
            "tein/process missing from safe"
        );
        // scheme/process-context shadow
        assert!(
            safe.iter().any(|m| m == "scheme/process-context"),
            "scheme/process-context missing from safe (shadow)"
        );
        // scheme/show + srfi/166
        assert!(
            safe.iter().any(|m| m == "scheme/show"),
            "scheme/show missing from safe"
        );
        assert!(
            safe.iter().any(|m| m == "srfi/166"),
            "srfi/166 missing from safe"
        );
        // excluded modules must not appear
        assert!(
            !safe.iter().any(|m| m == "scheme/eval"),
            "scheme/eval should not be in safe set"
        );
    }

    #[test]
    fn registry_all_allowlist_is_superset_of_safe() {
        let safe = registry_safe_allowlist();
        let all = registry_all_allowlist();
        // all must contain everything safe contains
        for module in &safe {
            assert!(
                all.iter().any(|m| m == module),
                "all_allowlist missing module from safe: {module}"
            );
        }
        // all must be strictly larger (unsafe modules like scheme/eval are included)
        assert!(
            all.len() > safe.len(),
            "registry_all_allowlist should be larger than safe"
        );
        // scheme/eval + scheme/repl must be present in all
        assert!(
            all.iter().any(|m| m == "scheme/eval"),
            "scheme/eval missing from all"
        );
        assert!(
            all.iter().any(|m| m == "tein/process"),
            "tein/process missing from all"
        );
    }

    #[test]
    fn registry_resolve_deps_resolves_transitive() {
        // scheme/char transitively pulls in chibi/char-set/full, chibi/iset/base, etc.
        let resolved = registry_resolve_deps(&["scheme/char"]);
        assert!(resolved.iter().any(|m| m == "scheme/char"));
        assert!(
            resolved.iter().any(|m| m == "chibi/char-set/full"),
            "scheme/char should transitively pull chibi/char-set/full"
        );
        assert!(
            resolved.iter().any(|m| m == "chibi/iset/base"),
            "scheme/char should transitively pull chibi/iset/base"
        );
        // srfi/39 comes from scheme/base (via chibi chain) — verify no duplicates
        let resolved_base = registry_resolve_deps(&["scheme/base"]);
        let count_srfi9 = resolved_base
            .iter()
            .filter(|m| m.as_str() == "srfi/9")
            .count();
        assert_eq!(
            count_srfi9, 1,
            "srfi/9 should appear exactly once (no duplicates)"
        );
    }

    #[test]
    fn registry_resolve_deps_unknown_path_passthrough() {
        // unknown paths should be included as-is, not panic
        let resolved = registry_resolve_deps(&["some/unknown/module"]);
        assert!(resolved.iter().any(|m| m == "some/unknown/module"));
    }
}

#[cfg(test)]
mod exports_tests {
    use super::*;

    #[test]
    fn module_exports_scheme_base_contains_arithmetic() {
        let exports = module_exports("scheme/base").expect("scheme/base should have exports");
        assert!(exports.contains(&"+"), "scheme/base must export '+'");
        assert!(exports.contains(&"map"), "scheme/base must export 'map'");
        assert!(
            exports.contains(&"define"),
            "scheme/base must export 'define'"
        );
    }

    #[test]
    fn module_exports_nonexistent_returns_none() {
        assert!(
            module_exports("nonexistent/module").is_none(),
            "unknown module should return None"
        );
    }

    #[test]
    fn module_exports_dynamic_module_tein_uuid() {
        let exports = module_exports("tein/uuid").expect("tein/uuid should have exports");
        assert!(
            exports.contains(&"make-uuid"),
            "tein/uuid must export 'make-uuid'"
        );
        assert!(exports.contains(&"uuid?"), "tein/uuid must export 'uuid?'");
        assert!(
            exports.contains(&"uuid-nil"),
            "tein/uuid must export 'uuid-nil'"
        );
    }

    #[test]
    fn unexported_stubs_with_base_allowed_excludes_base_exports() {
        let allowed = vec!["scheme/base".to_string()];
        let stubs = unexported_stubs(&allowed);
        // '+' is exported only by scheme/base — it must not appear in stubs when allowed
        assert!(
            !stubs.iter().any(|(name, _)| *name == "+"),
            "'+' from allowed scheme/base must not appear in stubs"
        );
        // 'number->string' is scheme/base-only
        assert!(
            !stubs.iter().any(|(name, _)| *name == "number->string"),
            "'number->string' from allowed scheme/base must not appear in stubs"
        );
    }

    #[test]
    fn unexported_stubs_with_empty_allowlist_includes_all_exports() {
        let stubs = unexported_stubs(&[]);
        // with nothing allowed, all module exports appear in stubs
        assert!(
            stubs.iter().any(|(name, _)| *name == "+"),
            "'+' should appear in stubs when nothing allowed"
        );
        assert!(
            stubs
                .iter()
                .any(|(name, module)| *name == "+" && *module == "scheme/base"),
            "stub for '+' should reference 'scheme/base'"
        );
    }

    #[test]
    fn unexported_stubs_records_providing_module() {
        let stubs = unexported_stubs(&[]);
        // json-parse should be attributed to tein/json
        if let Some((_, module)) = stubs.iter().find(|(name, _)| *name == "json-parse") {
            assert_eq!(
                *module, "tein/json",
                "json-parse should be attributed to tein/json"
            );
        }
        // make-uuid to tein/uuid
        if let Some((_, module)) = stubs.iter().find(|(name, _)| *name == "make-uuid") {
            assert_eq!(
                *module, "tein/uuid",
                "make-uuid should be attributed to tein/uuid"
            );
        }
    }

    #[test]
    fn unexported_stubs_dedup_skips_names_covered_by_allowed_modules() {
        // scheme/red re-exports '+' (and hundreds of other names) from scheme/base.
        // when scheme/base is allowed, scheme/red must NOT generate a stub for '+' —
        // that would clobber the real binding.
        let allowed = vec!["scheme/base".to_string()];
        let stubs = unexported_stubs(&allowed);
        // '+' is covered by allowed scheme/base — no stub from any module
        assert!(
            !stubs.iter().any(|(name, _)| *name == "+"),
            "'+' covered by allowed scheme/base must not appear in stubs from any module \
             (e.g. scheme/red)"
        );
        // a binding unique to scheme/red (not in scheme/base) should still produce a stub
        let red_unique = stubs
            .iter()
            .any(|(_, module)| *module == "scheme/red" || *module == "scheme/small");
        assert!(
            red_unique,
            "scheme/red or scheme/small should still contribute stubs for names \
             not covered by the allowlist"
        );
    }
}
