# documentation design

> approved 2026-02-24

## context

tein has extensive functionality (sandboxing, foreign types, custom ports, reader
extensions, macro hooks, managed contexts) but no user-facing documentation beyond
a vibes-y README and a stale internal handoff doc. polyglot devs (rust + scheme)
finding the project have no way to learn how to use it.

## decisions

- **audience**: polyglot devs comfortable with both rust and scheme
- **format**: README.md (showcase) + rustdoc (depth). no mdbook.
- **README style**: substance first — quick start and feature highlights up top,
  etymology/philosophy/why-scheme at the bottom
- **DEVELOPMENT.md**: rename to ARCHITECTURE.md (better describes its content)
- **approach**: readme-first — rewrite README, then module rustdoc, then per-item polish

## layer 1 — README.md rewrite

structure:
1. title + tagline (one line)
2. what is tein? (elevator pitch paragraph)
3. quick start (add dep → evaluate → result)
4. features — curated highlights with ~5-line code snippets each:
   - ContextBuilder & sandboxing
   - `#[scheme_fn]` — rust fns in scheme
   - foreign type protocol
   - custom ports
   - reader extensions
   - macro expansion hooks
   - managed contexts (ThreadLocalContext)
   - wall-clock timeouts
5. examples (list with one-line descriptions)
6. status
7. about (etymology, philosophy, why scheme, why chibi) — condensed from current README

## layer 2 — module-level rustdoc

each public module gets:
- purpose (1-2 sentences)
- usage pattern with tested code example
- key types and relationships
- design notes where relevant

modules needing docs:

| module | needs |
|--------|-------|
| `lib.rs` (crate root) | expanded overview, feature list, links to modules |
| `context` | builder pattern, eval, call, sandboxing guide |
| `sandbox` | presets reference, composition, security model |
| `foreign` | full protocol walkthrough, ForeignType impl guide |
| `managed` | persistent vs fresh, init closures, reset |
| `timeout` | usage pattern, relationship to managed |
| `value` | ensure all variants have conversion examples |
| `error` | when each variant occurs |

## layer 3 — per-item rustdoc polish

- fix 8 rustdoc warnings
- add inline examples to key ContextBuilder methods
- verify all public items have doc comments

## layer 4 — housekeeping

- rename DEVELOPMENT.md → ARCHITECTURE.md
- update stale content in ARCHITECTURE.md (test counts, examples list, feature coverage)
- sync AGENTS.md architecture section
