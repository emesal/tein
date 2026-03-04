# tein binary — standalone scheme interpreter/REPL

github issue: #42
date: 2026-03-04
status: approved

## motivation

a standalone `tein` binary makes tein usable as a scripting language from the
command line — as a REPL, a script runner, and a shebang interpreter.

## structure

new workspace member `tein-bin` (binary crate, `publish = false`). kept
separate from the `tein` library crate to avoid polluting the library with
binary-only dependencies. the produced binary is named `tein` via
`[[bin]] name = "tein"`.

`rustyline` moves from `tein`'s dev-dependencies to `tein-bin`'s regular
dependencies. arg parsing is manual (`std::env::args()`) — the CLI surface is
small enough that `clap` is not warranted.

## CLI

```
tein                              # REPL, standard env
tein script.scm [args...]         # eval file; args available via (command-line)
tein --sandbox script.scm         # sandboxed, Modules::Safe
tein --sandbox --all-modules ...  # sandboxed, Modules::All
```

`--all-modules` without `--sandbox` is an error (print to stderr, exit 2).

## mode dispatch

- no positional arg → REPL mode
- positional arg → script mode; remaining args after the filename are passed
  to `(command-line)` as `["tein", "script.scm", ...user_args]`

## shebang support

before handing file contents to the evaluator, check if the first two bytes
are `#!`. if so, seek past the first `\n` and feed only the remainder to
`ctx.evaluate()`. no scheme-level involvement — stripping happens in rust
before the reader sees the input.

typical usage in scripts:

```scheme
#!/usr/bin/env tein
#!/usr/bin/env -S tein --sandbox
#!/usr/bin/env -S tein --sandbox --all-modules
```

## Value::Exit variant

`(exit)` currently returns `Ok(Value)` indistinguishable from a normal return.
a new `Value::Exit(i32)` variant is added so both the binary and embedders can
detect and act on an exit signal.

r7rs semantics baked in at construction:
- `(exit)` or `(exit #t)` → `Value::Exit(0)`
- `(exit #f)` → `Value::Exit(1)`
- `(exit n)` (integer) → `Value::Exit(n)`
- `(exit other)` → `Value::Exit(0)`

the binary pattern-matches on `Value::Exit(n)` and calls `process::exit(n)`.
embedders who don't care can treat it like any other variant.

## exit codes

| situation | code |
|---|---|
| normal exit / REPL quit | 0 |
| `(exit n)` | n |
| scheme error in script mode | 1 |
| `--all-modules` without `--sandbox` | 2 |

## REPL

lifted from `examples/repl.rs` with minor adjustments:

- `paren_depth()` and readline loop move verbatim into `tein-bin`
- `Value::Exit(n)` → exit immediately and silently with code `n`
- `Value::Unspecified` → suppressed (no output for `define` etc)
- history file: `~/.tein_history`
- banner: `tein {version} — r7rs scheme` + `,help` hint
- scheme errors → print to stderr, continue

## testing

- unit tests in `tein-bin/src/main.rs`: arg parsing (flag combos, error
  cases, file + extra args), shebang stripping
- `Value::Exit` tests in `tein/src/context.rs`: verify `(exit)`, `(exit 0)`,
  `(exit 1)`, `(exit #t)`, `(exit #f)`, `(exit "str")` return correct variant
- no subprocess integration tests for now
