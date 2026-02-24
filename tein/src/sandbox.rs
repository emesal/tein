//! sandboxing presets and filesystem policy for restricted scheme environments
//!
//! tein's sandboxing has four independent layers:
//!
//! 1. **environment restriction** — expose only selected primitives via presets
//! 2. **step limits** — cap VM instructions per evaluation
//! 3. **file IO policy** — allowlist filesystem paths for reading/writing
//! 4. **module policy** — restrict `(import ...)` to VFS-only modules
//!
//! # presets
//!
//! each [`Preset`] defines a set of chibi-scheme primitive names. presets are
//! additive — combine them via [`ContextBuilder::preset()`](crate::ContextBuilder::preset).
//! core syntax (`define`, `lambda`, `if`, `set!`, `quote`, etc.) is always
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
//! # preset reference
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
//! # convenience builders
//!
//! two convenience methods on [`crate::ContextBuilder`] compose presets for common use cases:
//!
//! - [`.pure_computation()`](crate::ContextBuilder::pure_computation) — `ARITHMETIC` + `MATH` +
//!   `LISTS` + `VECTORS` + `STRINGS` + `CHARACTERS` + `TYPE_PREDICATES`
//! - [`.safe()`](crate::ContextBuilder::safe) — `pure_computation()` + `MUTATION` +
//!   `STRING_PORTS` + `STDOUT_ONLY` + `EXCEPTIONS`
//!
//! # file IO policy
//!
//! `FsPolicy` controls which filesystem paths scheme code can access.
//! registered internally via
//! [`ContextBuilder::file_read()`](crate::ContextBuilder::file_read) and
//! [`ContextBuilder::file_write()`](crate::ContextBuilder::file_write).
//! paths are canonicalised before prefix-checking, so symlink and `..`
//! traversals are resolved.
//!
//! # module policy
//!
//! when a sandboxed context uses the standard environment, the module
//! policy is automatically set to VFS-only — `(import ...)` can only
//! load modules embedded in tein's virtual filesystem, not from the
//! host filesystem. this prevents sandbox escapes via modules like
//! `(chibi process)` or `(chibi filesystem)`.

use std::cell::{Cell, RefCell};
use std::path::Path;

/// filesystem access policy for sandboxed IO
///
/// controls which paths scheme code can read from and write to.
/// uses prefix matching against canonicalised paths.
pub(crate) struct FsPolicy {
    /// allowed path prefixes for reading
    pub read_prefixes: Vec<String>,
    /// allowed path prefixes for writing
    pub write_prefixes: Vec<String>,
}

impl FsPolicy {
    /// check if a path is allowed for reading
    ///
    /// canonicalises the full path (file must exist for reads).
    /// returns false if path is invalid or canonicalisation fails.
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

    /// check if a path is allowed for writing
    ///
    /// canonicalises the parent directory (must exist), appends filename.
    /// the file itself doesn't need to exist (r7rs: open-output-file creates it).
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
    /// active filesystem policy for the current context (set during build, cleared on drop)
    pub(crate) static FS_POLICY: RefCell<Option<FsPolicy>> = const { RefCell::new(None) };
}

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

/// a named set of scheme primitives for environment restriction
///
/// used with [`ContextBuilder::preset()`](crate::ContextBuilder::preset)
/// to build allowlists. presets are derived from chibi's `opcodes.c`.
pub struct Preset {
    /// human-readable name for this preset
    pub name: &'static str,
    /// primitive names to allow when this preset is active
    pub primitives: &'static [&'static str],
}

/// basic arithmetic operations
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

/// transcendental math functions
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

/// list operations
pub const LISTS: Preset = Preset {
    name: "lists",
    primitives: &[
        "car", "cdr", "cons", "null?", "pair?", "list?", "length*", "reverse", "append2", "memq",
        "assq",
    ],
};

/// vector operations
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

/// string operations
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

/// character operations
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

/// type checking predicates
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

/// mutation operations (set-car!, set-cdr!, vector-set!, string-set!)
pub const MUTATION: Preset = Preset {
    name: "mutation",
    primitives: &["set-car!", "set-cdr!", "vector-set!", "string-set!"],
};

/// string port operations (in-memory io)
pub const STRING_PORTS: Preset = Preset {
    name: "string-ports",
    primitives: &[
        "open-input-string",
        "open-output-string",
        "get-output-string",
    ],
};

/// stdout-only output (no file io)
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

/// exception handling
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

/// bytevector operations
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

/// input reading operations
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

/// control flow primitives
pub const CONTROL: Preset = Preset {
    name: "control",
    primitives: &["apply1", "%call/cc"],
};

/// port-reading support primitives (used alongside file_read() policy)
///
/// these are the port operations needed to actually read data once a
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

/// port-writing support primitives (used alongside file_write() policy)
///
/// these are the port operations needed to actually write data once a
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

/// all presets known to tein, for stub registration during sandbox build.
///
/// used internally to determine which primitives should get sandbox stubs
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
