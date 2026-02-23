# REPL example implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** add an interactive scheme REPL as `tein/examples/repl.rs` with line editing, history, multi-line paren balancing, and meta-commands.

**Architecture:** single example file consuming the existing public API (`Context::new_standard()` + `evaluate()`). rustyline provides line editing and history. a small `paren_depth()` function tracks balanced input across lines.

**Tech Stack:** rust (edition 2024), tein (this crate), rustyline 17 with `with-file-history` feature.

**Design doc:** `docs/plans/2026-02-23-repl-example-design.md`

---

### Task 1: add rustyline dependency

**Files:**
- Modify: `tein/Cargo.toml`

**Step 1: add dev-dependency**

add to `tein/Cargo.toml`:

```toml
[dev-dependencies]
rustyline = { version = "17", features = ["with-file-history"] }
```

**Step 2: verify it compiles**

Run: `cargo check -p tein`
Expected: compiles with no errors

**Step 3: commit**

```bash
git add tein/Cargo.toml Cargo.lock
git commit -m "deps: add rustyline dev-dependency for REPL example"
```

---

### Task 2: write paren-balancing helper

this is the one piece of logic worth testing independently.

**Files:**
- Create: `tein/examples/repl.rs` (start the file, will be built up across tasks)

**Step 1: write the paren_depth function and tests inline**

`paren_depth(line: &str) -> i32` — returns the net paren depth change for a line, respecting string literals and line comments.

rules:
- `(` increments depth, `)` decrements
- inside `"..."`: skip all chars (handle `\"` escape)
- after `;` outside a string: skip rest of line (line comment)
- `#|...|#` block comments: NOT handled (intentional simplicity)

```rust
/// compute net paren depth change for a line, skipping strings and comments.
fn paren_depth(line: &str) -> i32 {
    let mut depth = 0i32;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                // skip string contents
                loop {
                    match chars.next() {
                        Some('\\') => { chars.next(); } // skip escaped char
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced() {
        assert_eq!(paren_depth("(+ 1 2)"), 0);
    }

    #[test]
    fn open() {
        assert_eq!(paren_depth("(define (f x)"), 2);
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
```

**Step 2: run the tests**

Run: `cargo test -p tein --example repl`
Expected: all 7 tests pass

note: `#[cfg(test)]` in examples requires running with `--example repl`. if cargo doesn't pick up `#[cfg(test)]` in examples, move the tests into a `#[test]` block that still runs. alternatively, test via a quick `assert!` in a separate test file. we'll see what works — the important thing is the function is tested.

*fallback*: if `#[cfg(test)]` doesn't work in examples, add a simple `#[test]` in `tein/tests/repl_paren_depth.rs` that imports nothing and just copies the function + tests. or just inline `debug_assert!` calls. pick whatever works cleanly.

**Step 3: commit**

```bash
git add tein/examples/repl.rs
git commit -m "feat(repl): add paren-depth balancing helper with tests"
```

---

### Task 3: write the REPL main function

**Files:**
- Modify: `tein/examples/repl.rs`

**Step 1: write the full REPL**

the complete `main()` function. key elements:

```rust
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use tein::{Context, Value};

// ... paren_depth from task 2 ...

/// history file path: ~/.tein_history
fn history_path() -> Option<std::path::PathBuf> {
    dirs-free approach: std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".tein_history"))
    // on platforms without HOME, skip history
}

fn main() {
    // banner
    println!("tein {} — r7rs scheme", env!("CARGO_PKG_VERSION"));
    println!("type ,help for commands, ,quit to exit\n");

    // context
    let ctx = match Context::new_standard() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: failed to initialize scheme context: {}", e);
            std::process::exit(1);
        }
    };

    // editor
    let mut rl = match DefaultEditor::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: failed to initialize editor: {}", e);
            std::process::exit(1);
        }
    };

    // load history (best-effort)
    if let Some(path) = history_path() {
        let _ = rl.load_history(&path);
    }

    let mut buffer = String::new();
    let mut depth = 0i32;

    loop {
        let prompt = if buffer.is_empty() { "tein> " } else { "  ... " };

        match rl.readline(prompt) {
            Ok(line) => {
                // meta-commands only on fresh input
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

                // accumulate
                if !buffer.is_empty() { buffer.push('\n'); }
                depth += paren_depth(&line);
                buffer.push_str(&line);

                // balanced (or over-closed) → evaluate
                if depth <= 0 {
                    let input = buffer.trim();
                    if !input.is_empty() {
                        let _ = rl.add_history_entry(input);
                        match ctx.evaluate(input) {
                            Ok(Value::Unspecified) => {} // suppress
                            Ok(value) => println!("{}", value),
                            Err(e) => eprintln!("error: {}", e),
                        }
                    }
                    buffer.clear();
                    depth = 0;
                }
                // else: depth > 0, keep reading
            }
            Err(ReadlineError::Interrupted) => {
                // ctrl-c: cancel current input
                if !buffer.is_empty() {
                    buffer.clear();
                    depth = 0;
                    println!("^C");
                }
            }
            Err(ReadlineError::Eof) => {
                // ctrl-d: exit
                println!();
                break;
            }
            Err(e) => {
                eprintln!("error: {}", e);
                break;
            }
        }
    }

    // save history (best-effort)
    if let Some(path) = history_path() {
        let _ = rl.save_history(&path);
    }
}
```

**Step 2: verify it compiles and runs**

Run: `cargo build -p tein --example repl`
Expected: compiles

Run: `echo '(+ 1 2)' | cargo run -p tein --example repl`
Expected: prints banner, then `3`, then exits on EOF

**Step 3: commit**

```bash
git add tein/examples/repl.rs
git commit -m "feat(repl): interactive scheme REPL with rustyline

standard r7rs env, multi-line paren balancing, history,
,help/,quit meta-commands. closes #14"
```

---

### Task 4: update docs and TODO

**Files:**
- Modify: `TODO.md` — check off REPL item
- Modify: `README.md` — add REPL to examples list (if examples are listed there)

**Step 1: update TODO.md**

change `- [ ] **REPL example**` to `- [x] **REPL example**`

**Step 2: update README.md if needed**

check if README lists examples. if so, add:
```
cargo run -p tein --example repl    # interactive scheme REPL
```

**Step 3: commit**

```bash
git add TODO.md README.md
git commit -m "docs: mark REPL example complete in TODO"
```

---

### Task 5: manual smoke test

not a commit — just verification.

**Step 1: interactive test**

Run: `cargo run -p tein --example repl`

test these scenarios:
1. `(+ 1 2)` → `3`
2. `(define (square x) (* x x))` → (no output, Unspecified)
3. `(square 7)` → `49`
4. multi-line: type `(define (fact n)`, press enter, see `  ... `, type `(if (= n 0) 1 (* n (fact (- n 1)))))`, see result suppressed, then `(fact 10)` → `3628800`
5. `,help` → shows commands
6. `,quit` → exits
7. ctrl-c mid-input → clears buffer
8. ctrl-d → exits
9. `(import (scheme cxr))` → (no output), `(caaar '(((1))))` → `1`
10. `(open-input-file "/etc/passwd")` → error (scheme error, no sandbox)
