# GitHub Ecosystem Sync Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Bring the tein github issue ecosystem in sync with ROADMAP.md — close completed milestones/issues, migrate orphaned issues, create M6–M12 milestones, and add tracking issues for all roadmap items.

**Architecture:** Pure gh CLI operations. No code changes. Work sequentially to avoid milestone number confusion (github auto-assigns numbers). Verify each milestone number after creation before assigning issues to it.

**Tech Stack:** `gh` CLI, github issues/milestones API.

---

## Preamble: how to get a milestone number

After `gh api repos/emesal/tein/milestones -X POST ...`, capture the `.number` field from the response. Every task below that assigns issues to a milestone does this explicitly.

---

### Task 1: Close completed milestones and issues

**Step 1: Close M4 and M5 milestones**

```bash
# Get milestone numbers for M4 and M5
gh api repos/emesal/tein/milestones --jq '.[] | {number, title, state}' | grep -A1 "Milestone [45]"
```

Expected: M4 is number 4, M5 is number 5 (verify before proceeding).

```bash
gh api repos/emesal/tein/milestones/4 -X PATCH -f state=closed
gh api repos/emesal/tein/milestones/5 -X PATCH -f state=closed
```

**Step 2: Close issue #16 (macro expansion hooks — done in M5)**

```bash
gh issue close 16 --comment "completed in milestone 5 — macro expansion hooks shipped in M5 with \`(tein macro)\` VFS module, \`set-macro-expand-hook!\`, and full rust API. see ROADMAP.md milestone 5."
```

**Step 3: Close issue #17 (custom reader extensions — done in M5)**

```bash
gh issue close 17 --comment "completed in milestone 5 — custom reader extensions shipped in M5 with \`(tein reader)\` VFS module, \`set-reader!\`, and full rust API. see ROADMAP.md milestone 5."
```

**Verify:**
```bash
gh issue list --state closed --limit 10 --json number,title,state
```
Expected: #16 and #17 appear as closed.

---

### Task 2: Create milestones M6 and M7 (already completed — create as closed)

**Step 1: Create M6**

```bash
M6=$(gh api repos/emesal/tein/milestones -X POST \
  -f title="Milestone 6 — Foreign Type Protocol" \
  -f description="Expose arbitrary rust types to scheme via a safe handle-map protocol. ForeignType trait, ForeignStore, Value::Foreign, (tein foreign) VFS module, and auto-generated convenience procs." \
  -f state=closed \
  --jq '.number')
echo "M6 milestone number: $M6"
```

**Step 2: Create M7**

```bash
M7=$(gh api repos/emesal/tein/milestones -X POST \
  -f title="Milestone 7 — Managed Contexts" \
  -f description="ThreadLocalContext: Send+Sync managed context on a dedicated thread. Persistent mode (state accumulates) and fresh mode (rebuilt per call). Init closure, reset(), shared channel protocol." \
  -f state=closed \
  --jq '.number')
echo "M7 milestone number: $M7"
```

**Verify:**
```bash
gh api repos/emesal/tein/milestones?state=closed --jq '.[] | {number, title, state}'
```
Expected: M6 and M7 present and closed alongside M1–M3.

---

### Task 3: Create milestones M8–M12 and Unscheduled (open)

Run each and capture the number — you'll need them for issue assignment in later tasks.

**Step 1: M8**
```bash
M8=$(gh api repos/emesal/tein/milestones -X POST \
  -f title="Milestone 8 — Rust Ecosystem Bridge" \
  -f description="Expose high-value rust crates as idiomatic r7rs scheme modules: (tein json), (tein regex), (tein crypto), (tein uuid). #[tein_module] proc macro auto-generates scheme glue from annotated rust." \
  --jq '.number')
echo "M8: $M8"
```

**Step 2: M9**
```bash
M9=$(gh api repos/emesal/tein/milestones -X POST \
  -f title="Milestone 9 — Tein as a Scheme" \
  -f description="Tein as a first-class scheme implementation: standalone tein binary, snow-fort package support, (tein wisp) syntax module, r5rs/r6rs compat layers, and a scheme-level test harness." \
  --jq '.number')
echo "M9: $M9"
```

