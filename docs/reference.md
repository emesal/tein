# reference

## Value variants

| Scheme type | Rust variant | Display example | extraction helper |
|-------------|-------------|-----------------|-------------------|
| exact integer | `Value::Integer(i64)` | `42` | `as_integer()` |
| inexact float | `Value::Float(f64)` | `3.14` | `as_float()` |
| boolean | `Value::Boolean(bool)` | `#t` / `#f` | `as_bool()` |
| string | `Value::String(String)` | `"hello"` | `as_string()` |
| symbol | `Value::Symbol(String)` | `foo` | `as_symbol()` |
| proper list | `Value::List(Vec<Value>)` | `(1 2 3)` | `as_list()` |
| improper pair | `Value::Pair(Box<Value>, Box<Value>)` | `(a . b)` | `as_pair()` |
| vector | `Value::Vector(Vec<Value>)` | `#(1 2 3)` | `as_vector()` |
| character | `Value::Char(char)` | `#\a` | `as_char()` |
| bytevector | `Value::Bytevector(Vec<u8>)` | `#u8(1 2 3)` | `as_bytevector()` |
| empty list | `Value::Nil` | `()` | `is_nil()` |
| unspecified | `Value::Unspecified` | `#<unspecified>` | `is_unspecified()` |
| port (opaque) | `Value::Port(sexp)` | `#<port>` | `as_port()` |
| hash table (opaque) | `Value::HashTable(sexp)` | `#<hash-table>` | — |
| procedure | `Value::Procedure(sexp)` | `#<procedure>` | `as_procedure()` |
| foreign object | `Value::Foreign { handle_id, type_name }` | `#<counter:1>` | `ctx.foreign_ref::<T>()` |
| other (unhandled) | `Value::Other(String)` | `#<...>` | — |

`Value` implements `Display` — produces scheme-readable output. `as_string()` and `as_list()`
return borrowed references (`&str` / `&[Value]`) — bind the `Value` before calling to avoid
lifetime issues.

## feature flags

| flag | default | description | deps |
|------|---------|-------------|------|
| `json` | yes | enables `(tein json)` with `json-parse` / `json-stringify` | `serde`, `serde_json` |
| `toml` | yes | enables `(tein toml)` with `toml-parse` / `toml-stringify` | `toml` |
| `uuid` | yes | enables `(tein uuid)` with `make-uuid`, `uuid?`, `uuid-nil` | `uuid` |
| `time` | yes | enables `(tein time)` with `current-second`, `current-jiffy` | none (`std::time`) |

Disable all with `default-features = false`:

```toml
tein = { git = "https://github.com/emesal/tein", default-features = false }
# re-enable selectively:
tein = { git = "https://github.com/emesal/tein", default-features = false, features = ["json", "uuid"] }
```

## VFS module list

All modules embedded in the VFS — available for import in `standard_env` contexts.

**`Safe` column:** ✓ = included in `Modules::Safe` preset.

### tein/* modules

| module | safe | description |
|--------|------|-------------|
| `tein/foreign` | ✓ | `foreign?`, `foreign-type`, `foreign-handle-id` |
| `tein/reader` | ✓ | `set-reader!`, `unset-reader!`, `reader-dispatch-chars` |
| `tein/macro` | ✓ | `set-macro-expand-hook!`, `unset-macro-expand-hook!`, `macro-expand-hook` |
| `tein/test` | ✓ | `test-equal`, `test-error`, `test-assert` |
| `tein/docs` | ✓ | `describe`, `module-doc`, `module-docs` |
| `tein/json` | ✓ | `json-parse`, `json-stringify` (feature: `json`) |
| `tein/toml` | ✓ | `toml-parse`, `toml-stringify` (feature: `toml`) |
| `tein/uuid` | ✓ | `make-uuid`, `uuid?`, `uuid-nil` (feature: `uuid`) |
| `tein/time` | ✓ | `current-second`, `current-jiffy`, `jiffies-per-second`, `timezone-offset-seconds` (feature: `time`) |
| `tein/safe-regexp` | ✓ | `regexp`, `regexp?`, `regexp-search`, `regexp-matches`, `regexp-matches?`, `regexp-replace`, `regexp-replace-all`, `regexp-extract`, `regexp-split`, `regexp-match-count`, `regexp-match-submatch`, `regexp-match->list`, `regexp-fold` (feature: `regex`) — linear-time via rust `regex` crate, no ReDoS |
| `tein/file` | ✓ | R7RS file operations with `FsPolicy` enforcement |
| `tein/load` | ✓ | `load` (VFS-restricted) |
| `tein/process` | ✓ | `exit`, `emergency-exit`, `command-line`, `get-environment-variable`, `get-environment-variables` |

