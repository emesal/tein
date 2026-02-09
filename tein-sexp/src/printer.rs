//! s-expression pretty printer
//!
//! provides compact and indented output. the compact form is what
//! `Display for Sexp` uses. the pretty printer breaks long lists
//! across lines with configurable indentation.

use crate::ast::{CommentKind, Sexp, SexpKind};

/// configuration for pretty printing
pub struct PrintConfig {
    /// spaces per indent level (default 2)
    pub indent: usize,
    /// line width before breaking lists (default 80)
    pub max_width: usize,
    /// include preserved comments in output (default true)
    pub emit_comments: bool,
}

impl Default for PrintConfig {
    fn default() -> Self {
        Self {
            indent: 2,
            max_width: 80,
            emit_comments: true,
        }
    }
}

/// compact single-line output (same as `Sexp::to_string()`)
pub fn to_string(sexp: &Sexp) -> String {
    sexp.to_string()
}

/// pretty-print with default settings
pub fn to_string_pretty(sexp: &Sexp) -> String {
    to_string_with(sexp, &PrintConfig::default())
}

/// pretty-print with custom configuration
pub fn to_string_with(sexp: &Sexp, config: &PrintConfig) -> String {
    let mut buf = String::new();
    print_sexp(&mut buf, sexp, 0, config);
    buf
}

/// print an s-expression, choosing compact or broken format based on width
fn print_sexp(buf: &mut String, sexp: &Sexp, indent_level: usize, config: &PrintConfig) {
    // emit leading comments
    if config.emit_comments {
        for comment in &sexp.comments {
            emit_indent(buf, indent_level, config);
            match comment.kind {
                CommentKind::Line => {
                    buf.push(';');
                    buf.push_str(&comment.text);
                    buf.push('\n');
                }
                CommentKind::Block => {
                    buf.push_str("#|");
                    buf.push_str(&comment.text);
                    buf.push_str("|#");
                    buf.push('\n');
                }
                CommentKind::Datum => {
                    buf.push_str("#; ");
                    buf.push_str(&comment.text);
                    buf.push('\n');
                }
            }
        }
    }

    match &sexp.kind {
        SexpKind::List(items) => {
            print_compound(buf, "(", items, None, indent_level, config);
        }
        SexpKind::DottedList(items, tail) => {
            print_compound(buf, "(", items, Some(tail), indent_level, config);
        }
        SexpKind::Vector(items) => {
            print_compound(buf, "#(", items, None, indent_level, config);
        }
        // atoms just use Display
        _ => buf.push_str(&sexp.to_string()),
    }
}

/// print a compound form (list, dotted list, or vector)
///
/// tries compact first; if it exceeds max_width, breaks across lines.
fn print_compound(
    buf: &mut String,
    open: &str,
    items: &[Sexp],
    tail: Option<&Sexp>,
    indent_level: usize,
    config: &PrintConfig,
) {
    // try compact first
    let compact = format_compact(open, items, tail);
    let current_indent = indent_level * config.indent;

    if current_indent + compact.len() <= config.max_width {
        buf.push_str(&compact);
        return;
    }

    // break across lines
    buf.push_str(open);
    let child_indent = indent_level + 1;

    for (i, item) in items.iter().enumerate() {
        if i == 0 {
            // first item on same line as opening paren
            print_sexp(buf, item, child_indent, config);
        } else {
            buf.push('\n');
            emit_indent(buf, child_indent, config);
            print_sexp(buf, item, child_indent, config);
        }
    }

    if let Some(t) = tail {
        buf.push('\n');
        emit_indent(buf, child_indent, config);
        buf.push_str(". ");
        print_sexp(buf, t, child_indent, config);
    }

    buf.push(')');
}

/// format a compound form compactly (single line)
fn format_compact(open: &str, items: &[Sexp], tail: Option<&Sexp>) -> String {
    let mut s = String::from(open);
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&item.to_string());
    }
    if let Some(t) = tail {
        s.push_str(" . ");
        s.push_str(&t.to_string());
    }
    s.push(')');
    s
}

