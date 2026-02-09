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

## ✅ milestone 1 — ergonomics & round-trip

*make the existing api a joy to use before adding new capabilities.*

- [x] **typed extraction helpers on Value**
  - `as_integer()`, `as_float()`, `as_string()`, `as_symbol()`, `as_bool()`
  - `as_list()`, `as_pair()`, `as_vector()`, `as_procedure()`
  - `is_nil()`, `is_unspecified()`, `is_procedure()`

- [x] **complete bidirectional value bridge**
  - `to_raw()` for all Value variants (lists, pairs, vectors, symbols, procedures)
  - recursive with depth limit (MAX_DEPTH = 10,000)

- [x] **multi-expression evaluation**
  - read loop with string port, returns last value
  - errors stop evaluation early

- [x] **file evaluation**
  - `ctx.load_file(path)` — delegates to evaluate

## ✅ milestone 2 — scheme as extension language

*transform tein from "eval strings" into a real scripting engine.*

- [x] **scheme→rust callbacks (procedures as values)**
  - `Value::Procedure` via `sexp_applicablep` (lambdas + builtins)
  - `ctx.call(proc, &[args])` builds arg list and applies

- [x] **variadic foreign functions**
  - `define_fn_variadic(name, f)` via `SEXP_PROC_VARIADIC`
  - replaces old `define_fn0..3` fixed-arity set

- [x] **higher-level ffi (proc macro)**
  - `#[scheme_fn] fn add(a: i64, b: i64) -> i64`
  - auto extraction for i64/f64/String/bool args + return
  - `Result<T, E>` support — Err becomes scheme exception string
  - panic safety at ffi boundary via `catch_unwind`

## 🚧 in progress

none currently

## 🗺️ roadmap

### milestone 3 — s-expression data format (`tein-sexp`)

*pure rust s-expression toolkit: parse, serialize, convert.*

separate workspace crate — no chibi dependency, no unsafe, independently publishable.

- [ ] **`Sexp` AST with source spans**
  - dedicated enum: `Atom` (integer, float, string, symbol, bool) + `List` + `Nil`
  - every node carries `Span { line, column, byte_offset, len }`
  - designed in from day one — impossible to retrofit without breaking changes
  - foundation for error reporting, editor integration, future LSP

- [ ] **r7rs-compatible reader**
  - proper list `(a b c)`, dotted pair `(a . b)`, vector `#(a b c)`
  - full string escapes: `\n`, `\t`, `\\`, `\"`, `\xNN;` hex escapes, `\` line continuation
  - `#;` datum comments, `#| block |#` nested comments, `;` line comments
  - symbol quoting: `|weird symbol|`
  - character literals: `#\a`, `#\space`, `#\newline`, `#\xNN`
  - `'x` → `(quote x)`, `` `x `` → `(quasiquote x)`, `,x` → `(unquote x)`

- [ ] **comment preservation mode**
  - optional parser mode that attaches comments to adjacent AST nodes
  - enables round-trip editing: read config → modify field → write back without losing comments
  - AST `Span` type designed to accommodate this from v1

- [ ] **pretty printer**
  - configurable output: compact vs indented
  - respects preserved comments when present

- [ ] **serde data format implementation**
  - `Deserializer`: s-expression source → rust structs via `#[derive(Deserialize)]`
  - `Serializer`: rust structs → s-expression string via `#[derive(Serialize)]`
  - alist convention: `((key val) ...)` with symbol keys → serde map
  - plain lists → serde sequence
  - free format conversion via serde ecosystem (json ↔ sexpr ↔ ron ↔ toml ↔ yaml)
  - source spans in error messages: "expected integer at line 3, column 7"

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