### scheme/* modules

| module | safe | description |
|--------|------|-------------|
| `scheme/base` | ✓ | core R7RS procedures |
| `scheme/char` | ✓ | character classification and conversion |
| `scheme/write` | ✓ | `display`, `write`, `newline` |
| `scheme/read` | ✓ | `read` |
| `scheme/inexact` | ✓ | `finite?`, `infinite?`, `nan?` |
| `scheme/lazy` | ✓ | `delay`, `force`, `promise?` |
| `scheme/case-lambda` | ✓ | `case-lambda` |
| `scheme/cxr` | ✓ | `caaar`…`cdddr` |
| `scheme/complex` | ✓ | `real-part`, `imag-part`, `angle`, `magnitude` |
| `scheme/list` | ✓ | SRFI-1 list library (chibi alias) |
| `scheme/vector` | ✓ | vector library |
| `scheme/sort` | ✓ | `list-sort`, `vector-sort` |
| `scheme/hash-table` | ✓ | R7RS hash tables |
| `scheme/bitwise` | ✓ | bitwise operations |
| `scheme/fixnum` | ✓ | fixnum-specific ops |
| `scheme/flonum` | ✓ | flonum-specific ops |
| `scheme/process-context` | ✓ | `command-line`, env vars (via shadow — see `tein/process`) |
| `scheme/file` | — | `open-input-file` etc. (use `tein/file` in sandbox) |
| `scheme/eval` | — | `eval`, `environment` |
| `scheme/repl` | — | `interaction-environment` |
| `scheme/time` | ✓ | `current-second`, `current-jiffy`, `jiffies-per-second` (re-exports `tein/time`; feature: `time`) |

### srfi/* modules (selected)

All `srfi/*` modules in the registry are `default_safe: true` except `srfi/18` (threads,
POSIX-only) and `srfi/146/hash` (depends on unsafe internals).

| module | description |
|--------|-------------|
| `srfi/1` | list library |
| `srfi/2` | `and-let*` |
| `srfi/8` | `receive` |
| `srfi/9` | `define-record-type` |
| `srfi/13` | string library |
| `srfi/14` | character sets |
| `srfi/19` | time data types and procedures (`make-time`, `make-date`, `date->string`, etc.; feature: `time`) |
| `srfi/27` | random number sources |
| `srfi/69` | basic hash tables |
| `srfi/98` | env var access |
| `srfi/115` | regular expressions |
| `srfi/125` | hash tables |
| `srfi/128` | comparators |
| `srfi/133` | vector library |
| `srfi/151` | bitwise operations |
| `srfi/166` | monadic formatting (`(scheme show)` alias) |

Full list: see `tein/src/vfs_registry.rs`.

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
In sandboxed contexts use `integer?` instead of `fixnum?` in your own code.

## known r7rs deviations

### exit and dynamic-wind (issue #101)

Both `exit` and `emergency-exit` in `(tein process)` have emergency-exit semantics —
neither runs `dynamic-wind` "after" thunks. R7RS `exit` should run them.

A future standalone interpreter host is expected to establish the unwind continuation
needed for this. The current tein library API does not establish one.
