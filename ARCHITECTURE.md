# tein architecture

> *Branch and rune-stick* — embeddable Chibi-Scheme for Rust

## Project status

### Completed milestones

**Milestone 1 — core types & ergonomics**
- Vendored Chibi-Scheme 0.11 with custom build system
- C FFI shim layer (`tein_shim.c`) for macro-based APIs
- Safe Rust wrappers around unsafe C functions
- All core value types: integers, floats, strings, symbols, booleans, lists, pairs, vectors, nil, procedures
- Typed extraction helpers (`as_integer()`, `as_list()`, `is_procedure()`, etc.)
- Bidirectional value bridge (`Value::to_raw()` ↔ `Value::from_raw()`)
- Multi-expression evaluation, file loading
- Tortoise-and-hare cycle detection, depth limits

**Milestone 2 — Scheme as extension language**
- Procedures as values via `sexp_applicablep`
- `ctx.call(proc, &[args])` for Rust→Scheme callbacks
- `define_fn_variadic` for registering Rust functions
- `#[scheme_fn]` proc macro for ergonomic FFI
- Panic safety at FFI boundary

**Milestone 3 — tein-sexp pure Rust s-expression crate**
- Separate workspace crate, no Chibi dependency
- `Sexp` AST with source spans
- R7RS-compatible lexer and parser
- Comment preservation mode
- Pretty printer with configurable output

**Milestone 4a — sandboxing & resource limits**
- `ContextBuilder` with fluent API for heap sizes, step limits, and environment restriction
- Fuel-based step limiting via thread-local counters + vm.c patch
- Allowlist-based sandbox presets using Chibi's null env (14 presets)
- `TimeoutContext` for wall-clock deadlines via dedicated thread
- `Error::StepLimitExceeded` and `Error::Timeout` variants

**Milestone 4b — parameterised IO presets**
- `FsPolicy` with path prefix matching and canonicalisation
- Wrapper foreign functions for all 4 file-open primitives
- `.file_read(&[...])` / `.file_write(&[...])` builder API
- Support presets (`FILE_READ_SUPPORT`, `FILE_WRITE_SUPPORT`) for port operations
- Path traversal and symlink protection via `canonicalize()`

**R7RS standard environment**
- VFS + static libs + eval.c patches for embedded module loading
- `Context::new_standard()` / `ContextBuilder::standard_env()` API
- ~200 bindings (map, for-each, values, dynamic-wind, etc.)
- `ModulePolicy`: VFS-only import restriction in sandboxed standard-env contexts
- C-level interception in `sexp_find_module_file_raw` via `tein_module_allowed()`

### Known limitations

1. **Limited type coverage**
   - Hash tables and ports are opaque (`Value::HashTable`, `Value::Port`) — no rich Rust API
   - Continuations surface as `Value::Procedure` (Chibi uses the same type tag)

## Architecture

### Directory structure
```
tein/
  src/
    lib.rs       — public API re-exports
    context.rs   — Context, ContextBuilder, evaluation, fuel mgmt, all tests
    value.rs     — Value enum: scheme↔rust conversion, cycle detection, Display
    error.rs     — Error enum (EvalError, TypeError, InitError, Utf8Error,
                   IoError, StepLimitExceeded, Timeout, SandboxViolation)
    ffi.rs       — unsafe C bindings + safe wrappers, `raw` module
    foreign.rs   — ForeignType trait, MethodFn/MethodContext, ForeignStore, dispatch
    managed.rs   — ThreadLocalContext: persistent/fresh managed context on dedicated thread
    port.rs      — PortStore: Read/Write bridge via thread-local trampoline
    sandbox.rs   — Preset type, FsPolicy, ModulePolicy, 16 const preset definitions
    thread.rs    — shared channel protocol (Request, Response, SendableValue, ForeignFnPtr)
    timeout.rs   — TimeoutContext: wall-clock timeout via dedicated thread
  target/chibi-scheme/  — fetched from emesal/chibi-scheme (branch emesal-tein) by build.rs
    tein_shim.c  — exports chibi C macros as real functions, fuel control,
                   environment manipulation, module import policy,
                   custom port creation, reader dispatch table,
                   macro expansion hook
    eval.c       — 4 patches: VFS module lookup (A + policy gate), VFS load (B),
                   VFS open-input-file (C), macro expansion hook (D)
    sexp.c       — 1 patch: reader dispatch table check before hardcoded # switch
    vm.c         — 2-line patch: fuel budget consumption at timeslice boundary
    lib/tein/foreign.sld/.scm — (tein foreign) predicates
    lib/tein/reader.sld/.scm/.c — (tein reader) C-backed dispatch fns via static lib init
    lib/tein/macro.sld/.scm/.c  — (tein macro) C-backed expansion hook fns via static lib init
    lib/tein/test.sld/.scm     — (tein test) pure-scheme assertion framework
  build.rs       — fetches chibi fork, compiles it, generates install.h, tein_vfs_data.h,
                   tein_clibs.c into OUT_DIR
  examples/      — basic.rs, floats.rs, ffi.rs, debug.rs, sandbox.rs,
                   foreign_types.rs, managed.rs, repl.rs
tein-macros/     — #[scheme_fn] proc macro crate
tein-sexp/       — pure Rust s-expression parser/printer
```

