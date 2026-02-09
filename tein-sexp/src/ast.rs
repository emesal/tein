//! s-expression abstract syntax tree
//!
//! core types for representing s-expressions with source location tracking.
//! every node carries a [`Span`] — programmatically-constructed nodes use
//! [`Span::NONE`].

use std::fmt;

/// source location for a parsed s-expression node
///
/// byte-level span within the original input, plus line/column for diagnostics.
/// lines and columns are 1-based; `line: 0` distinguishes [`Span::NONE`] from
/// any real source location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    /// byte offset from start of input
    pub offset: usize,
    /// byte length of this token/node
    pub len: usize,
    /// 1-based line number (0 = no source location)
    pub line: u32,
    /// 1-based column as byte offset within line
    pub column: u32,
}

impl Span {
    /// sentinel for programmatically-constructed nodes
    pub const NONE: Span = Span {
        offset: 0,
        len: 0,
        line: 0,
        column: 0,
    };

    /// true if this span has no source location
    pub fn is_none(self) -> bool {
        self.line == 0
    }

    /// merge two spans into one covering both (from start of `self` to end of `other`)
    pub fn merge(self, other: Span) -> Span {
        if self.is_none() {
            return other;
        }
        if other.is_none() {
            return self;
        }
        let end = other.offset + other.len;
        Span {
            offset: self.offset,
            len: end.saturating_sub(self.offset),
            line: self.line,
            column: self.column,
        }
    }
}

/// an s-expression with source location and optional comments
///
/// `Sexp` is a struct wrapping [`SexpKind`] + metadata, so fields like
/// span and comments can be added without changing pattern-matching code.
#[derive(Debug, Clone)]
pub struct Sexp {
    /// the actual s-expression variant
    pub kind: SexpKind,
    /// source location of this node
    pub span: Span,
    /// comments attached to this node (populated in comment-preservation mode)
    pub comments: Vec<Comment>,
}

/// the kind of s-expression
#[derive(Debug, Clone, PartialEq)]
pub enum SexpKind {
    /// integer literal
    Integer(i64),
    /// floating-point literal
    Float(f64),
    /// string literal
    String(String),
    /// symbol (identifier)
    Symbol(String),
    /// boolean `#t` or `#f`
    Boolean(bool),
    /// character literal `#\a`
    Char(char),
    /// proper list `(a b c)`
    List(Vec<Sexp>),
    /// dotted (improper) list `(a b . c)`
    DottedList(Vec<Sexp>, Box<Sexp>),
    /// vector `#(a b c)`
    Vector(Vec<Sexp>),
    /// empty list / nil `()`
    Nil,
}

/// a comment associated with an s-expression node
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    /// the comment text (without delimiters)
    pub text: String,
    /// source location of the comment
    pub span: Span,
    /// what kind of comment this is
    pub kind: CommentKind,
}

/// the kind of comment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentKind {
    /// `;` line comment
    Line,
    /// `#| ... |#` block comment
    Block,
    /// `#;` datum comment
    Datum,
}

// --- constructors ---

impl Sexp {
    /// create a sexp of the given kind with no source location
    fn new(kind: SexpKind) -> Self {
        Self {
            kind,
            span: Span::NONE,
            comments: Vec::new(),
        }
    }

    /// integer literal
    pub fn integer(n: i64) -> Self {
        Self::new(SexpKind::Integer(n))
    }

    /// floating-point literal
    pub fn float(f: f64) -> Self {
        Self::new(SexpKind::Float(f))
    }

    /// string literal
    pub fn string(s: impl Into<String>) -> Self {
        Self::new(SexpKind::String(s.into()))
    }

    /// symbol
    pub fn symbol(s: impl Into<String>) -> Self {
        Self::new(SexpKind::Symbol(s.into()))
    }

    /// boolean
    pub fn boolean(b: bool) -> Self {
        Self::new(SexpKind::Boolean(b))
    }

    /// character literal
    pub fn char(c: char) -> Self {
        Self::new(SexpKind::Char(c))
    }

    /// proper list
    pub fn list(items: Vec<Sexp>) -> Self {
        if items.is_empty() {
            Self::nil()
        } else {
            Self::new(SexpKind::List(items))
        }
    }

