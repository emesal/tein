//! Sandboxing presets and filesystem policy for restricted Scheme environments.
//!
//! tein's sandboxing has four independent layers:
//!
//! 1. **Environment restriction** — expose only selected primitives via presets
//! 2. **Step limits** — cap VM instructions per evaluation
//! 3. **File IO policy** — allowlist filesystem paths for reading/writing
//! 4. **Module policy** — restrict `(import ...)` to VFS-only modules
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
//! When a sandboxed context uses the standard environment, the module
//! policy is automatically set to VFS-only — `(import ...)` can only
//! load modules embedded in tein's virtual filesystem, not from the
//! host filesystem. This prevents sandbox escapes via modules like
//! `(chibi process)` or `(chibi filesystem)`.

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

/// Module import policy for sandboxed standard-env contexts.
///
/// Controls which modules can be loaded via `(import ...)`.
/// When a sandboxed context uses the standard environment, this is
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
    /// All modules allowed (unsandboxed or non-standard-env context).
    Unrestricted = 0,
    /// Only VFS modules allowed (sandboxed standard-env context).
    VfsOnly = 1,
}

thread_local! {
    /// Active module import policy (set during build, cleared on drop).
    pub(crate) static MODULE_POLICY: Cell<ModulePolicy> = const { Cell::new(ModulePolicy::Unrestricted) };
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