**Step 3: M10**
```bash
M10=$(gh api repos/emesal/tein/milestones -X POST \
  -f title="Milestone 10 — Capability Modules" \
  -f description="More rust-backed scheme modules building on the #[tein_module] infrastructure from M8: (tein http), (tein datetime), (tein tracing)." \
  --jq '.number')
echo "M10: $M10"
```

**Step 4: M11**
```bash
M11=$(gh api repos/emesal/tein/milestones -X POST \
  -f title="Milestone 11 — Performance & Throughput" \
  -f description="Context pool for high-throughput workloads. WASM target via emscripten. Compile-to-C pipeline via chibi's compile-to-c for near-native scheme speed." \
  --jq '.number')
echo "M11: $M11"
```

**Step 5: M12**
```bash
M12=$(gh api repos/emesal/tein/milestones -X POST \
  -f title="Milestone 12 — Stochastic Runtime Support" \
  -f description="Tein as a platform for a stochastic programming language. (tein rat) model ensemble library wrapping ratatoskr's ModelGateway. Stochastic core library: define~, intent, narrow, project, monad, with-context." \
  --jq '.number')
echo "M12: $M12"
```

**Step 6: Unscheduled**
```bash
UNSCHED=$(gh api repos/emesal/tein/milestones -X POST \
  -f title="Unscheduled" \
  -f description="Good ideas not yet assigned to a milestone. These are on the roadmap but timing is undefined." \
  --jq '.number')
echo "Unscheduled: $UNSCHED"
```

**Verify:**
```bash
gh api repos/emesal/tein/milestones?state=open --jq '.[] | {number, title}'
```
Expected: M8–M12 + Unscheduled all present.

---

### Task 4: Update orphaned issues to new milestones

You need the milestone numbers from task 3. Run `gh api repos/emesal/tein/milestones?state=open --jq '.[] | {number,title}'` to recover them if the shell session was reset.

**Step 1: #15 WASM target → M11**
```bash
# Get M11 number
M11=$(gh api repos/emesal/tein/milestones?state=open --jq '.[] | select(.title | contains("M11")) | .number')
gh api repos/emesal/tein/issues/15 -X PATCH -F milestone=$M11
gh issue edit 15 --body "## context

WASM target was originally scoped in M5 but deprioritised. now explicitly scheduled in M11 (performance & throughput).

chibi-scheme compiles via emscripten. the goal is enabling tein in browser and edge environments.

## scope

- confirm emscripten build path for chibi-scheme
- build.rs integration for WASM target detection
- CI target: \`wasm32-unknown-emscripten\` or \`wasm32-wasi\`
- document limitations (no threading, no dlopen — already disabled)"
```

**Step 2: #27 VFS-embedded docs → M9**
```bash
M9=$(gh api repos/emesal/tein/milestones?state=open --jq '.[] | select(.title | contains("M9")) | .number')
gh api repos/emesal/tein/issues/27 -X PATCH -F milestone=$M9
```

**Step 3: #29 wall-clock timeout → Unscheduled**
```bash
UNSCHED=$(gh api repos/emesal/tein/milestones?state=open --jq '.[] | select(.title == "Unscheduled") | .number')
gh api repos/emesal/tein/issues/29 -X PATCH -F milestone=$UNSCHED
```

**Step 4: #31 VFS re-export bug → M9, add bug label**
```bash
M9=$(gh api repos/emesal/tein/milestones?state=open --jq '.[] | select(.title | contains("M9")) | .number')
gh api repos/emesal/tein/issues/31 -X PATCH -F milestone=$M9
gh issue edit 31 --add-label "bug"
```

**Step 5: #32 scheme test harness → M9**
```bash
gh api repos/emesal/tein/issues/32 -X PATCH -F milestone=$M9
```

**Verify:**
```bash
gh issue list --state open --json number,title,milestone --jq '.[] | {number, title, milestone: .milestone.title}'
```
Expected: #15→M11, #27→M9, #29→Unscheduled, #31→M9, #32→M9.

---

### Task 5: Create M8 tracking issues

Get M8 number: `M8=$(gh api repos/emesal/tein/milestones?state=open --jq '.[] | select(.title | contains("M8")) | .number')`

