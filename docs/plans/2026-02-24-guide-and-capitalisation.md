# Guide and capitalisation implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Write a newcomer-friendly guide (`docs/guide.md`) and normalise all prose to sentence case throughout the project's markdown and Rust doc comments.

**Architecture:** Two sequential tracks. First, capitalisation normalisation across all existing docs and rustdoc (mechanical find-and-fix). Second, write the new guide from scratch covering every major tein feature with narrative explanations, the VFS model, decision trees, and working examples drawn from existing tests/examples.

**Capitalisation rules:**
- Headings and prose: sentence case (first word only, plus proper nouns)
- Proper nouns always capitalised: `Rust`, `Scheme`, `R7RS`, `Chibi`, `Chibi-Scheme`
- Acronyms always capitalised: `API`, `FFI`, `VFS`, `GC`, `RAII`, `REPL`, `AST`, `VM`
- tein stays lowercase (stylistic, like `git`)
- Type/function names unchanged: `Context`, `ContextBuilder`, `Value::Integer`, `#[scheme_fn]`, etc.
- Code blocks, inline code, scheme symbols: unchanged

**Tech stack:** Markdown, Rust rustdoc (`//!` and `///`), `cargo doc`, `cargo test --doc`

---

### Task 1: Capitalise README.md

**Files:**
- Modify: `README.md`

**Step 1: Apply sentence case to all headings and prose**

Open `README.md`. Current lowercase headings to fix:

```
## quick start          → ## Quick start
## features             → ## Features
### sandboxing & resource limits  → ### Sandboxing & resource limits
### `#[scheme_fn]` proc macro     → ### `#[scheme_fn]` proc macro   (no change, starts with code)
### foreign type protocol         → ### Foreign type protocol
### custom ports                  → ### Custom ports
### reader extensions             → ### Reader extensions
### macro expansion hooks         → ### Macro expansion hooks
### managed contexts              → ### Managed contexts
## examples             → ## Examples
## about                → ## About
```

Prose fixes — apply sentence case to all sentences. Key substitutions:
- `embeddable r7rs scheme interpreter` → `Embeddable R7RS Scheme interpreter`
- `safe rust API` → `Safe Rust API`
- `zero runtime dependencies` → `Zero runtime dependencies`
- `restrict the environment` → `Restrict the environment`
- `define scheme-callable functions` → `Define Scheme-callable functions`
- `expose rust types` → `Expose Rust types`
- `bridge rust Read/Write` → `Bridge Rust Read/Write`
- `register custom # dispatch` → `Register custom # dispatch`
- `intercept and transform` → `Intercept and transform`
- `thread-safe scheme evaluation` → `Thread-safe Scheme evaluation`
- `from old norse` → `From Old Norse`
- `**why scheme?**` → `**Why Scheme?**`
- `**why chibi?**` → `**Why Chibi?**`
- `**why tein?**` → `**Why tein?**`
- `*carved with care, grown with intention*` → unchanged (stylistic, no capital needed)

Also in the examples table, description column: capitalise first word of each description.
- `evaluate expressions, pattern-match on values` → `Evaluate expressions, pattern-match on values`
- `floating-point arithmetic` → `Floating-point arithmetic`
- etc.

**Step 2: Verify**

Read the file and eyeball that headings and prose look correct.

**Step 3: Commit**

```bash
git add README.md
git commit -m "docs: sentence case throughout README"
```

---

### Task 2: Capitalise ARCHITECTURE.md

**Files:**
- Modify: `ARCHITECTURE.md`

**Step 1: Apply sentence case to all headings**