/// emit indentation spaces
fn emit_indent(buf: &mut String, indent_level: usize, config: &PrintConfig) {
    for _ in 0..(indent_level * config.indent) {
        buf.push(' ');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn compact_output() {
        let sexp = parser::parse("(define x 42)").unwrap();
        assert_eq!(to_string(&sexp), "(define x 42)");
    }

    #[test]
    fn pretty_short_list_stays_compact() {
        let sexp = parser::parse("(+ 1 2)").unwrap();
        let pretty = to_string_pretty(&sexp);
        assert_eq!(pretty, "(+ 1 2)");
    }

    #[test]
    fn pretty_long_list_breaks() {
        // build a list that exceeds 80 chars
        let items: Vec<Sexp> = (0..20)
            .map(|i| Sexp::symbol(format!("very-long-name-{i}")))
            .collect();
        let sexp = Sexp::list(items);
        let pretty = to_string_pretty(&sexp);
        assert!(pretty.contains('\n'), "expected line breaks in: {pretty}");
        // first item should be on the same line as the paren
        assert!(pretty.starts_with("(very-long-name-0"));
    }

    #[test]
    fn pretty_nested_indentation() {
        let config = PrintConfig {
            indent: 2,
            max_width: 20,
            ..Default::default()
        };
        let sexp = parser::parse("(define (f x) (+ x 1))").unwrap();
        let pretty = to_string_with(&sexp, &config);
        assert!(pretty.contains('\n'));
        // each indented line should start with spaces
        for line in pretty.lines().skip(1) {
            assert!(line.starts_with("  "), "expected indent: {line:?}");
        }
    }

    #[test]
    fn pretty_dotted_list() {
        let config = PrintConfig {
            max_width: 10,
            ..Default::default()
        };
        let sexp = parser::parse("(long-name . value)").unwrap();
        let pretty = to_string_with(&sexp, &config);
        assert!(pretty.contains(". value"), "should have dot tail: {pretty}");
    }

    #[test]
    fn pretty_vector() {
        let config = PrintConfig {
            max_width: 10,
            ..Default::default()
        };
        let sexp = parser::parse("#(alpha beta gamma)").unwrap();
        let pretty = to_string_with(&sexp, &config);
        assert!(pretty.starts_with("#("));
        assert!(pretty.contains('\n'));
    }

    #[test]
    fn comment_round_trip() {
        let sexp = parser::parse_preserving("; a comment\n42").unwrap();
        let config = PrintConfig {
            emit_comments: true,
            ..Default::default()
        };
        let output = to_string_with(&sexp, &config);
        assert!(
            output.contains("; a comment"),
            "should contain comment: {output}"
        );
        assert!(output.contains("42"));
    }

    #[test]
    fn block_comment_round_trip() {
        let sexp = parser::parse_preserving("#| block |# 42").unwrap();
        let output = to_string_with(&sexp, &PrintConfig::default());
        assert!(output.contains("#| block |#"));
    }

    #[test]
    fn datum_comment_round_trip() {
        let sexp = parser::parse_preserving("#; skipped 42").unwrap();
        let output = to_string_with(&sexp, &PrintConfig::default());
        assert!(output.contains("#; skipped"));
    }

    #[test]
    fn comments_suppressed_when_disabled() {
        let sexp = parser::parse_preserving("; comment\n42").unwrap();
        let config = PrintConfig {
            emit_comments: false,
            ..Default::default()
        };
        let output = to_string_with(&sexp, &config);
        assert!(!output.contains(';'));
        assert_eq!(output, "42");
    }

    #[test]
    fn custom_indent() {
        let config = PrintConfig {
            indent: 4,
            max_width: 10,
            ..Default::default()
        };
        let sexp = parser::parse("(a b c)").unwrap();
        let pretty = to_string_with(&sexp, &config);
        if pretty.contains('\n') {
            let second_line = pretty.lines().nth(1).unwrap();
            assert!(
                second_line.starts_with("    "),
                "expected 4-space indent: {second_line:?}"
            );
        }
    }
}
