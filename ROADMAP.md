# tein roadmap

## what is tein?

tein is an embeddable r7rs scheme interpreter for rust, built on vendored chibi-scheme 0.11. it has a dual identity:

- **scheme embedded in rust** — rust applications add scheme as a scripting or extension language, with safe sandboxing, resource limits, and bidirectional rust↔scheme data exchange
- **scheme with rust inside** — scheme programs get access to the rust ecosystem via tein's module system: high-performance crates exposed as idiomatic r7rs libraries

long-term, tein aims to be a capable scheme implementation in its own right — one that just happens to be exceptionally easy to embed in rust, in the same spirit that chibi-scheme is easy to embed in C.

tein is a core dependency of chibi~ (an LLM agent harness). tein provides the sandboxed execution environment for LLM-synthesised tools, with the sandbox boundary serving as chibi~'s trust model. this makes production hardening and the rust ecosystem bridge high priorities.

---

## completed milestones

### milestone 1 — ergonomics & round-trip

- [x] typed extraction helpers on `Value`
- [x] complete bidirectional value bridge
- [x] multi-expression evaluation, file loading

### milestone 2 — scheme as extension language

- [x] scheme→rust callbacks (procedures as values)
- [x] variadic foreign functions (`define_fn_variadic`)
- [x] `#[scheme_fn]` proc macro for ergonomic FFI

### milestone 3 — tein-sexp pure rust s-expression crate

- [x] `Sexp` AST with source spans
- [x] r7rs-compatible reader (lists, pairs, vectors, strings, chars, comments)
- [x] comment preservation mode
- [x] pretty printer with configurable output

### milestone 4a — sandboxing & resource limits

- [x] `ContextBuilder` fluent API (heap sizes, step limits, presets)
- [x] fuel-based step limiting (thread-local counters + vm.c patch)
- [x] 14 allowlist-based sandbox presets (ARITHMETIC, MATH, LISTS, etc.)
- [x] `TimeoutContext` for wall-clock deadlines via dedicated thread
- [x] `Error::StepLimitExceeded` and `Error::Timeout`

### milestone 4b — production hardening

- [x] parameterised IO presets — `FsPolicy` with path prefix matching + canonicalisation
- [x] wrapper foreign functions for all four `open-*-file` primitives
- [x] `.file_read(&[...])` / `.file_write(&[...])` builder API
- [x] path traversal and symlink protection via `canonicalize()`
- [x] r7rs standard environment via VFS + static libs + eval.c patches
- [x] module import policy: VFS-only restriction in sandboxed standard-env contexts
- [x] sandboxed `(import ...)`: `.allow(&["import"])` + VFS-only gate
- [x] additional value types: char, bytevector, port (opaque), hash table

### milestone 5 — reach

- [x] REPL example (rustyline)
- [x] serde data format — s-expression ↔ rust structs via tein-sexp
- [x] macro expansion hooks — `(tein macro)` VFS module + rust API
- [x] custom ports — rust `Read`/`Write` as scheme input/output ports
- [x] custom reader extensions — `#x` hash dispatch, `(tein reader)` VFS module

### milestone 6 — foreign type protocol

- [x] `ForeignType` trait + `ForeignStore` handle-map per context
- [x] `Value::Foreign { handle_id, type_name }` with tagged-list wire format
- [x] `(tein foreign)` VFS module: `foreign?`, `foreign-type`, `foreign-handle-id`
- [x] `foreign-call` / `foreign-methods` / `foreign-types` / `foreign-type-methods` native fns
- [x] auto-generated `type-name?` predicates + `type-name-method` convenience procs
- [x] `ctx.foreign_value(v)`, `ctx.foreign_ref::<T>(&val)` rust-side API
- [x] LLM-friendly error messages (lists available methods on wrong-method call)

### milestone 7 — managed contexts

- [x] `ThreadLocalContext` — `Send + Sync` managed context on a dedicated thread
- [x] persistent mode (state accumulates) + fresh mode (rebuilt per call)
- [x] init closure, `reset()`, shared channel protocol (`thread.rs`)
- [x] `ContextBuilder` gains `Clone` (required for fresh mode rebuild)

---

## roadmap

### milestone 8 — rust ecosystem bridge

expose high-value rust crates as idiomatic r7rs scheme modules. this is the "scheme with rust inside" story — capabilities no pure-scheme implementation can match.

**`(tein json)`** — JSON via serde_json. faster and more correct than SRFI-180 (pure scheme). bidirectional: scheme values ↔ JSON strings, leveraging the existing serde foundation in tein-sexp.

**`(tein regex)`** — regex via the `regex` crate. dramatically faster than irregex. the perf story is the clearest argument here.

**`(tein crypto)`** — hashing (blake3, sha2) and CSPRNG (rand). pure-scheme crypto exists on snow-fort but is orders of magnitude slower.

**`(tein uuid)`** — UUID generation. trivial wrapper, no good pure-scheme equivalent.