```
# tein architecture              → # tein architecture   (project name, leave)
## project status                → ## Project status
### completed milestones         → ### Completed milestones
**milestone 1 — ...**           → **Milestone 1 — ...**  (bold labels = headings)
**milestone 2 — ...**           → **Milestone 2 — ...**
...all milestone bold labels...
### known limitations            → ### Known limitations
## architecture                  → ## Architecture
### directory structure          → ### Directory structure
### data flow                    → ### Data flow
### sandboxing flow              → ### Sandboxing flow
### IO policy flow               → ### IO policy flow
### module import policy         → ### Module import policy
### thread safety                → ### Thread safety
### key design decisions         → ### Key design decisions
## building & testing            → ## Building & testing
## adding a new scheme type      → ## Adding a new Scheme type
## registering rust functions in scheme  → ## Registering Rust functions in Scheme
## conventions                   → ## Conventions
## foreign type protocol         → ## Foreign type protocol
### architecture                 → ### Architecture
### implementing ForeignType     → ### Implementing ForeignType
### registration and use         → ### Registration and use
### dispatch chain               → ### Dispatch chain
### scheme-side protocol         → ### Scheme-side protocol
## custom port protocol          → ## Custom port protocol
### architecture                 → ### Architecture (second one)
### creating ports               → ### Creating ports
### chibi protocol details       → ### Chibi protocol details
## reader dispatch protocol      → ## Reader dispatch protocol
### architecture                 → ### Architecture (third one)
### usage                        → ### Usage
### design notes                 → ### Design notes
## macro expansion hook protocol → ## Macro expansion hook protocol
### architecture                 → ### Architecture (fourth one)
### usage                        → ### Usage (second one)
### design notes                 → ### Design notes (second one)
```

Prose: apply sentence case to each sentence in paragraph form. Code blocks unchanged.

Key substitution patterns in prose:
- Sentence-starting `the`, `a`, `an`, `this`, `each`, `both`, `when`, `note`, `see`, `use` → capitalise first word
- `chibi-scheme` at sentence start → `Chibi-Scheme`
- `rust` at sentence start → `Rust`
- `scheme` at sentence start → `Scheme`

The **VFS safety contract** and **security layers** table section prose needs sentence-case treatment.

**Step 2: Commit**

```bash
git add ARCHITECTURE.md
git commit -m "docs: sentence case throughout ARCHITECTURE.md"
```

---

### Task 3: Capitalise TODO.md

**Files:**
- Modify: `TODO.md`

**Step 1: Apply sentence case to all headings and list items**

```
# tein todo list                  → # tein todo list  (leave, project name)
## completed                      → ## Completed
## completed milestones           → ## Completed milestones
### milestone 1 — ...             → ### Milestone 1 — ...
### milestone 2 — ...             → ### Milestone 2 — ...
...etc...
## roadmap                        → ## Roadmap
### milestone 4b — ...            → ### Milestone 4b — ...
### milestone 5 — reach           → ### Milestone 5 — Reach
### milestone 6 — ...             → ### Milestone 6 — ...
### milestone 7 — ...             → ### Milestone 7 — ...
## ideas (unscheduled)            → ## Ideas (unscheduled)
```

List item descriptions: sentence-case the descriptive text where it's prose (not a code name).
- `**#1: implement pair/list value extraction**` → `**#1: Implement pair/list value extraction**`
- `typed extraction helpers on Value` → `Typed extraction helpers on Value`
- etc.

**Step 2: Commit**

```bash
git add TODO.md
git commit -m "docs: sentence case throughout TODO.md"
```

---

### Task 4: Capitalise lib.rs rustdoc

**Files:**
- Modify: `tein/src/lib.rs`

**Step 1: Apply sentence case to module-level doc (`//!` lines)**

The file is short (83 lines). Current text with needed changes:

