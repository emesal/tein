# tein binary — standalone scheme interpreter/REPL

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `tein` binary crate to the workspace providing a REPL, script runner, and shebang interpreter.

**Architecture:** New `tein-bin` workspace member (binary, `publish = false`) with manual CLI arg parsing. `Value::Exit(i32)` is added to the `tein` library so both the binary and embedders can distinguish an `(exit n)` call from a normal return. The REPL is lifted from `examples/repl.rs`.

**Tech Stack:** Rust, rustyline 17, tein library, std only for arg parsing.

**Design doc:** `docs/plans/2026-03-04-tein-binary-design.md`

---

### Task 1: Add `Value::Exit(i32)` to the tein library

**Files:**
- Modify: `tein/src/value.rs`
- Modify: `tein/src/context.rs`

**Step 1: Write failing tests** in `tein/src/context.rs`, inside the existing `#[cfg(test)]` block at the bottom of the file:

```rust
#[test]
fn exit_no_args_returns_exit_zero() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx.evaluate("(import (tein process)) (exit)").unwrap();
    assert_eq!(result, Value::Exit(0));
}

#[test]
fn exit_true_returns_exit_zero() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx.evaluate("(import (tein process)) (exit #t)").unwrap();
    assert_eq!(result, Value::Exit(0));
}

#[test]
fn exit_false_returns_exit_one() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx.evaluate("(import (tein process)) (exit #f)").unwrap();
    assert_eq!(result, Value::Exit(1));
}

#[test]
fn exit_integer_returns_exit_n() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx.evaluate("(import (tein process)) (exit 42)").unwrap();
    assert_eq!(result, Value::Exit(42));
}

#[test]
fn exit_string_returns_exit_zero() {
    // non-integer, non-boolean → 0 per r7rs
    let ctx = Context::new_standard().unwrap();
    let result = ctx.evaluate(r#"(import (tein process)) (exit "bye")"#).unwrap();
    assert_eq!(result, Value::Exit(0));
}
```

**Step 2: Run tests to verify they fail**

```bash
cargo test exit_no_args_returns_exit_zero exit_true_returns_exit_zero exit_false_returns_exit_one exit_integer_returns_exit_n exit_string_returns_exit_zero
```

Expected: compile error — `Value::Exit` does not exist.

**Step 3: Add `Value::Exit(i32)` variant**

In `tein/src/value.rs`, add to the `Value` enum after `Unspecified`:

