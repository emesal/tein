# documentation implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** give tein proper user-facing documentation — a curated README and thorough rustdoc.

**Architecture:** README.md as the showcase (substance first, poetry at bottom), rustdoc module
docs for depth, per-item polish for completeness. DEVELOPMENT.md renamed to ARCHITECTURE.md.

**Tech Stack:** markdown, rustdoc, `cargo doc`, `cargo test --doc`

---

### task 1: README.md rewrite

**Files:**
- Modify: `README.md`

**Step 1: rewrite README.md**

replace the current vibes-only README with a substance-first version. structure:

1. title + one-line tagline
2. "what is tein?" — elevator pitch paragraph
3. quick start — add dep, create context, evaluate
4. features — curated highlights with ~5-line snippets:
   - sandboxing & resource limits (ContextBuilder + presets + fuel + timeout)
   - `#[scheme_fn]` proc macro
   - foreign type protocol
   - custom ports
   - reader extensions
   - macro expansion hooks
   - managed contexts
5. examples — list all 8 with one-line descriptions
6. about — condensed etymology, philosophy, why scheme, why chibi

each feature highlight should be a real, working code snippet (will be validated
by reading the examples and tests for accuracy). link to rustdoc modules for depth.

use existing examples as the source for code snippets — don't invent new ones.

**Step 2: verify README renders correctly**

eyeball the markdown structure for correctness.

**Step 3: commit**

```bash
git add README.md
git commit -m "docs: rewrite README with substance-first structure"
```

---

### task 2: fix rustdoc warnings

**Files:**
- Modify: `tein/src/managed.rs`
- Modify: `tein/src/timeout.rs`
- Modify: `tein/src/value.rs`
- Modify: `tein/src/sandbox.rs`

there are 7 rustdoc warnings, all link resolution issues:

1. `managed.rs:3` — `[`Context`]` → `[`crate::Context`]`
2. `sandbox.rs:7` — links to private `FsPolicy` → use backtick-only (no link)
3. `timeout.rs:21` — `[`Context`]` → `[`crate::Context`]`
4. `value.rs:383` — `[`Context::define_fn_variadic`]` → `[`crate::Context::define_fn_variadic`]`
5. `value.rs:573` — `[`Context::call`]` → `[`crate::Context::call`]`
6. `value.rs:599` — `[`Context::call`]` → `[`crate::Context::call`]`
7. `value.rs:609` — `[`Context::call`]` → `[`crate::Context::call`]`

**Step 1: fix all 7 warnings**

apply the fixes listed above.

**Step 2: verify clean rustdoc build**

```bash
cd /home/fey/projects/tein/tein-dev && cargo doc 2>&1 | grep -c warning
```

expected: 0 (or just the summary line)

**Step 3: commit**

```bash
git add tein/src/managed.rs tein/src/timeout.rs tein/src/value.rs tein/src/sandbox.rs
git commit -m "docs: fix 7 rustdoc link resolution warnings"
```

---

### task 3: crate-level rustdoc (lib.rs)

**Files:**
- Modify: `tein/src/lib.rs`

**Step 1: expand crate-level doc comment**

