//! Sandboxing presets and filesystem policy for restricted Scheme environments.
//!
//! tein's sandboxing has four independent layers:
//!
//! 1. **Environment restriction** — expose only selected primitives via presets
//! 2. **Step limits** — cap VM instructions per evaluation
//! 3. **File IO policy** — allowlist filesystem paths for reading/writing
//! 4. **Module policy** — restrict `(import ...)` to safe modules (three-tier: Allowlist / VfsAll / Unrestricted)
//!
//! # Presets
//!
//! Each [`Preset`] defines a set of Chibi-Scheme primitive names. Presets are
//! additive — combine them via [`ContextBuilder::preset()`](crate::ContextBuilder::preset).
//! Core syntax (`define`, `lambda`, `if`, `set!`, `quote`, etc.) is always
//! available regardless of preset selection.
//!
//! ```
//! use tein::Context;
//! use tein::sandbox::{ARITHMETIC, LISTS};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ctx = Context::builder()
//!     .preset(&ARITHMETIC)
//!     .preset(&LISTS)
//!     .step_limit(50_000)
//!     .build()?;
//!
//! // arithmetic and list ops work
//! let result = ctx.evaluate("(+ 1 (car (cons 2 3)))")?;
//! assert_eq!(result, tein::Value::Integer(3));
//!
//! // string ops are blocked
//! assert!(ctx.evaluate(r#"(string-length "hello")"#).is_err());
//! # Ok(())
//! # }
//! ```
//!
//! # Preset reference
//!
//! | preset | primitives |
//! |--------|-----------|
//! | [`ARITHMETIC`] | `+`, `-`, `*`, `/`, `quotient`, `remainder`, `expt`, comparisons, exact↔inexact |
//! | [`MATH`] | `exp`, `ln`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan1`, `sqrt`, rounding |
//! | [`LISTS`] | `car`, `cdr`, `cons`, `null?`, `pair?`, `list?`, `length*`, `reverse`, `append2`, `memq`, `assq` |
//! | [`VECTORS`] | `vector-ref`, `vector-set!`, `vector-length`, `make-vector`, `list->vector` |
//! | [`STRINGS`] | `string-ref`, `string-length`, `substring`, `string?`, conversions, `make-string` |
//! | [`CHARACTERS`] | `char?`, `char->integer`, `integer->char`, `char-upcase`, `char-downcase` |
//! | [`TYPE_PREDICATES`] | `eq?`, `equal?`, `null?`, `symbol?`, `char?`, `fixnum?`, `flonum?`, type tests |
//! | [`MUTATION`] | `set-car!`, `set-cdr!`, `vector-set!`, `string-set!` |
//! | [`STRING_PORTS`] | `open-input-string`, `open-output-string`, `get-output-string` |
//! | [`STDOUT_ONLY`] | `write`, `write-char`, `flush-output`, `current-output-port`, `current-error-port` |
//! | [`EXCEPTIONS`] | `make-exception`, `raise`, exception accessors |
//! | [`BYTEVECTORS`] | `bytevector-u8-ref`, `bytevector-u8-set!`, `bytevector-length`, `make-bytevector` |
//! | [`IO_READ`] | `read`, `read-char`, `peek-char`, `char-ready?`, `current-input-port` |
//! | [`CONTROL`] | `apply1`, `%call/cc` |
//!
//! # Convenience builders
//!
//! Two convenience methods on [`crate::ContextBuilder`] compose presets for common use cases:
//!
//! - [`.pure_computation()`](crate::ContextBuilder::pure_computation) — `ARITHMETIC` + `MATH` +
//!   `LISTS` + `VECTORS` + `STRINGS` + `CHARACTERS` + `TYPE_PREDICATES`
//! - [`.safe()`](crate::ContextBuilder::safe) — `pure_computation()` + `MUTATION` +
//!   `STRING_PORTS` + `STDOUT_ONLY` + `EXCEPTIONS`
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
//! # Module policy
//!
//! Module imports in sandboxed standard-env contexts are restricted by a
//! three-tier policy:
//!
//! - **Allowlist** (default) — only [`SAFE_MODULES`] + transitive deps.
//!   extend with [`.allow_module()`](crate::ContextBuilder::allow_module).
//! - **VfsAll** — all curated VFS modules. set with [`.vfs_all()`](crate::ContextBuilder::vfs_all).
//! - **Unrestricted** — VFS + filesystem (unsandboxed contexts).