**`#[tein_module]` proc macro** — generalises the boilerplate for exposing rust crates as scheme modules. auto-generates scheme-side glue (predicates, constructors, method procs) from annotated rust. makes adding further modules fast and consistent.

```rust
// rough sketch — exact design tbd in implementation planning
#[tein_module("regex")]
mod regex_module {
    #[tein_fn] fn compile(pattern: &str) -> Result<Regex, Error> { ... }
    #[tein_type] impl Regex {
        #[tein_method] fn is_match(&self, text: &str) -> bool { ... }
    }
}
// → generates (tein regex) VFS module with make-regex, regex?, regex-is-match, etc.
```

**foreign type constructor macro** — ergonomic rust-side `make-type` registration, complementing `#[tein_module]` for simpler cases where a full module proc macro is overkill.

### milestone 9 — tein as a scheme

tein as a first-class scheme implementation, not just a rust library.

**`tein` binary** — a standalone scheme interpreter/REPL. open design question: one binary or several? chicken provides three (`chicken` compiles to C, `csc` compiles to binary, `csi` is the interpreter). tein is rooted in rust which changes the tradeoffs. to be decided during milestone planning.

**snow-fort package support** — two tiers:
- *vetted VFS packages*: curated snow-fort libraries embedded in the VFS at compile time by `build.rs`, available in sandboxed contexts. same trust level as existing VFS modules.
- *snow capability*: unvetted snow packages as a `ContextBuilder` capability (`.snow_packages(&["srfi/180"])`), following the same pattern as `file_read`/`file_write`. available only in contexts that explicitly grant it. fetched and embedded at compile time, not at runtime.

**r5rs/r6rs compatibility layers** — `ContextBuilder::r5rs_env()` / `ContextBuilder::r6rs_env()` for best-effort compatibility. chibi already has substantial r6rs support internally; the goal is exposing it properly rather than implementing it from scratch. expands the pool of available scheme code significantly. documented as best-effort, not full conformance.

**scheme test harness** — run `.scm` files as cargo integration tests. enables testing scheme-level behaviour idiomatically.

### milestone 10 — capability modules

more rust-backed scheme modules, building on the `#[tein_module]` infrastructure from M8.

**`(tein http)`** — HTTP client via `ureq` or `reqwest`. no good pure-scheme equivalent. the biggest capability unlock for scheme-as-scripting-language.

**`(tein datetime)`** — date/time via `chrono`. better timezone and formatting support than SRFI-19.

**`(tein tracing)`** — structured logging from scheme into rust's `tracing` ecosystem. scheme code generating structured spans that rust can consume. useful for chibi~ tool synthesis observability.

*further modules follow naturally from the `#[tein_module]` pattern — this milestone establishes the pattern is working well before adding more.*

### milestone 11 — performance & throughput

**context pool** — pool of `ThreadLocalContext` instances for high-throughput workloads. when chibi~ runs many parallel tool evaluations, context creation cost matters.

**WASM target** — chibi compiles via emscripten. enables tein in browser and edge environments. previously listed in milestone 5 but deprioritised; now explicitly on the roadmap.

**compile-to-C pipeline** — expose chibi's `compile-to-c` such that scheme files can be compiled to C and linked into rust binaries at build time via `build.rs`. scheme at near-native speed. complex to drive programmatically; needs careful design.

### milestone 12 — stochastic runtime

tein as a host platform for stochastic programming — a paradigm where bindings are probability distributions rather than values, programs carry semantic intent, and a compilation pipeline progressively collapses fuzzy specifications toward concrete outputs using the cheapest available strategy.

see `~/projects/chibi/backrooms/stochastic-programming.md` for the full design.

tein already has every primitive needed:

- continuations as residuals — chibi has first-class continuations; a residual node waiting for a model to fill in a value *is* a delimited continuation
- macro expansion hook — deterministic compilation passes expressible as macro transformations
- foreign type protocol — model handles, projection strategies, and the semantic knowledge base as rust-side objects
- sandboxing — the deterministic compilation phase runs isolated; model dispatch runs with appropriate capabilities

**`(tein llm)`** — rust-backed scheme module exposing LLM calls (via ratatosk or anthropic SDK directly) as first-class scheme values. the bridge between the stochastic runtime and actual models.

**stochastic core library** — `define~`, `intent`, `narrow`, `project`, `monad` forms as scheme macros. the deterministic compilation passes. the projection registry.

this milestone is the convergence point for the dual identity: rust ecosystem modules provide the cheap algorithmic projections; `(tein llm)` provides the model fallback; chibi~'s tool protocol is expressed in stochastic scheme.

---

## unscheduled ideas

- **context pool** — pool of `ThreadLocalContext` for high-throughput (absorbed into M11)
- **`build_managed` with timeout** — combine `ThreadLocalContext` + wall-clock deadline without needing two threads
- **hash table API** — expose `Value::HashTable` with rich rust methods rather than leaving it opaque
- **continuation API** — first-class access to scheme continuations from rust