```bash
gh issue create --milestone $M8 --label "enhancement" \
  --title "feat: \`(tein json)\` — JSON via serde_json" \
  --body "## summary

bidirectional JSON ↔ scheme value conversion via serde_json.

## scope

- \`ctx.evaluate(\"(json-parse \\\"...\\\")\") → Value\`
- \`ctx.evaluate(\"(json-stringify val)\") → String\`
- leverages existing serde foundation in tein-sexp
- VFS module \`(tein json)\` exposing: \`json-parse\`, \`json-stringify\`, \`json?\`
- round-trip tests for all value types

## depends on

M8 \`#[tein_module]\` proc macro (can be developed in parallel, module macro simplifies implementation)"

gh issue create --milestone $M8 --label "enhancement" \
  --title "feat: \`(tein regex)\` — regex via the \`regex\` crate" \
  --body "## summary

regular expression support in scheme via rust's \`regex\` crate.

## scope

- \`make-regex\`, \`regex?\`, \`regex-is-match\`, \`regex-find\`, \`regex-find-all\`, \`regex-replace\`, \`regex-replace-all\`
- VFS module \`(tein regex)\`
- Regex as a ForeignType handle
- error on invalid pattern → scheme error

## depends on

M8 \`#[tein_module]\` proc macro preferred; can use ForeignType protocol directly as fallback"

gh issue create --milestone $M8 --label "enhancement" \
  --title "feat: \`(tein crypto)\` — hashing and CSPRNG" \
  --body "## summary

cryptographic primitives via blake3, sha2, and rand crates.

## scope

- \`blake3-hash\`, \`sha256-hash\`, \`sha512-hash\` — bytevector → hex string
- \`random-bytes\`, \`random-integer\`, \`random-float\` — CSPRNG via rand
- VFS module \`(tein crypto)\`
- input accepts both strings (UTF-8 encoded) and bytevectors"

gh issue create --milestone $M8 --label "enhancement" \
  --title "feat: \`(tein uuid)\` — UUID generation" \
  --body "## summary

UUID generation via the \`uuid\` crate.

## scope

- \`make-uuid\` → UUID v4 string
- \`uuid?\` predicate
- \`uuid-nil\` constant
- VFS module \`(tein uuid)\`"

gh issue create --milestone $M8 --label "enhancement" \
  --title "feat: \`#[tein_module]\` proc macro for rust→scheme module generation" \
  --body "## summary

proc macro that auto-generates scheme-side glue (VFS module, predicates, constructors, method procs) from annotated rust.

## motivation

adding each new tein module currently requires writing repetitive boilerplate: ForeignType impl, register_foreign_type calls, VFS .sld + .scm files, individual scheme bindings. the proc macro collapses this into annotated rust.

## rough sketch (exact design tbd in planning)

\`\`\`rust
#[tein_module(\"regex\")]
mod regex_module {
    #[tein_fn] fn compile(pattern: &str) -> Result<Regex, Error> { ... }
    #[tein_type] impl Regex {
        #[tein_method] fn is_match(&self, text: &str) -> bool { ... }
    }
}
// → generates (tein regex) VFS module with make-regex, regex?, regex-is-match
\`\`\`

## scope

- proc macro crate (\`tein-macros\` or inline in \`tein\`)
- \`#[tein_module]\`, \`#[tein_type]\`, \`#[tein_fn]\`, \`#[tein_method]\` attributes
- auto-generates: ForeignType impl, VFS .sld + .scm, register calls
- replaces existing \`#[scheme_fn]\` where applicable or composes with it"

gh issue create --milestone $M8 --label "enhancement" \
  --title "feat: foreign type constructor macro" \
  --body "## summary

ergonomic rust-side macro for registering simple foreign types, for cases where the full \`#[tein_module]\` proc macro (see sibling issue) is overkill.

## motivation

\`register_foreign_type::<T>()\` + manual method listing is verbose for simple types. a declarative macro like \`define_foreign_type!\` would cover the common case cleanly.

## scope

- \`define_foreign_type!(TypeName, [method1, method2, ...])\` declarative macro
- complements \`#[tein_module]\` — different tradeoff (less magic, more explicit)
- may be superseded by \`#[tein_module]\` if that covers all cases cleanly; evaluate during M8 planning"
```

**Verify:**
```bash
gh issue list --milestone "Milestone 8*" --json number,title | jq '.'
```
Expected: 6 new issues for M8.

---

### Task 6: Create M9 tracking issues

