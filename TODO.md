# tein todo list

## ✅ completed

- [x] **#1: implement pair/list value extraction**
  - added car/cdr to ffi shim
  - proper list detection (ends in nil)
  - improper list/dotted pair support
  - recursive list walking
  - beautiful display formatting

- [x] **#2: add numeric types (floats)**
  - added float support via `sexp_flonump`
  - **critical fix**: check floats before integers!
  - `sexp_integerp` matches some floats, causes garbage reads
  - proper f64 extraction

- [x] **#3: add vector support**
  - shim: `tein_sexp_vectorp`, `tein_sexp_vector_length`, `tein_sexp_vector_data`
  - `Value::Vector(Vec<Value>)` with recursive extraction
  - display as `#(...)` matching scheme convention

- [x] **#4: better error messages from exceptions**
  - extract actual message via `sexp_exception_message`
  - append irritants via `sexp_exception_irritants`
  - e.g. `car: not a pair: (42)` instead of generic message

- [x] **#5: rust→scheme ffi**
  - `define_fn0`/`define_fn1`/`define_fn2`/`define_fn3` — type-safe per arity
  - follows chibi's calling convention (args as separate params)
  - `raw` module re-exports ffi helpers for building foreign functions
  - `Value::to_raw` for converting rust values back to scheme

## 🚧 in progress

none currently

## 🗺️ roadmap

### milestone 1 — ergonomics & round-trip

*make the existing api a joy to use before adding new capabilities.*

- [ ] **typed extraction helpers on Value**
  - `v.as_integer()? → i64`, `v.as_string()? → &str`, etc.
  - eliminates repetitive pattern matching for callers
  - small, self-contained, high-value

- [ ] **complete bidirectional value bridge**
  - `to_raw()` for all Value variants (lists, pairs, vectors, symbols)
  - every type that can come out of scheme can go back in
  - prerequisite for callbacks and higher-level ffi

- [ ] **multi-expression evaluation**
  - `evaluate("(define x 5) (+ x 3)")` → returns last value
  - essential for scripting / config-loading UX

- [ ] **file evaluation**
  - `ctx.load_file("config.scm")` — evaluate a whole file
  - natural extension of multi-expression support

### milestone 2 — scheme as extension language

*transform tein from "eval strings" into a real scripting engine.*

- [ ] **scheme→rust callbacks (procedures as values)**
  - `Value::Procedure` variant holding a callable sexp
  - `ctx.call(proc, &[args])` invokes a scheme lambda from rust
  - gateway to using scheme as a true extension language

- [ ] **variadic foreign functions**
  - `define_fn_variadic(name, f)` — rest-args support
  - covers more use cases without needing the proc macro yet

- [ ] **higher-level ffi (proc macro)**
  - `#[scheme_fn] fn add(a: i64, b: i64) -> i64`
  - automatic argument extraction and return conversion
  - error propagation from rust to scheme exceptions
  - depends on: complete value bridge, procedures as values

### milestone 3 — standalone s-expression toolkit

*high-utility, zero-eval tools for config and data.*

- [ ] **s-expression parser (no eval)**
  - standalone `tein::sexp::parse("(key (nested value))")` → rust AST
  - no chibi dependency — pure rust parser
  - feature-gated or separate crate (`tein-sexp`?)
  - s-exprs as a config format without the interpreter

- [ ] **serde integration** (feature-gated)
  - `#[derive(Deserialize)]` from s-expressions → rust structs
  - `#[derive(Serialize)]` from rust structs → s-expressions
  - depends on: s-expression parser
  - *chef's kiss* for config files

### milestone 4 — production hardening

*make tein safe for untrusted input and real deployments.*

- [ ] **sandboxed evaluation / resource limits**
  - cap evaluation time (step limit or wall-clock timeout)
  - cap memory usage (chibi heap limits are already partially there)
  - restrict available primitives (allowlist/denylist)
  - critical for agent DSLs and untrusted scheme

- [ ] **r7rs standard environment**
  - figure out static library setup (or accept dynamic loading)
  - alternative: manually expose needed functions incrementally
  - impacts: most r7rs functions unavailable until this lands

- [ ] **additional value types**
  - bytevectors
  - hash tables
  - ports (for io)
  - continuations (as opaque values, at minimum)

### milestone 5 — reach

*expand where tein can run and how people interact with it.*

- [ ] **REPL example**
  - interactive scheme session with rustyline
  - useful for testing and exploration

- [ ] **WASM target**
  - chibi-scheme can compile via emscripten
  - tein in the browser / wasm runtimes — unique offering

- [ ] **macro expansion hooks**
  - expose scheme macro system to rust

- [ ] **custom reader extensions**
  - extend scheme reader syntax from rust

## 💡 ideas (unscheduled)

- **norse naming for modules?**
  - core primitives: `yggdrasil`
  - io: `bifrost` (rainbow bridge)
  - macros: `galdr` (spells)
  - sexp parser: `rúnar` (runes/letters)

- **scheme test harness**
  - run `.scm` test files as cargo integration tests
  - would make r7rs conformance testing easier

- **context pooling / thread-local contexts**
  - since contexts are !Send, provide ergonomic per-thread patterns

- **scheme-defined foreign type protocol**
  - let rust types register as opaque scheme objects
  - scheme can hold references, pass them back to rust ffi

## 🐛 known issues (code review, 2026-02-05)

### critical

- [x] **CR-C1: `sexp_uint_t` declared as signed `c_long`** — ✅ fixed: `c_ulong`
- [x] **CR-C2: `unsafe impl Send` without documented invariants** — ✅ removed, documented why !Send
- [x] **CR-C3: `is_proper_list` infinite loop on circular lists** — ✅ tortoise-and-hare + depth limit

### important

- [x] **CR-I1: `Value::Unspecified` unreachable from `from_raw`** — ✅ void check added + test
- [x] **CR-I2: `transmute` for FFI fn pointer lacks type annotations** — ✅ explicit types added
- [x] **CR-I3: broken rustdoc links to `Context::define_fn_raw`** — ✅ updated to define_fn0..3
- [x] **CR-I4: `Value` missing `PartialEq` derive** — ✅ derived
- [x] **CR-I5: `to_raw` silently converts most variants to void** — ✅ returns Result, errors on unsupported
- [x] **CR-I6: all 3 doc-tests are `#[ignore]`-d** — ✅ now compile and run (23 tests total)

- [x] **CR-I7: gc pinning caused exponential memory allocation** — ✅ removed pinning
  - attempted fix with `sexp_preserve_object` caused exponential memory usage
  - each pin allocated a cons cell on global preservatives list
  - deeply nested structures (50+ levels) caused oom/slowdown
  - **actual fix**: removed pinning entirely; chibi's conservative gc scans the stack
  - stack-allocated sexps are automatically protected during iteration

### minor

- [x] **CR-M1: redundant `c_fname` clone** — ✅ removed
- [x] **CR-M2: 24 public unsafe fns missing `# Safety` docs** — ✅ module-level safety doc
- [x] **CR-M3: `rerun-if-changed` on directories doesn't track file changes** — ✅ tracks individual sources
- [x] **CR-M4: `Cargo.lock` both gitignored and committed** — ✅ removed from gitignore (tracked)
- [x] **CR-M5: `Display` for `Value::String` doesn't escape special chars** — ✅ escapes \", \\, \n, \r, \t
- [x] **CR-M6: README license "tbd" vs Cargo.toml MIT/Apache-2.0** — ✅ updated README

## 📝 notes

- check `DEVELOPMENT.md` for architecture details
- `examples/` directory has working code samples
- remember: floats before integers in type checking!