use std::cell::{Cell, RefCell};
use std::path::Path;

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

/// a module available in the VFS, with its transitive dependencies.
///
/// every module that tein allows through the VFS gate must have an entry
/// in either [`VFS_MODULES_SAFE`] or [`VFS_MODULES_ALL`]. the `deps` field
/// lists module path prefixes that this module imports transitively — resolved
/// at build time from `.sld` `(import ...)` chains, not parsed at runtime.
///
/// `(chibi)` (the primitive core) is never listed as a dep — it's always
/// available and not gated.
pub struct VfsModule {
    /// module path prefix, e.g. `"scheme/char"`, `"tein/json"`, `"srfi/1"`.
    pub path: &'static str,
    /// paths of modules this one depends on (vetted from `.sld` import chains).
    /// `(chibi)` primitive core is omitted — always available.
    pub deps: &'static [&'static str],
}

/// controls which VFS modules can be imported via `(import ...)`.
///
/// ## variants
///
/// | gate | what passes | use case |
/// |------|------------|----------|
/// | `Off` | VFS + filesystem — no restriction | unsandboxed contexts |
/// | `Allow(vec)` | only listed module prefixes (must be in VFS) | sandboxed contexts |
///
/// ## VFS safety contract
///
/// VFS modules are curated to ensure no module can bypass tein's safety layers
/// (preset allowlists, FsPolicy, fuel/timeout). capabilities exposed by
/// VFS modules remain subject to these controls.
///
/// ## default behaviour
///
/// sandboxed contexts (standard_env + presets) default to
/// `Allow(vfs_safe_allowlist())`. use [`.vfs_gate_all()`](crate::ContextBuilder::vfs_gate_all)
/// or [`.allow_module()`](crate::ContextBuilder::allow_module) to adjust.
///
/// ## modules NOT in the VFS registry
///
/// the following chibi modules exist in the VFS filesystem but are **not vetted**
/// and will be blocked by any active gate:
///
/// - `scheme/file` — raw filesystem IO, no policy checks. use `(tein file)` instead.
/// - `scheme/process-context` — `exit`/`emergency-exit` from `(chibi process)` kills the
///   host process, bypassing all rust error handling. use `(tein process)` instead.
/// - `scheme/load` — loads arbitrary files from filesystem. use `(tein load)` instead.
/// - `scheme/r5rs` — re-exports `scheme/file`, `scheme/load`, `scheme/process-context`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VfsGate {
    /// no restriction — VFS + filesystem modules all pass. used for unsandboxed contexts.
    Off,
    /// only listed module prefixes (+ their transitive deps) pass.
    /// deps are resolved automatically from [`VfsModule`] data.
    Allow(Vec<String>),
}

/// numeric gate level for C interop. mirrors `tein_vfs_gate` in `tein_shim.c`.
pub(crate) const GATE_OFF: u8 = 0;
/// numeric gate level for C interop — rust callback checks the allowlist.
pub(crate) const GATE_CHECK: u8 = 1;

