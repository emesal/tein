use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use tein::{Context, Value};

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

/// history file path: `~/.tein_history`, derived from `$HOME`.
/// returns `None` on platforms where `$HOME` is not set.
fn history_path() -> Option<std::path::PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join(".tein_history"))
}

fn main() {
    println!("tein {} — r7rs scheme", env!("CARGO_PKG_VERSION"));
    println!("type ,help for commands, ,quit to exit\n");

    let ctx = match Context::new_standard() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: failed to initialize scheme context: {}", e);
            std::process::exit(1);
        }
    };

    let mut rl = match DefaultEditor::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: failed to initialize editor: {}", e);
            std::process::exit(1);
        }
    };

    // load history — best-effort, silently skip failures
    if let Some(path) = history_path() {
        let _ = rl.load_history(&path);
    }

    let mut buffer = String::new();
    let mut depth = 0i32;

    loop {
        let prompt = if buffer.is_empty() { "tein> " } else { "  ... " };

        match rl.readline(prompt) {
            Ok(line) => {
                // meta-commands only on fresh input (no accumulated buffer)
                if buffer.is_empty() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Some(cmd) = trimmed.strip_prefix(',') {
                        match cmd.trim() {
                            "quit" | "q" => break,
                            "help" | "h" => {
                                println!(",help  — show this message");
                                println!(",quit  — exit the repl");
                                continue;
                            }
                            other => {
                                eprintln!("unknown command: ,{}", other);
                                continue;
                            }
                        }
                    }
                }

                // accumulate into multi-line buffer
                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                depth += paren_depth(&line);
                buffer.push_str(&line);

                // balanced (or over-closed) → evaluate
                if depth <= 0 {
                    let input = buffer.trim().to_owned();
                    buffer.clear();
                    depth = 0;

                    if !input.is_empty() {
                        let _ = rl.add_history_entry(&input);
                        match ctx.evaluate(&input) {
                            Ok(Value::Unspecified) => {} // suppress unspecified (e.g. define)
                            Ok(value) => println!("{}", value),
                            Err(e) => eprintln!("error: {}", e),
                        }
                    }
                }
                // else: depth > 0, keep reading with continuation prompt
            }
            Err(ReadlineError::Interrupted) => {
                // ctrl-c: cancel current input, back to fresh prompt
                if !buffer.is_empty() {
                    buffer.clear();
                    depth = 0;
                    println!("^C");
                }
            }
            Err(ReadlineError::Eof) => {
                // ctrl-d: exit gracefully
                println!();
                break;
            }
            Err(e) => {
                eprintln!("error: {}", e);
                break;
            }
        }
    }

    // save history — best-effort, silently skip failures
    if let Some(path) = history_path() {
        let _ = rl.save_history(&path);
    }
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
