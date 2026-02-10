//! sandboxing presets for restricted scheme environments
//!
//! each preset defines a set of chibi-scheme primitive names that can be
//! selectively allowed in a restricted context. presets are additive —
//! combine them via [`ContextBuilder::preset()`](crate::ContextBuilder::preset).

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