### Data flow

```
rust code → Context::evaluate()
  → arm_fuel() (if step limit configured)
  → ffi.rs safe wrappers → tein_shim.c → chibi-scheme vm
  → tein_fuel_consume_slice() at each timeslice boundary
  → sexp result → Value::from_raw() → check_fuel()
  → rust Value enum (or Error::StepLimitExceeded)
```

### Sandboxing flow

```
ContextBuilder::build() with presets:
  1. create context with full primitive env
  2. create null env (syntax-only: define, if, lambda, begin, quote)
  3. for each allowed primitive: look up in primitive env, copy to null env
  4. set null env as active → only allowed primitives accessible
```

### IO policy flow

```
ContextBuilder with file_read/file_write:
  1. capture original file-open procs from full env before restriction
  2. store in thread-local ORIGINAL_PROCS (4 slots, one per open-*-file)
  3. register wrapper foreign fns in restricted env
  4. set FsPolicy thread-local with path prefixes
  5. on call: wrapper extracts filename → canonicalises path →
     checks prefix match → delegates to original proc or returns error
  6. on Context::drop(): clear FsPolicy and ORIGINAL_PROCS thread-locals
```

### Module import policy

```
ContextBuilder with standard_env + presets:
  1. set MODULE_POLICY thread-local = VfsOnly
  2. set C-level tein_module_policy = 1 (vfs-only)
  3. load standard env (init-7, meta-7 via VFS — allowed under VfsOnly)
  4. apply sandbox restrictions (presets, IO wrappers)
  5. on (import ...): sexp_find_module_file_raw calls tein_module_allowed()
     → VFS paths (/vfs/lib/...) pass, filesystem paths blocked
  6. on Context::drop(): reset both thread-local and C-level to Unrestricted
```

**VFS safety contract**: VFS modules are safe by construction — tein curates
the embedded virtual filesystem to ensure no module can bypass the existing
safety layers (preset allowlists, FsPolicy, fuel/timeout). Capabilities
exposed by VFS modules remain subject to these controls.

**Security layers** (independent, composable):

| layer              | gates                                    |
|--------------------|------------------------------------------|
| module allowlist   | which libraries can be `import`ed        |
| preset allowlist   | which primitives/bindings are in scope   |
| FsPolicy           | which filesystem paths can be opened     |
| fuel/timeout       | resource exhaustion                      |

### Thread safety

- `Context` is intentionally !Send + !Sync (Chibi is not thread-safe)
- `TimeoutContext` wraps Context on a dedicated thread for wall-clock deadlines
- `ThreadLocalContext` generalises the pattern: persistent mode (state accumulates, `reset()` rebuilds) or fresh mode (context rebuilt before every call)
- Both types are `Send + Sync` via channel-based proxying; the Context itself never leaves its thread
- Shared channel protocol in `thread.rs`: `Request`/`Response`/`SendableValue`/`ForeignFnPtr`
- Fuel counters are `__thread` (thread-local) so parallel tests don't interfere

### Key design decisions

