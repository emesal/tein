# github ecosystem sync design

**date**: 2026-02-26
**status**: approved

## context

the ROADMAP.md was recently updated to reflect tein's current state and future direction (M1–M12 + unscheduled). the github issue ecosystem was last updated during earlier milestones and no longer matches. this plan brings github in sync: closing completed work, migrating orphaned issues, building out the full milestone structure, and creating tracking issues for all roadmap items.

## milestone changes

### close as completed

- **M4 — Production Hardening**: fully done (4a sandboxing + 4b hardening). close milestone.
- **M5 — Reach**: fully done (custom ports, reader extensions, macro hooks, REPL, serde data format). close milestone.

### new milestones to create

| # | title | description |
|---|-------|-------------|
| 6 | Milestone 6 — Foreign Type Protocol | Expose arbitrary rust types to scheme via a safe handle-map protocol. |
| 7 | Milestone 7 — Managed Contexts | `ThreadLocalContext`: persistent and fresh managed contexts on dedicated threads. |
| 8 | Milestone 8 — Rust Ecosystem Bridge | Expose high-value rust crates as idiomatic r7rs scheme modules. |
| 9 | Milestone 9 — Tein as a Scheme | Tein as a first-class scheme implementation: binary, package support, wisp, compat layers, test harness. |
| 10 | Milestone 10 — Capability Modules | More rust-backed scheme modules building on the `#[tein_module]` infrastructure. |
| 11 | Milestone 11 — Performance & Throughput | Context pool, WASM target, compile-to-C pipeline. |
| 12 | Milestone 12 — Stochastic Runtime Support | Tein as a platform for hosting a stochastic programming language. |

M6 and M7 are already complete — create them closed. M8–M12 are open future work.

## issue disposition

### close (implemented)

| # | title | reason |
|---|-------|--------|
| #16 | Macro expansion hooks | done in M5 |
| #17 | Custom reader extensions | done in M5 |

### update and reassign

| # | title | action |
|---|-------|--------|
| #15 | WASM target | move to M11, update body to reflect M11 context |
| #27 | VFS-embedded documentation for LLM schemers | assign to M9 |
| #29 | ThreadLocalContext: optional wall-clock timeout | assign to unscheduled milestone (create one) |
| #31 | fix: VFS module re-export pattern broken in sandboxed contexts | assign to M9, add `bug` label |
| #32 | feat: scheme-level r7rs conformance test harness | assign to M9 |

### new issues to create

**M8 — Rust Ecosystem Bridge**
- feat: `(tein json)` — JSON via serde_json
- feat: `(tein regex)` — regex via the `regex` crate
- feat: `(tein crypto)` — hashing (blake3, sha2) and CSPRNG
- feat: `(tein uuid)` — UUID generation
- feat: `#[tein_module]` proc macro for rust→scheme module generation
- feat: foreign type constructor macro

**M9 — Tein as a Scheme**
- feat: `tein` binary — standalone interpreter/REPL
- feat: snow-fort package support (vetted VFS + snow capability)
- feat: `(tein wisp)` — wisp syntax (SRFI-119) as a VFS module
- feat: r5rs/r6rs compatibility layers
*(existing: #27, #31, #32)*

**M10 — Capability Modules**
- feat: `(tein http)` — HTTP client via ureq/reqwest
- feat: `(tein datetime)` — date/time via chrono
- feat: `(tein tracing)` — structured logging into rust's tracing ecosystem

**M11 — Performance & Throughput**
- feat: context pool for high-throughput workloads
- feat: compile-to-C pipeline via chibi's `compile-to-c`
*(existing: #15 WASM)*

**M12 — Stochastic Runtime Support**
- feat: `(tein rat)` — model ensemble library wrapping ratatoskr's ModelGateway
- feat: stochastic core library — `define~`, `intent`, `narrow`, `project`, etc.

**Unscheduled**
- feat: `build_managed` with timeout (combined ThreadLocalContext + wall-clock deadline)
- feat: hash table API — rich rust methods on `Value::HashTable`
- feat: continuation API — first-class scheme continuations from rust
- feat: `(tein chibi)` — scheme module for chibi~'s tool/plugin protocol
*(existing: #29)*

## notes

- M6 and M7 milestones created as closed (already done), for historical completeness
- `(tein rat)` is a model ensemble library (wraps ratatoskr's ModelGateway for chat, generate, embed, NLI, token counting)
- #27 (LLM discoverability docs) fits M9 as a "tein as a scheme" ergonomics concern
- #31 (re-export bug) blocks `(tein reader)`/`(tein macro)` in sandboxed contexts — belongs in M9 where sandboxed scheme usage is prominent