```
//! # tein                              → //! # tein  (project name, leave)
//! embeddable r7rs scheme...           → //! Embeddable R7RS Scheme interpreter for Rust...
//! safe rust API wrapping...           → //! Safe Rust API wrapping...
//! ## quick start                      → //! ## Quick start
//! ## features                         → //! ## Features
//! - **sandboxing** — restrict...      → //! - **Sandboxing** — Restrict...
//! - **`#[scheme_fn]`** — define...    → //! - **`#[scheme_fn]`** — Define...
//! - **foreign types** — expose...     → //! - **Foreign types** — Expose...
//! - **custom ports** — bridge...      → //! - **Custom ports** — Bridge...
//! - **reader extensions** — register  → //! - **Reader extensions** — Register...
//! - **macro expansion hooks** — intercept → //! - **Macro expansion hooks** — Intercept...
//! - **managed contexts** — thread-safe → //! - **Managed contexts** — Thread-safe...
//! - **timeouts** — wall-clock...      → //! - **Timeouts** — Wall-clock...
//! ## safety model                     → //! ## Safety model
//! [`Context`] is intentionally...     → //! [`Context`] is intentionally... (starts with type name, fine)
//! for cross-thread use, wrap in...    → sentence-case this sentence
```

**Step 2: Verify doc tests still pass**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test --doc 2>&1 | tail -5
```

Expected: all pass.

**Step 3: Commit**

```bash
git add tein/src/lib.rs
git commit -m "docs: sentence case in lib.rs rustdoc"
```

---

### Task 5: Capitalise context.rs rustdoc

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Apply sentence case to module-level `//!` lines**

```
//! scheme evaluation context           → //! Scheme evaluation context
//! [`Context`] is a single-threaded... → //! [`Context`] is a single-threaded... (starts with type, fine)
//! # builder pattern                   → //! # Builder pattern
//! # evaluation                        → //! # Evaluation
//! # sandboxing                        → //! # Sandboxing
//! four independent layers of control: → //! Four independent layers of control:
//! see the [`sandbox`]...              → //! See the [`sandbox`]...
```

**Step 2: Apply sentence case to all `///` item doc comments**

context.rs has ~470 item doc lines. Scan all `///` lines and apply sentence case:
- First word of each doc sentence → capitalise
- All headings in `///` blocks (`# heading`) → sentence case
- Inline code/type names → unchanged

Key patterns to search for and fix:
- `/// scheme` at start → `/// Scheme`
- `/// rust` at start → `/// Rust`
- `/// the ` at start → `/// The `
- `/// a ` at start → `/// A `
- `/// an ` at start → `/// An `
- `/// configure` at start → `/// Configure`
- `/// evaluate` at start → `/// Evaluate`
- `/// load` at start → `/// Load`
- `/// restrict` at start → `/// Restrict`
- `/// enable` at start → `/// Enable`
- `/// set` at start → `/// Set`
- `/// add` at start → `/// Add`
- `/// returns` at start → `/// Returns`
- `/// panics` at start → `/// Panics`
- `/// errors` at start → `/// Errors`
- Section headings `# errors`, `# panics`, `# examples`, `# note`, `# safety` → `# Errors`, `# Panics`, `# Examples`, `# Note`, `# Safety`

**Step 3: Verify doc tests still pass**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test --doc 2>&1 | tail -5
```

**Step 4: Commit**

```bash
git add tein/src/context.rs
git commit -m "docs: sentence case in context.rs rustdoc"
```

---

### Task 6: Capitalise remaining src rustdoc

**Files:**
- Modify: `tein/src/sandbox.rs`
- Modify: `tein/src/foreign.rs`
- Modify: `tein/src/managed.rs`
- Modify: `tein/src/timeout.rs`
- Modify: `tein/src/value.rs`
- Modify: `tein/src/error.rs`

Apply the same sentence-case rules to each file's `//!` and `///` lines. Do all six in one pass to keep commits grouped sensibly.

**sandbox.rs** — module doc changes:
```
//! sandboxing presets and filesystem policy...   → //! Sandboxing presets and filesystem policy...
//! tein's sandboxing has four independent...     → //! tein's sandboxing has four independent... (tein is lowercase by design)
//! # presets                                     → //! # Presets
//! each [`Preset`] defines...                    → //! Each [`Preset`] defines...
//! # convenience builders                        → //! # Convenience builders
//! # security model                              → //! # Security model
//! # preset reference                            → //! # Preset reference
```