thread_local! {
    /// numeric gate level (0=off, 1=check). set during Context::build(), cleared on drop.
    pub(crate) static VFS_GATE: Cell<u8> = const { Cell::new(GATE_OFF) };

    /// the resolved allowlist, populated when gate is `Allow`.
    /// read by the C→rust callback (`tein_vfs_gate_check`) during module resolution.
    pub(crate) static VFS_ALLOWLIST: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

// ---------------------------------------------------------------------------
// VFS module registry
// ---------------------------------------------------------------------------

/// conservative sandbox set — default for sandboxed contexts.
///
/// tein modules are listed explicitly rather than via a `"tein/"` blanket
/// because `(tein process)` is intentionally excluded — `command-line` leaks
/// host argv. use `.allow_module("tein/process")` or `.vfs_gate_all()` to opt in.
///
/// `scheme/time` and `scheme/show` are excluded because they transitively
/// depend on unvetted modules (`scheme/process-context`, `scheme/file`).
/// see github issues #90 and #91 for safe alternatives.
pub const VFS_MODULES_SAFE: &[VfsModule] = &[
    // --- tein modules (tein/process excluded: leaks host argv) ---
    VfsModule { path: "tein/foreign", deps: &["scheme/base"] },
    VfsModule { path: "tein/reader", deps: &["scheme/base"] },
    VfsModule { path: "tein/macro", deps: &["scheme/base"] },
    VfsModule { path: "tein/test", deps: &["scheme/base", "scheme/write"] },
    VfsModule { path: "tein/docs", deps: &["scheme/base"] },
    VfsModule { path: "tein/json", deps: &["scheme/base"] },
    VfsModule { path: "tein/toml", deps: &["scheme/base"] },
    VfsModule { path: "tein/uuid", deps: &[] },
    VfsModule { path: "tein/file", deps: &["scheme/base"] },
    VfsModule { path: "tein/load", deps: &["scheme/base"] },
    // --- r7rs standard libraries (safe subset) ---
    VfsModule { path: "scheme/base", deps: &[
        "chibi/equiv", "chibi/string", "chibi/io", "chibi/ast",
        "srfi/9", "srfi/11", "srfi/39",
    ]},
    VfsModule { path: "scheme/bitwise", deps: &["srfi/151"] },
    VfsModule { path: "scheme/box", deps: &["srfi/111"] },
    VfsModule { path: "scheme/bytevector", deps: &["scheme/base", "srfi/151"] },
    VfsModule { path: "scheme/case-lambda", deps: &["srfi/16"] },
    VfsModule { path: "scheme/char", deps: &[
        "scheme/base", "chibi/char-set/full", "chibi/char-set/base", "chibi/iset/base",
    ]},
    VfsModule { path: "scheme/charset", deps: &["srfi/14"] },
    VfsModule { path: "scheme/comparator", deps: &["srfi/128"] },
    VfsModule { path: "scheme/complex", deps: &[] },
    VfsModule { path: "scheme/cxr", deps: &[] },
    VfsModule { path: "scheme/division", deps: &["scheme/base"] },
    VfsModule { path: "scheme/ephemeron", deps: &["srfi/124"] },
    VfsModule { path: "scheme/eval", deps: &[] },
    VfsModule { path: "scheme/fixnum", deps: &["srfi/143"] },
    VfsModule { path: "scheme/flonum", deps: &["srfi/144"] },
    VfsModule { path: "scheme/generator", deps: &["srfi/121"] },
    VfsModule { path: "scheme/hash-table", deps: &["srfi/125"] },
    VfsModule { path: "scheme/ideque", deps: &["srfi/134"] },
    VfsModule { path: "scheme/ilist", deps: &["srfi/116"] },
    VfsModule { path: "scheme/inexact", deps: &[] },
    VfsModule { path: "scheme/lazy", deps: &[] },
    VfsModule { path: "scheme/list", deps: &["srfi/1"] },
    VfsModule { path: "scheme/list-queue", deps: &["srfi/117"] },
    VfsModule { path: "scheme/lseq", deps: &["srfi/127"] },
    VfsModule { path: "scheme/mapping", deps: &["srfi/146"] },
    VfsModule { path: "scheme/read", deps: &["srfi/38"] },
    VfsModule { path: "scheme/regex", deps: &["srfi/115"] },
    VfsModule { path: "scheme/repl", deps: &[] },
    VfsModule { path: "scheme/rlist", deps: &["srfi/101"] },
    VfsModule { path: "scheme/set", deps: &["srfi/113"] },
    VfsModule { path: "scheme/sort", deps: &["srfi/132"] },
    VfsModule { path: "scheme/stream", deps: &["srfi/41"] },
    VfsModule { path: "scheme/text", deps: &["srfi/135"] },
    VfsModule { path: "scheme/vector", deps: &["srfi/133"] },
    VfsModule { path: "scheme/write", deps: &["srfi/38"] },
    // --- srfi modules (transitive deps of the above) ---
    VfsModule { path: "srfi/1", deps: &[] },
    VfsModule { path: "srfi/1/immutable", deps: &[] },
    VfsModule { path: "srfi/2", deps: &[] },
    VfsModule { path: "srfi/8", deps: &[] },
    VfsModule { path: "srfi/9", deps: &[] },
    VfsModule { path: "srfi/11", deps: &[] },
    VfsModule { path: "srfi/14", deps: &["chibi/char-set"] },
    VfsModule { path: "srfi/16", deps: &[] },
    VfsModule { path: "srfi/27", deps: &[] },
    VfsModule { path: "srfi/38", deps: &["srfi/69", "chibi/ast"] },
    VfsModule { path: "srfi/39", deps: &[] },
    VfsModule { path: "srfi/41", deps: &["scheme/base", "scheme/lazy", "srfi/1"] },
    VfsModule { path: "srfi/69", deps: &["srfi/9"] },
    VfsModule { path: "srfi/95", deps: &[] },
    VfsModule { path: "srfi/101", deps: &[
        "scheme/base", "srfi/16", "srfi/1", "srfi/125", "srfi/151",
    ]},
    VfsModule { path: "srfi/111", deps: &["scheme/base"] },
    VfsModule { path: "srfi/113", deps: &["scheme/base", "srfi/1", "srfi/125", "srfi/128"] },
    VfsModule { path: "srfi/115", deps: &["chibi/regexp"] },
    VfsModule { path: "srfi/116", deps: &["scheme/base", "srfi/1/immutable", "srfi/128"] },
    VfsModule { path: "srfi/117", deps: &["scheme/base", "srfi/1"] },
    VfsModule { path: "srfi/121", deps: &["scheme/base", "srfi/130"] },
    VfsModule { path: "srfi/124", deps: &["chibi/weak", "scheme/base"] },
    VfsModule { path: "srfi/125", deps: &["scheme/base", "srfi/128", "srfi/69", "chibi/ast"] },
    VfsModule { path: "srfi/127", deps: &["scheme/base", "srfi/1"] },
    VfsModule { path: "srfi/128", deps: &[
        "scheme/base", "scheme/char", "srfi/27", "srfi/69", "srfi/95",
        "srfi/98", "srfi/151", "chibi/ast",
    ]},
    VfsModule { path: "srfi/130", deps: &["scheme/base", "scheme/char", "scheme/write", "chibi/string"] },
    VfsModule { path: "srfi/132", deps: &["scheme/base", "srfi/95"] },
    VfsModule { path: "srfi/133", deps: &["scheme/base"] },
    VfsModule { path: "srfi/134", deps: &[
        "scheme/base", "srfi/16", "srfi/1", "srfi/9", "srfi/121",
    ]},
    VfsModule { path: "srfi/135", deps: &["scheme/base", "srfi/16", "scheme/char", "srfi/135/kernel8"] },
    VfsModule { path: "srfi/135/kernel8", deps: &["scheme/base"] },
    VfsModule { path: "srfi/141", deps: &["scheme/base", "scheme/division"] },
    VfsModule { path: "srfi/143", deps: &["scheme/base", "srfi/141", "srfi/151"] },
    VfsModule { path: "srfi/144", deps: &["srfi/141"] },
    VfsModule { path: "srfi/145", deps: &["scheme/base", "chibi/assert"] },
    VfsModule { path: "srfi/146", deps: &[
        "scheme/base", "srfi/16", "srfi/1", "srfi/2", "srfi/8",
        "srfi/121", "srfi/128", "srfi/145",
    ]},
    VfsModule { path: "srfi/146/hamt", deps: &[
        "scheme/base", "srfi/16", "srfi/143", "srfi/151",
        "srfi/146/hamt-misc", "srfi/146/vector-edit",
    ]},
    VfsModule { path: "srfi/146/hamt-map", deps: &[
        "scheme/base", "srfi/16", "srfi/1", "srfi/146/hamt", "srfi/146/hamt-misc",
    ]},
    VfsModule { path: "srfi/146/hamt-misc", deps: &["scheme/base", "srfi/16", "srfi/125", "srfi/128"] },
    VfsModule { path: "srfi/146/vector-edit", deps: &["scheme/base"] },
    VfsModule { path: "srfi/146/hash", deps: &[
        "scheme/base", "srfi/16", "srfi/1", "srfi/8", "srfi/121",
        "srfi/128", "srfi/145", "srfi/146/hamt-map",
    ]},
    VfsModule { path: "srfi/151", deps: &[] },
    VfsModule { path: "srfi/165", deps: &[
        "scheme/base", "srfi/1", "srfi/111", "srfi/125", "srfi/128", "srfi/146",
    ]},
    VfsModule { path: "srfi/98", deps: &[] },
    // --- chibi internal modules (transitive deps) ---
    VfsModule { path: "chibi/ast", deps: &[] },
    VfsModule { path: "chibi/assert", deps: &[] },
    VfsModule { path: "chibi/equiv", deps: &["srfi/69"] },
    VfsModule { path: "chibi/io", deps: &["chibi/ast"] },
    VfsModule { path: "chibi/optional", deps: &[] },
    VfsModule { path: "chibi/string", deps: &["chibi/ast", "chibi/char-set/base"] },
    VfsModule { path: "chibi/weak", deps: &[] },
    VfsModule { path: "chibi/time", deps: &[] },
    VfsModule { path: "chibi/char-set", deps: &["chibi/char-set/base", "chibi/char-set/extras"] },
    VfsModule { path: "chibi/char-set/base", deps: &["chibi/iset/base"] },
    VfsModule { path: "chibi/char-set/full", deps: &["chibi/iset/base", "chibi/char-set/base"] },
    VfsModule { path: "chibi/char-set/ascii", deps: &["chibi/iset/base", "chibi/char-set/base"] },
    VfsModule { path: "chibi/char-set/extras", deps: &["chibi/iset", "chibi/char-set/base"] },
    VfsModule { path: "chibi/char-set/boundary", deps: &["chibi/char-set"] },
    VfsModule { path: "chibi/iset", deps: &[
        "scheme/base", "chibi/iset/base", "chibi/iset/iterators", "chibi/iset/constructors",
    ]},
    VfsModule { path: "chibi/iset/base", deps: &["srfi/9", "srfi/151"] },
    VfsModule { path: "chibi/iset/iterators", deps: &["chibi/iset/base", "srfi/9", "srfi/151"] },
    VfsModule { path: "chibi/iset/constructors", deps: &[
        "chibi/iset/base", "chibi/iset/iterators", "srfi/151",
    ]},
    VfsModule { path: "chibi/regexp", deps: &[
        "srfi/69", "scheme/char", "srfi/9", "chibi/char-set",
        "chibi/char-set/full", "chibi/char-set/ascii", "srfi/151",
        "chibi/char-set/boundary",
    ]},
    VfsModule { path: "chibi/show/shared", deps: &["scheme/base", "scheme/write", "srfi/69"] },
];

/// all vetted VFS modules — superset of [`VFS_MODULES_SAFE`].
///
/// includes modules that are safe by implementation but expose sensitive
/// information or capabilities that the conservative sandbox excludes.
/// use [`.vfs_gate_all()`](crate::ContextBuilder::vfs_gate_all) to enable.
///
/// **not included** (unvetted — blocked by any active gate):
/// - `scheme/file` — raw filesystem IO with no policy checks. use `(tein file)`.
/// - `scheme/process-context` — `exit`/`emergency-exit` kills host process. use `(tein process)`.
/// - `scheme/load` — loads arbitrary filesystem files. use `(tein load)`.
/// - `scheme/r5rs` — re-exports the above three unvetted modules.
pub const VFS_MODULES_ALL: &[VfsModule] = &[
    // tein/process: safe rust-backed exit, but leaks host argv via command-line
    VfsModule { path: "tein/process", deps: &["scheme/base"] },
    // scheme/time: depends on scheme/process-context + scheme/file transitively.
    // allowed in ALL because the deps are also available when the user opts in.
    // see #90 for a future safe (tein time) alternative.
    VfsModule { path: "scheme/time", deps: &[
        "scheme/time/tai", "scheme/time/tai-to-utc-offset",
    ]},
    VfsModule { path: "scheme/time/tai", deps: &["scheme/base", "scheme/time/tai-to-utc-offset"] },
    VfsModule { path: "scheme/time/tai-to-utc-offset", deps: &[
        "scheme/base", "scheme/read", "srfi/18",
    ]},
    VfsModule { path: "srfi/18", deps: &["srfi/9", "chibi/ast", "chibi/time"] },
    // scheme/show: depends on scheme/file via srfi/166/columnar.
    // see #91 for a future safe (tein show) alternative.
    VfsModule { path: "scheme/show", deps: &["srfi/166"] },
    VfsModule { path: "scheme/mapping/hash", deps: &["srfi/146/hash"] },
    VfsModule { path: "srfi/166", deps: &[
        "srfi/166/base", "srfi/166/pretty", "srfi/166/columnar",
        "srfi/166/unicode", "srfi/166/color",
    ]},
    VfsModule { path: "srfi/166/base", deps: &[
        "scheme/base", "scheme/char", "scheme/complex", "scheme/inexact",
        "scheme/repl", "scheme/write", "srfi/1", "srfi/69", "srfi/130",
        "srfi/165", "chibi/show/shared",
    ]},
    VfsModule { path: "srfi/166/pretty", deps: &[
        "scheme/base", "scheme/char", "scheme/write", "chibi/show/shared",
        "srfi/1", "srfi/69", "srfi/130", "srfi/166/base", "srfi/166/color",
    ]},
    VfsModule { path: "srfi/166/columnar", deps: &[
        "scheme/base", "scheme/char", "srfi/1", "srfi/117", "srfi/130",
        "srfi/166/base", "chibi/optional",
    ]},
    VfsModule { path: "srfi/166/unicode", deps: &[
        "scheme/base", "scheme/char", "srfi/130", "srfi/151", "srfi/166/base",
    ]},
    VfsModule { path: "srfi/166/color", deps: &[
        "scheme/base", "srfi/130", "srfi/166/base",
    ]},
];

/// resolve a set of module paths to the complete transitive closure of their deps.
///
/// looks up each path in both [`VFS_MODULES_SAFE`] and [`VFS_MODULES_ALL`],
/// follows `deps` recursively, and returns a deduplicated flat list of all
/// module path prefixes (including the input paths themselves).
///
/// unknown paths (not in any registry) are included as-is but not expanded.
pub fn resolve_module_deps(paths: &[&str]) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut stack: Vec<&str> = paths.to_vec();

    while let Some(path) = stack.pop() {
        if !seen.insert(path) {
            continue;
        }
        result.push(path.to_string());

        // look up in both registries
        let module = VFS_MODULES_SAFE
            .iter()
            .chain(VFS_MODULES_ALL.iter())
            .find(|m| m.path == path);

        if let Some(m) = module {
            for dep in m.deps {
                if !seen.contains(dep) {
                    stack.push(dep);
                }
            }
        }
    }

    result
}

