//! recursive descent s-expression parser
//!
//! parses s-expression source text into a [`Sexp`] AST. supports comment
//! preservation for round-tripping config files without losing comments.

use crate::ast::{Comment, CommentKind, Sexp, SexpKind, Span};
use crate::error::{ParseError, Result};
use crate::lexer::{Lexer, Token, TokenKind};

/// parse a single s-expression from input
///
/// returns the first complete s-expression. trailing input after the
/// expression is ignored (use [`parse_all`] to parse everything).
///
/// ```
/// use tein_sexp::parser;
///
/// let sexp = parser::parse("(+ 1 2)").unwrap();
/// assert_eq!(sexp.to_string(), "(+ 1 2)");
/// ```
pub fn parse(input: &str) -> Result<Sexp> {
    let mut parser = Parser::new(input, false);
    parser.parse_expr()
}

/// parse all s-expressions from input
///
/// ```
/// use tein_sexp::parser;
///
/// let sexps = parser::parse_all("1 2 3").unwrap();
/// assert_eq!(sexps.len(), 3);
/// ```
pub fn parse_all(input: &str) -> Result<Vec<Sexp>> {
    let mut parser = Parser::new(input, false);
    parser.parse_all_exprs()
}

/// parse a single s-expression, preserving comments
///
/// comments are attached to the nearest following s-expression node.
pub fn parse_preserving(input: &str) -> Result<Sexp> {
    let mut parser = Parser::new(input, true);
    parser.parse_expr()
}

/// parse all s-expressions, preserving comments
pub fn parse_all_preserving(input: &str) -> Result<Vec<Sexp>> {
    let mut parser = Parser::new(input, true);
    parser.parse_all_exprs()
}

