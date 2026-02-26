# tein roadmap

## what is tein?

tein is an embeddable r7rs scheme interpreter for rust, built on vendored chibi-scheme 0.11. it has a dual identity:

- **scheme embedded in rust** — rust applications add scheme as a scripting or extension language, with safe sandboxing, resource limits, and bidirectional rust↔scheme data exchange
- **scheme with rust inside** — scheme programs get access to the rust ecosystem via tein's module system: high-performance crates exposed as idiomatic r7rs libraries

long-term, tein aims to be a capable scheme implementation in its own right — one that just happens to be exceptionally easy to embed in rust, in the same spirit that chibi-scheme is easy to embed in C.

tein is a core dependency of chibi~ (an LLM agent harness). tein provides the sandboxed execution environment for LLM-synthesised tools, with the sandbox boundary serving as chibi~'s trust model. this makes production hardening and the rust ecosystem bridge high priorities.

the two identities converge in agentic and stochastic use cases: the rust ecosystem provides high-performance building blocks exposed as scheme modules; scheme coordinates, composes, and expresses intent. tein is the platform where that meeting happens — generalist by design, with the module system determining what any given application can reach.

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

expose high-value rust crates as idiomatic r7rs scheme modules. this is the "scheme with rust inside" story — building blocks that scheme programs can import and compose freely.

**`(tein json)`** — JSON via serde_json. bidirectional: scheme values ↔ JSON strings, leveraging the existing serde foundation in tein-sexp.

**`(tein regex)`** — regex via the `regex` crate.

**`(tein crypto)`** — hashing (blake3, sha2) and CSPRNG (rand).

**`(tein uuid)`** — UUID generation.

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

**`tein` binary** — a standalone scheme interpreter/REPL. open design question: one binary or several? chicken provides three (`chicken` compiles to C, `csc` compiles to binary, `csi` is the interpreter). tein is rooted in rust which changes the tradeoffs. to be decided during milestone planning. the custom reader extension (M5, done) already provides the hook for future file-format extensions (e.g. `.ssp` stochastic scheme programs, if that path is pursued).

**snow-fort package support** — two tiers:
- *vetted VFS packages*: curated snow-fort libraries embedded in the VFS at compile time by `build.rs`, available in sandboxed contexts. same trust level as existing VFS modules.
- *snow capability*: unvetted snow packages as a `ContextBuilder` capability (`.snow_packages(&["srfi/180"])`), following the same pattern as `file_read`/`file_write`. available only in contexts that explicitly grant it. fetched and embedded at compile time, not at runtime.

**`(tein wisp)`** — wisp syntax (SRFI-119) as a VFS module. a pure-scheme R7RS port of the wisp preprocessor, vendored into the VFS. transforms indentation-based wisp source into s-expressions before evaluation — no second VM, full scheme semantics underneath. depends on `(tein regex)` (M8). exposes `wisp-read` / `wisp-eval` / `wisp-load` as entry points. serves as the first "alternate surface syntax" stepping stone, and as a foundation for more distinct scripting languages (e.g. the stochastic language's surface syntax) down the road.

**r5rs/r6rs compatibility layers** — `ContextBuilder::r5rs_env()` / `ContextBuilder::r6rs_env()` for best-effort compatibility. chibi already has substantial r6rs support internally; the goal is exposing it properly rather than implementing it from scratch. expands the pool of available scheme code significantly. documented as best-effort, not full conformance.

**scheme test harness** — run `.scm` files as cargo integration tests. enables testing scheme-level behaviour idiomatically.

### milestone 10 — capability modules

more rust-backed scheme modules, building on the `#[tein_module]` infrastructure from M8.

**`(tein http)`** — HTTP client via `ureq` or `reqwest`.

**`(tein datetime)`** — date/time via `chrono`. better timezone and formatting support than SRFI-19.

**`(tein tracing)`** — structured logging from scheme into rust's `tracing` ecosystem. scheme code generating structured spans that rust can consume.

*further modules follow naturally from the `#[tein_module]` pattern — this milestone establishes the pattern is working well before adding more.*

### milestone 11 — performance & throughput

**context pool** — pool of `ThreadLocalContext` instances for high-throughput workloads. relevant when many scheme evaluations run in parallel — tool execution, stochastic program dispatch, etc.

**WASM target** — chibi compiles via emscripten. enables tein in browser and edge environments. previously listed in milestone 5 but deprioritised; now explicitly on the roadmap.

**compile-to-C pipeline** — expose chibi's `compile-to-c` such that scheme files can be compiled to C and linked into rust binaries at build time via `build.rs`. scheme at near-native speed. complex to drive programmatically; needs careful design.

### milestone 12 — stochastic runtime support

tein as a platform for hosting a stochastic programming language — a language extension implemented *in* tein scheme via the module system. the stochastic language is not tein scheme; it is a library that uses tein as its substrate.

see `~/projects/chibi/backrooms/stochastic-programming.md` for the full design.

tein already has every primitive the stochastic language needs:

- **continuations as residuals** — chibi has first-class continuations; a residual node waiting for a model to fill in a value *is* a delimited continuation
- **macro expansion hook** — deterministic compilation passes can be expressed as macro transformations on the stochastic IR
- **foreign type protocol** — model handles, projection strategies, and the knowledge base are rust-side objects exposed to scheme
- **sandboxing** — the deterministic compilation phase runs isolated; model dispatch runs with appropriate capabilities granted

**`(tein rat)`** — rust-backed scheme module wrapping ratatoskr's `ModelGateway`: chat, generate, embed, NLI, token counting. model access for any scheme program that wants it, not just the stochastic language. use r7rs `only`/`prefix` import forms for granularity.

**stochastic core library** — `define~`, `intent`, `narrow`, `project`, `monad`, `with-context`, `register-projection` as scheme macros and procedures. the deterministic compilation passes. the projection registry as a foreign type. this is the stochastic language itself, delivered as a tein module.

the milestone is the point where tein's two identities are exercised simultaneously: rust ecosystem modules (M8, M10) provide cheap algorithmic building blocks; `(tein rat)` provides the model bridge; the stochastic language coordinates and composes them in scheme.

---

## unscheduled ideas

- **`build_managed` with timeout** — combine `ThreadLocalContext` + wall-clock deadline without needing two threads
- **hash table API** — expose `Value::HashTable` with rich rust methods rather than leaving it opaque
- **continuation API** — first-class access to scheme continuations from rust
- **`(tein chibi)`** — scheme module speaking chibi~'s tool/plugin protocol: call chibi~'s tools from tein programs, hook into the plugin architecture. depends on chibi~'s protocol stabilising; not a tein prerequisite.