/// build the default safe allowlist — [`VFS_MODULES_SAFE`] with all transitive deps resolved.
pub(crate) fn vfs_safe_allowlist() -> Vec<String> {
    let paths: Vec<&str> = VFS_MODULES_SAFE.iter().map(|m| m.path).collect();
    resolve_module_deps(&paths)
}

/// build the full allowlist — all modules from both registries with deps resolved.
pub(crate) fn vfs_all_allowlist() -> Vec<String> {
    let paths: Vec<&str> = VFS_MODULES_SAFE
        .iter()
        .chain(VFS_MODULES_ALL.iter())
        .map(|m| m.path)
        .collect();
    resolve_module_deps(&paths)
}

/// A named set of Scheme primitives for environment restriction.
///
/// Used with [`ContextBuilder::preset()`](crate::ContextBuilder::preset)
/// to build allowlists. Presets are derived from Chibi's `opcodes.c`.
pub struct Preset {
    /// Human-readable name for this preset.
    pub name: &'static str,
    /// Primitive names to allow when this preset is active.
    pub primitives: &'static [&'static str],
}

/// Basic arithmetic operations.
pub const ARITHMETIC: Preset = Preset {
    name: "arithmetic",
    primitives: &[
        "+",
        "-",
        "*",
        "/",
        "quotient",
        "remainder",
        "expt",
        "<",
        "<=",
        ">",
        ">=",
        "=",
        "exact->inexact",
        "inexact->exact",
    ],
};

