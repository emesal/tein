/// compute net paren depth change for a line, skipping strings and comments.
///
/// rules:
/// - `(` increments depth, `)` decrements
/// - inside `"..."`: skip all chars (handle `\"` escape)
/// - after `;` outside a string: skip rest of line (line comment)
/// - `#|...|#` block comments: not handled (intentional simplicity)
fn paren_depth(line: &str) -> i32 {
    let mut depth = 0i32;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                // skip string contents
                loop {
                    match chars.next() {
                        Some('\\') => {
                            chars.next(); // skip escaped char
                        }
                        Some('"') | None => break,
                        _ => {}
                    }
                }
            }
            ';' => break, // line comment — rest of line ignored
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn main() {
    todo!("repl main — implemented in task 3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced() {
        assert_eq!(paren_depth("(+ 1 2)"), 0);
    }

    #[test]
    fn open() {
        // (define (f x) — outer ( + inner (f x) balanced = net 1
        assert_eq!(paren_depth("(define (f x)"), 1);
    }

    #[test]
    fn close() {
        assert_eq!(paren_depth("  (+ x 1))"), -1);
    }

    #[test]
    fn string_with_parens() {
        assert_eq!(paren_depth(r#"(display "(hi)")"#), 0);
    }

    #[test]
    fn string_with_escape() {
        assert_eq!(paren_depth(r#"(display "a\"b")"#), 0);
    }

    #[test]
    fn line_comment() {
        assert_eq!(paren_depth("(define x ; todo)"), 1);
    }

    #[test]
    fn empty() {
        assert_eq!(paren_depth(""), 0);
    }
}
