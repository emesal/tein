# reader extensions design

two features that compose: custom ports (rust Read/Write bridged to chibi ports) and hash dispatch reader extensions (scheme-definable `#x` syntax).

## motivation

tein as an extension language for an LLM agent harness. agents write scheme code as plugins — custom ports let the host wire IO through its permission system, reader extensions let agents define DSL syntax on the fly. sandboxing controls what's available.

## feature 1: custom ports

### rust API

```rust
// input: anything implementing Read
let port = ctx.open_input_port(my_reader)?;    // -> Value (port)

// output: anything implementing Write
let port = ctx.open_output_port(my_writer)?;   // -> Value (port)

// read one s-expression from a port
let val = ctx.read(&port)?;

// read+eval loop from a port
let val = ctx.evaluate_port(&port)?;
```

trait-based, no separate callback API. `Read`/`Write` is the single mechanism.

### mechanism

- rust `Read`/`Write` impls stored in thread-local `PortStore` (keyed by port ID)
- `extern "C"` trampoline in `tein_shim.c` looks up impl by port ID, delegates
- RAII guard cleans up on drop (same pattern as `ForeignStoreGuard`)
- chibi's `sexp_make_custom_input_port` / `sexp_make_custom_output_port` under the hood

### scheme side

ports created from rust are regular chibi ports. scheme uses `(read port)`, `(write obj port)`, `(display obj port)` etc. no special handling.

### data flow

```
rust Read/Write impl
  -> thread-local PortStore (keyed by port ID)
    -> extern "C" trampoline (tein_shim.c)
      -> chibi custom port callback
        -> normal chibi read/write machinery
```

## feature 2: hash dispatch reader extensions

### C-side patch

patch `sexp.c` `#` dispatch: before the hardcoded switch, check a dispatch table (hash table on the context). if handler found, call it with the port. otherwise fall through to existing r7rs behaviour.

single-character dispatch — the handler reads further characters from the port if it needs multi-character prefixes (e.g. `#json{...}` handler registered on `#\j`, reads `son` itself).

r7rs reserved characters (`#t`, `#f`, `#\`, `#(`, `#u`, `#;`, `#|`, `#!`, `#b`, `#o`, `#d`, `#x`, `#e`, `#i`, digits for `#n=`/`#n#`) cannot be overridden.

### scheme API via `(tein reader)` VFS module

```scheme
(import (tein reader))

;; register
(set-reader! #\j
  (lambda (port)
    (let ((form (read port)))
      `(json-parse ',form))))

;; unregister
(unset-reader! #\j)

;; introspection
(reader-dispatch-chars)  ;; -> (#\j #\p ...)
```

### error messages (LLM-friendly)

- `unknown reader dispatch #q -- no handler registered. currently registered: #j, #p`
- `reader dispatch #t is reserved by r7rs and cannot be overridden`

### rust side

no dedicated registration API. host pre-registers via `ctx.evaluate()` or init closures. one mechanism.

### sandboxing

`set-reader!` only available via `(tein reader)` module import. module only importable if sandbox policy allows it. same gating pattern as `(tein foreign)`.

## module structure

- `src/port.rs` — PortStore, Read/Write bridge, RAII guard, public API on Context
- `(tein reader)` VFS module — set-reader!, unset-reader!, reader-dispatch-chars
- `tein_shim.c` — custom port wrappers + dispatch table helpers
- `sexp.c` — patch: dispatch table check before hardcoded # switch

## dependency order

custom ports first, then hash dispatch. reader dispatch handlers receive the port as argument — if backed by a rust Read impl, bytes flow from rust through the custom port into the handler.

## test strategy

- custom ports: round-trip read/write through Cursor, channel-based, error propagation
- reader dispatch: register from scheme, use from scheme, error messages, reserved character rejection, introspection, unregister, sandbox policy gating