/// Transcendental math functions.
pub const MATH: Preset = Preset {
    name: "math",
    primitives: &[
        "exp",
        "ln",
        "sin",
        "cos",
        "tan",
        "asin",
        "acos",
        "atan1",
        "sqrt",
        "exact-sqrt",
        "round",
        "truncate",
        "floor",
        "ceiling",
    ],
};

/// List operations.
pub const LISTS: Preset = Preset {
    name: "lists",
    primitives: &[
        "car", "cdr", "cons", "null?", "pair?", "list?", "length*", "reverse", "append2", "memq",
        "assq",
    ],
};

/// Vector operations.
pub const VECTORS: Preset = Preset {
    name: "vectors",
    primitives: &[
        "vector-ref",
        "vector-set!",
        "vector-length",
        "make-vector",
        "list->vector",
    ],
};

/// String operations.
pub const STRINGS: Preset = Preset {
    name: "strings",
    primitives: &[
        "string-ref",
        "string-length",
        "substring",
        "string?",
        "string->number",
        "string->symbol",
        "symbol->string",
        "string-cmp",
        "string-concatenate",
        "make-string",
    ],
};

/// Character operations.
pub const CHARACTERS: Preset = Preset {
    name: "characters",
    primitives: &[
        "char?",
        "char->integer",
        "integer->char",
        "char-upcase",
        "char-downcase",
    ],
};

