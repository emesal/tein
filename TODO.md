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

## 🐛 known issues

- none! tests passing, no crashes :3

## 📝 notes

- check `DEVELOPMENT.md` for architecture details
- `examples/` directory has working code samples
- remember: floats before integers in type checking!
