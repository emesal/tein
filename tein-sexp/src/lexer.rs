//! character-level tokenizer with span tracking
//!
//! the lexer produces a stream of [`Token`]s from s-expression source text.
//! each token carries a [`Span`] for error reporting and comment preservation.
//! whitespace is consumed between tokens. comments are emitted as tokens so
//! the parser can decide whether to collect or discard them.

use crate::ast::Span;
use crate::error::{ParseError, Result};

/// a token with its source span
#[derive(Debug, Clone)]
pub struct Token {
    /// what kind of token this is
    pub kind: TokenKind,
    /// source location
    pub span: Span,
}

/// token variants
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// `(`
    LeftParen,
    /// `)`
    RightParen,
    /// `[`
    LeftBracket,
    /// `]`
    RightBracket,
    /// `#(`
    HashParen,
    /// `.` (dotted pair separator)
    Dot,
    /// `'`
    Quote,
    /// `` ` ``
    Quasiquote,
    /// `,`
    Unquote,
    /// `,@`
    UnquoteSplicing,
    /// `#;`
    DatumComment,
    /// integer literal
    Integer(i64),
    /// bignum literal (integer that overflows i64)
    Bignum(String),
    /// rational literal (`numerator`, `denominator` as decimal strings)
    Rational(String, String),
    /// float literal
    Float(f64),
    /// `#u8(`
    HashU8Paren,
    /// string literal (content, escapes resolved)
    String(String),
    /// symbol / identifier
    Symbol(String),
    /// `#t` or `#f`
    Boolean(bool),
    /// `#\x` character literal
    Char(char),
    /// `;` line comment (text without the `;` or newline)
    LineComment(String),
    /// `#| ... |#` block comment (text without delimiters)
    BlockComment(String),
    /// end of input
    Eof,
}

/// character-level lexer for s-expressions
///
/// tracks position (byte offset, line, column) through the input.
/// supports one-token lookahead via [`peek_token`](Lexer::peek_token).
pub struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: u32,
    column: u32,
    /// buffered lookahead token
    peeked: Option<Token>,
}