/// Type checking predicates.
pub const TYPE_PREDICATES: Preset = Preset {
    name: "type-predicates",
    primitives: &[
        "eq?",
        "equal?",
        "null?",
        "symbol?",
        "char?",
        "fixnum?",
        "flonum?",
        "pair?",
        "string?",
        "vector?",
        "bytevector?",
        "closure?",
        "exception?",
        "list?",
    ],
};

/// Mutation operations (set-car!, set-cdr!, vector-set!, string-set!).
pub const MUTATION: Preset = Preset {
    name: "mutation",
    primitives: &["set-car!", "set-cdr!", "vector-set!", "string-set!"],
};

/// String port operations (in-memory IO).
pub const STRING_PORTS: Preset = Preset {
    name: "string-ports",
    primitives: &[
        "open-input-string",
        "open-output-string",
        "get-output-string",
    ],
};

/// Stdout-only output (no file IO).
pub const STDOUT_ONLY: Preset = Preset {
    name: "stdout-only",
    primitives: &[
        "write",
        "write-char",
        "flush-output",
        "current-output-port",
        "current-error-port",
    ],
};

/// Exception handling.
pub const EXCEPTIONS: Preset = Preset {
    name: "exceptions",
    primitives: &[
        "make-exception",
        "raise",
        "exception-kind",
        "exception-irritants",
        "exception?",
    ],
};