/// recursive descent parser for s-expressions
struct Parser<'a> {
    lexer: Lexer<'a>,
    preserve_comments: bool,
    /// comments collected before the next expression
    pending_comments: Vec<Comment>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, preserve_comments: bool) -> Self {
        Self {
            lexer: Lexer::new(input),
            preserve_comments,
            pending_comments: Vec::new(),
        }
    }

    /// parse all expressions until EOF
    fn parse_all_exprs(&mut self) -> Result<Vec<Sexp>> {
        let mut exprs = Vec::new();
        loop {
            self.skip_or_collect_comments()?;
            if self.at_eof()? {
                break;
            }
            exprs.push(self.parse_expr()?);
        }
        Ok(exprs)
    }

    /// parse a single expression, consuming leading comments
    fn parse_expr(&mut self) -> Result<Sexp> {
        self.skip_or_collect_comments()?;

        let tok = self.lexer.next_token()?;
        let mut sexp = match tok.kind {
            TokenKind::Integer(n) => Sexp {
                kind: SexpKind::Integer(n),
                span: tok.span,
                comments: Vec::new(),
            },
            TokenKind::Float(f) => Sexp {
                kind: SexpKind::Float(f),
                span: tok.span,
                comments: Vec::new(),
            },
            TokenKind::String(s) => Sexp {
                kind: SexpKind::String(s),
                span: tok.span,
                comments: Vec::new(),
            },
            TokenKind::Symbol(s) => Sexp {
                kind: SexpKind::Symbol(s),
                span: tok.span,
                comments: Vec::new(),
            },
            TokenKind::Boolean(b) => Sexp {
                kind: SexpKind::Boolean(b),
                span: tok.span,
                comments: Vec::new(),
            },
            TokenKind::Char(c) => Sexp {
                kind: SexpKind::Char(c),
                span: tok.span,
                comments: Vec::new(),
            },
            TokenKind::LeftParen | TokenKind::LeftBracket => {
                self.parse_list_or_dotted(tok.span, &tok.kind)?
            }
            TokenKind::HashParen => self.parse_vector(tok.span)?,
            TokenKind::HashU8Paren => self.parse_bytevector(tok.span)?,
            TokenKind::Bignum(s) => Sexp {
                kind: SexpKind::Bignum(s),
                span: tok.span,
                comments: Vec::new(),
            },
            TokenKind::Rational(n, d) => {
                let num = parse_number_string(&n);
                let den = parse_number_string(&d);
                Sexp {
                    kind: SexpKind::Rational(Box::new(num), Box::new(den)),
                    span: tok.span,
                    comments: Vec::new(),
                }
            }
            TokenKind::Quote => self.parse_sugar("quote", tok.span)?,
            TokenKind::Quasiquote => self.parse_sugar("quasiquote", tok.span)?,
            TokenKind::Unquote => self.parse_sugar("unquote", tok.span)?,
            TokenKind::UnquoteSplicing => self.parse_sugar("unquote-splicing", tok.span)?,
            TokenKind::DatumComment => {
                // read and discard the next datum
                let datum = self.parse_expr()?;
                if self.preserve_comments {
                    self.pending_comments.push(Comment {
                        text: datum.to_string(),
                        span: tok.span.merge(datum.span),
                        kind: CommentKind::Datum,
                    });
                }
                // recurse to get the actual expression
                return self.parse_expr();
            }
            TokenKind::RightParen | TokenKind::RightBracket => {
                return Err(ParseError::new("unexpected closing delimiter", tok.span));
            }
            TokenKind::Dot => {
                return Err(ParseError::new("unexpected '.'", tok.span));
            }
            TokenKind::Eof => {
                return Err(ParseError::new("unexpected end of input", tok.span));
            }
            // comments should have been consumed already
            TokenKind::LineComment(_) | TokenKind::BlockComment(_) => {
                unreachable!("comments should be consumed before parse_expr")
            }
        };

        // attach any pending comments to this node
        if !self.pending_comments.is_empty() {
            sexp.comments = std::mem::take(&mut self.pending_comments);
        }

        Ok(sexp)
    }

    /// parse a list or dotted list after consuming `(`
    fn parse_list_or_dotted(&mut self, open_span: Span, open_kind: &TokenKind) -> Result<Sexp> {
        let close = match open_kind {
            TokenKind::LeftBracket => TokenKind::RightBracket,
            _ => TokenKind::RightParen,
        };

        self.skip_or_collect_comments()?;

        // empty list?
        if self.peek_is(&close)? {
            let close_tok = self.lexer.next_token()?;
            return Ok(Sexp {
                kind: SexpKind::Nil,
                span: open_span.merge(close_tok.span),
                comments: Vec::new(),
            });
        }

        let mut items = Vec::new();

        loop {
            self.skip_or_collect_comments()?;

            if self.peek_is(&close)? {
                let close_tok = self.lexer.next_token()?;
                return Ok(Sexp {
                    kind: SexpKind::List(items),
                    span: open_span.merge(close_tok.span),
                    comments: Vec::new(),
                });
            }

            if self.peek_is(&TokenKind::Dot)? {
                // dotted pair: (a b . c)
                let dot_tok = self.lexer.next_token()?;
                if items.is_empty() {
                    return Err(ParseError::new(
                        "unexpected '.' at start of list",
                        dot_tok.span,
                    ));
                }
                let tail = self.parse_expr()?;
                self.skip_or_collect_comments()?;
                let close_tok = self.expect_token(&close)?;
                return Ok(Sexp {
                    kind: SexpKind::DottedList(items, Box::new(tail)),
                    span: open_span.merge(close_tok.span),
                    comments: Vec::new(),
                });
            }

            if self.at_eof()? {
                return Err(ParseError::new("unterminated list", open_span));
            }

            items.push(self.parse_expr()?);
        }
    }

    /// parse a vector after consuming `#(`
    fn parse_vector(&mut self, open_span: Span) -> Result<Sexp> {
        let mut items = Vec::new();

        loop {
            self.skip_or_collect_comments()?;

            if self.peek_is(&TokenKind::RightParen)? {
                let close_tok = self.lexer.next_token()?;
                return Ok(Sexp {
                    kind: SexpKind::Vector(items),
                    span: open_span.merge(close_tok.span),
                    comments: Vec::new(),
                });
            }

            if self.at_eof()? {
                return Err(ParseError::new("unterminated vector", open_span));
            }

            items.push(self.parse_expr()?);
        }
    }

    /// parse a bytevector after consuming `#u8(`
    fn parse_bytevector(&mut self, open_span: Span) -> Result<Sexp> {
        let mut bytes = Vec::new();

        loop {
            self.skip_or_collect_comments()?;

            if self.peek_is(&TokenKind::RightParen)? {
                let close_tok = self.lexer.next_token()?;
                return Ok(Sexp {
                    kind: SexpKind::Bytevector(bytes),
                    span: open_span.merge(close_tok.span),
                    comments: Vec::new(),
                });
            }

            if self.at_eof()? {
                return Err(ParseError::new("unterminated bytevector", open_span));
            }

            let elem_tok = self.lexer.next_token()?;
            match elem_tok.kind {
                TokenKind::Integer(n) if (0..=255).contains(&n) => {
                    bytes.push(n as u8);
                }
                TokenKind::Integer(n) => {
                    return Err(ParseError::new(
                        format!("bytevector element out of range: {n}"),
                        elem_tok.span,
                    ));
                }
                _ => {
                    return Err(ParseError::new(
                        "expected integer in bytevector",
                        elem_tok.span,
                    ));
                }
            }
        }
    }

    /// parse quote sugar: `'x` → `(quote x)`, etc.
    fn parse_sugar(&mut self, name: &str, prefix_span: Span) -> Result<Sexp> {
        let inner = self.parse_expr()?;
        let span = prefix_span.merge(inner.span);
        Ok(Sexp {
            kind: SexpKind::List(vec![
                Sexp {
                    kind: SexpKind::Symbol(name.to_string()),
                    span: prefix_span,
                    comments: Vec::new(),
                },
                inner,
            ]),
            span,
            comments: Vec::new(),
        })
    }

    /// skip or collect comment tokens depending on mode
    fn skip_or_collect_comments(&mut self) -> Result<()> {
        loop {
            let tok = self.lexer.peek_token()?;
            match &tok.kind {
                TokenKind::LineComment(_) | TokenKind::BlockComment(_) => {
                    let tok = self.lexer.next_token()?;
                    if self.preserve_comments {
                        let (text, kind) = match tok.kind {
                            TokenKind::LineComment(t) => (t, CommentKind::Line),
                            TokenKind::BlockComment(t) => (t, CommentKind::Block),
                            _ => unreachable!(),
                        };
                        self.pending_comments.push(Comment {
                            text,
                            span: tok.span,
                            kind,
                        });
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    /// check if the next token matches the expected kind
    fn peek_is(&mut self, expected: &TokenKind) -> Result<bool> {
        let tok = self.lexer.peek_token()?;
        Ok(std::mem::discriminant(&tok.kind) == std::mem::discriminant(expected))
    }

    /// check if we're at end of input
    fn at_eof(&mut self) -> Result<bool> {
        self.peek_is(&TokenKind::Eof)
    }

    /// consume a token, returning an error if it doesn't match
    fn expect_token(&mut self, expected: &TokenKind) -> Result<Token> {
        let tok = self.lexer.next_token()?;
        if std::mem::discriminant(&tok.kind) == std::mem::discriminant(expected) {
            Ok(tok)
        } else {
            Err(ParseError::new(
                format!("expected {expected:?}, got {:?}", tok.kind),
                tok.span,
            ))
        }
    }
}

/// parse a numeric string (from a rational component) as Integer or Bignum Sexp
fn parse_number_string(s: &str) -> Sexp {
    match s.parse::<i64>() {
        Ok(n) => Sexp::integer(n),
        Err(_) => Sexp::bignum(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- atom parsing ---

    #[test]
    fn parse_integer() {
        let s = parse("42").unwrap();
        assert_eq!(s, Sexp::integer(42));
    }

    #[test]
    fn parse_negative_integer() {
        let s = parse("-7").unwrap();
        assert_eq!(s, Sexp::integer(-7));
    }

    #[test]
    fn parse_float() {
        let s = parse("3.125").unwrap();
        assert_eq!(s, Sexp::float(3.125));
    }

    #[test]
    fn parse_string() {
        let s = parse(r#""hello""#).unwrap();
        assert_eq!(s, Sexp::string("hello"));
    }

    #[test]
    fn parse_symbol() {
        let s = parse("foo").unwrap();
        assert_eq!(s, Sexp::symbol("foo"));
    }

    #[test]
    fn parse_boolean() {
        assert_eq!(parse("#t").unwrap(), Sexp::boolean(true));
        assert_eq!(parse("#f").unwrap(), Sexp::boolean(false));
    }

    #[test]
    fn parse_char() {
        let s = parse("#\\a").unwrap();
        assert_eq!(s, Sexp::char('a'));
    }

    // --- lists ---

    #[test]
    fn parse_empty_list() {
        let s = parse("()").unwrap();
        assert!(s.is_nil());
    }

    #[test]
    fn parse_simple_list() {
        let s = parse("(1 2 3)").unwrap();
        assert_eq!(
            s,
            Sexp::list(vec![Sexp::integer(1), Sexp::integer(2), Sexp::integer(3)])
        );
    }

    #[test]
    fn parse_nested_list() {
        let s = parse("(a (b c))").unwrap();
        let expected = Sexp::list(vec![
            Sexp::symbol("a"),
            Sexp::list(vec![Sexp::symbol("b"), Sexp::symbol("c")]),
        ]);
        assert_eq!(s, expected);
    }

    #[test]
    fn parse_dotted_pair() {
        let s = parse("(a . b)").unwrap();
        assert_eq!(
            s,
            Sexp::dotted_list(vec![Sexp::symbol("a")], Sexp::symbol("b"))
        );
    }

    #[test]
    fn parse_dotted_list_multiple() {
        let s = parse("(a b . c)").unwrap();
        assert_eq!(
            s,
            Sexp::dotted_list(
                vec![Sexp::symbol("a"), Sexp::symbol("b")],
                Sexp::symbol("c")
            )
        );
    }

    #[test]
    fn parse_brackets_as_lists() {
        let s = parse("[1 2 3]").unwrap();
        assert_eq!(
            s,
            Sexp::list(vec![Sexp::integer(1), Sexp::integer(2), Sexp::integer(3)])
        );
    }

    // --- vectors ---

    #[test]
    fn parse_vector() {
        let s = parse("#(1 2 3)").unwrap();
        assert_eq!(
            s,
            Sexp::vector(vec![Sexp::integer(1), Sexp::integer(2), Sexp::integer(3)])
        );
    }

    #[test]
    fn parse_empty_vector() {
        let s = parse("#()").unwrap();
        assert_eq!(s, Sexp::vector(vec![]));
    }

    // --- quote sugar ---

    #[test]
    fn parse_quote() {
        let s = parse("'x").unwrap();
        assert_eq!(
            s,
            Sexp::list(vec![Sexp::symbol("quote"), Sexp::symbol("x")])
        );
    }

    #[test]
    fn parse_quasiquote() {
        let s = parse("`x").unwrap();
        assert_eq!(
            s,
            Sexp::list(vec![Sexp::symbol("quasiquote"), Sexp::symbol("x")])
        );
    }

    #[test]
    fn parse_unquote() {
        let s = parse(",x").unwrap();
        assert_eq!(
            s,
            Sexp::list(vec![Sexp::symbol("unquote"), Sexp::symbol("x")])
        );
    }

    #[test]
    fn parse_unquote_splicing() {
        let s = parse(",@x").unwrap();
        assert_eq!(
            s,
            Sexp::list(vec![Sexp::symbol("unquote-splicing"), Sexp::symbol("x")])
        );
    }

    // --- parse_all ---

    #[test]
    fn parse_all_multiple() {
        let sexps = parse_all("1 2 3").unwrap();
        assert_eq!(sexps.len(), 3);
        assert_eq!(sexps[0], Sexp::integer(1));
        assert_eq!(sexps[1], Sexp::integer(2));
        assert_eq!(sexps[2], Sexp::integer(3));
    }

    #[test]
    fn parse_all_empty() {
        let sexps = parse_all("").unwrap();
        assert!(sexps.is_empty());
    }

    #[test]
    fn parse_all_with_comments() {
        let sexps = parse_all("; comment\n42").unwrap();
        assert_eq!(sexps.len(), 1);
        assert_eq!(sexps[0], Sexp::integer(42));
    }

    // --- datum comments ---

    #[test]
    fn parse_datum_comment() {
        let s = parse("#; skip-me 42").unwrap();
        assert_eq!(s, Sexp::integer(42));
    }

    #[test]
    fn parse_datum_comment_list() {
        let s = parse("#;(skip this) (keep this)").unwrap();
        assert_eq!(
            s,
            Sexp::list(vec![Sexp::symbol("keep"), Sexp::symbol("this")])
        );
    }

    // --- comment preservation ---

    #[test]
    fn preserve_line_comment() {
        let s = parse_preserving("; hello\n42").unwrap();
        assert_eq!(s, Sexp::integer(42));
        assert_eq!(s.comments.len(), 1);
        assert_eq!(s.comments[0].text, " hello");
        assert_eq!(s.comments[0].kind, CommentKind::Line);
    }

    #[test]
    fn preserve_block_comment() {
        let s = parse_preserving("#| world |# 42").unwrap();
        assert_eq!(s, Sexp::integer(42));
        assert_eq!(s.comments.len(), 1);
        assert_eq!(s.comments[0].text, " world ");
        assert_eq!(s.comments[0].kind, CommentKind::Block);
    }

    #[test]
    fn preserve_datum_comment() {
        let s = parse_preserving("#; skipped 42").unwrap();
        assert_eq!(s, Sexp::integer(42));
        assert_eq!(s.comments.len(), 1);
        assert_eq!(s.comments[0].text, "skipped");
        assert_eq!(s.comments[0].kind, CommentKind::Datum);
    }

    // --- spans ---

    #[test]
    fn parsed_atoms_have_spans() {
        let s = parse("  42").unwrap();
        assert_eq!(s.span.offset, 2);
        assert_eq!(s.span.len, 2);
        assert_eq!(s.span.line, 1);
        assert_eq!(s.span.column, 3);
    }

    #[test]
    fn parsed_list_span_covers_parens() {
        let s = parse("(a b)").unwrap();
        assert_eq!(s.span.offset, 0);
        assert_eq!(s.span.len, 5);
    }

    // --- error cases ---

    #[test]
    fn error_unmatched_close_paren() {
        assert!(parse(")").is_err());
    }

    #[test]
    fn error_unterminated_list() {
        assert!(parse("(1 2").is_err());
    }

    #[test]
    fn error_unexpected_dot() {
        assert!(parse(".").is_err());
    }

    #[test]
    fn error_dot_at_start_of_list() {
        assert!(parse("(. a)").is_err());
    }

    #[test]
    fn error_empty_input() {
        assert!(parse("").is_err());
    }

    // --- round-trip ---

    #[test]
    fn round_trip_atoms() {
        for input in [
            "42",
            "-7",
            "3.125",
            "\"hello\"",
            "foo",
            "#t",
            "#f",
            "#\\a",
            "()",
        ] {
            let s = parse(input).unwrap();
            assert_eq!(s.to_string(), input, "round-trip failed for: {input}");
        }
    }

    #[test]
    fn round_trip_lists() {
        for input in ["(1 2 3)", "(a (b c))", "(a . b)", "(a b . c)", "#(1 2 3)"] {
            let s = parse(input).unwrap();
            assert_eq!(s.to_string(), input, "round-trip failed for: {input}");
        }
    }

    #[test]
    fn round_trip_quote_sugar() {
        // quote sugar expands, so round-trip is the expanded form
        let s = parse("'x").unwrap();
        assert_eq!(s.to_string(), "(quote x)");
    }

    // --- numeric tower ---

    #[test]
    fn parse_bignum() {
        let sexp = parse("99999999999999999999999999").unwrap();
        assert_eq!(sexp.as_bignum(), Some("99999999999999999999999999"));
    }

    #[test]
    fn parse_rational() {
        let sexp = parse("3/4").unwrap();
        let (n, d) = sexp.as_rational().unwrap();
        assert_eq!(n.as_integer(), Some(3));
        assert_eq!(d.as_integer(), Some(4));
    }

    #[test]
    fn parse_rational_negative_numerator() {
        let sexp = parse("-1/2").unwrap();
        let (n, d) = sexp.as_rational().unwrap();
        assert_eq!(n.as_integer(), Some(-1));
        assert_eq!(d.as_integer(), Some(2));
    }

    #[test]
    fn parse_bytevector() {
        let sexp = parse("#u8(1 2 3)").unwrap();
        assert_eq!(sexp.as_bytevector(), Some([1u8, 2, 3].as_slice()));
    }

    #[test]
    fn parse_bytevector_empty() {
        let sexp = parse("#u8()").unwrap();
        assert_eq!(sexp.as_bytevector(), Some([].as_slice()));
    }

    #[test]
    fn roundtrip_bignum() {
        assert_eq!(
            parse("99999999999999999999999999").unwrap().to_string(),
            "99999999999999999999999999"
        );
    }

    #[test]
    fn roundtrip_rational() {
        assert_eq!(parse("3/4").unwrap().to_string(), "3/4");
    }

    #[test]
    fn roundtrip_bytevector() {
        assert_eq!(parse("#u8(1 2 3)").unwrap().to_string(), "#u8(1 2 3)");
    }

    // --- complex expressions ---

    #[test]
    fn parse_define() {
        let s = parse("(define (square x) (* x x))").unwrap();
        assert_eq!(s.to_string(), "(define (square x) (* x x))");
    }

    #[test]
    fn parse_alist() {
        let s = parse("((name . \"alice\") (age . 30))").unwrap();
        let expected = Sexp::list(vec![
            Sexp::dotted_list(vec![Sexp::symbol("name")], Sexp::string("alice")),
            Sexp::dotted_list(vec![Sexp::symbol("age")], Sexp::integer(30)),
        ]);
        assert_eq!(s, expected);
    }
}