the current doc is just a quick start. expand to include:
- brief description (what tein is, what it's built on)
- quick start (keep existing, maybe expand slightly)
- feature overview with links to modules:
  - sandboxing → [`sandbox`] module + [`ContextBuilder`]
  - foreign types → [`foreign`] module
  - managed contexts → [`managed`] module
  - timeouts → [`TimeoutContext`]
  - `#[scheme_fn]` macro
- note about safety model (!Send + !Sync, why)

keep it concise — this is a navigation hub, not a tutorial.

**Step 2: verify doc-tests pass**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test --doc
```

**Step 3: commit**

```bash
git add tein/src/lib.rs
git commit -m "docs: expand crate-level rustdoc with feature overview"
```

---

### task 4: context module rustdoc

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: expand module-level doc comment**

current doc is just `//! scheme evaluation context`. expand to cover:
- what Context is (single-threaded scheme evaluation environment)
- ContextBuilder pattern — how to configure and build
- evaluation: `evaluate()`, `call()`, `load_file()`
- sandboxing summary: presets, `.pure_computation()`, `.safe()`, `.allow()`
- IO policy: `.file_read()`, `.file_write()`
- link to [`sandbox`] for preset details
- link to [`foreign`] for foreign type registration
- code example showing builder → evaluate → extract

**Step 2: add/improve doc comments on key ContextBuilder methods**

ensure each builder method has a clear doc comment explaining:
- what it does
- when you'd use it
- any gotchas

focus on: `preset()`, `allow()`, `pure_computation()`, `safe()`,
`file_read()`, `file_write()`, `standard_env()`, `step_limit()`.

**Step 3: verify doc-tests pass**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test --doc
```

**Step 4: commit**

```bash
git add tein/src/context.rs
git commit -m "docs: context module rustdoc — builder, eval, sandboxing"
```

---

### task 5: sandbox module rustdoc

**Files:**
- Modify: `tein/src/sandbox.rs`

**Step 1: expand module-level doc comment**

current doc is decent but could use:
- a quick usage example (builder with presets)
- reference table of all 16 presets with what each contains
- composition guide — which presets work well together
- security model summary (4 independent layers)
- note on `ModulePolicy` (automatic VfsOnly when standard_env + presets)

**Step 2: add doc comments to each preset constant**

each `pub const PRESET: Preset` should document what primitives it includes
and when you'd use it.

**Step 3: verify doc-tests pass**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test --doc
```

**Step 4: commit**

```bash
git add tein/src/sandbox.rs
git commit -m "docs: sandbox module — presets reference, security model"
```

---

### task 6: foreign module rustdoc

**Files:**
- Modify: `tein/src/foreign.rs`

**Step 1: expand module-level doc comment**

the current doc is good but add:
- complete usage example (define type, register, create value, call from scheme)
- dispatch chain explanation
- MethodContext docs
- note about FOREIGN_STORE_PTR lifecycle

**Step 2: improve ForeignType trait docs**

ensure `type_name()` and `methods()` have clear doc comments with guidance
on naming conventions (kebab-case for type_name, etc.).

**Step 3: verify doc-tests pass**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test --doc
```

**Step 4: commit**

```bash
git add tein/src/foreign.rs
git commit -m "docs: foreign module — protocol walkthrough, impl guide"
```

---

### task 7: remaining module rustdoc (managed, timeout, value, error)

**Files:**
- Modify: `tein/src/managed.rs`
- Modify: `tein/src/timeout.rs`
- Modify: `tein/src/value.rs`
- Modify: `tein/src/error.rs`

**Step 1: managed.rs module doc**

expand to include:
- persistent vs fresh mode comparison
- init closure semantics (when it runs)
- `reset()` behaviour
- code example showing basic usage

**Step 2: timeout.rs module doc**

expand to include:
- when to use TimeoutContext vs ThreadLocalContext
- code example

**Step 3: value.rs module doc**

expand to include:
- overview of all variants
- conversion patterns (from_raw / to_raw)
- extraction helpers pattern
- note on type check ordering (float before int)

**Step 4: error.rs module doc**

expand to include:
- when each Error variant is produced
- example of error handling pattern

**Step 5: verify doc-tests pass**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test --doc
```

**Step 6: commit**

```bash
git add tein/src/managed.rs tein/src/timeout.rs tein/src/value.rs tein/src/error.rs
git commit -m "docs: managed, timeout, value, error module rustdoc"
```

---

### task 8: housekeeping — DEVELOPMENT.md → ARCHITECTURE.md

**Files:**
- Rename: `DEVELOPMENT.md` → `ARCHITECTURE.md`
- Modify: `ARCHITECTURE.md` (update stale content)
- Modify: `AGENTS.md` (update reference if any)

**Step 1: rename the file**

```bash
cd /home/fey/projects/tein/tein-dev && git mv DEVELOPMENT.md ARCHITECTURE.md
```

**Step 2: update stale content in ARCHITECTURE.md**

- update test count (currently says 165, should reflect actual)
- update examples list (currently missing repl, foreign_types, managed)
- add reader dispatch protocol section if missing
- add macro expansion hooks section if missing
- verify architecture diagram matches current code

**Step 3: update any references**

check AGENTS.md, CLAUDE.md, and any other files that reference DEVELOPMENT.md
and update them to ARCHITECTURE.md.

**Step 4: commit**

```bash
git add ARCHITECTURE.md AGENTS.md CLAUDE.md
git commit -m "docs: rename DEVELOPMENT.md to ARCHITECTURE.md, update stale content"
```

---

### task 9: final verification

**Step 1: full doc build**

```bash
cd /home/fey/projects/tein/tein-dev && cargo doc 2>&1 | grep warning
```

expected: no warnings

**Step 2: doc-tests**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test --doc
```

expected: all pass

**Step 3: full test suite**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test
```

expected: all pass, nothing broken
