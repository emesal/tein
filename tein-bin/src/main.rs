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

fn main() {}

#[cfg(test)]
mod tests {
    use super::*;

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
