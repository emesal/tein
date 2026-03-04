use std::path::PathBuf;

/// CLI mode.
#[derive(Debug, PartialEq)]
enum Mode {
    Repl,
    Script { path: PathBuf, extra_args: Vec<String> },
}

/// Parsed CLI arguments.
#[derive(Debug)]
struct Args {
    mode: Mode,
    sandbox: bool,
    all_modules: bool,
}

/// Parse CLI args (does not include argv[0]).
///
/// Returns `Err(message)` for invalid combinations.
fn parse_args(raw: Vec<String>) -> Result<Args, String> {
    let mut sandbox = false;
    let mut all_modules = false;
    let mut positional: Vec<String> = vec![];

    for arg in raw {
        match arg.as_str() {
            "--sandbox" => sandbox = true,
            "--all-modules" => all_modules = true,
            other if other.starts_with("--") => {
                return Err(format!("unknown flag: {}", other));
            }
            _ => positional.push(arg),
        }
    }

    if all_modules && !sandbox {
        return Err("--all-modules requires --sandbox".to_string());
    }

    let mode = if positional.is_empty() {
        Mode::Repl
    } else {
        let path = PathBuf::from(&positional[0]);
        let extra_args = positional[1..].to_vec();
        Mode::Script { path, extra_args }
    };

    Ok(Args { mode, sandbox, all_modules })
}

/// Strip shebang line if present.
///
/// If the source starts with `#!`, returns the content after the first `\n`.
/// Otherwise returns the source unchanged. Handles files with no trailing newline.
fn strip_shebang(src: &str) -> &str {
    if src.starts_with("#!") {
        match src.find('\n') {
            Some(pos) => &src[pos + 1..],
            None => "",
        }
    } else {
        src
    }
}

/// Run a scheme script file.
///
/// Reads the file, strips shebang if present, evaluates via tein.
/// Returns the process exit code: 0 on success, 1 on eval error.
/// `Value::Exit(n)` propagates `n` directly.
fn run_script(path: &std::path::Path, args: &Args) -> i32 {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tein: error reading {}: {}", path.display(), e);
            return 1;
        }
    };

    let source = strip_shebang(&source);

    let ctx = match build_context_script(args, path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("tein: failed to initialize context: {}", e);
            return 1;
        }
    };

    match ctx.evaluate(source) {
        Ok(tein::Value::Exit(n)) => n,
        Ok(_) => 0,
        Err(e) => {
            eprintln!("tein: {}", e);
            1
        }
    }
}

/// Build a tein Context for script mode.
///
/// Sets `(command-line)` to `["tein", path, ...extra_args]` for sandboxed contexts.
/// Unsandboxed contexts use real `std::env::args()` which is already correct.
fn build_context_script(
    args: &Args,
    script_path: &std::path::Path,
) -> tein::Result<tein::Context> {
    use tein::sandbox::Modules;

    if args.sandbox {
        let modules = if args.all_modules { Modules::All } else { Modules::Safe };
        let path_str = script_path.to_str().unwrap_or("");
        let Mode::Script { extra_args, .. } = &args.mode else {
            unreachable!("build_context_script called in non-script mode")
        };
        let mut cmd = vec!["tein", path_str];
        cmd.extend(extra_args.iter().map(String::as_str));

        tein::Context::builder()
            .standard_env()
            .sandboxed(modules)
            .command_line(&cmd)
            .build()
    } else {
        tein::Context::builder().standard_env().build()
    }
}

/// Build a tein Context for REPL mode.
fn build_context_repl(args: &Args) -> tein::Result<tein::Context> {
    use tein::sandbox::Modules;

    if args.sandbox {
        let modules = if args.all_modules { Modules::All } else { Modules::Safe };
        tein::Context::builder().standard_env().sandboxed(modules).build()
    } else {
        tein::Context::builder().standard_env().build()
    }
}

/// Compute net paren depth change for a line, skipping strings and comments.
///
/// Rules:
/// - `(` increments depth, `)` decrements
/// - inside `"..."`: skip all chars (handle `\"` escape)
/// - after `;` outside a string: skip rest of line (line comment)
/// - `#|...|#` block comments: not handled (intentional simplicity)
fn paren_depth(line: &str) -> i32 {
    let mut depth = 0i32;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => loop {
                match chars.next() {
                    Some('\\') => {
                        chars.next();
                    }
                    Some('"') | None => break,
                    _ => {}
                }
            },
            ';' => break,
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// History file path: `~/.tein_history`, derived from `$HOME`.
/// Returns `None` on platforms where `$HOME` is not set.
fn history_path() -> Option<std::path::PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join(".tein_history"))
}

