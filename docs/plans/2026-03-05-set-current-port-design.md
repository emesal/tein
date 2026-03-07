# design: `set_current_output_port` API + REPL TrackingWriter

closes #121 (repl: eliminate spurious blank lines after each expression)
closes #120 (repl: flush stdout mid-eval for streaming output)

## problem

the REPL emits a spurious blank line after every expression because it
unconditionally prints `\n` after eval. this works around rustyline's
`\r\x1b[K` prompt redraw erasing mid-line chibi output. additionally,
mid-eval streaming output is fully buffered because rustyline's raw mode
causes libc to downgrade stdout to fully-buffered.

## solution

expose chibi's `sexp_set_parameter` to rust. add general-purpose
`Context::set_current_{output,input,error}_port` API. use it in the REPL
to install a `TrackingWriter`-backed custom port that flushes eagerly and
tracks whether the last written byte was `\n`.

## layer 1: C shim (`tein_shim.c`)

```c
void tein_sexp_set_parameter(sexp ctx, sexp env, sexp name, sexp value) {
    sexp_set_parameter(ctx, env, name, value);
}

sexp tein_sexp_global_cur_out_symbol(sexp ctx) {
    return sexp_global(ctx, SEXP_G_CUR_OUT_SYMBOL);
}
sexp tein_sexp_global_cur_in_symbol(sexp ctx) {
    return sexp_global(ctx, SEXP_G_CUR_IN_SYMBOL);
}
sexp tein_sexp_global_cur_err_symbol(sexp ctx) {
    return sexp_global(ctx, SEXP_G_CUR_ERR_SYMBOL);
}
```

rationale for separate symbol getters: `SEXP_G_CUR_*_SYMBOL` are
macro-expanded enum indices into chibi's global table. three tiny getters
is cleaner than exposing the raw global table with a numeric index.

## layer 2: FFI (`ffi.rs`)

extern declarations for all four shim functions, plus safe wrappers.
symbol getters return `sexp` (no allocation, no exception possible).

## layer 3: Context API (`context.rs`)

three public methods:

```rust
pub fn set_current_output_port(&self, port: &Value) -> Result<()>
pub fn set_current_input_port(&self, port: &Value) -> Result<()>
pub fn set_current_error_port(&self, port: &Value) -> Result<()>
```

internally each:
1. extracts raw `sexp` via `as_port()`, errors if not a port
2. gets env via `sexp_context_env(self.ctx)`
3. gets symbol via the appropriate `sexp_global_cur_*_symbol`
4. calls `sexp_set_parameter(ctx, env, symbol, raw_port)`

a private `set_port_parameter` helper avoids repeating this across three
methods.

no GC rooting needed: `sexp_set_parameter` mutates an existing opcode
data cons cell (or creates one that's immediately rooted in the opcode).

## layer 4: REPL (`tein-bin/src/main.rs`)

`TrackingWriter` struct:

```rust
struct TrackingWriter {
    last_was_newline: Cell<bool>,
}
```

implements `Write` via a `SharedTrackingWriter(Rc<TrackingWriter>)` newtype:
- `write()`: writes to `std::io::stdout()`, sets `last_was_newline` based
  on last byte, flushes stdout on every write (solves #120).
- `flush()`: flushes stdout.

REPL setup:
1. create `TrackingWriter`, wrap in `Rc`
2. `ctx.open_output_port(SharedTrackingWriter(tracker.clone()))?`
3. `ctx.set_current_output_port(&port)?`

REPL eval loop:
- remove `flush-output` wrapping format string
- remove unconditional `println!()`
- after eval, emit `\n` only if `!tracker.last_was_newline.get()`

## testing

library tests (context.rs):
- `test_set_current_output_port`: custom port backed by `Vec<u8>`,
  verify `(display "hello")` writes to vec
- `test_set_current_input_port`: `Cursor` reader, verify `(read)` reads
  from it
- `test_set_current_error_port`: same pattern with error port
- `test_set_port_rejects_non_port`: pass `Value::Integer`, expect
  `Error::TypeError`

REPL tests (tein-bin):
- `test_tracking_writer_tracks_newline`: write bytes, verify flag
- `test_tracking_writer_empty_write`: verify flag unchanged

## docs updates

- AGENTS.md: mention `set_current_*_port` near custom port flow
- docs/reference.md: add three methods to Context API reference
