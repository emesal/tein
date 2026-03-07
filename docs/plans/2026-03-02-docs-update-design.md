# docs update — design

**date:** 2026-03-02
**branch:** docs branch (to be created)
**scope:** full docs restructure + README rewrite + ARCHITECTURE.md + ROADMAP.md sync

---

## motivation

M8 (rust ecosystem bridge) is mostly shipped — `(tein json)`, `(tein toml)`, `(tein uuid)`,
`(tein time)`, `#[tein_module]`, `#[tein_const]`, `(tein docs)`, cdylib extensions — but user
docs haven't kept up. README still describes json/uuid as roadmap items. guide.md is missing
entire feature areas. ARCHITECTURE.md and ROADMAP.md have drifted from reality.

goal: a docs structure that is ready to grow, written for the actual audience (rust+r7rs
familiarity, no prior embedding experience required), and that reflects the agent-friendly
design philosophy of tein.

---

## file structure

```
README.md               — pitch + one snippet + links to docs/
docs/
  guide.md              — index/TOC, one paragraph per doc, when to reach for each
  quickstart.md         — Context::new, evaluate, Value, #[tein_fn] — working in 5 min
  embedding.md          — context types, builder API, Value enum, ctx.call(), custom ports
  sandboxing.md         — four-layer model, Modules, FsPolicy, VFS gate, timeout, future hooks
  rust-scheme-bridge.md — #[tein_fn], #[tein_module], ForeignType, reader ext, macro hooks
  modules.md            — (tein json/toml/uuid/time/process/docs) — usage + representation tables
  extensions.md         — tein-ext, #[tein_module(ext=true)], load_extension(), stable C ABI
  tein-for-agents.md    — sandbox as trust boundary, LLM-navigable errors, (tein docs),
                          (tein introspect) forward pointer, design principles
  reference.md          — Value variant table, feature flags, VFS module list,
                          scheme env quirks, r7rs deviations
  plans/                — existing design/impl plans, untouched
```

---

## README

**remove:** roadmap section (lives in ROADMAP.md + github milestones)

**keep:** pitch paragraph, quick start snippet, about blurb, license, examples table

**structure:**
```
# tein
> tagline

one-paragraph pitch (dual identity)

## quick start
[dep + 5-line evaluate example]

## what tein can do today
bullet list of features, no code, links into docs/

## docs
table linking all docs/ files

## examples
existing examples table

## about / license
```

---

## per-doc content scope

### quickstart.md
- `Context::new()` vs `Context::new_standard()`
- evaluate, pattern-match on `Value`
- `#[tein_fn]` + `define_fn_variadic` minimal example
- feature flags (dep snippet with/without defaults)
- pointer to embedding.md for depth

### embedding.md
- context type comparison table (Context / TimeoutContext / ThreadLocalContext)
- `ContextBuilder` API — all builder methods
- `Value` enum — all variants, display format, extraction helpers
- `ctx.call()` — calling scheme procedures from rust
- custom ports — `open_input_port`, `open_output_port`, `read`, `evaluate_port`

### sandboxing.md
- four-layer model overview (module restriction / step limits / FsPolicy / VFS gate)
- `Modules` variants (`Safe`, `All`, `None`, `only(&[...])`)
- `allow_module()` — extending a preset
- UX stubs — what they are, what errors look like
- `TimeoutContext` — wall-clock deadlines
- `FsPolicy` — path prefix matching, canonicalisation, traversal protection
- `Error::SandboxViolation`
- forward section: where the sandbox is heading (host callbacks, interceptable ops)

### rust-scheme-bridge.md
- `#[tein_fn]` standalone + inside module, supported types, Result errors
- `#[tein_module]` full pattern:
  - free fns
  - `#[tein_type]` / `#[tein_methods]`
  - `#[tein_const]`
  - naming conventions (\_q→?, \_bang→!, kebab-case, module prefix rules)