Item docs on each `pub const PRESET`: sentence-case description lines.

**foreign.rs** — module doc changes:
```
//! foreign type protocol...               → //! Foreign type protocol...
//! enables rust types to be exposed...    → //! Enables Rust types to be exposed...
//! # architecture                         → //! # Architecture
//! foreign objects are stored...          → //! Foreign objects are stored...
//! # dispatch chain                       → //! # Dispatch chain
//! when scheme calls e.g. ...             → //! When Scheme calls e.g. ...
//! # complete example                     → //! # Complete example
```

**managed.rs** — module doc changes:
```
//! managed context on a dedicated thread  → //! Managed context on a dedicated thread
//! [`ThreadLocalContext`] runs a...        → unchanged (starts with type name)
//! # modes                                → //! # Modes
//! # when to use                          → //! # When to use
//! # example                              → //! # Example
```

**timeout.rs** — module doc changes:
```
//! wall-clock timeout wrapper...           → //! Wall-clock timeout wrapper...
//! [`TimeoutContext`] runs a...             → unchanged (starts with type name)
//! requires `step_limit`...                → //! Requires `step_limit`...
//! # when to use                           → //! # When to use
//! # example                              → //! # Example
```

**value.rs** — module doc changes:
```
//! scheme value representation             → //! Scheme value representation
//! [`Value`] is the safe rust...           → //! [`Value`] is the safe Rust...
//! # variants                              → //! # Variants
//! # conversion                            → //! # Conversion
//! `Value::from_raw()` converts chibi...   → //! `Value::from_raw()` converts Chibi...
```

**error.rs** — module doc changes:
```
//! error types for tein                    → //! Error types for tein
//! all fallible operations in tein...      → //! All fallible operations in tein...
//! # when each variant occurs              → //! # When each variant occurs
//! # example                              → //! # Example
```

**Step 1: Edit all six files**

Work through each file applying the rules above.