/// Bytevector operations.
pub const BYTEVECTORS: Preset = Preset {
    name: "bytevectors",
    primitives: &[
        "bytevector-u8-ref",
        "bytevector-u8-set!",
        "bytevector-length",
        "make-bytevector",
        "bytevector?",
    ],
};

/// Input reading operations.
pub const IO_READ: Preset = Preset {
    name: "io-read",
    primitives: &[
        "read",
        "read-char",
        "peek-char",
        "char-ready?",
        "current-input-port",
    ],
};

/// Control flow primitives.
pub const CONTROL: Preset = Preset {
    name: "control",
    primitives: &["apply1", "%call/cc"],
};

/// Port-reading support primitives (used alongside file_read() policy).
///
/// These are the port operations needed to actually read data once a
/// file port has been opened via the policy-checked wrapper.
pub const FILE_READ_SUPPORT: Preset = Preset {
    name: "file-read-support",
    primitives: &[
        "close-input-port",
        "read",
        "read-char",
        "peek-char",
        "char-ready?",
        "current-input-port",
    ],
};

/// Port-writing support primitives (used alongside file_write() policy).
///
/// These are the port operations needed to actually write data once a
/// file port has been opened via the policy-checked wrapper.
pub const FILE_WRITE_SUPPORT: Preset = Preset {
    name: "file-write-support",
    primitives: &[
        "close-output-port",
        "write",
        "write-char",
        "flush-output",
        "current-output-port",
        "current-error-port",
    ],
};

