# docs update — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Full docs restructure — new `docs/` files covering all M8-shipped features, README
rewrite, ARCHITECTURE.md and ROADMAP.md sync to reality.

**Architecture:** README becomes a lean landing page. `docs/guide.md` becomes a TOC/index.
Eight new focused docs replace the single monolithic `guide.md` walkthrough. All content is
user-facing (rust+r7rs audience, no prior embedding experience required). Agent-friendliness
is a first-class narrative in `tein-for-agents.md`.

**Tech Stack:** Markdown only. No code changes. Read source files and AGENTS.md for accuracy.

**Source of truth for content:** `tein/src/` (types, APIs), `AGENTS.md` (flows, quirks),
`target/chibi-scheme/lib/tein/` (scheme module exports), `tein/src/vfs_registry.rs` (module list).

**Branch:** `docs/restructure-2603` (rebased onto `origin/dev` after PR #100 merged)

---

## progress

- [x] Task 1: branch created (`docs/restructure-2603`)
- [x] Task 2: README.md rewritten (commits 7fcdd31, d8bddff)
- [x] Task 3: docs/quickstart.md (commits ff8c812, 4e759bd — fixed Context::new() description + extraction helper lifetimes)
- [x] Task 4: docs/embedding.md (commits 9c23282, a4488d2 — fixed stale variable name in port example)
- [x] Task 5: docs/sandboxing.md (commit 6f69989 — also fixed Modules::Safe docstring in sandbox.rs, commit 068c514)
- [x] Task 6: docs/rust-scheme-bridge.md (commit 378018a — register_reader takes b'j' u8, not char)
- [x] Task 7: docs/modules.md (commit 1268f66 — tein/process IS in Modules::Safe; added tein/file and tein/load sections)
- [x] Task 8: docs/extensions.md (commit 236f792)
- [x] Task 9: docs/tein-for-agents.md (commit ec59b92)
- [x] Task 10: docs/reference.md (commit 9d1b15b — Value::Unspecified displays as #<unspecified>; added Vector + Other variants; tein/process IS in Safe)
- [x] Task 11: docs/guide.md rewrite as index (commit 09542f5)
- [x] Task 12: ARCHITECTURE.md update (commit bd8be05 — M8 complete, 7 eval.c patches, new src files, VFS shadow + exit flows, docs/ note, b'j' fix)
- [x] Task 13: ROADMAP.md update (commit cd85f1b — M8 items to completed, open M8 work listed, scheme test harness marked shipped)
- [x] Task 14: final pass + PR (PR #102)

## corrections discovered during implementation

- `Value::Void` → `Value::Unspecified` (actual variant name in value.rs)
- `is_void()` → `is_unspecified()` (actual method name)
- heap builder method is `heap_max`/`heap_size`, not `heap_limit`
- `tein/process` IS in `Modules::Safe` (trampolines neuter env/argv) — AGENTS.md note was stale
- `as_string()` and `as_list()` borrow from Value — must bind Value before calling
- `Context::new()` provides primitive env (all opcodes), not just core syntax forms

## notes for AGENTS.md (collect at end)

- AGENTS.md stale: "Modules::Safe excludes (tein process)" — trampolines now neuter it; it's in Safe
- docs/ structure: guide.md (index), quickstart, embedding, sandboxing, rust-scheme-bridge, modules, extensions, tein-for-agents, reference
- when adding features: update relevant docs/ file + reference.md VFS module list

---

### Task 1: Create the docs branch

**Step 1: Create branch**

```bash
just docs branch-name-2603
```

If `just` doesn't have a `docs` recipe, use:

```bash
git checkout -b docs/restructure-2603
```

**Step 2: Confirm clean working tree**

```bash
git status
```

Expected: nothing to commit.

---

### Task 2: Rewrite README.md

**Files:**
- Modify: `README.md`

**Step 1: Read current README**

Read `README.md` in full. Note: the "What tein is becoming" roadmap section must be removed.
The examples table, quick start snippet, about blurb, and license stay.

**Step 2: Rewrite**

New structure — write the whole file:

```markdown
# tein

> Branch and rune-stick — embeddable R7RS Scheme for Rust

**tein** is an embeddable R7RS Scheme interpreter for Rust, built on
[chibi-scheme](https://github.com/ashinn/chibi-scheme). Safe Rust API wrapping unsafe C FFI.
Zero runtime dependencies.

tein has a dual identity:

- **scheme embedded in rust** — add Scheme as a scripting or extension language to any Rust
  application. composable sandboxing, resource limits, bidirectional data exchange
- **scheme with rust inside** — Scheme programs get access to the Rust ecosystem via tein's
  module system: high-value crates exposed as idiomatic R7RS libraries

## Quick start

```toml
[dependencies]
tein = { git = "https://github.com/emesal/tein" }
```

```rust
use tein::{Context, Value};

let ctx = Context::new()?;
let result = ctx.evaluate("(+ 1 2 3)")?;
assert_eq!(result, Value::Integer(6));
```

## What tein can do today

- **[Sandboxing](docs/sandboxing.md)** — composable module restriction, file IO policy,
  step limits, wall-clock timeouts. designed as a trust boundary for LLM-synthesised code
- **[Rust↔Scheme bridge](docs/rust-scheme-bridge.md)** — `#[tein_fn]`, `#[tein_module]`,
  `ForeignType` — expose Rust functions and types to Scheme with zero boilerplate
- **[Built-in modules](docs/modules.md)** — `(tein json)`, `(tein toml)`, `(tein uuid)`,
  `(tein time)`, `(tein process)`, `(tein docs)` — Rust crates as idiomatic R7RS libraries
- **[cdylib extensions](docs/extensions.md)** — load `.so` extensions at runtime via a
  stable C ABI; write extension crates that depend on `tein-ext`, never on `tein`
- **[Managed contexts](docs/embedding.md)** — `ThreadLocalContext` is `Send + Sync`,
  safe to share across threads; persistent or fresh-per-evaluation modes
- **[Custom ports](docs/embedding.md)** — bridge Rust `Read`/`Write` into Scheme's port system
- **[Reader extensions](docs/rust-scheme-bridge.md)** — register custom `#` dispatch characters
- **[Macro hooks](docs/rust-scheme-bridge.md)** — intercept and transform macro expansions

## Docs

| doc | description |
|-----|-------------|
| [guide](docs/guide.md) | index and reading order |
| [quickstart](docs/quickstart.md) | working code in 5 minutes |
| [embedding](docs/embedding.md) | context types, Value enum, builder API |
| [sandboxing](docs/sandboxing.md) | module policy, file IO, resource limits |
| [rust-scheme bridge](docs/rust-scheme-bridge.md) | `#[tein_fn]`, `#[tein_module]`, foreign types |
| [modules](docs/modules.md) | built-in `(tein *)` modules |
| [extensions](docs/extensions.md) | cdylib extension system |
| [tein for agents](docs/tein-for-agents.md) | tein as an agent execution platform |
| [reference](docs/reference.md) | Value types, feature flags, VFS modules, quirks |

## Examples

| example | description |
|---------|-------------|
| `basic` | evaluate expressions, pattern-match on values |
| `floats` | floating-point arithmetic |
| `ffi` | `#[tein_fn]` proc macro, Rust↔Scheme calls |
| `debug` | float/integer type inspection |
| `sandbox` | step limits, restricted environments, timeouts, file IO policies |
| `foreign_types` | foreign type protocol — registration, dispatch, introspection |
| `managed` | `ThreadLocalContext` persistent and fresh modes |
| `repl` | interactive Scheme REPL with readline |

```bash
cargo run --example sandbox
```

## About

from Old Norse *tein* (teinn): **branch** — like the branches of an abstract syntax tree;
**rune-stick** — carved wood for writing magical symbols. code as data, data as runes.

**why Scheme?** homoiconic syntax, proper tail calls, hygienic macros, minimalist elegance.
**why Chibi?** tiny (~200kb), zero external deps, full R7RS-small, designed for embedding.

*carved with care, grown with intention*

---

## License

ISC
```

**Step 3: Commit**

```bash
git add README.md
git commit -m "docs: rewrite README — lean landing page, remove roadmap section"
```

---

### Task 3: Write docs/quickstart.md

**Files:**
- Create: `docs/quickstart.md`

**Content:**

```markdown
# tein quickstart

get a Scheme expression evaluating in Rust in under five minutes.

## dependency

```toml
[dependencies]
tein = { git = "https://github.com/emesal/tein" }
```

By default this enables all feature-gated modules (`json`, `toml`, `uuid`, `time`). To
minimise dependencies:

```toml
tein = { git = "https://github.com/emesal/tein", default-features = false }
```

## first evaluation

```rust
use tein::{Context, Value};

let ctx = Context::new()?;
let result = ctx.evaluate("(+ 1 2 3)")?;
assert_eq!(result, Value::Integer(6));
```

`Context::new()` gives you a minimal environment: core syntax only (`define`, `lambda`,
`if`, `let`, `begin`, `quote`, `set!`). For the full R7RS standard library (`map`,
`for-each`, `string-split`, `call/cc`, etc.):

```rust
let ctx = Context::new_standard()?;
let result = ctx.evaluate("(map (lambda (x) (* x x)) (list 1 2 3 4 5))")?;
// Value::List([Value::Integer(1), Value::Integer(4), ...])
```

## working with values

`evaluate()` returns a `Value` — a Rust enum covering every Scheme type:

```rust
match ctx.evaluate("(list 1 \"hello\" #t)")? {
    Value::List(items) => {
        println!("integer: {}", items[0]);   // 1
        println!("string: {}",  items[1]);   // hello
        println!("bool: {}",    items[2]);   // #t
    }
    _ => unreachable!(),
}
```

Extraction helpers for common types:

```rust
let n: i64  = ctx.evaluate("42")?.as_integer().unwrap();
let s: &str = ctx.evaluate(r#""hello""#)?.as_string().unwrap();
let b: bool = ctx.evaluate("#t")?.as_bool().unwrap();
```

See [embedding.md](embedding.md) for the full `Value` variant table.

## calling rust from scheme

The `#[tein_fn]` proc macro generates a Chibi-compatible wrapper from a plain Rust function:

```rust
use tein::tein_fn;

#[tein_fn]
fn square(n: i64) -> i64 { n * n }

let ctx = Context::new()?;
ctx.define_fn_variadic("square", __tein_square)?;

assert_eq!(ctx.evaluate("(square 7)")?, Value::Integer(49));
```

The generated wrapper is named `__tein_{fn_name}`. Supported argument and return types:
`i64`, `f64`, `String`, `bool`. Functions can return `Result<T, E: Display>` to raise
Scheme errors.

See [rust-scheme-bridge.md](rust-scheme-bridge.md) for the full `#[tein_module]` pattern,
foreign types, and more.

## sandboxing in one step

```rust
use tein::sandbox::Modules;

let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .step_limit(50_000)
    .build()?;

// file IO blocked, infinite loops terminated
assert!(ctx.evaluate(r#"(open-input-file "/etc/passwd")"#).is_err());
```

See [sandboxing.md](sandboxing.md) for the full four-layer model.
```

**Step 1: Write the file, commit**

```bash
git add docs/quickstart.md
git commit -m "docs: add quickstart.md"
```

---

### Task 4: Write docs/embedding.md

**Files:**
- Create: `docs/embedding.md`

Read `tein/src/context.rs` (ContextBuilder methods), `tein/src/value.rs` (Value variants),
`tein/src/port.rs` (port API), `tein/src/managed.rs`, `tein/src/timeout.rs` before writing.
The current `docs/guide.md` sections "Choosing a context type", "Custom ports" are good
source material — rewrite, don't copy verbatim.

**Content outline:**

```markdown
# embedding tein

## context types

Comparison table: Context / TimeoutContext / ThreadLocalContext
— thread safety, timeout type, state persistence, use case

## Context and ContextBuilder

### minimal context
Context::new() — core syntax only
Context::new_standard() — full R7RS standard library

### builder API
ContextBuilder methods table:
  .standard_env()
  .step_limit(n)
  .heap_limit(bytes)
  .sandboxed(modules)        — see sandboxing.md
  .file_read(&[...])         — see sandboxing.md
  .file_write(&[...])        — see sandboxing.md
  .allow_module("...")       — see sandboxing.md
  .build()
  .build_timeout(duration)
  .build_managed(init)
  .build_managed_fresh(init)

### evaluating expressions
ctx.evaluate(expr) — returns last Value, string input
ctx.evaluate_port(&port) — read+eval loop from a port
ctx.call(&proc, &[args]) — call a Scheme procedure from Rust

## Value

Full variant table:
| Scheme type       | Rust variant           | Display  |
|-------------------|------------------------|----------|
| integer           | Value::Integer(i64)    | 42       |
| float             | Value::Float(f64)      | 3.14     |
| boolean           | Value::Boolean(bool)   | #t / #f  |
| string            | Value::String(String)  | "hello"  |
| symbol            | Value::Symbol(String)  | foo      |
| list              | Value::List(Vec<Value>)| (1 2 3)  |
| pair (improper)   | Value::Pair(Box, Box)  | (a . b)  |
| nil / empty list  | Value::Nil             | ()       |
| void              | Value::Void            | (void)   |
| char              | Value::Char(char)      | #\a      |
| bytevector        | Value::Bytevector(Vec<u8>) | #u8(...)  |
| port (opaque)     | Value::Port            | #<port>  |
| hash table (opaque)| Value::HashTable      | #<hash-table> |
| procedure         | Value::Procedure       | #<procedure> |
| foreign object    | Value::Foreign{...}    | #<counter:1> |

### extraction helpers
as_integer(), as_float(), as_bool(), as_string(), as_symbol(),
as_list(), is_nil(), is_void(), is_procedure()

### display
Value implements Display — produces scheme-readable output

## TimeoutContext

Wall-clock deadline wrapping a Context on a dedicated thread.
Requires step_limit (so the context thread can terminate after timeout fires).

```rust
use std::time::Duration;

let ctx = Context::builder()
    .step_limit(1_000_000)
    .build_timeout(Duration::from_secs(5))?;

let result = ctx.evaluate("(+ 1 2)")?;
```

Errors: Error::Timeout, Error::StepLimitExceeded

## ThreadLocalContext

Send + Sync — wrap in Arc, share across threads. Evaluation proxied over
a channel to a dedicated thread that owns the Context.

### persistent mode
State accumulates. reset() rebuilds and re-runs init.

```rust
let ctx = Context::builder()
    .standard_env()
    .step_limit(1_000_000)
    .build_managed(|ctx| {
        ctx.evaluate("(define counter 0)")?;
        Ok(())
    })?;

ctx.evaluate("(set! counter (+ counter 1))")?;
assert_eq!(ctx.evaluate("counter")?, Value::Integer(1));

ctx.reset()?;
assert_eq!(ctx.evaluate("counter")?, Value::Integer(0));
```

### fresh mode
Context rebuilt before every evaluation — no state leakage.

```rust
let ctx = Context::builder()
    .step_limit(100_000)
    .build_managed_fresh(|ctx| {
        ctx.evaluate("(define x 10)")?;
        Ok(())
    })?;

ctx.evaluate("(set! x 99)")?;
assert_eq!(ctx.evaluate("x")?, Value::Integer(10));  // x reset
```

### when to use which mode
- persistent — REPL, stateful scripting, accumulated definitions
- fresh — untrusted user scripts, request handlers, isolation required

## custom ports

Bridge Rust Read/Write into Scheme's port system.

### input ports

```rust
let source = std::io::Cursor::new("(+ 1 2) (+ 3 4)");
let port = ctx.open_input_port(source)?;

let expr = ctx.read(&port)?;              // one s-expression, no eval
let result = ctx.evaluate_port(&port)?;   // read+eval loop, last result
```

### output ports

```rust
let buf: Vec<u8> = Vec::new();
let port = ctx.open_output_port(std::io::Cursor::new(buf))?;
// pass port Value to Scheme's display/write/newline
```

Any type implementing std::io::Read / std::io::Write works.
```

**Step 1: Write the file, commit**

```bash
git add docs/embedding.md
git commit -m "docs: add embedding.md — context types, Value enum, builder API, ports"
```

---

### Task 5: Write docs/sandboxing.md

**Files:**
- Create: `docs/sandboxing.md`

Read `tein/src/sandbox.rs` (Modules, FsPolicy), `tein/src/context.rs` (builder sandbox methods),
`AGENTS.md` sandboxing flow section. Current `docs/guide.md` sandboxing section is good
reference — rewrite user-facing with more depth.

**Content outline:**

```markdown
# sandboxing

tein's sandbox is designed as a trust boundary — the layer that separates
untrusted Scheme code from host capabilities. composable: use only the layers you need.

## the four layers

| layer | what it controls | API |
|-------|-----------------|-----|
| module restriction | which R7RS libraries Scheme can import | .sandboxed(Modules::...) |
| VFS gate | enforces module restriction at import time | set automatically by .sandboxed() |
| file IO policy | which filesystem paths can be opened | .file_read(), .file_write() |
| step limit / timeout | resource exhaustion | .step_limit(n), .build_timeout(d) |

All four are independent and composable.

## module restriction

### Modules variants

```rust
use tein::sandbox::Modules;

// conservative safe set: scheme/base, scheme/write, scheme/read,
// all srfi/*, all tein/* — no scheme/eval, scheme/repl
.sandboxed(Modules::Safe)

// all vetted VFS modules — superset of Safe, includes scheme/eval, scheme/repl
.sandboxed(Modules::All)

// syntax only: define, if, lambda, begin, quote, import — all imports blocked
.sandboxed(Modules::None)

// explicit list — transitive deps resolved automatically
.sandboxed(Modules::only(&["scheme/base", "scheme/write"]))
```

### allow_module()

Extend any Modules preset:

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .allow_module("tein/process")  // explicitly add process module
    .build()?;
```

### UX stubs

With Modules::None, tein injects stubs for every known binding. Calling a stubbed
binding returns an informative error instead of an opaque undefined-variable error:

```scheme
(map (lambda (x) x) '(1 2 3))
;; => "sandbox: 'map' requires (import (scheme base))"
```

This means LLM-generated code gets actionable error messages even in fully restricted envs.

### what Modules::Safe includes / excludes

Safe includes: scheme/base, scheme/char, scheme/write, scheme/read, scheme/complex,
scheme/inexact, scheme/lazy, scheme/case-lambda, scheme/cxr, all srfi/*, all tein/*
except tein/process (see below)

Safe excludes: scheme/eval, scheme/repl, scheme/load, scheme/file (use tein/file instead)

tein/process is excluded from Safe because command-line leaks host argv. Add explicitly
with .allow_module("tein/process") when needed.

## file IO policy

Allow filesystem access to specific path prefixes only. Applied in sandboxed contexts.

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .file_read(&["/data/config/"])
    .file_write(&["/tmp/output/"])
    .build()?;
```

- paths are canonicalised before matching — protects against `../` traversal
- symlinks resolved — protects against symlink-based escapes
- open-input-file / open-output-file are the gated primitives
- file-exists? and delete-file are separate rust trampolines, also gated

Violations return Error::SandboxViolation with the attempted path.

## step limits

Cap total VM instructions:

```rust
let ctx = Context::builder()
    .step_limit(50_000)
    .build()?;

match ctx.evaluate("((lambda () (define (f) (f)) (f)))") {
    Err(tein::Error::StepLimitExceeded) => { /* expected */ }
    _ => panic!(),
}
```

step_limit is required when using TimeoutContext (so the context thread can terminate
after a wall-clock timeout fires).

## wall-clock timeouts

```rust
use std::time::Duration;

let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .step_limit(10_000_000)
    .build_timeout(Duration::from_secs(2))?;

match ctx.evaluate("(define (f) (f)) (f)") {
    Err(tein::Error::Timeout) => { /* expected */ }
    _ => panic!(),
}
```

## where the sandbox is heading

The current sandbox controls *what* Scheme code can access. the next layer is
*how* — host callbacks that intercept specific operations:

- environment variable reads (issue #99)
- per-call file IO interception (not just prefix policy)
- network access gates
- custom syscall-level policy handlers

the goal is a fully configurable permission system where the host can observe, modify,
or deny any privileged operation at runtime — not just at context-build time.
```

**Step 1: Write the file, commit**

```bash
git add docs/sandboxing.md
git commit -m "docs: add sandboxing.md — four-layer model, Modules, FsPolicy, timeout"
```

---

### Task 6: Write docs/rust-scheme-bridge.md

**Files:**
- Create: `docs/rust-scheme-bridge.md`

Read `tein-macros/src/lib.rs` (macro docs), `tein/src/foreign.rs` (ForeignType trait),
`tein/src/context.rs` (register_foreign_type, foreign_value, foreign_ref, register_reader,
set_macro_expand_hook). Current `docs/guide.md` sections on #[tein_fn], foreign types,
reader extensions, macro hooks are good reference.

**Content outline:**

```markdown
# rust–scheme bridge

expose Rust functions, types, and constants to Scheme with zero FFI boilerplate.

## #[tein_fn] — standalone functions

```rust
use tein::tein_fn;

#[tein_fn]
fn square(n: i64) -> i64 { n * n }

ctx.define_fn_variadic("square", __tein_square)?;
assert_eq!(ctx.evaluate("(square 7)")?, Value::Integer(49));
```

Generates `__tein_{fn_name}` with chibi's variadic FFI signature.

### supported types

| rust type | scheme type |
|-----------|-------------|
| i64 | integer |
| f64 | float |
| String | string |
| bool | boolean |
| Value | any scheme value |

Return Result<T, E: Display> — Err becomes a scheme error string.
Return () for void.

### the Value argument type

Use `value: Value` to accept any scheme value (e.g. for predicates):

```rust
#[tein_fn]
fn my_pred(value: Value) -> bool {
    matches!(value, Value::Integer(_))
}
```

## #[tein_module] — full module pattern

Groups functions, types, and constants into an importable `(tein module-name)` VFS module.

```rust
use tein_macros::{tein_module, tein_fn, tein_type, tein_methods, tein_const};

#[tein_module("mymod")]
pub(crate) mod mymod_impl {
    /// greet someone
    #[tein_fn]
    pub fn greet(name: String) -> String {
        format!("hello, {name}!")
    }

    /// the answer to everything
    #[tein_const]
    pub const ANSWER: i64 = 42;

    /// a counter type
    #[tein_type]
    pub struct Counter { pub n: i64 }

    #[tein_methods]
    impl Counter {
        pub fn get(&self) -> i64 { self.n }
        pub fn increment(&mut self) -> i64 { self.n += 1; self.n }
    }
}

// register before importing
mymod_impl::register_module_mymod(&ctx)?;
// scheme: (import (tein mymod))
//   mymod-greet, answer, counter?, counter-get, counter-increment
```

### naming conventions

| rust name | scheme name | note |
|-----------|-------------|------|
| `greet` (free fn) | `mymod-greet` | module prefix added |
| `ANSWER` (const) | `answer` | no module prefix |
| `Counter` (type) | `counter` | kebab-case |
| `get` (method) | `counter-get` | type prefix added |
| `is_valid_q` | `mymod-is-valid?` | `_q` → `?` |
| `set_bang` | `mymod-set!` | `_bang` → `!` |
| `_` in name | `-` | underscores → hyphens |

Override with `#[tein_fn(name = "scheme-name")]` or `#[tein_type(name = "scheme-name")]`.

### #[tein_const] note

Constants get no module prefix — `#[tein_const] pub const MY_VALUE` → scheme name `my-value`.
Free fns do get the prefix (`mymod-fn-name`).

## ForeignType — manual implementation

Alternative to `#[tein_module]` when you need more control:

```rust
use tein::{ForeignType, MethodFn, Value};

struct Counter { n: i64 }

impl ForeignType for Counter {
    fn type_name() -> &'static str { "counter" }
    fn methods() -> &'static [(&'static str, MethodFn)] {
        &[
            ("get", |obj, _ctx, _args| {
                Ok(Value::Integer(obj.downcast_ref::<Counter>().unwrap().n))
            }),
            ("increment", |obj, _ctx, _args| {
                let c = obj.downcast_mut::<Counter>().unwrap();
                c.n += 1;
                Ok(Value::Integer(c.n))
            }),
        ]
    }
}

ctx.register_foreign_type::<Counter>()?;
// auto-generated: counter?, counter-get, counter-increment
```

### creating and using foreign values

```rust
let val = ctx.foreign_value(Counter { n: 0 })?;

// call from rust
let inc = ctx.evaluate("counter-increment")?;
ctx.call(&inc, &[val.clone()])?;

// or from scheme
ctx.evaluate("(counter-get my-counter)")?;  // => Value::Integer(1)
```

Get a typed reference back:

```rust
let c_ref = ctx.foreign_ref::<Counter>(&val)?;  // &Counter
```

### introspection

```scheme
(foreign-types)                ; => ("counter") — all registered type names
(foreign-methods "counter")    ; => (get increment)
(foreign-type my-counter)      ; => "counter"
(foreign-handle-id my-counter) ; => 1  (monotonic handle ID)
```

Error messages list available methods on wrong-method call — designed to be useful in
LLM tool errors.

## ctx.call() — calling scheme from rust

Retrieve a scheme procedure and call it:

```rust
ctx.evaluate("(define (add a b) (+ a b))")?;
let add_fn = ctx.evaluate("add")?;
let result = ctx.call(&add_fn, &[Value::Integer(3), Value::Integer(4)])?;
assert_eq!(result, Value::Integer(7));
```

## reader extensions

Register custom `#` dispatch characters:

### from rust

```rust
let handler = ctx.evaluate("(lambda (port) 42)")?;
ctx.register_reader('j', &handler)?;
assert_eq!(ctx.evaluate("#j")?, Value::Integer(42));
```

### from scheme

```scheme
(import (tein reader))
(set-reader! #\j (lambda (port) (list 'json (read port))))
;; #j(1 2 3) → (json (1 2 3))
```

Other exports: `unset-reader!`, `reader-dispatch-chars`

Reserved characters (cannot override): `t`, `f`, `\`, `(`, numeric prefixes.

The dispatch table is thread-local and cleared on Context drop.

## macro expansion hooks

Intercept every macro expansion at analysis time. Return value replaces the
expansion and is re-analysed (replace-and-reanalyse semantics).

```scheme
(import (tein macro))

;; observe without changing
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    expanded))

;; transform: log every 'when expansion
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    (when (eq? name 'when)
      (display (list 'expanding name unexpanded)))
    expanded))
```

### from rust

```rust
let hook = ctx.evaluate("(lambda (name pre post env) post)")?;
ctx.set_macro_expand_hook(&hook)?;
// later:
ctx.unset_macro_expand_hook();
```

Hook args: `name` (symbol), `unexpanded` form, `expanded` form, syntactic environment.
Recursion guard prevents hook triggering on its own macro usage.
Hook cleared on Context drop.
```

**Step 1: Write the file, commit**

```bash
git add docs/rust-scheme-bridge.md
git commit -m "docs: add rust-scheme-bridge.md — tein_fn, tein_module, ForeignType, reader/macro hooks"
```

---

### Task 7: Write docs/modules.md

**Files:**
- Create: `docs/modules.md`

Read `target/chibi-scheme/lib/tein/json.scm`, `toml.scm`, `uuid.scm` (if exists — may be
dynamic), `time.scm`, `process.scm`, `docs.scm` for actual exports.
Read `tein/src/json.rs` and `tein/src/toml.rs` for representation tables.
Read `tein/src/uuid.rs` and `tein/src/time.rs` for #[tein_module] exports.

**Content outline:**

```markdown
# tein modules

built-in Scheme libraries backed by Rust crates. import them like any R7RS library.

all tein/* modules are in the `Modules::Safe` preset — no explicit allow_module() needed.
enable or disable at the cargo level with feature flags (see [reference.md](reference.md)).

---

## (tein json)

**feature:** `json` (default) | **deps added:** `serde`, `serde_json`

```scheme
(import (tein json))

(json-parse "{\"x\": 1, \"y\": [2, 3]}")
;; => (("x" . 1) ("y" 2 3))

(json-stringify '(("name" . "tein") ("version" . 1)))
;; => "{\"name\":\"tein\",\"version\":1}"
```

### representation

| JSON | Scheme |
|------|--------|
| object `{"k": v}` | alist `((k . v) ...)` |
| empty `{}` | `'()` (same as empty array — known ambiguity) |
| array `[...]` | list `(...)` |
| empty `[]` | `'()` |
| string | string |
| integer | integer |
| float | flonum |
| `true` / `false` | `#t` / `#f` |
| `null` | symbol `null` |

### exports
`json-parse`, `json-stringify`

---

## (tein toml)

**feature:** `toml` (default) | **deps added:** `toml`

```scheme
(import (tein toml))

(define doc (toml-parse "[server]\nhost = \"localhost\"\nport = 8080\n"))
;; => (("server" ("host" . "localhost") ("port" . 8080)))
```

### representation

| TOML | Scheme |
|------|--------|
| table | alist `((key . val) ...)` |
| array | list |
| string | string |
| integer | integer |
| float | flonum |
| boolean | boolean |
| datetime | tagged list `(toml-datetime "iso-string")` |

All four TOML datetime variants (offset datetime, local datetime, local date, local time)
use the same `toml-datetime` tag — the string content distinguishes them.

### exports
`toml-parse`, `toml-stringify`

---

## (tein uuid)

**feature:** `uuid` (default) | **deps added:** `uuid`

```scheme
(import (tein uuid))

(make-uuid)  ; => "f47ac10b-58cc-4372-a567-0e02b2c3d479"
(uuid? "f47ac10b-58cc-4372-a567-0e02b2c3d479")  ; => #t
uuid-nil     ; => "00000000-0000-0000-0000-000000000000"
```

### exports
`make-uuid`, `uuid?`, `uuid-nil`

---

## (tein time)

**feature:** `time` (default) | **deps added:** none (pure std::time)

```scheme
(import (tein time))

(current-second)         ; => 1740902400.0  (POSIX seconds, inexact)
(current-jiffy)          ; => 12345678      (nanoseconds, exact integer)
(jiffies-per-second)     ; => 1000000000
```

**jiffy epoch note:** `current-jiffy` counts nanoseconds from a process-relative epoch set
on the first call anywhere in the process. this epoch is shared across all Context instances —
per r7rs, it is "constant within a single run of the program".

### exports
`current-second`, `current-jiffy`, `jiffies-per-second`

---

## (tein process)

**feature:** none (always available) | **sandbox:** excluded from `Modules::Safe` by default

`(tein process)` provides `exit` and `emergency-exit` — an escape hatch that stops
evaluation and returns a value to the rust caller.

```scheme
(import (tein process))

(exit)        ; => Ok(Value::Integer(0)) in rust
(exit #t)     ; => Ok(Value::Integer(0))
(exit #f)     ; => Ok(Value::Integer(1))
(exit "done") ; => Ok(Value::String("done"))
```

This is tein's mechanism for clean early returns from scheme code — useful when scheme
is acting as a script that wants to signal a result code.

**r7rs deviation:** both `exit` and `emergency-exit` have emergency-exit semantics in tein —
neither runs `dynamic-wind` "after" thunks. r7rs `exit` should run them. see issue #101.

**sandbox caveat:** excluded from `Modules::Safe` because command-line leaks host argv
via `(command-line)`. enable explicitly:

```rust
.sandboxed(Modules::Safe)
.allow_module("tein/process")
```

### exports
`exit`, `emergency-exit`

---

## (tein docs)

**feature:** none | **sandbox:** included in `Modules::Safe`

provides runtime access to module documentation alists generated by `#[tein_module]`.
designed for LLM context dumps — a module can describe itself to an agent.

```scheme
(import (tein docs))

;; given a doc alist from a #[tein_module]:
(describe mymod-docs)
;; => "(tein mymod)\n  mymod-greet — greet someone\n  answer — the answer to everything\n"

(module-doc mymod-docs 'mymod-greet)
;; => "greet someone"

(module-docs mymod-docs)
;; => ((mymod-greet . "greet someone") (answer . "the answer to everything") ...)
```

See [tein-for-agents.md](tein-for-agents.md) for how `(tein docs)` fits into the agent
tooling story.

### exports
`describe`, `module-doc`, `module-docs`
```

**Step 1: Write the file, commit**

```bash
git add docs/modules.md
git commit -m "docs: add modules.md — (tein json/toml/uuid/time/process/docs)"
```

---

### Task 8: Write docs/extensions.md

**Files:**
- Create: `docs/extensions.md`

Read `tein-ext/src/lib.rs` (vtable, API version, type aliases), `tein/src/foreign.rs`
(ExtTypeEntry, ext dispatch), `tein/src/context.rs` (load_extension),
`tein-macros/src/lib.rs` (ext = true path in tein_module), `tests/ext_loading.rs`.
Read `tein-test-ext/` as a concrete example to reference.

**Content outline:**

```markdown
# cdylib extensions

load compiled Rust code into a tein context at runtime via a stable C ABI.

## when to use extensions vs. inline #[tein_module]

| | inline #[tein_module] | cdylib extension |
|--|----------------------|-----------------|
| compiled into host binary | yes | no |
| distributable separately | no | yes |
| can use private crates | yes | no |
| stable ABI | n/a | yes |
| unloadable | n/a | no |

use extensions when you want to ship Scheme modules as standalone `.so` files —
plugins, optional capabilities, or code that updates independently of the host binary.

## extension crate structure

Extension crates depend on `tein-ext` and `tein-macros` — never on `tein` itself.
This keeps the dependency footprint minimal and avoids ABI coupling.

```toml
[dependencies]
tein-ext = { git = "https://github.com/emesal/tein" }
tein-macros = { git = "https://github.com/emesal/tein" }

[lib]
crate-type = ["cdylib"]
```

## writing an extension

`#[tein_module("name", ext = true)]` generates everything needed:

```rust
use tein_macros::{tein_module, tein_fn, tein_type, tein_methods};

#[tein_module("myext", ext = true)]
mod myext_impl {
    #[tein_fn]
    pub fn hello(name: String) -> String {
        format!("hello from extension, {name}!")
    }

    #[tein_type]
    pub struct Widget { pub id: i64 }

    #[tein_methods]
    impl Widget {
        pub fn get_id(&self) -> i64 { self.id }
    }
}
```

The macro generates:
- `tein_ext_init` — the C entry point resolved by `load_extension()`
- API version check at init time
- VFS module registration via the host vtable
- Foreign type registration via the host vtable

## loading an extension from rust

```rust
ctx.load_extension("./libmyext.so")?;

// scheme can now:
// (import (tein myext))
// (myext-hello "world")   => "hello from extension, world!"
// (make-widget 42)        => #<widget:1>
// (widget-get-id w)       => 42
```

The library is loaded once and leaked — no unload mechanism exists. The extension
stays live for the process lifetime.

## stable C ABI

`tein-ext` defines the `TeinExtApi` vtable — a C-compatible struct of function pointers
that the host populates and passes to `tein_ext_init`. Extensions call host capabilities
through this vtable, never through direct function calls.

The `TEIN_EXT_API_VERSION` constant is checked at init time. A version mismatch returns
`TEIN_EXT_ERR_VERSION` and the extension is rejected.

When adding fields to `TeinExtApi`, bump `TEIN_EXT_API_VERSION`.

## caveats

- **no unload** — `Library::new()` is leaked; `dlclose()` is never called. the
  extension's code stays mapped for the process lifetime.
- **linux only today** — `.dylib` (macOS) and `.dll` (Windows) loading not yet
  implemented. tracked in issue #66.
- **panic safety** — panics in extension code unwind across the FFI boundary, which
  is undefined behaviour. extension methods should catch panics or avoid them.
```

**Step 1: Write the file, commit**

```bash
git add docs/extensions.md
git commit -m "docs: add extensions.md — cdylib extension system, tein-ext, stable ABI"
```

---

### Task 9: Write docs/tein-for-agents.md

**Files:**
- Create: `docs/tein-for-agents.md`

This doc is a narrative/pitch, not a reference. Write it to be useful to someone evaluating
tein as an execution environment for LLM-generated code. Tone: precise, honest about what's
shipped vs. planned. Forward pointers to issues should link to GitHub issue numbers.

**Content outline:**

```markdown
# tein for LLM agents

tein is designed with LLM coding agents as a first-class audience. this doc explains
how — what properties tein has that make it a good execution substrate for agent-generated
code, and what the roadmap holds for agent tooling specifically.

## why scheme for agent tools

Scheme is a natural fit for agent-executed code:

- **homoiconic** — code is data. agents can construct, inspect, and transform programs as
  lists. the macro system means agents can extend the language itself.
- **sandboxable** — R7RS has a clean separation between what's in scope and what can be
  imported. tein's sandbox maps directly onto this: the null env is exactly the set of
  capabilities the host grants.
- **minimal and predictable** — a small, well-specified language with no hidden globals,
  no ambient capabilities, no implicit IO. agents can reason about what code will do.
- **composable** — `(import ...)` is the capability system. an agent knows exactly what
  it has access to by looking at its imports.

## the sandbox as a trust boundary

tein's sandbox is designed to be the trust boundary between agent-generated code and the
host environment. an agent gets exactly the capabilities the host grants — nothing more.

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)  // known-safe module set
    .step_limit(100_000)        // no infinite loops
    .file_read(&["/data/"])     // explicit filesystem grant
    .build()?;
```

the sandbox has four independent layers:

1. **module restriction** — which R7RS libraries can be imported
2. **VFS gate** — enforces restriction at the C level (not bypassable from scheme)
3. **file IO policy** — path-prefix-based filesystem access with traversal protection
4. **resource limits** — step limits and wall-clock timeouts

an agent running inside a `Modules::Safe` context cannot import `scheme/eval`, cannot
load arbitrary files, cannot spawn processes, and cannot run forever. these are hard
guarantees enforced at the C level and in the rust sandbox layer.

## LLM-navigable error messages

tein's error messages are designed to be useful to an LLM reading them:

**wrong module:**
```scheme
(map (lambda (x) x) '(1 2))
;; sandbox: 'map' requires (import (scheme base))
```

**wrong foreign method:**
```scheme
(counter-frobnicate c)
;; no method 'frobnicate' on type 'counter'. available: get, increment
```

**sandbox violation:**
```scheme
(open-input-file "/etc/passwd")
;; sandbox: file read denied: /etc/passwd (not under allowed prefix)
```

these errors tell the agent exactly what to do next — import the right module, use the
right method name, adjust the file policy. no cryptic C-level error codes.

## self-describing environments: (tein docs)

`#[tein_module]` generates documentation alists from rust doc comments. scheme code can
query these at runtime:

```scheme
(import (tein docs))
(describe mymod-docs)
;; (tein mymod)
;;   mymod-greet — greet someone by name
;;   answer — the answer to everything (42)
;;   counter? — predicate for counter type
;;   counter-get — get the counter value
;;   counter-increment — increment and return new value
```

an agent can dump this into its context before writing code that uses the module —
zero latency, no external tooling required.

## introspectable foreign types

every registered foreign type exposes its own metadata:

```scheme
(foreign-types)               ; all type names in this context
(foreign-methods "counter")   ; method names for a specific type
(foreign-type obj)            ; type name of a foreign value
```

agents can discover what types and methods exist without needing documentation.

## predictable scope

tein's sandboxed contexts use a null env — a clean environment with only the explicitly
granted bindings. there are no hidden globals, no ambient `load` or `eval` unless
granted, no way to reach outside the sandbox via side channels.

what an agent sees is what it gets. `(define x 1)` binds `x` in the null env and
nowhere else. imports work exactly as documented.

## what's coming for agent tooling

**(tein introspect)** — planned in milestone 9 (issue #83). environment introspection API:
query live bindings, procedure arity, module exports, binding metadata — all from within
a running scheme context. an agent inside a tein sandbox would be able to ask "what
procedures are in scope?" and "what arguments does this function take?" without an
external LSP or static analyser.

**fake environment variables** — planned (issue #99). host-injectable env var overrides
so agents can run with a controlled `getenv` view, separate from the host process environment.

**`Modules::Safe` vet** — ongoing (issue #92). systematic review of all chibi/* and srfi/*
VFS modules to confirm sandbox safety. as more modules are vetted, Modules::Safe grows.
```

**Step 1: Write the file, commit**

```bash
git add docs/tein-for-agents.md
git commit -m "docs: add tein-for-agents.md — sandbox model, LLM-navigable errors, agent design"
```

---

### Task 10: Write docs/reference.md

**Files:**
- Create: `docs/reference.md`

Read `tein/src/value.rs` (all variants), `Cargo.toml` (feature flags), `tein/src/vfs_registry.rs`
(full VFS_REGISTRY), `ARCHITECTURE.md` scheme env quirks section, `AGENTS.md` r7rs deviations.

**Content outline:**

```markdown
# reference

## Value variants

| Scheme type | Rust variant | Display example |
|-------------|-------------|-----------------|
| exact integer | Value::Integer(i64) | 42 |
| inexact float | Value::Float(f64) | 3.14 |
| boolean | Value::Boolean(bool) | #t |
| string | Value::String(String) | "hello" |
| symbol | Value::Symbol(String) | foo |
| proper list | Value::List(Vec<Value>) | (1 2 3) |
| improper pair | Value::Pair(Box<Value>, Box<Value>) | (a . b) |
| empty list / nil | Value::Nil | () |
| void | Value::Void | |
| character | Value::Char(char) | #\a |
| bytevector | Value::Bytevector(Vec<u8>) | #u8(1 2 3) |
| port (opaque) | Value::Port | #<port> |
| hash table (opaque) | Value::HashTable | #<hash-table> |
| procedure | Value::Procedure | #<procedure> |
| foreign object | Value::Foreign{handle_id, type_name} | #<counter:1> |

## feature flags

| flag | default | description | deps |
|------|---------|-------------|------|
| json | yes | enables (tein json) with json-parse/json-stringify | serde, serde_json |
| toml | yes | enables (tein toml) with toml-parse/toml-stringify | toml |
| uuid | yes | enables (tein uuid) with make-uuid, uuid?, uuid-nil | uuid |
| time | yes | enables (tein time) with current-second, current-jiffy | none (std::time) |

Disable all with `default-features = false` for a minimal build:

```toml
tein = { git = "...", default-features = false }
# re-enable selectively:
tein = { git = "...", default-features = false, features = ["json", "uuid"] }
```

## VFS module list

All modules embedded in the VFS — available for import in standard-env contexts.
`Safe` column: included in Modules::Safe preset.

| module | safe | description |
|--------|------|-------------|
| tein/foreign | yes | foreign? predicate, type/handle accessors |
| tein/reader | yes | set-reader!, unset-reader!, reader-dispatch-chars |
| tein/macro | yes | set-macro-expand-hook!, unset-macro-expand-hook! |
| tein/test | yes | test-equal, test-error, test-assert (scheme test framework) |
| tein/docs | yes | describe, module-doc, module-docs |
| tein/json | yes | json-parse, json-stringify (feature: json) |
| tein/toml | yes | toml-parse, toml-stringify (feature: toml) |
| tein/uuid | yes | make-uuid, uuid?, uuid-nil (feature: uuid) |
| tein/time | yes | current-second, current-jiffy, jiffies-per-second (feature: time) |
| tein/file | yes | tein-safe wrappers for open-input-file etc. |
| tein/load | yes | load (VFS-restricted) |
| tein/process | yes* | exit, emergency-exit (*excluded from Safe by default) |
| scheme/base | yes | core R7RS procedures |
| scheme/char | yes | character classification and conversion |
| scheme/write | yes | display, write, newline |
| scheme/read | yes | read |
| scheme/inexact | yes | finite?, infinite?, nan? |
| scheme/lazy | yes | delay, force, promise? |
| scheme/case-lambda | yes | case-lambda |
| scheme/cxr | yes | caaar...cdddr |
| scheme/complex | yes | real-part, imag-part, angle, magnitude |
| scheme/eval | no | eval, environment (excluded from Safe) |
| scheme/repl | no | interaction-environment (excluded from Safe) |
| scheme/file | no | open-input-file (use tein/file in sandbox) |
| srfi/1 | yes | list library |
| srfi/13 | yes | string library |
| srfi/14 | yes | character sets |
| ... | yes | (full srfi/* list in vfs_registry.rs) |

## scheme environment quirks

### what's available without any import

In a `Context::new_standard()` / `.standard_env()` context, these are available
without importing anything:

- control: `cond`, `case`, `and`, `or`, `do`, `when`, `unless`
- binding: `let`, `let*`, `letrec`, `letrec*`, named `let`
- continuations: `dynamic-wind`, `call/cc`, `call-with-current-continuation`,
  `values`, `call-with-values`
- exceptions: `with-exception-handler`, `raise`, `raise-continuable`
- syntax: `define-syntax`, `syntax-rules`, `let-syntax`, `letrec-syntax`, `quasiquote`
- eval: `eval`, `interaction-environment`, `scheme-report-environment`

### what requires (import (scheme base))

- `define-values`, `guard`, `error-object?`, `error-object-message`, `error-object-irritants`
- `floor/`, `truncate/`
- `define-record-type` — syntax is present without import but accessor/mutator generation
  is broken without the import
- bytevector API: `bytevector`, `make-bytevector`, `bytevector-u8-ref`, etc.

### call/cc re-entry

Calling a saved continuation from a separate `ctx.evaluate()` call does not re-enter
(C stack boundary). Within a single evaluate call, re-entry fails when mutable state
is in top-level `define`s — use `let` bindings instead:

```scheme
;; works:
(let ((k #f) (n 0))
  (call/cc (lambda (c) (set! k c)))
  (set! n (+ n 1))
  (if (< n 3) (k 'ignored) n))  ; => 3

;; does NOT work (top-level defines reset on re-entry):
(define saved-k #f)
(define counter 0)
(call/cc (lambda (k) (set! saved-k k)))
(set! counter (+ counter 1))
(if (< counter 3) (saved-k #f) counter)  ; => 1, not 3
```

### define-values in single-batch evaluate

`define-values` introducing top-level bindings mid-batch can corrupt subsequent
expression evaluation in the same `evaluate()` call. Use `call-with-values`:

```scheme
;; instead of:
(define-values (q r) (floor/ 13 4))
(test-equal "q" 3 q)

;; use:
(call-with-values (lambda () (floor/ 13 4))
  (lambda (q r) (test-equal "q" 3 q)))
```

### let binding order

`let` bindings are evaluated in unspecified order. For sequential side-effectful
operations (e.g. multiple `read` calls), use `let*`.

### (tein foreign) in standard env

`foreign.scm` uses `fixnum?` which is a chibi builtin but not exported by `(scheme base)`.
`(import (tein foreign))` works in unsandboxed contexts where `fixnum?` is in the toplevel.
In sandboxed contexts use the inline predicates or `integer?` instead of `fixnum?`.

## known r7rs deviations

### exit and dynamic-wind (issue #101)

Both `exit` and `emergency-exit` in `(tein process)` have emergency-exit semantics —
neither runs `dynamic-wind` "after" thunks. R7RS `exit` should run them.

A future standalone interpreter host is expected to establish the unwind continuation
needed for this. The current tein library API does not establish one.
```

**Step 1: Write the file, commit**

```bash
git add docs/reference.md
git commit -m "docs: add reference.md — Value types, feature flags, VFS modules, env quirks"
```

---

### Task 11: Rewrite docs/guide.md as index

**Files:**
- Modify: `docs/guide.md` (full rewrite)

Current `guide.md` is a monolithic walkthrough (~550 lines). Replace it with a TOC/index.

**Content:**

```markdown
# tein docs

tein is an embeddable R7RS Scheme interpreter for Rust. this is the documentation index.

## reading order

**"i want to embed scheme in my rust app"**
→ [quickstart](quickstart.md) → [embedding](embedding.md) → [sandboxing](sandboxing.md)

**"i want to expose rust functions and types to scheme"**
→ [quickstart](quickstart.md) → [rust–scheme bridge](rust-scheme-bridge.md)

**"i'm building an agent execution environment"**
→ [tein for agents](tein-for-agents.md) → [sandboxing](sandboxing.md) → [modules](modules.md)

**"i want to write a tein extension module"**
→ [rust–scheme bridge](rust-scheme-bridge.md) → [extensions](extensions.md)

**"i need the full API / value type reference"**
→ [reference](reference.md)

## docs

| doc | what it covers |
|-----|---------------|
| [quickstart](quickstart.md) | `Context::new`, `evaluate`, `Value`, `#[tein_fn]` — working in 5 minutes |
| [embedding](embedding.md) | context types, `ContextBuilder` API, `Value` enum, `ctx.call()`, custom ports |
| [sandboxing](sandboxing.md) | four-layer sandbox model, `Modules`, `FsPolicy`, step limits, timeouts |
| [rust–scheme bridge](rust-scheme-bridge.md) | `#[tein_fn]`, `#[tein_module]`, `ForeignType`, reader extensions, macro hooks |
| [modules](modules.md) | built-in `(tein json/toml/uuid/time/process/docs)` modules |
| [extensions](extensions.md) | cdylib extension system, `tein-ext`, stable C ABI |
| [tein for agents](tein-for-agents.md) | sandbox as trust boundary, LLM-navigable errors, agent design |
| [reference](reference.md) | `Value` variants, feature flags, VFS module list, scheme env quirks |

## for contributors

[ARCHITECTURE.md](../ARCHITECTURE.md) — internal architecture, data flows, chibi safety invariants.
[AGENTS.md](../AGENTS.md) — coding conventions, workflow, project principles.
[ROADMAP.md](../ROADMAP.md) — milestone plan and github issues.
[docs/plans/](plans/) — design documents and implementation plans.
```

**Step 1: Write the file, commit**

```bash
git add docs/guide.md
git commit -m "docs: rewrite guide.md as index/TOC — links all new docs/"
```

---

### Task 12: Update ARCHITECTURE.md

**Files:**
- Modify: `ARCHITECTURE.md`

Read the full current `ARCHITECTURE.md`. The following sections need updating:

1. **"Current milestone"** section — M8 is not "in progress"; json/toml/uuid/time/tein_module/
   extensions are shipped. Update to reflect reality (see closed M8 issues in the plan).

2. **Commands section** — update test count to match current `just test` output.
   Run `just test 2>&1 | tail -5` to get current counts.

3. **Directory structure** — add missing src files:
   - `vfs_registry.rs` — VFS module registry, single source of truth
   - `thread.rs` — shared channel protocol (Request/Response/SendableValue)
   - `sexp_bridge.rs` — Value ↔ Sexp shared layer for format modules
   - `json.rs` — json_parse + json_stringify_raw
   - `toml.rs` — toml_parse + toml_stringify_raw
   - `uuid.rs` — #[tein_module]: make-uuid, uuid?, uuid-nil
   - `time.rs` — #[tein_module]: current-second, current-jiffy, jiffies-per-second

4. **eval.c patch count** — currently says "4 patches". It's 7 patches (A–G). Update.

5. **tein scheme modules list** — currently only lists foreign/reader/macro/test. Add:
   json, toml, uuid, time, file, load, process, docs

6. **Add note** at top or in "Building & testing" that user-facing docs live in `docs/`.

7. **Add missing flow descriptions** (copy from AGENTS.md and condense):
   - VFS shadow injection flow (after sandboxing flow)
   - FS policy gate flow (C-level, after IO policy flow)
   - exit escape hatch flow

**Step 1: Read full ARCHITECTURE.md, make targeted edits, commit**

```bash
git add ARCHITECTURE.md
git commit -m "docs: update ARCHITECTURE.md — M8 status, src files, eval.c patches, new flows"
```

---

### Task 13: Update ROADMAP.md

**Files:**
- Modify: `ROADMAP.md`

The following M8 items are shipped (closed issues) and must move from the roadmap section
into the "completed milestones" section:

**completed in M8:**
- `(tein json)` — JSON via serde_json
- `(tein toml)` — TOML parsing and serialisation
- `(tein uuid)` — UUID generation
- `(tein time)` — r7rs time procedures
- `#[tein_module]` proc macro — rust→scheme module generation
- `#[tein_const]` — constants in tein_module
- doc attr scraping in `#[tein_module]`
- `(tein docs)` module — runtime doc access
- cdylib extension system — `tein-ext`, stable C ABI, `load_extension()`
- type parity — close tein-sexp ↔ chibi-scheme type gap
- feature-gated format modules — `json`/`toml`/`uuid`/`time` feature flags

**still open in M8 (keep in roadmap):**
- `(tein regex)` / SRFI-115 (`(chibi regexp)`) — issue #85, #37
- `(tein crypto)` — issue #38
- cross-platform cdylib (.dylib, .dll) — issue #66
- SRFI-19 time data types via rust trampolines — issue #84
- foreign type constructor macro — issue #41

**Step 1: Read full ROADMAP.md, make targeted edits, commit**

```bash
git add ROADMAP.md
git commit -m "docs: update ROADMAP.md — move shipped M8 items to completed, list remaining M8 work"
```

---

### Task 14: Final pass and PR

**Step 1: Review all new files**

Check each new docs/ file:
- links between docs are correct (relative paths)
- no placeholder text or "TBD"
- code examples are valid (match actual API)
- feature flag names match Cargo.toml

**Step 2: Run just lint to confirm no accidental code changes**

```bash
just lint
```

Expected: clean (we only touched markdown).

**Step 3: Push and open PR**

```bash
git push -u origin docs/restructure-2603
gh pr create \
  --title "docs: full restructure — 8 new docs/, README rewrite, ARCHITECTURE/ROADMAP sync" \
  --body "$(cat <<'EOF'
## Summary

- Rewrites README as a lean landing page (removes stale roadmap section)
- Replaces monolithic `docs/guide.md` walkthrough with a TOC/index + 8 focused docs
- New: quickstart, embedding, sandboxing, rust-scheme-bridge, modules, extensions, tein-for-agents, reference
- Updates ARCHITECTURE.md: M8 status, src file list, eval.c patch count, missing flow descriptions
- Updates ROADMAP.md: moves shipped M8 items (json/toml/uuid/time/tein_module/extensions/docs) to completed

## Audience

Rust developers familiar with Rust and basic R7RS. No prior embedding experience required.
LLM coding agents are a first-class audience — reflected in `tein-for-agents.md` and
throughout the sandboxing and error message sections.

## Test plan

- [ ] All relative links between docs resolve correctly
- [ ] Code examples match actual API (Value variants, builder methods, macro signatures)
- [ ] Feature flag names match Cargo.toml
- [ ] `just lint` clean
EOF
)"
```

---

## notes for AGENTS.md

After completing this plan, add to AGENTS.md:
- docs/ structure is now: guide.md (index), quickstart, embedding, sandboxing,
  rust-scheme-bridge, modules, extensions, tein-for-agents, reference
- ARCHITECTURE.md is contributor-facing; user docs live in docs/
- when adding new features, update the relevant docs/ file + reference.md VFS module list