    /// dotted (improper) list
    pub fn dotted_list(items: Vec<Sexp>, tail: Sexp) -> Self {
        Self::new(SexpKind::DottedList(items, Box::new(tail)))
    }

    /// vector
    pub fn vector(items: Vec<Sexp>) -> Self {
        Self::new(SexpKind::Vector(items))
    }

    /// nil / empty list
    pub fn nil() -> Self {
        Self::new(SexpKind::Nil)
    }
}

// --- accessors ---

impl Sexp {
    /// extract as integer, if this is an `Integer`
    pub fn as_integer(&self) -> Option<i64> {
        match &self.kind {
            SexpKind::Integer(n) => Some(*n),
            _ => None,
        }
    }

    /// extract as float, if this is a `Float`
    pub fn as_float(&self) -> Option<f64> {
        match &self.kind {
            SexpKind::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// extract as string slice, if this is a `String`
    pub fn as_string(&self) -> Option<&str> {
        match &self.kind {
            SexpKind::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// extract as symbol name, if this is a `Symbol`
    pub fn as_symbol(&self) -> Option<&str> {
        match &self.kind {
            SexpKind::Symbol(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// extract as boolean, if this is a `Boolean`
    pub fn as_bool(&self) -> Option<bool> {
        match &self.kind {
            SexpKind::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// extract as char, if this is a `Char`
    pub fn as_char(&self) -> Option<char> {
        match &self.kind {
            SexpKind::Char(c) => Some(*c),
            _ => None,
        }
    }

    /// extract as list slice, if this is a `List`
    pub fn as_list(&self) -> Option<&[Sexp]> {
        match &self.kind {
            SexpKind::List(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// extract as dotted list parts, if this is a `DottedList`
    pub fn as_dotted_list(&self) -> Option<(&[Sexp], &Sexp)> {
        match &self.kind {
            SexpKind::DottedList(items, tail) => Some((items.as_slice(), tail.as_ref())),
            _ => None,
        }
    }

    /// extract as vector slice, if this is a `Vector`
    pub fn as_vector(&self) -> Option<&[Sexp]> {
        match &self.kind {
            SexpKind::Vector(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// returns true if this is `Nil`
    pub fn is_nil(&self) -> bool {
        matches!(self.kind, SexpKind::Nil)
    }
}

// --- equality (ignores span and comments) ---

impl PartialEq for Sexp {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Eq for Sexp {}

// --- display (compact scheme-compatible output) ---

impl fmt::Display for Sexp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            SexpKind::Integer(n) => write!(f, "{n}"),
            SexpKind::Float(fl) => format_float(f, *fl),
            SexpKind::String(s) => write_escaped_string(f, s),
            SexpKind::Symbol(s) => write_symbol(f, s),
            SexpKind::Boolean(b) => write!(f, "{}", if *b { "#t" } else { "#f" }),
            SexpKind::Char(c) => write_char_literal(f, *c),
            SexpKind::List(items) => write_list(f, items),
            SexpKind::DottedList(items, tail) => write_dotted_list(f, items, tail),
            SexpKind::Vector(items) => {
                write!(f, "#(")?;
                write_space_separated(f, items)?;
                write!(f, ")")
            }
            SexpKind::Nil => write!(f, "()"),
        }
    }
}

/// format a float, ensuring it always has a decimal point
fn format_float(f: &mut fmt::Formatter<'_>, fl: f64) -> fmt::Result {
    if fl.is_nan() {
        write!(f, "+nan.0")
    } else if fl.is_infinite() {
        if fl.is_sign_positive() {
            write!(f, "+inf.0")
        } else {
            write!(f, "-inf.0")
        }
    } else {
        let s = format!("{fl}");
        write!(f, "{s}")?;
        // ensure there's always a decimal point
        if !s.contains('.') {
            write!(f, ".0")?;
        }
        Ok(())
    }
}

/// write a string with scheme escape sequences
fn write_escaped_string(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    write!(f, "\"")?;
    for ch in s.chars() {
        match ch {
            '"' => write!(f, "\\\"")?,
            '\\' => write!(f, "\\\\")?,
            '\n' => write!(f, "\\n")?,
            '\r' => write!(f, "\\r")?,
            '\t' => write!(f, "\\t")?,
            '\x07' => write!(f, "\\a")?,
            '\x08' => write!(f, "\\b")?,
            '\0' => write!(f, "\\0")?,
            c => write!(f, "{c}")?,
        }
    }
    write!(f, "\"")
}

/// write a symbol, quoting with `|...|` if it contains special characters
fn write_symbol(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    if s.is_empty() || needs_quoting(s) {
        write!(f, "|{s}|")
    } else {
        write!(f, "{s}")
    }
}

/// true if a symbol needs `|...|` quoting
fn needs_quoting(s: &str) -> bool {
    // r7rs peculiar identifiers: +, -, ...
    if matches!(s, "+" | "-" | "...") {
        return false;
    }
    // +/- followed by sign-subsequent → peculiar identifier (e.g. +inf.0)
    if (s.starts_with('+') || s.starts_with('-')) && s.len() > 1 {
        let rest = &s[1..];
        let mut chars = rest.chars();
        match chars.next() {
            Some(c) if is_sign_subsequent(c) => {
                return !chars.all(is_symbol_subsequent);
            }
            Some('.') => {
                // +. prefix: next must be dot-subsequent
                match chars.next() {
                    Some(c) if is_dot_subsequent(c) => {
                        return !chars.all(is_symbol_subsequent);
                    }
                    _ => return true,
                }
            }
            _ => return true,
        }
    }
    let mut chars = s.chars();
    // first char must be a symbol initial
    match chars.next() {
        Some(c) if is_symbol_initial(c) => {}
        _ => return true,
    }
    // rest must be symbol subsequent
    chars.all(is_symbol_subsequent).not()
}

/// r7rs sign-subsequent: a char that can follow +/- in a peculiar identifier
fn is_sign_subsequent(c: char) -> bool {
    is_symbol_initial(c) || matches!(c, '+' | '-' | '@')
}

/// r7rs dot-subsequent: a char that can follow `.` in a peculiar identifier
fn is_dot_subsequent(c: char) -> bool {
    is_sign_subsequent(c) || c == '.'
}

/// helper trait for negation in method chains
trait Not {
    fn not(self) -> bool;
}
impl Not for bool {
    fn not(self) -> bool {
        !self
    }
}

/// r7rs initial character for identifiers
fn is_symbol_initial(c: char) -> bool {
    c.is_ascii_alphabetic() || is_special_initial(c)
}

/// r7rs special initial characters
fn is_special_initial(c: char) -> bool {
    matches!(
        c,
        '!' | '$' | '%' | '&' | '*' | '/' | ':' | '<' | '=' | '>' | '?' | '^' | '_' | '~'
    )
}

/// r7rs subsequent character for identifiers
fn is_symbol_subsequent(c: char) -> bool {
    is_symbol_initial(c) || c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | '@')
}

/// write an r7rs character literal
fn write_char_literal(f: &mut fmt::Formatter<'_>, c: char) -> fmt::Result {
    match c {
        ' ' => write!(f, "#\\space"),
        '\n' => write!(f, "#\\newline"),
        '\t' => write!(f, "#\\tab"),
        '\r' => write!(f, "#\\return"),
        '\0' => write!(f, "#\\null"),
        '\x07' => write!(f, "#\\alarm"),
        '\x08' => write!(f, "#\\backspace"),
        '\x1b' => write!(f, "#\\escape"),
        '\x7f' => write!(f, "#\\delete"),
        c => write!(f, "#\\{c}"),
    }
}

/// write a space-separated list of items (no enclosing parens)
fn write_space_separated(f: &mut fmt::Formatter<'_>, items: &[Sexp]) -> fmt::Result {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            write!(f, " ")?;
        }
        write!(f, "{item}")?;
    }
    Ok(())
}

/// write a proper list
fn write_list(f: &mut fmt::Formatter<'_>, items: &[Sexp]) -> fmt::Result {
    write!(f, "(")?;
    write_space_separated(f, items)?;
    write!(f, ")")
}

/// write a dotted list
fn write_dotted_list(f: &mut fmt::Formatter<'_>, items: &[Sexp], tail: &Sexp) -> fmt::Result {
    write!(f, "(")?;
    write_space_separated(f, items)?;
    write!(f, " . {tail})")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- span tests ---

    #[test]
    fn span_none_has_zero_line() {
        assert!(Span::NONE.is_none());
    }

    #[test]
    fn span_real_is_not_none() {
        let s = Span {
            offset: 0,
            len: 1,
            line: 1,
            column: 1,
        };
        assert!(!s.is_none());
    }

    #[test]
    fn span_merge() {
        let a = Span {
            offset: 0,
            len: 3,
            line: 1,
            column: 1,
        };
        let b = Span {
            offset: 5,
            len: 2,
            line: 1,
            column: 6,
        };
        let merged = a.merge(b);
        assert_eq!(merged.offset, 0);
        assert_eq!(merged.len, 7);
        assert_eq!(merged.line, 1);
        assert_eq!(merged.column, 1);
    }

    #[test]
    fn span_merge_with_none() {
        let real = Span {
            offset: 5,
            len: 3,
            line: 2,
            column: 1,
        };
        assert_eq!(Span::NONE.merge(real), real);
        assert_eq!(real.merge(Span::NONE), real);
    }

    // --- constructor / display tests ---

    #[test]
    fn display_integer() {
        assert_eq!(Sexp::integer(42).to_string(), "42");
        assert_eq!(Sexp::integer(-7).to_string(), "-7");
        assert_eq!(Sexp::integer(0).to_string(), "0");
    }

    #[test]
    fn display_float() {
        assert_eq!(Sexp::float(3.125).to_string(), "3.125");
        assert_eq!(Sexp::float(1.0).to_string(), "1.0");
        assert_eq!(Sexp::float(f64::NAN).to_string(), "+nan.0");
        assert_eq!(Sexp::float(f64::INFINITY).to_string(), "+inf.0");
        assert_eq!(Sexp::float(f64::NEG_INFINITY).to_string(), "-inf.0");
    }

    #[test]
    fn display_string() {
        assert_eq!(Sexp::string("hello").to_string(), "\"hello\"");
        assert_eq!(
            Sexp::string("a\"b\\c\nd").to_string(),
            "\"a\\\"b\\\\c\\nd\""
        );
        assert_eq!(Sexp::string("").to_string(), "\"\"");
    }

    #[test]
    fn display_symbol() {
        assert_eq!(Sexp::symbol("foo").to_string(), "foo");
        assert_eq!(Sexp::symbol("hello-world?").to_string(), "hello-world?");
        // symbols needing quoting
        assert_eq!(Sexp::symbol("").to_string(), "||");
        assert_eq!(Sexp::symbol("has space").to_string(), "|has space|");
    }

    #[test]
    fn display_boolean() {
        assert_eq!(Sexp::boolean(true).to_string(), "#t");
        assert_eq!(Sexp::boolean(false).to_string(), "#f");
    }

    #[test]
    fn display_char() {
        assert_eq!(Sexp::char('a').to_string(), "#\\a");
        assert_eq!(Sexp::char(' ').to_string(), "#\\space");
        assert_eq!(Sexp::char('\n').to_string(), "#\\newline");
        assert_eq!(Sexp::char('\t').to_string(), "#\\tab");
        assert_eq!(Sexp::char('\0').to_string(), "#\\null");
    }

    #[test]
    fn display_list() {
        let l = Sexp::list(vec![Sexp::integer(1), Sexp::integer(2), Sexp::integer(3)]);
        assert_eq!(l.to_string(), "(1 2 3)");
    }

    #[test]
    fn display_nested_list() {
        let inner = Sexp::list(vec![Sexp::symbol("b"), Sexp::integer(2)]);
        let outer = Sexp::list(vec![Sexp::symbol("a"), inner]);
        assert_eq!(outer.to_string(), "(a (b 2))");
    }

    #[test]
    fn display_dotted_list() {
        let d = Sexp::dotted_list(vec![Sexp::integer(1), Sexp::integer(2)], Sexp::integer(3));
        assert_eq!(d.to_string(), "(1 2 . 3)");
    }

    #[test]
    fn display_vector() {
        let v = Sexp::vector(vec![Sexp::integer(1), Sexp::integer(2)]);
        assert_eq!(v.to_string(), "#(1 2)");
    }

    #[test]
    fn display_nil() {
        assert_eq!(Sexp::nil().to_string(), "()");
    }

    #[test]
    fn empty_list_is_nil() {
        assert_eq!(Sexp::list(vec![]), Sexp::nil());
    }

    // --- equality tests ---

    #[test]
    fn equality_ignores_span() {
        let a = Sexp {
            kind: SexpKind::Integer(42),
            span: Span {
                offset: 0,
                len: 2,
                line: 1,
                column: 1,
            },
            comments: vec![],
        };
        let b = Sexp {
            kind: SexpKind::Integer(42),
            span: Span {
                offset: 100,
                len: 2,
                line: 5,
                column: 10,
            },
            comments: vec![],
        };
        assert_eq!(a, b);
    }

    #[test]
    fn equality_ignores_comments() {
        let a = Sexp::integer(42);
        let mut b = Sexp::integer(42);
        b.comments.push(Comment {
            text: "hello".to_string(),
            span: Span::NONE,
            kind: CommentKind::Line,
        });
        assert_eq!(a, b);
    }

    #[test]
    fn inequality_different_kinds() {
        assert_ne!(Sexp::integer(42), Sexp::float(42.0));
        assert_ne!(Sexp::string("foo"), Sexp::symbol("foo"));
    }

    // --- accessor tests ---

    #[test]
    fn accessors() {
        assert_eq!(Sexp::integer(7).as_integer(), Some(7));
        assert_eq!(Sexp::float(2.5).as_float(), Some(2.5));
        assert_eq!(Sexp::string("hi").as_string(), Some("hi"));
        assert_eq!(Sexp::symbol("x").as_symbol(), Some("x"));
        assert_eq!(Sexp::boolean(true).as_bool(), Some(true));
        assert_eq!(Sexp::char('z').as_char(), Some('z'));
        assert!(Sexp::nil().is_nil());
    }

    #[test]
    fn accessors_wrong_type_return_none() {
        assert_eq!(Sexp::integer(7).as_string(), None);
        assert_eq!(Sexp::string("hi").as_integer(), None);
        assert!(!Sexp::integer(1).is_nil());
    }

    #[test]
    fn accessor_list() {
        let l = Sexp::list(vec![Sexp::integer(1), Sexp::integer(2)]);
        let items = l.as_list().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].as_integer(), Some(1));
    }

    #[test]
    fn accessor_dotted_list() {
        let d = Sexp::dotted_list(vec![Sexp::integer(1)], Sexp::integer(2));
        let (items, tail) = d.as_dotted_list().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(tail.as_integer(), Some(2));
    }

    #[test]
    fn accessor_vector() {
        let v = Sexp::vector(vec![Sexp::symbol("a")]);
        let items = v.as_vector().unwrap();
        assert_eq!(items[0].as_symbol(), Some("a"));
    }

    // --- symbol quoting edge cases ---

    #[test]
    fn symbol_starting_with_digit_is_quoted() {
        assert_eq!(Sexp::symbol("1foo").to_string(), "|1foo|");
    }

    #[test]
    fn symbol_with_special_initials() {
        // these are valid unquoted
        for s in [
            "!", "$", "%", "&", "*", "/", ":", "<", "=", ">", "?", "^", "_", "~",
        ] {
            assert_eq!(Sexp::symbol(s).to_string(), s, "failed for {s}");
        }
    }

    #[test]
    fn symbol_plus_minus_are_peculiar() {
        // + and - alone are r7rs peculiar identifiers, no quoting needed
        assert_eq!(Sexp::symbol("+").to_string(), "+");
        assert_eq!(Sexp::symbol("-").to_string(), "-");
        assert_eq!(Sexp::symbol("...").to_string(), "...");
    }
}
