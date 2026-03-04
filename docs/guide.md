# tein docs

tein is an embeddable R7RS Scheme interpreter for Rust. this is the documentation index.

## running tein from the command line

tein ships a standalone binary for running Scheme scripts and an interactive REPL.

```sh
tein                              # start the REPL
tein script.scm [args...]         # run a script
tein --sandbox script.scm         # sandboxed (safe module set)
tein --sandbox --all-modules ...  # sandboxed (full VFS module set)
```

Scripts can use a shebang line:

```scheme
#!/usr/bin/env tein
(display "hello, world!")
(newline)
```

For sandboxed scripts, pass flags after `tein` via `/usr/bin/env -S`:

```scheme
#!/usr/bin/env -S tein --sandbox
(display "sandboxed!")
(newline)
```

`(exit n)` sets the process exit code. `(command-line)` returns `("tein" "script.scm" ...)`.

## reading order

**"i want to embed scheme in my rust app"**
→ [quickstart](quickstart.md) → [embedding](embedding.md) → [sandboxing](sandboxing.md)

**"i want to expose rust functions and types to scheme"**
→ [quickstart](quickstart.md) → [rust–scheme bridge](rust-scheme-bridge.md)

**"i'm building an agent execution environment"**
→ [tein for agents](tein-for-agents.md) → [sandboxing](sandboxing.md) → [modules](modules.md)

**"i want to write a tein extension module"**
→ [rust–scheme bridge](rust-scheme-bridge.md) → [extensions](extensions.md)

**"i need the full API / value type reference"**
→ [reference](reference.md)

## docs

| doc | what it covers |
|-----|---------------|
| [quickstart](quickstart.md) | `Context::new`, `evaluate`, `Value`, `#[tein_fn]` — working in 5 minutes |
| [embedding](embedding.md) | context types, `ContextBuilder` API, `Value` enum, `ctx.call()`, custom ports |
| [sandboxing](sandboxing.md) | four-layer sandbox model, `Modules`, `FsPolicy`, step limits, timeouts |
| [rust–scheme bridge](rust-scheme-bridge.md) | `#[tein_fn]`, `#[tein_module]`, `ForeignType`, reader extensions, macro hooks |
| [modules](modules.md) | built-in `(tein json/toml/uuid/time/process/file/docs/load)` modules |
| [extensions](extensions.md) | cdylib extension system, `tein-ext`, stable C ABI |
| [tein for agents](tein-for-agents.md) | sandbox as trust boundary, LLM-navigable errors, agent design |
| [reference](reference.md) | `Value` variants, feature flags, VFS module list, scheme env quirks |

## for contributors

[ARCHITECTURE.md](../ARCHITECTURE.md) — internal architecture, data flows, chibi safety invariants.
[AGENTS.md](../AGENTS.md) — coding conventions, workflow, project principles.
[ROADMAP.md](../ROADMAP.md) — milestone plan and github issues.
[docs/plans/](plans/) — design documents and implementation plans.