fn run_repl(args: &Args) {
    use rustyline::DefaultEditor;
    use rustyline::error::ReadlineError;

    println!("tein {} — r7rs scheme", env!("CARGO_PKG_VERSION"));
    println!("type ,help for commands, ,quit to exit\n");

    let ctx = match build_context_repl(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("tein: failed to initialize context: {}", e);
            std::process::exit(1);
        }
    };

    let mut rl = match DefaultEditor::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tein: failed to initialize editor: {}", e);
            std::process::exit(1);
        }
    };

    if let Some(path) = history_path() {
        let _ = rl.load_history(&path);
    }

    let mut buffer = String::new();
    let mut depth = 0i32;

    loop {
        let prompt = if buffer.is_empty() { "tein> " } else { "  ... " };

        match rl.readline(prompt) {
            Ok(line) => {
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

                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                depth += paren_depth(&line);
                buffer.push_str(&line);

                if depth <= 0 {
                    let input = buffer.trim().to_owned();
                    buffer.clear();
                    depth = 0;

                    if !input.is_empty() {
                        let _ = rl.add_history_entry(&input);
                        match ctx.evaluate(&input) {
                            Ok(tein::Value::Unspecified) => {}
                            Ok(tein::Value::Exit(n)) => {
                                if let Some(path) = history_path() {
                                    let _ = rl.save_history(&path);
                                }
                                std::process::exit(n);
                            }
                            Ok(value) => println!("{}", value),
                            Err(e) => eprintln!("error: {}", e),
                        }
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                if !buffer.is_empty() {
                    buffer.clear();
                    depth = 0;
                    println!("^C");
                }
            }
            Err(ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(e) => {
                eprintln!("error: {}", e);
                break;
            }
        }
    }

    if let Some(path) = history_path() {
        let _ = rl.save_history(&path);
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("tein: {}", e);
            eprintln!("usage: tein [--sandbox] [--all-modules] [script.scm [args...]]");
            std::process::exit(2);
        }
    };

    match &args.mode {
        Mode::Repl => run_repl(&args),
        Mode::Script { path, .. } => {
            let code = run_script(path, &args);
            std::process::exit(code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paren_balanced() {
        assert_eq!(paren_depth("(+ 1 2)"), 0);
    }

    #[test]
    fn paren_open() {
        assert_eq!(paren_depth("(define (f x)"), 1);
    }

    #[test]
    fn paren_close() {
        assert_eq!(paren_depth("  (+ x 1))"), -1);
    }

    #[test]
    fn paren_string_with_parens() {
        assert_eq!(paren_depth(r#"(display "(hi)")"#), 0);
    }

    #[test]
    fn paren_string_with_escape() {
        assert_eq!(paren_depth(r#"(display "a\"b")"#), 0);
    }

    #[test]
    fn paren_line_comment() {
        assert_eq!(paren_depth("(define x ; todo)"), 1);
    }

    #[test]
    fn paren_empty() {
        assert_eq!(paren_depth(""), 0);
    }

    #[test]
    fn shebang_stripped() {
        let src = "#!/usr/bin/env tein\n(+ 1 2)";
        assert_eq!(strip_shebang(src), "(+ 1 2)");
    }

    #[test]
    fn no_shebang_unchanged() {
        let src = "(+ 1 2)";
        assert_eq!(strip_shebang(src), "(+ 1 2)");
    }

    #[test]
    fn shebang_only_no_newline() {
        let src = "#!/usr/bin/env tein";
        assert_eq!(strip_shebang(src), "");
    }

    #[test]
    fn hash_not_shebang_unchanged() {
        // #| block comment — not a shebang
        let src = "#| comment |#\n(+ 1 2)";
        assert_eq!(strip_shebang(src), "#| comment |#\n(+ 1 2)");
    }

    #[test]
    fn no_args_is_repl() {
        let args = parse_args(vec![]).unwrap();
        assert_eq!(args.mode, Mode::Repl);
        assert!(!args.sandbox);
        assert!(!args.all_modules);
    }

    #[test]
    fn file_arg_is_script() {
        let args = parse_args(vec!["script.scm".into()]).unwrap();
        assert_eq!(
            args.mode,
            Mode::Script { path: "script.scm".into(), extra_args: vec![] }
        );
    }

    #[test]
    fn file_with_extra_args() {
        let args = parse_args(vec!["script.scm".into(), "foo".into(), "bar".into()]).unwrap();
        assert_eq!(
            args.mode,
            Mode::Script {
                path: "script.scm".into(),
                extra_args: vec!["foo".into(), "bar".into()]
            }
        );
    }

    #[test]
    fn sandbox_flag() {
        let args = parse_args(vec!["--sandbox".into()]).unwrap();
        assert!(args.sandbox);
        assert_eq!(args.mode, Mode::Repl);
    }

    #[test]
    fn sandbox_with_all_modules() {
        let args = parse_args(vec!["--sandbox".into(), "--all-modules".into()]).unwrap();
        assert!(args.sandbox);
        assert!(args.all_modules);
    }

    #[test]
    fn all_modules_without_sandbox_is_error() {
        let err = parse_args(vec!["--all-modules".into()]).unwrap_err();
        assert!(err.contains("--all-modules"));
    }

    #[test]
    fn sandbox_with_file() {
        let args = parse_args(vec!["--sandbox".into(), "script.scm".into()]).unwrap();
        assert!(args.sandbox);
        assert_eq!(
            args.mode,
            Mode::Script { path: "script.scm".into(), extra_args: vec![] }
        );
    }
}