Get M9 number: `M9=$(gh api repos/emesal/tein/milestones?state=open --jq '.[] | select(.title | contains("M9")) | .number')`

```bash
gh issue create --milestone $M9 --label "enhancement" \
  --title "feat: \`tein\` binary — standalone scheme interpreter/REPL" \
  --body "## summary

a standalone \`tein\` binary making tein a usable scheme implementation from the command line.

## open design question

one binary or several? chicken provides three (\`chicken\`, \`csc\`, \`csi\`). tein is rust-rooted which changes the tradeoffs. to be decided during M9 planning.

## scope (minimum)

- \`tein\` binary crate in workspace
- REPL mode (adapts the existing REPL example from M5)
- file evaluation mode: \`tein script.scm\`
- \`--sandbox\` flag for preset-based restriction
- integrate standard env + VFS modules

## notes

the custom reader extension (M5) already provides the hook for future file-format extensions (e.g. \`.ssp\` stochastic scheme programs)."

gh issue create --milestone $M9 --label "enhancement" \
  --title "feat: snow-fort package support" \
  --body "## summary

two-tier snow-fort integration:

1. **vetted VFS packages** — curated snow-fort libraries embedded in the VFS at compile time by \`build.rs\`. same trust level as existing VFS modules. available in sandboxed contexts.

2. **snow capability** — unvetted snow packages as a \`ContextBuilder\` capability (\`.snow_packages(&[\"srfi/180\"])\`). follows the same pattern as \`file_read\`/\`file_write\`. available only in contexts that explicitly grant it. fetched and embedded at compile time, not at runtime.

## scope

- \`build.rs\` fetching and embedding of curated packages
- \`ContextBuilder::snow_packages(&[...])\` builder API
- module policy enforcement (same VFS-only gate as existing modules)
- documentation of the vetted package list"

gh issue create --milestone $M9 --label "enhancement" \
  --title "feat: \`(tein wisp)\` — wisp syntax (SRFI-119) as a VFS module" \
  --body "## summary

wisp syntax support via a pure-scheme R7RS port of the wisp preprocessor, vendored into the VFS. transforms indentation-based wisp source into s-expressions before evaluation — no second VM, full scheme semantics underneath.

## motivation

wisp serves as the first 'alternate surface syntax' stepping stone, and as a foundation for more distinct scripting languages (e.g. stochastic language surface syntax) down the road.

## scope

- \`wisp-read\` / \`wisp-eval\` / \`wisp-load\` as entry points
- VFS module \`(tein wisp)\`
- pure-scheme implementation (port of existing wisp preprocessor)
- integration with custom reader extension (M5) for \`.wisp\` file loading

## depends on

\`(tein regex)\` (M8) — wisp preprocessing requires regex"

gh issue create --milestone $M9 --label "enhancement" \
  --title "feat: r5rs/r6rs compatibility layers" \
  --body "## summary

\`ContextBuilder::r5rs_env()\` and \`ContextBuilder::r6rs_env()\` for best-effort compatibility with older scheme code.

## approach

chibi already has substantial r6rs support internally. the goal is exposing it properly rather than implementing it from scratch. documented as best-effort, not full conformance.

## motivation

expands the pool of available scheme code that can run in tein without modification. enables porting existing r5rs/r6rs libraries."
```

**Verify:**
```bash
gh issue list --milestone "Milestone 9*" --json number,title | jq '.'
```
Expected: 4 new issues + existing #27, #31, #32 = 7 total on M9.

---

### Task 7: Create M10 tracking issues

Get M10 number: `M10=$(gh api repos/emesal/tein/milestones?state=open --jq '.[] | select(.title | contains("M10")) | .number')`