**Step 2: Verify**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test --doc 2>&1 | tail -5
```

Expected: all pass. Also:

```bash
cd /home/fey/projects/tein/tein-dev && cargo doc 2>&1 | grep warning
```

Expected: no warnings.

**Step 3: Commit**

```bash
git add tein/src/sandbox.rs tein/src/foreign.rs tein/src/managed.rs tein/src/timeout.rs tein/src/value.rs tein/src/error.rs
git commit -m "docs: sentence case in sandbox, foreign, managed, timeout, value, error rustdoc"
```

---

### Task 7: Write docs/guide.md

**Files:**
- Create: `docs/guide.md`

**Step 1: Write the guide**

Create `docs/guide.md` with the following structure and content. Use sentence case throughout. Draw all code examples from existing examples and tests — no invented code.

````markdown
# tein guide

A walkthrough for developers embedding Scheme in Rust with tein. Covers every major
feature with context, worked examples, and decision guides.

**Prerequisites:** familiarity with Rust, basic Scheme syntax (or willingness to learn
as you go). No prior embedding experience required.

---

## Table of contents

1. [The big picture](#the-big-picture)
2. [Your first evaluation](#your-first-evaluation)
3. [Choosing a context type](#choosing-a-context-type)
4. [The virtual filesystem](#the-virtual-filesystem)
5. [Sandboxing and resource limits](#sandboxing-and-resource-limits)
6. [Calling Rust from Scheme](#calling-rust-from-scheme)
7. [Foreign type protocol](#foreign-type-protocol)
8. [Custom ports](#custom-ports)
9. [Reader extensions](#reader-extensions)
10. [Macro expansion hooks](#macro-expansion-hooks)
11. [Managed contexts](#managed-contexts)

---

## The big picture

tein gives you a full R7RS Scheme interpreter inside your Rust program. You write Scheme
expressions as strings (or load `.scm` files), evaluate them, and get back Rust values.
You can also go the other way: expose Rust functions and types to Scheme code.

Everything runs on a single chibi-scheme heap per `Context`. Chibi-Scheme is a compact,
embeddable Scheme interpreter (~200kb) with zero external dependencies — exactly the
right tool for scripting, configuration, DSLs, and sandboxed evaluation.

---

## Your first evaluation

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

`Context::new()` gives you a minimal environment: core syntax only (`define`, `lambda`,
`if`, `let`, `begin`, `quote`, `set!`). If you need the full R7RS standard library
(`map`, `for-each`, `string-split`, etc.), use `Context::new_standard()` instead.

### Extracting values

`evaluate()` returns a `Value` — a Rust enum covering every Scheme type:

```rust
match ctx.evaluate("(list 1 2 3)")? {
    Value::List(items) => println!("got {} items", items.len()),
    Value::Nil         => println!("empty list"),
    other              => println!("unexpected: {other}"),
}
```

All variants have extraction helpers:

```rust
let n = ctx.evaluate("42")?.as_integer().unwrap();      // i64
let s = ctx.evaluate(r#""hello""#)?.as_str().unwrap();  // &str
let b = ctx.evaluate("#t")?.as_bool().unwrap();         // bool
```

See the [`value`](https://docs.rs/tein/latest/tein/value/) module for the full variant
table and conversion notes.

---

## Choosing a context type

tein offers three context types. Pick the simplest one that fits your needs:

| | `Context` | `TimeoutContext` | `ThreadLocalContext` |
|---|---|---|---|
| **Thread safety** | `!Send + !Sync` | `!Send + !Sync` | `Send + Sync` |
| **Timeout** | step limit only | wall-clock deadline | step limit only |
| **State** | persists | persists | persistent or fresh |
| **Use case** | single-threaded scripting | untrusted code with deadlines | shared across threads, servers |

**`Context`** is the baseline. Use it when your evaluation happens on one thread and you
control the code being run.

**`TimeoutContext`** wraps a `Context` on a dedicated thread and kills evaluation after
a wall-clock deadline. Requires `step_limit` to be set so the context thread can
terminate after the timeout fires.

```rust
use std::time::Duration;

let ctx = Context::builder()
    .step_limit(1_000_000)
    .build_timeout(Duration::from_secs(5))?;

let result = ctx.evaluate("(+ 1 2)")?;
```

**`ThreadLocalContext`** (`ThreadLocalContext` in the [`managed`](https://docs.rs/tein/latest/tein/managed/)
module) is `Send + Sync` — safe to wrap in `Arc` and share across threads. Evaluation
requests are proxied over a channel to a dedicated thread that owns the context.

---

## The virtual filesystem

One of tein's key design decisions: standard library modules are **embedded directly
in the binary** via a virtual filesystem (VFS), not loaded from disk.

This matters for several reasons:

- **Portability**: your binary works without the host having Chibi-Scheme installed
- **Security**: sandboxed contexts can only import from the VFS — not arbitrary files
- **Auditability**: the VFS contains only curated modules that tein has vetted

### What's in the VFS

The VFS contains the full R7RS standard library (loaded when you call `.standard_env()`
or `Context::new_standard()`), plus tein's own extension modules:

- `(scheme base)`, `(scheme write)`, `(scheme file)`, `(scheme char)`, ... — R7RS standard
- `(tein foreign)` — predicates for foreign type objects
- `(tein reader)` — reader dispatch functions (`set-reader!`, etc.)
- `(tein macro)` — macro expansion hook functions

### Importing modules

In a standard-env context, Scheme code can use `import`:

```scheme
(import (scheme base))
(import (srfi 1))   ; SRFI-1 list library, also in the VFS
```

In a sandboxed context, `import` is not available unless you explicitly allow it:

```rust
let ctx = Context::builder()
    .standard_env()
    .preset(&tein::sandbox::ARITHMETIC)
    .allow(&["import"])   // enable (import ...) — VFS-only, filesystem blocked
    .build()?;
```

```scheme
(import (scheme base))         ; works — VFS module
(import (scheme file))         ; blocked — sandbox preset doesn't include file ops
```

The module policy is automatic: any context with `.standard_env()` + a preset
restricts `import` to VFS-only. Filesystem modules are always blocked in sandboxed
contexts, even if you enable `import`.

---

## Sandboxing and resource limits

tein's sandboxing has four independent layers you can combine freely.

### Layer 1: Environment restriction (presets)

Presets define which Scheme primitives are visible. A sandboxed context starts with
core syntax only and gets exactly the primitives you add via presets.

```rust
use tein::sandbox::{ARITHMETIC, LISTS, STRINGS};

let ctx = Context::builder()
    .preset(&ARITHMETIC)  // +, -, *, /, =, <, >, number?, ...
    .preset(&LISTS)       // cons, car, cdr, list, map, filter, ...
    .preset(&STRINGS)     // string-length, substring, string-append, ...
    .build()?;
```

Available presets: `ARITHMETIC`, `MATH`, `LISTS`, `STRINGS`, `CHARS`, `BOOLEANS`,
`EQUIVALENCE`, `VECTORS`, `BYTEVECTORS`, `IO`, `PORTS`, `CONTROL`, `TAIL_CALLS`,
`EXCEPTIONS`, `PREDICATES`, `FILE_READ_SUPPORT`, `FILE_WRITE_SUPPORT`.

Convenience builders combine common presets:

```rust
let ctx = Context::builder()
    .pure_computation()   // ARITHMETIC + MATH + LISTS + STRINGS + BOOLEANS + EQUIVALENCE
    .build()?;

let ctx = Context::builder()
    .safe()               // pure_computation + CHARS + VECTORS + CONTROL + TAIL_CALLS
    .build()?;
```

You can also allow individual bindings by name:

```rust
let ctx = Context::builder()
    .preset(&ARITHMETIC)
    .allow(&["display", "newline"])  // add specific bindings
    .build()?;
```

### Layer 2: Step limits

Cap the total number of VM instructions:

```rust
let ctx = Context::builder()
    .step_limit(50_000)
    .build()?;

match ctx.evaluate("((lambda () (define (f) (f)) (f)))") {
    Err(tein::Error::StepLimitExceeded) => { /* expected */ }
    _ => panic!("should have hit limit"),
}
```

### Layer 3: File IO policy

Allow filesystem access only to specific path prefixes:

```rust
let ctx = Context::builder()
    .standard_env()
    .preset(&tein::sandbox::IO)
    .preset(&tein::sandbox::FILE_READ_SUPPORT)
    .file_read(&["/data/config/"])    // read allowed under this prefix
    .file_write(&["/tmp/output/"])   // write allowed under this prefix
    .build()?;
```

Path canonicalisation protects against `../` traversal and symlink attacks.

### Layer 4: Module policy

When `.standard_env()` + any preset is used, `import` is automatically restricted
to VFS modules. No configuration needed — this is automatic.

See the [`sandbox`](https://docs.rs/tein/latest/tein/sandbox/) module for the full
preset reference.

---

## Calling Rust from Scheme

### With `#[scheme_fn]`

The `#[scheme_fn]` proc macro is the easiest way to expose a Rust function to Scheme:

```rust
use tein::{Context, scheme_fn};

#[scheme_fn]
fn square(n: i64) -> i64 {
    n * n
}

let ctx = Context::new()?;
ctx.define_fn_variadic("square", __tein_square)?;

assert_eq!(ctx.evaluate("(square 7)")?, tein::Value::Integer(49));
```

The macro generates a `__tein_<name>` wrapper with the chibi FFI signature. Supported
argument and return types: `i64`, `f64`, `bool`, `String`, `&str`. Functions can return
`Result<T, String>` to signal Scheme errors:

```rust
#[scheme_fn]
fn safe_div(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 { Err("division by zero".to_string()) }
    else { Ok(a / b) }
}
```

### Via `ctx.call()`

You can also retrieve a Scheme procedure and call it from Rust:

```rust
let ctx = Context::new()?;
ctx.evaluate("(define (add a b) (+ a b))")?;

let add_fn = ctx.evaluate("add")?;
let result = ctx.call(&add_fn, &[Value::Integer(3), Value::Integer(4)])?;
assert_eq!(result, Value::Integer(7));
```

`ctx.call()` takes `&[Value]` as arguments, so you can pass any Rust values that
implement the `to_raw()` conversion.

---

## Foreign type protocol

The foreign type protocol lets you expose full Rust types — with methods — as
first-class Scheme objects.

### Implementing `ForeignType`

```rust
use tein::{ForeignType, MethodFn, Value};

struct Counter { n: i64 }

impl ForeignType for Counter {
    fn type_name() -> &'static str { "counter" }  // kebab-case, used as Scheme name prefix
    fn methods() -> &'static [(&'static str, MethodFn)] {
        &[
            ("increment", |obj, _ctx, _args| {
                let c = obj.downcast_mut::<Counter>().unwrap();
                c.n += 1;
                Ok(Value::Integer(c.n))
            }),
            ("get", |obj, _ctx, _args| {
                let c = obj.downcast_ref::<Counter>().unwrap();
                Ok(Value::Integer(c.n))
            }),
        ]
    }
}
```

### Registering and using the type

```rust
let ctx = Context::new_standard()?;
ctx.register_foreign_type::<Counter>()?;

// auto-generated: counter?, counter-increment, counter-get
```

Create a foreign value and interact with it from Scheme:

```rust
let c = ctx.foreign_value(Counter { n: 0 })?;

// pass it to scheme
ctx.evaluate("(define my-counter ...)")?; // or pass via ctx.call()

// call methods
let inc = ctx.evaluate("counter-increment")?;
ctx.call(&inc, std::slice::from_ref(&c))?;
let result = ctx.call(&ctx.evaluate("counter-get")?, std::slice::from_ref(&c))?;
assert_eq!(result, Value::Integer(1));
```

Or from Scheme entirely:

```scheme
(counter-increment my-counter)
(counter-get my-counter)       ; => 1
(counter? my-counter)          ; => #t
```

### Introspection

Four built-in functions are available once any foreign type is registered:

```scheme
(foreign-types)               ; => ("counter") — all registered type names
(foreign-methods "counter")   ; => ("increment" "get") — methods for a type
(foreign-type my-counter)     ; => "counter"
(foreign-handle-id my-counter); => 1  (monotonic handle ID)
```

Error messages are LLM-friendly — calling an unknown method lists available ones.

---

## Custom ports

Bridge Rust `Read`/`Write` objects into Scheme's port system.

### Input ports

```rust
use tein::Context;

let ctx = Context::new_standard()?;

// any type implementing std::io::Read works
let source = std::io::Cursor::new("(+ 1 2) (+ 3 4)");
let port = ctx.open_input_port(source)?;

// read one s-expression
let expr = ctx.read(&port)?;

// or read+eval everything
let result = ctx.evaluate_port(&port)?;
```

`ctx.read()` returns a `Value` without evaluating. `ctx.evaluate_port()` reads and
evaluates expressions in a loop, returning the last result.

### Output ports

```rust
let buf: Vec<u8> = Vec::new();
let port = ctx.open_output_port(std::io::Cursor::new(buf))?;
ctx.evaluate("(display \"hello\" port)")?;  // pass port to Scheme
```

Custom ports work via Chibi's `fopencookie` mechanism — your Rust reader/writer is
called directly from within Scheme evaluation.

---

## Reader extensions

Extend Scheme's `#` reader syntax with custom dispatch characters.

### From Rust

```rust
let ctx = Context::new_standard()?;
let handler = ctx.evaluate("(lambda (port) 42)")?;
ctx.register_reader('j', &handler)?;

assert_eq!(ctx.evaluate("#j")?, Value::Integer(42));
```

The handler receives the input port positioned after the dispatch character. Read
further from the port if your syntax needs more input.

### From Scheme

```scheme
(set-reader! #\j (lambda (port) (list 'json (read port))))
;; #j(1 2 3) → (json (1 2 3))
```

Or with the `(tein reader)` module:

```scheme
(import (tein reader))
(set-reader! #\j (lambda (port) (read port)))
```

### Reserved characters

The following `#` dispatch characters cannot be overridden (R7RS reserved):
`t`, `f`, `\`, `(`, and numeric prefix characters.

`reader-dispatch-chars` returns the list of currently registered characters.

The dispatch table is thread-local and cleared when the `Context` drops.

---

## Macro expansion hooks

Intercept every macro expansion at analysis time. The hook receives the macro name,
the unexpanded form, the expanded form, and the syntactic environment. Its return
value replaces the expansion (replace-and-reanalyse semantics).

### From Scheme

```scheme
(import (tein macro))

;; observe expansions without changing them
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    (display name)
    expanded))  ; return expanded unchanged