**GC safety — `ffi::GcRoot`**: Chibi's conservative stack scanning is disabled in our build. The GC does NOT see Rust locals — only objects reachable from the context's heap roots survive collection. Any `sexp` held as a Rust local across an allocation point must be rooted via `ffi::GcRoot`, an RAII guard that calls `sexp_preserve_object` on creation and `sexp_release_object` on drop.

Allocating FFI calls (trigger GC, require rooting across):
- `sexp_make_flonum`, `sexp_c_str`, `sexp_intern` — create heap objects
- `sexp_cons`, `sexp_make_vector` — create containers
- `sexp_symbol_to_string` — allocates a string from a symbol
- `sexp_open_input_string`, `sexp_read`, `sexp_evaluate` — evaluation machinery
- `sexp_load_standard_env`, `sexp_make_null_env` — env construction
- `sexp_env_define`, `env_copy_named`, `sexp_define_foreign_proc` — env mutation
- `sexp_preserve_object` itself — allocates a cons cell on the preservatives list

Non-allocating FFI calls (safe, no rooting needed):
- Type predicates: `sexp_integerp`, `sexp_flonump`, `sexp_pairp`, etc.
- Value extractors: `sexp_unbox_fixnum`, `sexp_flonum_value`, `sexp_string_data`, `sexp_car`, `sexp_cdr`, `sexp_vector_data`
- Immediate constructors: `sexp_make_fixnum`, `sexp_make_boolean`, `get_null`, `get_void`
- `sexp_vector_set` — writes to an existing vector slot, no allocation

C-side equivalent: use `sexp_gc_var` / `sexp_gc_preserve` / `sexp_gc_release` (see eval.c patches).

**Vendoring Chibi**: source bundled, compiled via build.rs, zero external deps.

**Shim layer**: Chibi uses C macros extensively; `tein_shim.c` exports them as real functions for Rust FFI.

**Fuel implementation**: Chibi's VM creates child contexts per eval, so context-level refuel doesn't work. Thread-local counters + a 2-line vm.c patch intercept the timeslice boundary to implement true total-fuel budgeting. When fuel limiting is inactive, behaviour is identical to stock Chibi.

**Type checking order**: check `sexp_flonump` BEFORE `sexp_integerp`. The integer predicate includes `_or_integer_flonump` and matches floats like 4.0, producing garbage.

**VFS path prefix**: use `/vfs/lib` not `vfs://...` — Chibi's `sexp_add_path` splits on `:`, so colons in paths break module resolution.

**`sexp_load_standard_env` signature**: the version parameter is `sexp` (a tagged fixnum via `sexp_make_fixnum`), NOT `sexp_uint_t`. This is a Chibi API quirk.

**Rename bindings in standard env**: the standard env stores most bindings as *renames* (via `SEXP_USE_RENAME_BINDINGS`), not direct bindings. `sexp_env_ref` with a bare symbol won't find them. `tein_env_copy_named` in `tein_shim.c` handles this by walking both direct bindings and renames with synclo unwrapping. Note: the env parent chain terminates with NULL, and `sexp_envp(NULL)` segfaults because `sexp_pointerp(NULL)` returns true (`SEXP_POINTER_TAG == 0`). The env walk loop must guard against NULL explicitly.

**`import` in sandboxed envs**: `import` is not core syntax — it's a binding from `repl-import` in the meta env, spliced into the standard env during `sexp_load_standard_env`. It can be copied into the restricted null env via `.allow(&["import"])` like any other binding. The module policy (VFS-only) still applies, so only curated VFS modules are importable. Both `source_env` and `null_env` must be GC-rooted during sandbox build, since `sexp_intern`, `env_copy_named`, and `sexp_define_foreign_proc` all allocate.

**`let` in sandboxed standard env**: closures from the standard env (e.g. `for-each`) reference the full env internally, but `let`-bound variables in user code live in the restricted null env. Using `define` for top-level bindings works; `let` inside `for-each` callbacks does not. This is a scope chain issue specific to the null env sandbox approach.

## Building & testing

```bash
cargo build                        # build (compiles vendored chibi-scheme)
cargo test                         # all tests (208 lib + 12 scheme_fn + 6 scheme + 24 doc)
cargo test test_name               # single test by name
cargo test --lib -- --nocapture    # lib tests with stdout
cargo clippy                       # lint
cargo fmt --check                  # format check
cargo run --example basic          # run an example
cargo run --example sandbox        # sandboxing demo
cargo clean && cargo build         # nuclear option if ffi gets weird
```