```bash
gh issue create --milestone $M10 --label "enhancement" \
  --title "feat: \`(tein http)\` — HTTP client" \
  --body "## summary

HTTP client support in scheme via \`ureq\` or \`reqwest\`.

## scope

- \`http-get\`, \`http-post\`, \`http-request\` (full control)
- headers, query params, body as scheme values
- response: status, headers, body as bytevector or string
- VFS module \`(tein http)\`
- timeout integration with fuel/wall-clock limits

## depends on

\`#[tein_module]\` proc macro (M8)"

gh issue create --milestone $M10 --label "enhancement" \
  --title "feat: \`(tein datetime)\` — date/time via chrono" \
  --body "## summary

date/time support via the \`chrono\` crate. better timezone and formatting than SRFI-19.

## scope

- datetime construction, parsing, formatting
- arithmetic (add duration, diff, compare)
- timezone-aware operations
- VFS module \`(tein datetime)\`
- SRFI-19 compatibility where sensible

## depends on

\`#[tein_module]\` proc macro (M8)"

gh issue create --milestone $M10 --label "enhancement" \
  --title "feat: \`(tein tracing)\` — structured logging into rust's tracing ecosystem" \
  --body "## summary

structured logging from scheme into rust's \`tracing\` ecosystem. scheme code generates structured spans that rust can consume.

## scope

- \`trace!\`, \`debug!\`, \`info!\`, \`warn!\`, \`error!\` scheme procs
- span creation and context propagation
- field values as scheme values → tracing field values
- VFS module \`(tein tracing)\`
- no-op when tracing subscriber not configured (zero cost)

## depends on

\`#[tein_module]\` proc macro (M8)"
```

**Verify:**
```bash
gh issue list --milestone "Milestone 10*" --json number,title | jq '.'
```
Expected: 3 issues.

---

### Task 8: Create M11 tracking issues

Get M11 number: `M11=$(gh api repos/emesal/tein/milestones?state=open --jq '.[] | select(.title | contains("M11")) | .number')`

```bash
gh issue create --milestone $M11 --label "enhancement" \
  --title "feat: context pool for high-throughput workloads" \
  --body "## summary

pool of \`ThreadLocalContext\` instances for workloads that evaluate many scheme expressions in parallel — tool execution, stochastic program dispatch, etc.

## scope

- \`ContextPool\` struct wrapping N \`ThreadLocalContext\` instances
- \`pool.evaluate(expr)\` dispatches to an idle context
- configurable pool size, builder passed to all instances
- backpressure handling when all contexts are busy
- metrics: utilisation, queue depth

## depends on

M7 \`ThreadLocalContext\` (done)"

gh issue create --milestone $M11 --label "enhancement" \
  --title "feat: compile-to-C pipeline via chibi's \`compile-to-c\`" \
  --body "## summary

expose chibi's \`compile-to-c\` so scheme files can be compiled to C and linked into rust binaries at build time via \`build.rs\`. near-native scheme speed.

## motivation

some hot paths in chibi~ tool execution are scheme code. compiling them to C eliminates interpreter overhead.

## scope

- \`build.rs\` integration: \`tein::compile_scheme_to_c(\"path/to/file.scm\")\`
- compiled C linked as part of the rust binary
- scheme module compatibility (VFS modules compile correctly)
- documentation of limitations

## notes

complex to drive programmatically — needs careful design during M11 planning."
```

**Verify:**
```bash
gh issue list --milestone "Milestone 11*" --json number,title | jq '.'
```
Expected: 2 new issues + existing #15 WASM = 3 total on M11.

---

### Task 9: Create M12 tracking issues

Get M12 number: `M12=$(gh api repos/emesal/tein/milestones?state=open --jq '.[] | select(.title | contains("M12")) | .number')`

```bash
gh issue create --milestone $M12 --label "enhancement" \
  --title "feat: \`(tein rat)\` — model ensemble library" \
  --body "## summary

scheme module wrapping ratatoskr's \`ModelGateway\`: chat, generate, embed, NLI, and token counting. model access for any scheme program, not just the stochastic language.

## scope

- \`rat-chat\`, \`rat-generate\`, \`rat-embed\`, \`rat-nli\`, \`rat-count-tokens\`
- model selection as a parameter (use r7rs \`only\`/\`prefix\` import forms for granularity)
- async-aware: integrates with \`ThreadLocalContext\` channel pattern
- VFS module \`(tein rat)\`
- ModelGateway as a ForeignType handle

## depends on

ratatoskr's ModelGateway API stabilising. \`#[tein_module]\` proc macro (M8) preferred."

gh issue create --milestone $M12 --label "enhancement" \
  --title "feat: stochastic core library" \
  --body "## summary

the stochastic programming language delivered as a tein module. scheme macros and procedures implementing the stochastic runtime on top of tein's primitives.

## scope

- \`define~\`, \`intent\`, \`narrow\`, \`project\`, \`monad\`, \`with-context\`, \`register-projection\`
- deterministic compilation passes as macro transformations on the stochastic IR
- projection registry as a ForeignType
- VFS module \`(tein stochastic)\` or similar

## primitives already available in tein

- continuations as residuals (chibi first-class continuations)
- macro expansion hook (M5) — compilation passes as macro transformations
- foreign type protocol (M6) — model handles and projection registry
- sandboxing (M4) — deterministic compilation phase runs isolated

## depends on

\`(tein rat)\` (M12), \`#[tein_module]\` proc macro (M8)

## notes

full design in \`~/projects/chibi/backrooms/stochastic-programming.md\`"
```