;; or transform
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    (if (eq? name 'when)
      (list 'begin expanded '(display "when expanded\n"))
      expanded)))
```

### From Rust

```rust
let hook = ctx.evaluate("(lambda (name pre post env) post)")?;
ctx.set_macro_expand_hook(&hook)?;
// later:
ctx.unset_macro_expand_hook()?;
```

The recursion guard prevents the hook from triggering on its own macro usage.
The hook is cleared when the `Context` drops.

---

## Managed contexts

`ThreadLocalContext` is `Send + Sync` — safe to share across threads.

### Persistent mode

State accumulates across evaluations. `reset()` rebuilds the context and re-runs
the init closure.

```rust
let ctx = Context::builder()
    .standard_env()
    .step_limit(1_000_000)
    .build_managed(|ctx| {
        ctx.evaluate("(define counter 0)")?;
        Ok(())
    })?;

ctx.evaluate("(set! counter (+ counter 1))")?;
ctx.evaluate("(set! counter (+ counter 1))")?;
assert_eq!(ctx.evaluate("counter")?, tein::Value::Integer(2));

ctx.reset()?;
assert_eq!(ctx.evaluate("counter")?, tein::Value::Integer(0));  // back to init state
```

### Fresh mode

The context is rebuilt before every evaluation — no state leakage between calls.

```rust
let ctx = Context::builder()
    .step_limit(100_000)
    .build_managed_fresh(|ctx| {
        ctx.evaluate("(define x 10)")?;
        Ok(())
    })?;

ctx.evaluate("(set! x 99)")?;
assert_eq!(ctx.evaluate("x")?, tein::Value::Integer(10));  // x is back to 10
```

Fresh mode is ideal for deterministic evaluation where each call must see exactly
the same initial state — sandboxed user scripts, test runners, etc.

### Choosing a mode

- **Persistent** — REPL, stateful scripting, accumulated definitions
- **Fresh** — untrusted user scripts, request handlers, anything needing isolation
````

**Step 2: Verify the file looks right**

Read `docs/guide.md` and eyeball the structure.

**Step 3: Commit**

```bash
git add docs/guide.md
git commit -m "docs: newcomer guide — VFS, sandboxing, all features"
```

---

### Task 8: Final verification

**Step 1: Full doc build, no warnings**

```bash
cd /home/fey/projects/tein/tein-dev && cargo doc 2>&1 | grep warning
```

Expected: no warnings (or only non-doc warnings unrelated to this work).

**Step 2: Doc tests all pass**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test --doc 2>&1 | tail -10
```

Expected: all pass.

**Step 3: Full test suite green**

```bash
cd /home/fey/projects/tein/tein-dev && cargo test 2>&1 | tail -5
```

Expected: all pass.