- `ForeignType` trait — manual implementation alternative to `#[tein_module]`
- auto-generated predicates, method procs, introspection fns
- `ctx.foreign_value()`, `ctx.foreign_ref::<T>()`
- reader extensions — `register_reader()` + `(tein reader)` + `set-reader!`
- macro expansion hooks — `set_macro_expand_hook!` + `(tein macro)`

### modules.md
one section per module, each with: import form, exports list, feature flag, representation
table where applicable, usage example

- `(tein json)` — json-parse, json-stringify, representation table (object→alist etc.)
- `(tein toml)` — toml-parse, toml-stringify, datetime tag note
- `(tein uuid)` — make-uuid, uuid?, uuid-nil constant
- `(tein time)` — current-second, current-jiffy, jiffies-per-second, jiffy epoch note
- `(tein process)` — exit, emergency-exit, sandbox caveat (excluded from Modules::Safe)
- `(tein docs)` — doc query API

### extensions.md
- when to use cdylib vs inline `#[tein_module]` (decision guide)
- `tein-ext` crate — what it is, dependency model (never depend on `tein`)
- `#[tein_module("name", ext = true)]` — what the macro generates
- `ctx.load_extension(path)` — host side
- stable C ABI / vtable — why it's stable, API version field
- foreign types in extensions — `ExtTypeEntry`, method dispatch
- caveats: no unload, leaked library handle, linux-only today (issue #66)

### tein-for-agents.md
- why scheme for agent tool environments (homoiconic, sandboxable, composable, minimal)
- tein's sandbox as a trust boundary — the "sandbox = policy" model
- LLM-navigable error messages — design principle, examples
- `(tein docs)` — self-describing module environments
- `(tein introspect)` — planned, forward pointer to issue #83
- design principles tein follows for agent friendliness:
  - single-source names (no hidden aliases)
  - predictable scope (null env, no globals)
  - introspectable foreign types (foreign-types, foreign-methods)
  - composable sandbox layers

### reference.md
- `Value` variant table — scheme type, rust variant, display format
- feature flags table — name, default, description, deps pulled in
- VFS module list — all modules in VFS_REGISTRY with brief description
- scheme env quirks — rewritten user-facing (not "findings from test coverage"):
  - what's available without any import
  - what requires `(import (scheme base))`
  - call/cc re-entry caveat
  - define-values in single-batch evaluate
  - let binding order
- known r7rs deviations — exit/dynamic-wind, link to issue #101

### guide.md (index)
- replaces current walkthrough
- one paragraph per doc explaining what it covers and when to reach for it
- reading order suggestion for different use cases:
  - "i want to embed scheme" → quickstart → embedding → sandboxing
  - "i want to call rust from scheme" → quickstart → rust-scheme-bridge
  - "i'm building an agent harness" → tein-for-agents → sandboxing → modules
  - "i want to extend tein" → extensions

---

## ARCHITECTURE.md updates

- fix completed milestones: M8 partially done (not "current milestone in progress")
- update test count in commands section
- add newer src files: `vfs_registry.rs`, `thread.rs`, `sexp_bridge.rs`
- fix eval.c patch count: 7 patches (A–G), not 4
- add flow descriptions missing from current doc:
  - VFS shadow injection flow
  - FS policy gate flow (C-level)
  - exit escape hatch flow
- note that the detailed user-facing docs live in `docs/`

## ROADMAP.md updates

- move shipped M8 items to completed milestones:
  `(tein json)`, `(tein toml)`, `(tein uuid)`, `(tein time)`, `#[tein_module]`,
  `#[tein_const]`, `(tein docs)`, cdylib extension system, doc attr scraping,
  type parity, feature-gated format modules
- M8 remaining: `(tein regex)`, `(tein crypto)`, cross-platform cdylib (#66),
  SRFI-115 (#85), SRFI-19 via rust (#84), foreign type constructor macro (#41)

---

## what we're not doing

- no rustdoc changes (separate task)
- `docs/plans/` untouched
- `AGENTS.md` untouched
- `(tein introspect)` gets a forward pointer only — not documented as shipped
- scheme env quirks section in reference.md is a rewrite, not a copy of ARCHITECTURE.md