impl<'a> Lexer<'a> {
    /// create a new lexer over the given input
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
            line: 1,
            column: 1,
            peeked: None,
        }
    }

    /// advance past the peeked token and return it, or lex the next token
    pub fn next_token(&mut self) -> Result<Token> {
        if let Some(tok) = self.peeked.take() {
            return Ok(tok);
        }
        self.lex_token()
    }

    /// peek at the next token without consuming it
    pub fn peek_token(&mut self) -> Result<&Token> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lex_token()?);
        }
        Ok(self.peeked.as_ref().unwrap())
    }

    /// the main lexing loop: skip whitespace, then dispatch on the next character
    fn lex_token(&mut self) -> Result<Token> {
        self.skip_whitespace();

        if self.at_end() {
            return Ok(self.make_token(TokenKind::Eof, 0));
        }

        let start_pos = self.pos;
        let start_line = self.line;
        let start_col = self.column;
        let ch = self.peek_char().unwrap();

        let kind = match ch {
            '(' => {
                self.advance();
                TokenKind::LeftParen
            }
            ')' => {
                self.advance();
                TokenKind::RightParen
            }
            '[' => {
                self.advance();
                TokenKind::LeftBracket
            }
            ']' => {
                self.advance();
                TokenKind::RightBracket
            }
            '\'' => {
                self.advance();
                TokenKind::Quote
            }
            '`' => {
                self.advance();
                TokenKind::Quasiquote
            }
            ',' => {
                self.advance();
                if self.peek_char() == Some('@') {
                    self.advance();
                    TokenKind::UnquoteSplicing
                } else {
                    TokenKind::Unquote
                }
            }
            '"' => self.lex_string()?,
            ';' => self.lex_line_comment(),
            '#' => self.lex_hash()?,
            '.' => self.lex_dot_or_number()?,
            '+' | '-' => self.lex_sign_or_number()?,
            c if c.is_ascii_digit() => self.lex_number()?,
            c if is_symbol_initial(c) => self.lex_symbol(),
            '|' => self.lex_quoted_symbol()?,
            c => {
                self.advance();
                return Err(ParseError::new(
                    format!("unexpected character: {c:?}"),
                    self.span_from(start_pos, start_line, start_col),
                ));
            }
        };

        Ok(Token {
            kind,
            span: self.span_from(start_pos, start_line, start_col),
        })
    }

    // --- character-level helpers ---

    /// true if we've consumed all input
    fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// peek at the current character without advancing
    fn peek_char(&self) -> Option<char> {
        if self.at_end() {
            return None;
        }
        // we work byte-by-byte for ascii, but need to handle utf-8
        let remaining = &self.input[self.pos..];
        remaining.chars().next()
    }

    /// peek at the character after the current one
    fn peek_char2(&self) -> Option<char> {
        if self.at_end() {
            return None;
        }
        let remaining = &self.input[self.pos..];
        let mut chars = remaining.chars();
        chars.next();
        chars.next()
    }

    /// advance past the current character, updating position tracking
    fn advance(&mut self) -> Option<char> {
        let remaining = &self.input[self.pos..];
        let ch = remaining.chars().next()?;
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += ch.len_utf8() as u32;
        }
        Some(ch)
    }

    /// skip whitespace characters
    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// build a span from start position to current position
    fn span_from(&self, start_pos: usize, start_line: u32, start_col: u32) -> Span {
        Span {
            offset: start_pos,
            len: self.pos - start_pos,
            line: start_line,
            column: start_col,
        }
    }

    /// make a token at the current position with the given length
    fn make_token(&self, kind: TokenKind, len: usize) -> Token {
        Token {
            kind,
            span: Span {
                offset: self.pos,
                len,
                line: self.line,
                column: self.column,
            },
        }
    }

    // --- lexing subroutines ---

    /// lex a string literal, resolving escape sequences
    fn lex_string(&mut self) -> Result<TokenKind> {
        let start_pos = self.pos;
        let start_line = self.line;
        let start_col = self.column;
        self.advance(); // consume opening "

        let mut s = String::new();

        loop {
            match self.advance() {
                None => {
                    return Err(ParseError::new(
                        "unterminated string literal",
                        self.span_from(start_pos, start_line, start_col),
                    ));
                }
                Some('"') => return Ok(TokenKind::String(s)),
                Some('\\') => {
                    let esc = self.lex_string_escape(start_pos, start_line, start_col)?;
                    s.push(esc);
                }
                Some(c) => s.push(c),
            }
        }
    }

    /// lex a single string escape sequence (backslash already consumed)
    fn lex_string_escape(&mut self, str_start: usize, str_line: u32, str_col: u32) -> Result<char> {
        match self.advance() {
            None => Err(ParseError::new(
                "unterminated string escape",
                self.span_from(str_start, str_line, str_col),
            )),
            Some('n') => Ok('\n'),
            Some('t') => Ok('\t'),
            Some('r') => Ok('\r'),
            Some('\\') => Ok('\\'),
            Some('"') => Ok('"'),
            Some('a') => Ok('\x07'),
            Some('b') => Ok('\x08'),
            Some('0') => Ok('\0'),
            Some('x') => self.lex_hex_escape(str_start, str_line, str_col),
            Some('\n') => {
                // line continuation: \ followed by newline, skip leading whitespace
                while let Some(c) = self.peek_char() {
                    if c == ' ' || c == '\t' {
                        self.advance();
                    } else {
                        break;
                    }
                }
                // line continuation produces no character; recurse to get the next
                // actual character from the string body (not another escape)
                match self.advance() {
                    None => Err(ParseError::new(
                        "unterminated string literal after line continuation",
                        self.span_from(str_start, str_line, str_col),
                    )),
                    Some('"') => Err(ParseError::new(
                        "string ended immediately after line continuation",
                        self.span_from(str_start, str_line, str_col),
                    )),
                    Some('\\') => self.lex_string_escape(str_start, str_line, str_col),
                    Some(c) => Ok(c),
                }
            }
            Some(c) => Err(ParseError::new(
                format!("unknown string escape: \\{c}"),
                self.span_from(str_start, str_line, str_col),
            )),
        }
    }

    /// lex `\xNN;` hex character escape
    fn lex_hex_escape(&mut self, str_start: usize, str_line: u32, str_col: u32) -> Result<char> {
        let mut hex = String::new();
        loop {
            match self.peek_char() {
                Some(';') => {
                    self.advance();
                    break;
                }
                Some(c) if c.is_ascii_hexdigit() => {
                    hex.push(c);
                    self.advance();
                }
                _ => {
                    return Err(ParseError::new(
                        "unterminated hex escape (expected ';')",
                        self.span_from(str_start, str_line, str_col),
                    ));
                }
            }
        }
        if hex.is_empty() {
            return Err(ParseError::new(
                "empty hex escape \\x;",
                self.span_from(str_start, str_line, str_col),
            ));
        }
        let code = u32::from_str_radix(&hex, 16).map_err(|_| {
            ParseError::new(
                format!("invalid hex escape: \\x{hex};"),
                self.span_from(str_start, str_line, str_col),
            )
        })?;
        char::from_u32(code).ok_or_else(|| {
            ParseError::new(
                format!("invalid unicode scalar value: \\x{hex};"),
                self.span_from(str_start, str_line, str_col),
            )
        })
    }

    /// lex a `;` line comment
    fn lex_line_comment(&mut self) -> TokenKind {
        self.advance(); // consume ;
        let mut text = String::new();
        while let Some(ch) = self.peek_char() {
            if ch == '\n' {
                break;
            }
            text.push(ch);
            self.advance();
        }
        TokenKind::LineComment(text)
    }

    /// lex a `#`-prefixed token: `#t`, `#f`, `#(`, `#\`, `#;`, `#| ... |#`
    fn lex_hash(&mut self) -> Result<TokenKind> {
        let start_pos = self.pos;
        let start_line = self.line;
        let start_col = self.column;
        self.advance(); // consume #

        match self.peek_char() {
            Some('t') => {
                self.advance();
                // accept #t followed by delimiter or eof
                if self.peek_char().is_none_or(is_delimiter) {
                    Ok(TokenKind::Boolean(true))
                } else {
                    // could be #true
                    self.lex_hash_boolean_long("true", true, start_pos, start_line, start_col)
                }
            }
            Some('f') => {
                self.advance();
                if self.peek_char().is_none_or(is_delimiter) {
                    Ok(TokenKind::Boolean(false))
                } else {
                    self.lex_hash_boolean_long("false", false, start_pos, start_line, start_col)
                }
            }
            Some('(') => {
                self.advance();
                Ok(TokenKind::HashParen)
            }
            Some('u') => {
                // check for #u8(
                self.advance(); // consume u
                if self.peek_char() == Some('8') {
                    self.advance(); // consume 8
                    if self.peek_char() == Some('(') {
                        self.advance(); // consume (
                        return Ok(TokenKind::HashU8Paren);
                    }
                }
                Err(ParseError::new(
                    "unexpected character after #: 'u'",
                    self.span_from(start_pos, start_line, start_col),
                ))
            }
            Some('\\') => {
                self.advance();
                self.lex_char_literal(start_pos, start_line, start_col)
            }
            Some(';') => {
                self.advance();
                Ok(TokenKind::DatumComment)
            }
            Some('|') => {
                self.advance();
                self.lex_block_comment(start_pos, start_line, start_col)
            }
            Some(c) => Err(ParseError::new(
                format!("unexpected character after #: {c:?}"),
                self.span_from(start_pos, start_line, start_col),
            )),
            None => Err(ParseError::new(
                "unexpected end of input after #",
                self.span_from(start_pos, start_line, start_col),
            )),
        }
    }

    /// lex `#true` or `#false` long-form booleans (already consumed `#t` or `#f`)
    fn lex_hash_boolean_long(
        &mut self,
        expected: &str,
        value: bool,
        start_pos: usize,
        start_line: u32,
        start_col: u32,
    ) -> Result<TokenKind> {
        // we already consumed # and the first letter, check the rest
        for expected_ch in expected.chars().skip(1) {
            match self.peek_char() {
                Some(c) if c == expected_ch => {
                    self.advance();
                }
                _ => {
                    return Err(ParseError::new(
                        format!("invalid boolean literal, expected #{expected}"),
                        self.span_from(start_pos, start_line, start_col),
                    ));
                }
            }
        }
        if self.peek_char().is_none_or(is_delimiter) {
            Ok(TokenKind::Boolean(value))
        } else {
            Err(ParseError::new(
                format!("invalid boolean literal, expected #{expected}"),
                self.span_from(start_pos, start_line, start_col),
            ))
        }
    }

    /// lex a character literal after `#\`
    fn lex_char_literal(
        &mut self,
        start_pos: usize,
        start_line: u32,
        start_col: u32,
    ) -> Result<TokenKind> {
        let ch = self.advance().ok_or_else(|| {
            ParseError::new(
                "unexpected end of input in character literal",
                self.span_from(start_pos, start_line, start_col),
            )
        })?;

        // check for named characters or hex
        if ch.is_ascii_alphabetic() && self.peek_char().is_some_and(|c| c.is_ascii_alphabetic()) {
            // named character: collect the rest
            let mut name = String::new();
            name.push(ch);
            while let Some(c) = self.peek_char() {
                if c.is_ascii_alphabetic() {
                    name.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
            match name.as_str() {
                "space" => Ok(TokenKind::Char(' ')),
                "newline" => Ok(TokenKind::Char('\n')),
                "tab" => Ok(TokenKind::Char('\t')),
                "return" => Ok(TokenKind::Char('\r')),
                "null" | "nul" => Ok(TokenKind::Char('\0')),
                "alarm" => Ok(TokenKind::Char('\x07')),
                "backspace" => Ok(TokenKind::Char('\x08')),
                "escape" => Ok(TokenKind::Char('\x1b')),
                "delete" => Ok(TokenKind::Char('\x7f')),
                _ => Err(ParseError::new(
                    format!("unknown character name: {name}"),
                    self.span_from(start_pos, start_line, start_col),
                )),
            }
        } else if ch == 'x' && self.peek_char().is_some_and(|c| c.is_ascii_hexdigit()) {
            // hex character: #\xNN
            let mut hex = String::new();
            while let Some(c) = self.peek_char() {
                if c.is_ascii_hexdigit() {
                    hex.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
            let code = u32::from_str_radix(&hex, 16).map_err(|_| {
                ParseError::new(
                    format!("invalid hex character literal: #\\x{hex}"),
                    self.span_from(start_pos, start_line, start_col),
                )
            })?;
            char::from_u32(code).map(TokenKind::Char).ok_or_else(|| {
                ParseError::new(
                    format!("invalid unicode scalar value: #\\x{hex}"),
                    self.span_from(start_pos, start_line, start_col),
                )
            })
        } else {
            Ok(TokenKind::Char(ch))
        }
    }

    /// lex a `#| ... |#` block comment (potentially nested)
    fn lex_block_comment(
        &mut self,
        start_pos: usize,
        start_line: u32,
        start_col: u32,
    ) -> Result<TokenKind> {
        let mut text = String::new();
        let mut depth: u32 = 1;

        while depth > 0 {
            match self.advance() {
                None => {
                    return Err(ParseError::new(
                        "unterminated block comment",
                        self.span_from(start_pos, start_line, start_col),
                    ));
                }
                Some('#') if self.peek_char() == Some('|') => {
                    self.advance();
                    depth += 1;
                    text.push_str("#|");
                }
                Some('|') if self.peek_char() == Some('#') => {
                    self.advance();
                    depth -= 1;
                    if depth > 0 {
                        text.push_str("|#");
                    }
                }
                Some(c) => text.push(c),
            }
        }

        Ok(TokenKind::BlockComment(text))
    }

    /// lex `.` — either the dot separator or a number starting with `.`
    fn lex_dot_or_number(&mut self) -> Result<TokenKind> {
        if self.peek_char2().is_some_and(|c| c.is_ascii_digit()) {
            // .123 is a float
            self.lex_number()
        } else {
            self.advance();
            if self.peek_char().is_some_and(|c| c == '.') {
                // `..` is the start of `...` (ellipsis symbol)
                self.advance();
                if self.peek_char() == Some('.') {
                    self.advance();
                    Ok(TokenKind::Symbol("...".to_string()))
                } else {
                    // `..` is not valid
                    Ok(TokenKind::Symbol("..".to_string()))
                }
            } else {
                Ok(TokenKind::Dot)
            }
        }
    }

    /// lex `+` or `-` — could be a symbol or a number
    fn lex_sign_or_number(&mut self) -> Result<TokenKind> {
        let next = self.peek_char2();
        match next {
            Some(c) if c.is_ascii_digit() || c == '.' => self.lex_number(),
            // +inf.0, -inf.0, +nan.0
            Some('i') | Some('n') => self.lex_special_number_or_symbol(),
            _ => {
                // bare + or - is a symbol
                Ok(self.lex_symbol())
            }
        }
    }

    /// lex +inf.0, -inf.0, +nan.0 or fall back to symbol
    fn lex_special_number_or_symbol(&mut self) -> Result<TokenKind> {
        // peek ahead to see if this is a special float
        let remaining = &self.input[self.pos..];
        if remaining.starts_with("+inf.0") || remaining.starts_with("-inf.0") {
            let sign = self.advance().unwrap(); // + or -
            // consume "inf.0"
            for _ in 0..5 {
                self.advance();
            }
            if self.peek_char().is_none_or(is_delimiter) {
                let val = if sign == '+' {
                    f64::INFINITY
                } else {
                    f64::NEG_INFINITY
                };
                Ok(TokenKind::Float(val))
            } else {
                // not actually inf.0, it's a symbol like +inf.0xyz
                // backtrack is complex, just treat as symbol from remaining
                // actually let's just collect the rest as a symbol
                let mut s = format!("{sign}inf.0");
                while let Some(c) = self.peek_char() {
                    if is_delimiter(c) {
                        break;
                    }
                    s.push(c);
                    self.advance();
                }
                Ok(TokenKind::Symbol(s))
            }
        } else if remaining.starts_with("+nan.0") || remaining.starts_with("-nan.0") {
            let _sign = self.advance().unwrap();
            for _ in 0..5 {
                self.advance();
            }
            if self.peek_char().is_none_or(is_delimiter) {
                Ok(TokenKind::Float(f64::NAN))
            } else {
                let mut s = String::from(&remaining[..6]);
                while let Some(c) = self.peek_char() {
                    if is_delimiter(c) {
                        break;
                    }
                    s.push(c);
                    self.advance();
                }
                Ok(TokenKind::Symbol(s))
            }
        } else {
            Ok(self.lex_symbol())
        }
    }

    /// lex a number (integer or float)
    fn lex_number(&mut self) -> Result<TokenKind> {
        let start = self.pos;

        // optional sign
        if self.peek_char() == Some('+') || self.peek_char() == Some('-') {
            self.advance();
        }

        // integer part
        while let Some(c) = self.peek_char() {
            if c.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }

        let mut is_float = false;

        // fractional part
        if self.peek_char() == Some('.') {
            // make sure the next char after . is a digit or delimiter (not a symbol char)
            // this prevents `1.foo` from being parsed as a float
            let after_dot = self.peek_char2();
            if after_dot.is_none_or(|c| c.is_ascii_digit() || is_delimiter(c)) {
                is_float = true;
                self.advance(); // consume .
                while let Some(c) = self.peek_char() {
                    if c.is_ascii_digit() {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }

        // exponent part
        if self.peek_char() == Some('e') || self.peek_char() == Some('E') {
            is_float = true;
            self.advance();
            if self.peek_char() == Some('+') || self.peek_char() == Some('-') {
                self.advance();
            }
            while let Some(c) = self.peek_char() {
                if c.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let text = &self.input[start..self.pos];

        if is_float {
            let val: f64 = text.parse().map_err(|_| {
                ParseError::new(
                    format!("invalid float literal: {text}"),
                    Span {
                        offset: start,
                        len: self.pos - start,
                        line: self.line,
                        column: self.column,
                    },
                )
            })?;
            return Ok(TokenKind::Float(val));
        }

        // check for rational: integer `/` integer (no whitespace allowed)
        if self.peek_char() == Some('/') {
            let slash_pos = self.pos;
            self.advance(); // consume /
            let den_start = self.pos;
            // optional sign on denominator
            if self.peek_char() == Some('+') || self.peek_char() == Some('-') {
                self.advance();
            }
            let den_digits_start = self.pos;
            while let Some(c) = self.peek_char() {
                if c.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
            if self.pos > den_digits_start {
                let num_str = self.input[start..slash_pos].to_string();
                let den_str = self.input[den_start..self.pos].to_string();
                return Ok(TokenKind::Rational(num_str, den_str));
            } else {
                // no digits after / — backtrack, treat / as part of next token
                self.pos = slash_pos;
                // revert any sign character we may have consumed
            }
        }

        // integer or bignum
        match text.parse::<i64>() {
            Ok(val) => Ok(TokenKind::Integer(val)),
            Err(_) => Ok(TokenKind::Bignum(text.to_string())),
        }
    }

    /// lex a bare symbol (unquoted identifier)
    fn lex_symbol(&mut self) -> TokenKind {
        let mut s = String::new();
        // first char: we accept symbol initials + peculiar identifiers (+ - .)
        if let Some(c) = self.peek_char() {
            s.push(c);
            self.advance();
        }
        // subsequent chars
        while let Some(c) = self.peek_char() {
            if is_symbol_subsequent(c) {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        TokenKind::Symbol(s)
    }

    /// lex a `|...|` quoted symbol
    fn lex_quoted_symbol(&mut self) -> Result<TokenKind> {
        let start_pos = self.pos;
        let start_line = self.line;
        let start_col = self.column;
        self.advance(); // consume opening |

        let mut s = String::new();
        loop {
            match self.advance() {
                None => {
                    return Err(ParseError::new(
                        "unterminated quoted symbol",
                        self.span_from(start_pos, start_line, start_col),
                    ));
                }
                Some('|') => return Ok(TokenKind::Symbol(s)),
                Some('\\') => {
                    // only \| and \\ are valid inside |...|
                    match self.advance() {
                        Some('|') => s.push('|'),
                        Some('\\') => s.push('\\'),
                        Some(c) => {
                            s.push('\\');
                            s.push(c);
                        }
                        None => {
                            return Err(ParseError::new(
                                "unterminated quoted symbol escape",
                                self.span_from(start_pos, start_line, start_col),
                            ));
                        }
                    }
                }
                Some(c) => s.push(c),
            }
        }
    }
}

// --- character classification ---

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

/// true if `c` is a delimiter (terminates tokens)
fn is_delimiter(c: char) -> bool {
    c.is_ascii_whitespace()
        || matches!(
            c,
            '(' | ')' | '[' | ']' | '"' | ';' | '#' | '|' | '\'' | '`' | ','
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// lex all tokens from input into a vec
    fn lex_all(input: &str) -> Result<Vec<Token>> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token()?;
            if tok.kind == TokenKind::Eof {
                break;
            }
            tokens.push(tok);
        }
        Ok(tokens)
    }

    /// lex input and return just the token kinds
    fn lex_kinds(input: &str) -> Result<Vec<TokenKind>> {
        Ok(lex_all(input)?.into_iter().map(|t| t.kind).collect())
    }

    // --- basic tokens ---

    #[test]
    fn lex_parens_and_brackets() {
        let kinds = lex_kinds("( ) [ ]").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::LeftParen,
                TokenKind::RightParen,
                TokenKind::LeftBracket,
                TokenKind::RightBracket,
            ]
        );
    }

    #[test]
    fn lex_quote_forms() {
        let kinds = lex_kinds("' ` , ,@").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Quote,
                TokenKind::Quasiquote,
                TokenKind::Unquote,
                TokenKind::UnquoteSplicing,
            ]
        );
    }

    #[test]
    fn lex_hash_paren_and_datum_comment() {
        let kinds = lex_kinds("#( #;").unwrap();
        assert_eq!(kinds, vec![TokenKind::HashParen, TokenKind::DatumComment]);
    }

    #[test]
    fn lex_dot() {
        let kinds = lex_kinds(".").unwrap();
        assert_eq!(kinds, vec![TokenKind::Dot]);
    }

    // --- numbers ---

    #[test]
    fn lex_integers() {
        let kinds = lex_kinds("0 42 -7 +3").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Integer(0),
                TokenKind::Integer(42),
                TokenKind::Integer(-7),
                TokenKind::Integer(3),
            ]
        );
    }

    #[test]
    fn lex_floats() {
        let kinds = lex_kinds("3.125 -0.5 +1.0 .25 1e10 2.5e-3").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Float(3.125),
                TokenKind::Float(-0.5),
                TokenKind::Float(1.0),
                TokenKind::Float(0.25),
                TokenKind::Float(1e10),
                TokenKind::Float(2.5e-3),
            ]
        );
    }

    #[test]
    fn lex_special_floats() {
        let kinds = lex_kinds("+inf.0 -inf.0 +nan.0").unwrap();
        assert_eq!(kinds.len(), 3);
        assert_eq!(kinds[0], TokenKind::Float(f64::INFINITY));
        assert_eq!(kinds[1], TokenKind::Float(f64::NEG_INFINITY));
        // nan != nan, so check manually
        match &kinds[2] {
            TokenKind::Float(f) => assert!(f.is_nan()),
            other => panic!("expected Float(NaN), got {other:?}"),
        }
    }

    // --- strings ---

    #[test]
    fn lex_simple_string() {
        let kinds = lex_kinds(r#""hello""#).unwrap();
        assert_eq!(kinds, vec![TokenKind::String("hello".to_string())]);
    }

    #[test]
    fn lex_string_escapes() {
        let kinds = lex_kinds(r#""\n\t\r\\\"\a\b\0""#).unwrap();
        assert_eq!(
            kinds,
            vec![TokenKind::String("\n\t\r\\\"\x07\x08\0".to_string())]
        );
    }

    #[test]
    fn lex_string_hex_escape() {
        let kinds = lex_kinds(r#""\x41;""#).unwrap();
        assert_eq!(kinds, vec![TokenKind::String("A".to_string())]);
    }

    #[test]
    fn lex_string_line_continuation() {
        let input = "\"hello\\\n    world\"";
        let kinds = lex_kinds(input).unwrap();
        assert_eq!(kinds, vec![TokenKind::String("helloworld".to_string())]);
    }

    #[test]
    fn lex_unterminated_string() {
        assert!(lex_kinds(r#""hello"#).is_err());
    }

    // --- symbols ---

    #[test]
    fn lex_symbols() {
        let kinds = lex_kinds("foo bar-baz? set! *global*").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Symbol("foo".to_string()),
                TokenKind::Symbol("bar-baz?".to_string()),
                TokenKind::Symbol("set!".to_string()),
                TokenKind::Symbol("*global*".to_string()),
            ]
        );
    }

    #[test]
    fn lex_quoted_symbol() {
        let kinds = lex_kinds("|hello world|").unwrap();
        assert_eq!(kinds, vec![TokenKind::Symbol("hello world".to_string())]);
    }

    #[test]
    fn lex_plus_minus_as_symbols() {
        let kinds = lex_kinds("+ -").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Symbol("+".to_string()),
                TokenKind::Symbol("-".to_string()),
            ]
        );
    }

    #[test]
    fn lex_ellipsis() {
        let kinds = lex_kinds("...").unwrap();
        assert_eq!(kinds, vec![TokenKind::Symbol("...".to_string())]);
    }

    // --- booleans ---

    #[test]
    fn lex_booleans() {
        let kinds = lex_kinds("#t #f #true #false").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Boolean(true),
                TokenKind::Boolean(false),
                TokenKind::Boolean(true),
                TokenKind::Boolean(false),
            ]
        );
    }

    // --- characters ---

    #[test]
    fn lex_char_literals() {
        let kinds = lex_kinds("#\\a #\\space #\\newline #\\tab").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Char('a'),
                TokenKind::Char(' '),
                TokenKind::Char('\n'),
                TokenKind::Char('\t'),
            ]
        );
    }

    #[test]
    fn lex_char_hex() {
        let kinds = lex_kinds("#\\x41").unwrap();
        assert_eq!(kinds, vec![TokenKind::Char('A')]);
    }

    #[test]
    fn lex_char_named() {
        let kinds =
            lex_kinds("#\\return #\\null #\\alarm #\\backspace #\\escape #\\delete").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Char('\r'),
                TokenKind::Char('\0'),
                TokenKind::Char('\x07'),
                TokenKind::Char('\x08'),
                TokenKind::Char('\x1b'),
                TokenKind::Char('\x7f'),
            ]
        );
    }

    // --- comments ---

    #[test]
    fn lex_line_comment() {
        let kinds = lex_kinds("; this is a comment\n42").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::LineComment(" this is a comment".to_string()),
                TokenKind::Integer(42),
            ]
        );
    }

    #[test]
    fn lex_block_comment() {
        let kinds = lex_kinds("#| block |# 42").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::BlockComment(" block ".to_string()),
                TokenKind::Integer(42),
            ]
        );
    }

    #[test]
    fn lex_nested_block_comment() {
        let kinds = lex_kinds("#| outer #| inner |# still outer |# 42").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::BlockComment(" outer #| inner |# still outer ".to_string()),
                TokenKind::Integer(42),
            ]
        );
    }

    #[test]
    fn lex_unterminated_block_comment() {
        assert!(lex_kinds("#| unterminated").is_err());
    }

    // --- span tracking ---

    #[test]
    fn span_tracking() {
        let tokens = lex_all("(foo 42)").unwrap();
        assert_eq!(tokens.len(), 4);

        // (
        assert_eq!(tokens[0].span.offset, 0);
        assert_eq!(tokens[0].span.len, 1);
        assert_eq!(tokens[0].span.line, 1);
        assert_eq!(tokens[0].span.column, 1);

        // foo
        assert_eq!(tokens[1].span.offset, 1);
        assert_eq!(tokens[1].span.len, 3);

        // 42
        assert_eq!(tokens[2].span.offset, 5);
        assert_eq!(tokens[2].span.len, 2);

        // )
        assert_eq!(tokens[3].span.offset, 7);
        assert_eq!(tokens[3].span.len, 1);
    }

    #[test]
    fn span_multiline() {
        let tokens = lex_all("foo\nbar").unwrap();
        assert_eq!(tokens[0].span.line, 1);
        assert_eq!(tokens[1].span.line, 2);
        assert_eq!(tokens[1].span.column, 1);
    }

    // --- complex sequences ---

    #[test]
    fn lex_full_expression() {
        let kinds = lex_kinds("(define (square x) (* x x))").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::LeftParen,
                TokenKind::Symbol("define".to_string()),
                TokenKind::LeftParen,
                TokenKind::Symbol("square".to_string()),
                TokenKind::Symbol("x".to_string()),
                TokenKind::RightParen,
                TokenKind::LeftParen,
                TokenKind::Symbol("*".to_string()),
                TokenKind::Symbol("x".to_string()),
                TokenKind::Symbol("x".to_string()),
                TokenKind::RightParen,
                TokenKind::RightParen,
            ]
        );
    }

    #[test]
    fn lex_dotted_pair() {
        let kinds = lex_kinds("(a . b)").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::LeftParen,
                TokenKind::Symbol("a".to_string()),
                TokenKind::Dot,
                TokenKind::Symbol("b".to_string()),
                TokenKind::RightParen,
            ]
        );
    }

    #[test]
    fn lex_peek_does_not_consume() {
        let mut lexer = Lexer::new("42");
        let peeked = lexer.peek_token().unwrap().kind.clone();
        let next = lexer.next_token().unwrap().kind;
        assert_eq!(peeked, next);
        assert_eq!(next, TokenKind::Integer(42));
    }

    #[test]
    fn lex_empty_input() {
        let kinds = lex_kinds("").unwrap();
        assert!(kinds.is_empty());
    }

    #[test]
    fn lex_whitespace_only() {
        let kinds = lex_kinds("   \n\t  ").unwrap();
        assert!(kinds.is_empty());
    }

    // --- numeric tower ---

    #[test]
    fn lex_bignum() {
        let kinds = lex_kinds("99999999999999999999999999").unwrap();
        assert!(matches!(&kinds[0], TokenKind::Bignum(s) if s == "99999999999999999999999999"));
    }

    #[test]
    fn lex_negative_bignum() {
        let kinds = lex_kinds("-99999999999999999999999999").unwrap();
        assert!(matches!(&kinds[0], TokenKind::Bignum(s) if s == "-99999999999999999999999999"));
    }

    #[test]
    fn lex_rational() {
        let kinds = lex_kinds("3/4").unwrap();
        assert!(matches!(&kinds[0], TokenKind::Rational(n, d) if n == "3" && d == "4"));
    }

    #[test]
    fn lex_negative_rational() {
        let kinds = lex_kinds("-1/2").unwrap();
        assert!(matches!(&kinds[0], TokenKind::Rational(n, d) if n == "-1" && d == "2"));
    }

    #[test]
    fn lex_bytevector_prefix() {
        let kinds = lex_kinds("#u8(1 2 3)").unwrap();
        assert_eq!(kinds[0], TokenKind::HashU8Paren);
        assert_eq!(kinds[1], TokenKind::Integer(1));
    }
}