## Adding a new Scheme type

1. Add predicate wrapper to `tein_shim.c` in the fork (emesal/chibi-scheme, branch emesal-tein)
2. Add extern declaration + safe wrapper in `src/ffi.rs`
3. Add variant to `Value` enum in `src/value.rs`
4. Add extraction in `Value::from_raw()` (respect type check ordering!)
5. Add `to_raw()` conversion
6. Add Display impl
7. Add test in `src/context.rs`

## Registering Rust functions in Scheme

**Via proc macro (recommended):**
```rust
#[scheme_fn]
fn add(a: i64, b: i64) -> i64 { a + b }

ctx.define_fn_variadic("add", __tein_add)?;
```

**Via raw FFI:**
```rust
unsafe extern "C" fn my_fn(
    ctx: raw::sexp, _self: raw::sexp,
    _n: raw::sexp_sint_t, args: raw::sexp,
) -> raw::sexp { ... }

ctx.define_fn_variadic("my-fn", my_fn)?;
```

## Conventions

- Edition 2024: `unsafe fn` bodies need inner `unsafe { }` blocks
- Every public item has a docstring
- Comments explain *why*, code shows *what*
- Lowercase style, casual but precise
- Norse mythology naming theme
- See TODO.md for roadmap

## Foreign type protocol

**Milestone 6** — expose Rust types as first-class Scheme objects with method dispatch,
introspection, and LLM-friendly error messages. Zero C changes.

### Architecture

Foreign objects are tagged lists `(__tein-foreign "type-name" handle-id)` stored in a
per-context `ForeignStore` keyed by `u64` handle IDs. Scheme sees them as opaque values
manipulated via the `(tein foreign)` protocol. Rust data never crosses the FFI boundary.

```
ForeignStore (per Context)
  types: HashMap<&'static str, TypeEntry { methods: &'static [(&'static str, MethodFn)] }>
  instances: HashMap<u64, ForeignObject { data: Box<dyn Any>, type_name: &'static str }>
  next_id: u64  (monotonically increasing, starts at 1)
```

### Implementing ForeignType

```rust
use tein::{ForeignType, MethodFn, Value};

struct MyType { value: i64 }

impl ForeignType for MyType {
    fn type_name() -> &'static str { "my-type" }
    fn methods() -> &'static [(&'static str, MethodFn)] {
        &[
            ("get", |obj, _ctx, _args| {
                let t = obj.downcast_ref::<MyType>().unwrap();
                Ok(Value::Integer(t.value))
            }),
        ]
    }
}
```

### Registration and use

```rust
ctx.register_foreign_type::<MyType>()?;
// scheme now has: my-type?, my-type-get, foreign-call, foreign-types, ...

let val = ctx.foreign_value(MyType { value: 42 })?;
let result = ctx.call(&ctx.evaluate("my-type-get")?, &[val])?;
// result == Value::Integer(42)
```

### Dispatch chain

Scheme `(my-type-get obj)` → convenience lambda → `(apply foreign-call obj 'get args)` →
`foreign_call_wrapper` (extern "C") → reads `FOREIGN_STORE_PTR` thread-local →
`dispatch_foreign_call` → looks up method → calls `MethodFn(&mut dyn Any, ...)` → `Value`

The `FOREIGN_STORE_PTR` thread-local is set by `evaluate()`/`call()` via `ForeignStoreGuard`
RAII, ensuring the pointer is always valid during Scheme execution and cleared on all exit paths.

### Scheme-side protocol

`foreign.scm` defines predicates/accessors using only primitives always available:
- `foreign?` — uses `pair?`, `eq?`, `string?`, `fixnum?` (not `integer?` — not a Chibi primitive)
- `foreign-type` — returns the type-name string
- `foreign-handle-id` — returns the handle ID fixnum

Uses `car`/`cdr` chains instead of `cadr`/`caddr` (those require `scheme/cxr`).

## Custom port protocol

