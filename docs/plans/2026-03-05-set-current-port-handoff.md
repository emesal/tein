# set_current_port handoff

## status

branch: `feature/set-current-port-2603`

tasks 1–5 complete and committed. task 6 (lint + full test) **not yet done**.
currently mid-debugging a REPL display issue — fix implemented, needs verification.

## what was done

### tasks 1–5 (committed)
- C shim: `tein_sexp_set_parameter` + port symbol getters in chibi fork (pushed)
- FFI bindings + safe wrappers in `ffi.rs`
- `Context::set_current_output_port` / `set_current_input_port` / `set_current_error_port` with full tests
- REPL `TrackingWriter` + `SharedTrackingWriter` in `tein-bin/src/main.rs`
- docs: AGENTS.md, docs/embedding.md, docs/reference.md updated

### diagnostic example (should be cleaned up or kept as example)
- `tein/examples/port_redirect.rs` — diagnostic example proving port redirect works without rustyline
- `tein-bin/examples/port_repl_sim.rs` — diagnostic example simulating REPL port setup

## the bug we found and fixed

### root cause
chibi's custom port (non-`SEXP_USE_STRING_STREAMS` path) uses a **buffer**. `(display "hello")` writes into the buffer via `sexp_buffered_write_string_n` but does NOT flush. the flush only happens when:
1. the buffer fills up (>4096 bytes), or
2. `(flush-output port)` is explicitly called

in the old REPL code, eval was wrapped in a scheme let that called `(flush-output ...)` explicitly. we removed that wrapper when introducing TrackingWriter. result: `(display "hello")` fills the buffer, `flush-output` is never called, `sexp_buffered_flush` never invokes the write proc, our `SharedTrackingWriter::write` is never called, stdout never gets the bytes.

### fix applied (not yet verified)
in `run_repl`, user input is now wrapped again:
```rust
let flushed = format!(
    "(let ((__r__ (begin {}))) (flush-output (current-output-port)) __r__)",
    input
);
match ctx.evaluate(&flushed) { ... }
```

this is the same pattern as the old code, but now we have `TrackingWriter` to suppress spurious blank lines — so blank lines are fixed AND output is visible.

## what still needs doing

1. **verify the fix works** — run the REPL interactively:
   - `(display "hello")` → should show `hello`, no blank line after
   - `(display "hello\n")` → should show `hello`, no blank line after
   - `'meow` → should show `meow`
   - `(+ 1 2)` → should show `3`

2. **run just test** — full test suite must pass (929 tests, 5 skipped)

3. **run just lint** — clean

4. **clean up diagnostic artifacts**:
   - `tein/examples/port_redirect.rs` — keep as useful example OR remove
   - `tein-bin/examples/port_repl_sim.rs` — remove (debug only)
   - `tein/src/context.rs` — added `test_set_current_output_port_survives_multiple_evals` test (keep it, it's a good regression test)

5. **commit** the fix with: `fix(repl): flush custom port after each eval`

6. **update AGENTS.md note**: the "no GC rooting needed" note is correct. add note about buffering: chibi custom port (non-STRING_STREAMS) buffers writes — `flush-output` must be called explicitly to drain to the write proc.

7. **close issues** — commits should reference `closes #120, closes #121`

8. **PR** — create PR to `dev` (NOT main)

## current git state

clean after commits. pending:
- the `flushed` wrap fix in `tein-bin/src/main.rs` (not committed yet)
- `tein/examples/port_redirect.rs` (not committed, new file)
- `tein-bin/examples/port_repl_sim.rs` (not committed, new file)
- diagnostic/debug changes in `tein/src/context.rs` (extra test added)
- chibi fork pushed clean (shim debug logs removed)

## chibi fork

chibi fork is clean — debug logs were added and removed. the `tein_dbg_print_cur_out` function was added to shim but already removed. current shim head: `68ea2119`. target/chibi-scheme was rebuilt clean.

## key insight for future reference

chibi's `sexp_make_custom_port` (without `SEXP_USE_STRING_STREAMS`, which is our case) creates a buffered port. the write proc is only called during `sexp_buffered_flush`, not on every write. so any code that installs a custom port as `current-output-port` MUST ensure `(flush-output)` is called after scheme code that produces output — otherwise output is silently buffered and the write proc never fires.

this is NOT a bug in tein's implementation — it's correct chibi behaviour. the REPL just needs to flush explicitly.