```rust
/// Exit signal from `(exit)` or `(exit n)`.
///
/// Returned when scheme code calls `(exit)`, `(exit #t)`, `(exit #f)`,
/// or `(exit n)`. The `i32` is the exit code:
/// - `(exit)` / `(exit #t)` → 0
/// - `(exit #f)` → 1
/// - `(exit n)` (integer) → n (clamped to i32)
/// - `(exit other)` → 0
///
/// Embedders who need to propagate the exit signal should match on this
/// variant. Embedders who don't care can treat it like any other value.
Exit(i32),
```

Also update the module-level doc table at the top of `value.rs` to add:

```
//! | `Exit(i32)` | — | exit code from `(exit n)` |
```

**Step 4: Update `check_exit` in `context.rs`**

The current `check_exit` (line ~2122) returns `Some(Ok(Value::Integer(0)))` for no-args exit and `Some(unsafe { Value::from_raw(...) })` for others. Replace the whole method body:

```rust
fn check_exit(&self) -> Option<Result<Value>> {
    if EXIT_REQUESTED.with(|c| c.replace(false)) {
        let raw = EXIT_VALUE.with(|c| c.replace(std::ptr::null_mut()));
        // release GC root — sexp_release_object is a no-op for immediates
        if !raw.is_null() {
            unsafe { ffi::sexp_release_object(self.ctx, raw) };
        }
        let code = unsafe { exit_code_from_raw(raw) };
        Some(Ok(Value::Exit(code)))
    } else {
        None
    }
}
```

Then add the helper function `exit_code_from_raw` as a private free function near `check_exit` (outside `impl Context`):

```rust
/// Convert a raw sexp exit value to an i32 exit code.
///
/// - null / void / #t → 0
/// - #f → 1
/// - fixnum → value (clamped to i32)
/// - anything else → 0
unsafe fn exit_code_from_raw(raw: ffi::sexp) -> i32 {
    unsafe {
        if raw.is_null() || ffi::sexp_voidp(raw) != 0 {
            return 0;
        }
        if ffi::sexp_booleanp(raw) != 0 {
            return if ffi::sexp_truep(raw) != 0 { 0 } else { 1 };
        }
        if ffi::sexp_integerp(raw) != 0 {
            return ffi::sexp_unbox_fixnum(raw) as i32;
        }
        0
    }
}
```

**Step 5: Add `PartialEq` derive and `Display` impl for `Exit`**

`Value` already derives `Debug` and `Clone`. Check if it derives `PartialEq` — if not, add it (required for the test assertions). In `value.rs`, the `Display` impl will need a new arm:

```rust
Value::Exit(n) => write!(f, "#<exit {}>", n),
```

Add this arm to the existing `impl fmt::Display for Value` block.

**Step 6: Run tests**

```bash
cargo test exit_
```

Expected: all 5 tests pass.

**Step 7: Run full test suite**

```bash
just test
```

Expected: all tests pass (no regressions — existing tests don't exercise `Value::Exit`).

**Step 8: Commit**

```bash
git add tein/src/value.rs tein/src/context.rs
git commit -m "feat(value): add Value::Exit(i32) for (exit n) escape hatch"
```

---

### Task 2: Create `tein-bin` crate scaffold

**Files:**
- Create: `tein-bin/Cargo.toml`
- Create: `tein-bin/src/main.rs`
- Modify: `Cargo.toml` (workspace)

**Step 1: Add `tein-bin` to workspace**

In the root `Cargo.toml`, update members:

```toml
members = ["tein", "tein-macros", "tein-sexp", "tein-ext", "tein-test-ext", "tein-bin"]
```

**Step 2: Create `tein-bin/Cargo.toml`**

```toml
[package]
name = "tein-bin"
version = "0.1.0"
edition = "2024"
authors = ["fey"]
license = "ISC"
publish = false
description = "standalone tein scheme interpreter and REPL"
repository = "https://github.com/emesal/tein"

[[bin]]
name = "tein"
path = "src/main.rs"

[dependencies]
tein = { path = "../tein" }
rustyline = { version = "17", features = ["with-file-history"] }
```

**Step 3: Remove `rustyline` from `tein`'s dev-dependencies**

In `tein/Cargo.toml`, remove this line from `[dev-dependencies]`:

```toml
rustyline = { version = "17", features = ["with-file-history"] }
```

(The `repl` example will break — we fix it in Task 5.)

**Step 4: Create minimal `tein-bin/src/main.rs`**

```rust
fn main() {}
```

**Step 5: Verify it builds**

```bash
cargo build -p tein-bin
```

Expected: builds cleanly.

**Step 6: Commit**

```bash
git add tein-bin/ Cargo.toml tein/Cargo.toml
git commit -m "chore: scaffold tein-bin crate"
```

---

### Task 3: Arg parsing

**Files:**
- Modify: `tein-bin/src/main.rs`

**Step 1: Write failing tests** (add to `main.rs`):

```rust
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
        assert_eq!(args.mode, Mode::Script { path: "script.scm".into(), extra_args: vec![] });
    }

    #[test]
    fn file_with_extra_args() {
        let args = parse_args(vec!["script.scm".into(), "foo".into(), "bar".into()]).unwrap();
        assert_eq!(
            args.mode,
            Mode::Script { path: "script.scm".into(), extra_args: vec!["foo".into(), "bar".into()] }
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
        assert_eq!(args.mode, Mode::Script { path: "script.scm".into(), extra_args: vec![] });
    }
}
```

**Step 2: Run tests to verify they fail**

```bash
cargo test -p tein-bin
```

Expected: compile errors — `parse_args`, `Mode`, `Args` not defined.

**Step 3: Implement arg parsing**

Replace `main.rs` with:

```rust
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
    // ... (tests from step 1)
}
```

**Step 4: Run tests**

```bash
cargo test -p tein-bin
```

Expected: all arg parsing tests pass.

**Step 5: Commit**

```bash
git add tein-bin/src/main.rs
git commit -m "feat(tein-bin): arg parsing — sandbox/all-modules/script/repl modes"
```

---

### Task 4: Shebang stripping

**Files:**
- Modify: `tein-bin/src/main.rs`

**Step 1: Write failing tests** (add to test module):

```rust
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
```

**Step 2: Run tests to verify they fail**

```bash
cargo test -p tein-bin strip_shebang
```

**Step 3: Implement `strip_shebang`**

Add above `fn main()`:

```rust
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
```

**Step 4: Run tests**

```bash
cargo test -p tein-bin strip_shebang
```

Expected: all 4 tests pass.

**Step 5: Commit**

```bash
git add tein-bin/src/main.rs
git commit -m "feat(tein-bin): shebang stripping"
```

---

### Task 5: Script mode

**Files:**
- Modify: `tein-bin/src/main.rs`

**Step 1: Implement `run_script`**

Add below `strip_shebang`:

```rust
/// Run a scheme script file.
///
/// Reads the file, strips shebang if present, evaluates via tein.
/// `command_line` is `["tein", path, ...extra_args]` passed to `(command-line)`.
///
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

    let ctx = build_context(args, path);
    let ctx = match ctx {
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

/// Build a tein Context from parsed CLI args.
///
/// For script mode, `script_path` is used to set `(command-line)`.
fn build_context(args: &Args, script_path: &std::path::Path) -> tein::error::Result<tein::Context> {
    use tein::sandbox::Modules;

    if args.sandbox {
        let modules = if args.all_modules { Modules::All } else { Modules::Safe };
        // build command-line vec: ["tein", path, ...extra_args]
        let mut cmd: Vec<&str> = vec!["tein"];
        let path_str = script_path.to_str().unwrap_or("");
        cmd.push(path_str);
        let Mode::Script { extra_args, .. } = &args.mode else { unreachable!() };
        let extra: Vec<&str> = extra_args.iter().map(String::as_str).collect();
        cmd.extend(extra.iter());

        tein::ContextBuilder::new()
            .standard_env()
            .sandboxed(modules)
            .command_line(&cmd)
            .build()
    } else {
        tein::ContextBuilder::new().standard_env().build()
    }
}
```

**Step 2: Wire up `main`**

```rust
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

fn run_repl(_args: &Args) {
    // placeholder — implemented in task 6
    eprintln!("tein: REPL not yet implemented");
    std::process::exit(1);
}
```

**Step 3: Build to check for compile errors**

```bash
cargo build -p tein-bin
```

**Step 4: Smoke test with a real scheme file**

```bash
echo '(display "hello") (newline)' > /tmp/test.scm
cargo run -p tein-bin -- /tmp/test.scm
```

Expected: prints `hello`.

```bash
echo '(exit 42)' >> /tmp/test.scm
# (now has display + exit 42)
```

Wait — we need `(import (tein process))` for exit. Test instead:

```bash
printf '#!/usr/bin/env tein\n(display "hello")\n(newline)\n' > /tmp/shebang.scm
cargo run -p tein-bin -- /tmp/shebang.scm
```

Expected: prints `hello` (shebang stripped, no error).

**Step 5: Commit**

```bash
git add tein-bin/src/main.rs
git commit -m "feat(tein-bin): script mode with sandbox flags"
```

---

### Task 6: REPL

**Files:**
- Modify: `tein-bin/src/main.rs`

**Step 1: Write failing tests** for `paren_depth` (add to test module):

```rust
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
```

**Step 2: Run tests to verify they fail**

```bash
cargo test -p tein-bin paren_
```

**Step 3: Add `paren_depth` and `history_path`**

Copy verbatim from `tein/examples/repl.rs`:

```rust
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
            '"' => {
                loop {
                    match chars.next() {
                        Some('\\') => { chars.next(); }
                        Some('"') | None => break,
                        _ => {}
                    }
                }
            }
            ';' => break,
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// History file path: `~/.tein_history`.
fn history_path() -> Option<std::path::PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join(".tein_history"))
}
```

**Step 4: Run paren_depth tests**

```bash
cargo test -p tein-bin paren_
```

Expected: all 7 pass.

**Step 5: Implement `run_repl`**

Replace the placeholder `run_repl` with the full implementation:

```rust
fn run_repl(args: &Args) {
    use rustyline::DefaultEditor;
    use rustyline::error::ReadlineError;

    println!("tein {} — r7rs scheme", env!("CARGO_PKG_VERSION"));
    println!("type ,help for commands, ,quit to exit\n");

    // for REPL, script_path is irrelevant — pass a dummy
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
                    if trimmed.is_empty() { continue; }
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

                if !buffer.is_empty() { buffer.push('\n'); }
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

/// Build context for REPL mode.
fn build_context_repl(args: &Args) -> tein::error::Result<tein::Context> {
    use tein::sandbox::Modules;
    if args.sandbox {
        let modules = if args.all_modules { Modules::All } else { Modules::Safe };
        tein::ContextBuilder::new()
            .standard_env()
            .sandboxed(modules)
            .build()
    } else {
        tein::ContextBuilder::new().standard_env().build()
    }
}
```

Also remove the `_args: &Args` placeholder from the old `run_repl` signature and update `main` to call `run_repl(&args)` (no change needed if already done).

**Step 6: Fix the `repl` example**

Since `rustyline` is no longer in `tein`'s dev-deps, `tein/examples/repl.rs` will no longer compile. The simplest fix: remove the example (it's superseded by the binary). Delete `tein/examples/repl.rs` and remove it from `AGENTS.md`'s command list:

In `AGENTS.md`, the commands section references `cargo run --example basic ... (basic|floats|ffi|debug|sandbox|foreign_types|managed)` — remove `repl` from that list (it wasn't listed there, check anyway).

Actually just delete the file:

```bash
rm tein/examples/repl.rs
```

**Step 7: Build and smoke-test REPL**

```bash
cargo build -p tein-bin
```

Then manually test:

```bash
cargo run -p tein-bin
# should show banner, tein> prompt
# type (+ 1 2) → 3
# ,quit → exits
```

**Step 8: Run full test suite**

```bash
just test
```

Expected: all tests pass.

**Step 9: Commit**

```bash
git add tein-bin/src/main.rs tein/examples/repl.rs
git commit -m "feat(tein-bin): REPL mode with history, paren tracking, exit handling"
```

---

### Task 7: Update AGENTS.md and docs

**Files:**
- Modify: `AGENTS.md`
- Modify: `docs/guide.md` (or wherever the binary is documented)

**Step 1: Update `AGENTS.md` commands section**

Add `tein-bin` build/run commands:

```
cargo build -p tein-bin              # build the tein binary
cargo run -p tein-bin                # run REPL
cargo run -p tein-bin -- script.scm  # run script
cargo test -p tein-bin               # unit tests (arg parsing, shebang, paren_depth)
```

Remove `repl` from the examples list in the `cargo run --example` line.

**Step 2: Check if `docs/guide.md` mentions the binary**

```bash
grep -n "binary\|tein-bin\|script\|shebang" docs/guide.md | head -20
```

Add a section if absent. Minimal content:

```markdown
## Running tein as a script interpreter

tein ships a standalone binary for running scheme scripts from the command line.

```sh
tein script.scm          # run a script
tein                     # start the REPL
tein --sandbox script.scm          # sandboxed (safe module set)
tein --sandbox --all-modules ...   # sandboxed (full VFS module set)
```

Scripts can use a shebang line:

```scheme
#!/usr/bin/env tein
(display "hello, world!")
(newline)
```

For sandboxed scripts:

```scheme
#!/usr/bin/env -S tein --sandbox
(display "sandboxed!")
(newline)
```
```

**Step 3: Commit**

```bash
git add AGENTS.md docs/guide.md
git commit -m "docs: document tein binary, update AGENTS.md commands"
```

---

### Task 8: Collect AGENTS.md gotchas

During implementation, note any surprises in `AGENTS.md`. At minimum add:

- `Value::Exit(i32)` in the "adding a new scheme type" section is NOT applicable — `Exit` is not a scheme type, it's a rust-side signal. `from_raw` never produces it; `check_exit` does.
- `tein-bin` is not published; `rustyline` is a regular dep of `tein-bin`, not a dev-dep of `tein`.

**Step 1: Add gotcha to AGENTS.md**

Find the `## critical gotchas` section and add:

```markdown
**`Value::Exit(i32)` is not a scheme type**: it is produced only by `check_exit()` when the `EXIT_REQUESTED` thread-local is set. `Value::from_raw()` never produces it. do not add it to the type-checking dispatch in `from_raw`.
```

**Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "docs(agents): add Value::Exit gotcha"
```

---

### Final: lint

```bash
just lint
git add -p
git commit -m "style: cargo fmt + clippy for tein-bin"
```