Bridges Rust `Read`/`Write` objects to Chibi's custom port mechanism via thread-local trampoline — same pattern as ForeignStore.

### Architecture

- **PortStore** (`port.rs`): per-context map from port ID → `Box<dyn Read>` or `Box<dyn Write>`
- **PORT_STORE_PTR** (`context.rs`): thread-local raw pointer, set before evaluate/call via `PortStoreGuard` RAII
- **port_read_trampoline** / **port_write_trampoline**: extern "C" fns called by Chibi's `sexp_cookie_reader`/`writer` via `fopencookie`

### Creating ports

```rust
let port = ctx.open_input_port(std::io::Cursor::new(b"(+ 1 2)"))?;
let val = ctx.read(&port)?;           // read one s-expression
let result = ctx.evaluate_port(&port)?; // read+eval loop
```

Output ports work similarly via `open_output_port`. Pass the port value to Scheme's `display`/`write`/`write-char`.

### Chibi protocol details

- Read callback receives `(buf start end)` where `buf[0..start)` has valid data from prior partial fills
- Return value must be `start + new_bytes_read` (Chibi copies from position 0)
- `flush-output` is the primitive name; `flush-output-port` requires `(scheme extras)`

## Reader dispatch protocol

Extends Chibi's `#` reader syntax with user-defined handlers via a C-level dispatch table.

### Architecture

- **tein_reader_dispatch[128]** (`tein_shim.c`): thread-local table mapping ASCII chars → Scheme procs
- **sexp.c patch**: reader checks dispatch table before hardcoded `#` switch — `tein_reader_dispatch_get(c1)` → `sexp_apply1` if handler found
- **(tein reader)** C-backed module: `reader.c` implements `sexp_init_library` which registers `set-reader!`/`unset-reader!`/`reader-dispatch-chars` as native fns into the module env when `(import (tein reader))` triggers the static library init via `include-shared`

### Usage

```rust
// from rust
let handler = ctx.evaluate("(lambda (port) 42)")?;
ctx.register_reader('j', &handler)?;
assert_eq!(ctx.evaluate("#j")?, Value::Integer(42));
```

```scheme
;; from scheme (requires import)
(import (tein reader))
(set-reader! #\j (lambda (port) (list 'json (read port))))
;; #j(1 2 3) → (json (1 2 3))
```

### Design notes