**Verify:**
```bash
gh issue list --milestone "Milestone 12*" --json number,title | jq '.'
```
Expected: 2 issues.

---

### Task 10: Create Unscheduled tracking issues

Get Unscheduled number: `UNSCHED=$(gh api repos/emesal/tein/milestones?state=open --jq '.[] | select(.title == "Unscheduled") | .number')`

```bash
gh issue create --milestone $UNSCHED --label "enhancement" \
  --title "feat: \`build_managed\` with timeout — combined ThreadLocalContext + wall-clock deadline" \
  --body "## summary

combine \`ThreadLocalContext\` + wall-clock deadline in a single builder call, without needing two separate threads (the current workaround is layering \`TimeoutContext\` over the result).

## proposal

\`ContextBuilder::build_managed_with_timeout(init, duration)\` — or a \`timeout(Duration)\` option on the managed builder — using \`recv_timeout\` on the channel.

## notes

step_limit is always required regardless (guarantees thread termination). timeout is purely additive."

gh issue create --milestone $UNSCHED --label "enhancement" \
  --title "feat: hash table API — rich rust methods on \`Value::HashTable\`" \
  --body "## summary

\`Value::HashTable\` is currently opaque — no rust-side API to inspect or manipulate hash tables. this adds rich methods.

## scope

- \`Value::HashTable\` exposes get/set/delete/keys/values/len from rust
- bidirectional conversion with \`HashMap<Value, Value>\`
- tests covering all hash table operations from rust"

gh issue create --milestone $UNSCHED --label "enhancement" \
  --title "feat: continuation API — first-class scheme continuations from rust" \
  --body "## summary

expose chibi's first-class continuations to the rust API. capture, store, and invoke continuations from rust code.

## motivation

continuations are the primitive underlying the stochastic language's residual nodes. direct rust access enables richer M12 patterns.

## notes

complex interaction with GC and thread safety — needs careful design."

gh issue create --milestone $UNSCHED --label "enhancement" \
  --title "feat: \`(tein chibi)\` — scheme module for chibi~'s tool/plugin protocol" \
  --body "## summary

scheme module that speaks chibi~'s tool/plugin protocol: call chibi~'s tools from tein programs, hook into the plugin architecture.

## notes

depends on chibi~'s protocol stabilising — not a tein prerequisite. unscheduled until chibi~ protocol is stable."
```

**Verify:**
```bash
gh issue list --milestone "Unscheduled" --json number,title | jq '.'
```
Expected: 4 new issues + existing #29 wall-clock timeout = 5 total on Unscheduled.

---

### Task 11: Final verification

**Step 1: Full milestone overview**
```bash
gh api repos/emesal/tein/milestones?state=all --jq '.[] | {number, title, state, open_issues, closed_issues}' | sort
```

Expected:
- M1–M3: closed, 0 open
- M4–M5: closed (just closed)
- M6–M7: closed, 0 open
- M8: open, 6 issues
- M9: open, 7 issues (4 new + #27, #31, #32)
- M10: open, 3 issues
- M11: open, 3 issues (2 new + #15)
- M12: open, 2 issues
- Unscheduled: open, 5 issues (4 new + #29)

**Step 2: Confirm no orphaned open issues**
```bash
gh issue list --state open --json number,title,milestone --jq '.[] | select(.milestone == null) | {number, title}'
```
Expected: empty — all open issues have a milestone.

**Step 3: Confirm #16 and #17 are closed**
```bash
gh issue list --state closed --json number,title,state --jq '.[] | select(.number == 16 or .number == 17)'
```
Expected: both closed.
