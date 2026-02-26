# REPL example

**date**: 2026-02-23
**issue**: #14
**status**: approved

## motivation

an interactive REPL is the most natural way to explore a scheme interpreter. it exercises the full public API, serves as a demo, and gives users a tool for testing and experimentation. lives alongside existing examples (basic, sandbox, ffi, etc.).

## design

### placement

single file: `tein/examples/repl.rs`. rustyline added as `[dev-dependencies]` in `tein/Cargo.toml`. no library code changes — purely a consumer of the existing public API.

### context

`Context::new_standard()` — full r7rs environment with imports. no step limits or sandboxing by default.

### core loop

```
startup:
  print banner (tein version, help hint)
  create Context::new_standard()
  create rustyline Editor, load ~/.tein_history

loop:
  prompt "tein> "
  read line
  if meta-command (starts with ',') → dispatch
  else → accumulate into buffer
    if parens balanced → evaluate, print result (skip Unspecified), clear buffer
    if not balanced → continue with "  ... " continuation prompt
  on Ctrl-D → exit gracefully
  on Ctrl-C → clear current buffer, fresh prompt
```

### multi-line input

simple state machine tracking paren depth outside strings and line comments:

- count `(` and `)` while not inside a string or comment
- handle `\"` escapes inside strings
- `;` starts a line comment (skip to EOL)
- if depth > 0 after a line → continue reading
- if depth <= 0 → submit buffer to evaluate

this is intentionally simple (~20 lines). no block comments, no quasiquote awareness — just enough to handle normal multi-line definitions. malformed input gets submitted and scheme reports the syntax error.

### meta-commands

comma-prefixed, following the chibi/guile convention:

| command | action |
|---------|--------|
| `,help` | list available commands |
| `,quit` | exit the REPL |

### output

- results printed via `Value`'s `Display` impl
- `Value::Unspecified` suppressed (e.g. from `define`)
- errors printed to stderr with `"error: "` prefix

### history

`~/.tein_history`, loaded on startup, saved on exit. best-effort — if the file can't be read or written, silently continue.

### error handling

| case | behaviour |
|------|-----------|
| empty / whitespace input | skip, re-prompt |
| eval error | print to stderr, continue |
| `Ctrl-C` | clear current buffer, fresh prompt |
| `Ctrl-D` | save history, exit |
| excess `)` (depth goes ≤ 0) | submit as-is, let scheme report syntax error |
| rustyline fatal error | print and exit |
| history file inaccessible | silently ignore |

## non-goals

- syntax highlighting, bracket matching, tab completion — possible future enhancements
- CLI flags (--sandbox, --step-limit) — would need clap, overkill for an example
- separate binary crate — stays as an example