/// All presets known to tein, for stub registration during sandbox build.
///
/// Used internally to determine which primitives should get sandbox stubs
/// when they aren't included in a context's allowlist.
pub(crate) const ALL_PRESETS: &[&Preset] = &[
    &ARITHMETIC,
    &MATH,
    &LISTS,
    &VECTORS,
    &STRINGS,
    &CHARACTERS,
    &TYPE_PREDICATES,
    &MUTATION,
    &STRING_PORTS,
    &STDOUT_ONLY,
    &EXCEPTIONS,
    &BYTEVECTORS,
    &IO_READ,
    &CONTROL,
    &FILE_READ_SUPPORT,
    &FILE_WRITE_SUPPORT,
];

/// Primitives that are **always** stubbed out in sandboxed contexts,
/// regardless of preset configuration.
///
/// These provide direct access to unrestricted environments and cannot
/// be safely exposed in any sandboxed context. Unlike [`ALL_PRESETS`],
/// these are never allowable — there is no preset that grants them.
///
/// A sandboxed scheme program holding any of these can call
/// `(eval code (interaction-environment))` to execute arbitrary code
/// in the full unrestricted environment, completely defeating presets.
///
/// Note: `compile` and `generate` are NOT listed here even though they
/// could theoretically be misused, because chibi uses `compile` internally
/// during macro expansion. Stubbing it breaks standard library features.
/// `eval` + environment accessors are sufficient to close the escape hatch.
///
/// Note: `%meta-env`, `find-module-file`, `env-exports`, `env-parent`, `%import`
/// are used by chibi's init-7 / meta-7 *during C-side initialisation*, not at
/// runtime from Scheme. They are safe to stub once the sandbox env is built.
pub(crate) const ALWAYS_STUB: &[&str] = &[
    // environment escape — direct access to unrestricted or meta environments
    "eval",
    "interaction-environment",
    "primitive-environment",
    "scheme-report-environment",
    "current-environment",
    "set-current-environment!",
    "%meta-env",
    // environment introspection — allows mapping the env chain from scheme
    "env-parent",
    "env-exports",
    // module system — filesystem module loading and path manipulation
    "%load",
    "%import",
    "load-module-file",
    "find-module-file",
    "add-module-directory",
    "current-module-path",
    // process info — exposes binary path and arguments
    "command-line",
    // type/vm system mutation — could enable type confusion or VM side-channels
    "register-simple-type",
    "register-optimization!",
    "print-vm-profile",
    "reset-vm-profile",
];
