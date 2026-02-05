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

## 📋 pending (prioritized)

### medium priority

- [ ] **setup r7rs standard environment**
  - complex: requires static library mechanism
  - or: enable dynamic loading (conflicts with vendoring)
  - alternative: manually expose needed functions
  - impacts: most r7rs functions unavailable currently

- [ ] **add more value types**
  - procedures (scheme functions as values)
  - bytevectors
  - hash tables
  - ports (for io)

- [ ] **higher-level ffi**
  - proc macro: `#[scheme_fn] fn add(a: i64, b: i64) -> i64`
  - automatic argument extraction and return conversion
  - error propagation from rust to scheme

- [ ] **add repl example**
  - interactive scheme session
  - useful for testing
  - could use rustyline for nice editing

### low priority

- [ ] **continuation support**
  - represent call/cc continuations as values?
  - challenging but cool

- [ ] **macro expansion hooks**
  - expose scheme macro system to rust

- [ ] **custom reader extensions**
  - extend scheme reader syntax

## 💡 ideas

- **naming scheme modules with norse terms?**
  - core primitives: `yggdrasil`
  - io: `bifrost` (rainbow bridge)
  - macros: `galdr` (spells)

- **proc macro for rust fn → scheme**
  ```rust
  #[scheme_fn]
  fn add(a: i64, b: i64) -> i64 {
      a + b
  }
  // exposes (add x y) in scheme
  ```

- **s-expression parser separate from evaluation**
  - `sexp::parse("(+ 1 2)")` → rust ast
  - useful for config files without full evaluation

- **sandboxing / resource limits**
  - cap evaluation time
  - cap memory usage
  - restrict available functions

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