- Reserved R7RS chars (`#t`, `#f`, `#\`, `#(`, numeric prefixes, etc.) cannot be overridden
- Dispatch table is thread-local, matching Chibi's !Send context model
- Table cleared on `Context::drop()` so next context on the thread starts clean
- Handler return value becomes the reader result — gets evaluated by `evaluate()`, so return self-evaluating datums (numbers, strings, lists) or use `read()` for raw datum access

## Macro expansion hook protocol

Intercepts Chibi's macro expansion at analysis time — replace-and-reanalyse semantics.

### Architecture

- **tein_macro_expand_hook** (`tein_shim.c`): thread-local slot for a Scheme proc, with GC preservation
- **tein_macro_expand_hook_active** (`tein_shim.c`): thread-local recursion guard (prevents hook from triggering on its own macro usage)
- **eval.c patch D**: in `analyze_macro_once()`, after macro expansion, checks hook → if set and not active, calls `sexp_apply(ctx, hook, (name unexpanded expanded env))` → hook return value replaces expanded form → `goto loop` reanalyses
- **(tein macro)** C-backed module: `macro.c` implements `sexp_init_library` which registers `set-macro-expand-hook!`/`unset-macro-expand-hook!`/`macro-expand-hook` as native fns into the module env when `(import (tein macro))` triggers the static library init via `include-shared`

### Usage

```rust
// from rust
let hook = ctx.evaluate("(lambda (name pre post env) post)")?;
ctx.set_macro_expand_hook(&hook)?;
```

```scheme
;; from scheme
(import (tein macro))
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    expanded))  ; observe or transform
```

### Design notes

- Hook receives 4 args: macro name (symbol), unexpanded form, expanded form, syntactic environment
- Return value replaces the expansion — returning the expanded form unchanged is a no-op observation
- Recursion guard prevents infinite loops when the hook itself uses macros
- Hook cleared on `Context::drop()`

---

## Scheme environment quirks

Findings from comprehensive r7rs test coverage (tasks 7–16 of the scheme test coverage plan).
These apply to `Context::new_standard()` and inform how to write `.scm` test files.

### Import requirements

Procedures available **without any import** (loaded by init-7.scm into the standard toplevel):

- control flow: `cond`, `case`, `and`, `or`, `do`, `when`, `unless`
- binding: `let`, `let*`, `letrec`, `letrec*`, named `let`
- continuations: `dynamic-wind`, `call/cc`, `call-with-current-continuation`, `values`,
  `call-with-values`
- exceptions: `with-exception-handler`, `raise`, `raise-continuable`
- syntax: `define-syntax`, `syntax-rules`, `let-syntax`, `letrec-syntax`, `quasiquote`
- eval: `eval`, `interaction-environment`, `scheme-report-environment`

Require **`(import (scheme base))`**:

- `when`, `unless` (also in init-7, but `(scheme base)` version recommended for consistency)
- `define-values`, `guard`, `error-object?`, `error-object-message`, `error-object-irritants`
- `floor/`, `truncate/`
- `define-record-type` — syntax is present without import but accessor/mutator generation
  is broken without `(import (scheme base))` (chibi compilation environment issue)
- bytevector API: `bytevector`, `make-bytevector`, `bytevector-u8-ref`, `bytevector-u8-set!`,
  `bytevector-length`, `bytevector-copy`, `bytevector-append`, `utf8->string`, `string->utf8`

Require other imports:

- `(import (scheme inexact))` — `finite?`, `infinite?`, `nan?`
- `(import (scheme lazy))` — `delay`, `force`, `promise?`, `make-promise`
- `(import (scheme case-lambda))` — `case-lambda`
- `(scheme eval)` module not available — use `eval` etc. directly (no import needed)

### call/cc re-entry and top-level defines

Calling a saved continuation from a separate `ctx.evaluate()` call does not re-enter (C stack
boundary). Within a single evaluate call, re-entry also fails when mutable state is in
top-level `define`s — chibi's batch-compiled toplevel re-executes from the continuation point
but define bindings reset. Keep mutable state in `let` scope:

```scheme
;; works:
(let ((k #f) (n 0))
  (call/cc (lambda (c) (set! k c)))
  (set! n (+ n 1))
  (if (< n 3) (k 'ignored) n))  ; => 3

;; does NOT work (returns 1, not 3):
(define saved-k #f)
(define counter 0)
(call/cc (lambda (k) (set! saved-k k)))
(set! counter (+ counter 1))
(if (< counter 3) (saved-k #f) counter)
```

### define-values in single-batch evaluate

`define-values` introducing toplevel bindings mid-batch can corrupt subsequent expression
evaluation in the same `evaluate()` call. Use `call-with-values` instead:

```scheme
;; instead of:
(define-values (q r) (floor/ 13 4))
(test-equal "q" 3 q)

;; use:
(call-with-values (lambda () (floor/ 13 4))
  (lambda (q r) (test-equal "q" 3 q)))
```

### let binding order

`let` bindings are evaluated in unspecified order. For sequential side-effectful operations
(e.g. multiple `read` calls on a port), use `let*`.

### raise-continuable return value

The handler's return value flows back to the `raise-continuable` call site:
`(+ 1 (raise-continuable x))` with a handler returning 99 yields **100** (not 99).

### stream-cons must be a macro

`(define (stream-cons h t) (cons h (delay t)))` evaluates `t` eagerly. Use `define-syntax`:

```scheme
(define-syntax stream-cons
  (syntax-rules () ((stream-cons h t) (cons h (delay t)))))
```

### (tein foreign) import in standard env

`lib/tein/foreign.scm` uses `fixnum?` which is available in the standard context toplevel
(chibi builtin) but is not exported by `(scheme base)`. Since `foreign.sld` only imports
`(scheme base)`, `(import (tein foreign))` fails in standard env with "undefined variable:
fixnum?". The pure-scheme predicates (`foreign?`, `foreign-type`, `foreign-handle-id`) can
be used inline with `integer?` replacing `fixnum?`.

### condition/report-string

`condition/report-string` does not exist. Use `error-object-message` instead.
