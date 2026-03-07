# design: r7rs-compliant `exit` with dynamic-wind cleanup

**issue:** #101
**date:** 2026-03-07
**status:** approved

## summary

tein's `exit` currently has `emergency-exit` semantics — it immediately halts the VM without running `dynamic-wind` "after" thunks. r7rs requires `exit` to unwind the dynamic-wind stack and flush/close ports before returning. `emergency-exit` is correct as-is (immediate abort).

## approach

`exit` becomes a scheme procedure that manually unwinds the `%dk` dynamic-wind stack via `travel-to-point!`, flushes/closes ports, then delegates to the rust `emergency-exit` trampoline.

no continuation wrapping of `evaluate()` needed — `travel-to-point!` and `%dk` are accessible from `(import (chibi))` and handle the unwind directly.

### validated prototype

```scheme
(import (chibi))

(define (find-root point)
  (let ((parent (vector-ref point 3)))
    (if parent (find-root parent) point)))

(define %exit-root (find-root (%dk)))

(define (exit . args)
  (travel-to-point! (%dk) %exit-root)
  (%dk %exit-root)
  (apply emergency-exit args))
```

tested: nested `dynamic-wind` "after" thunks run in correct (innermost-first) order before exit halts the VM.

### key discovery: `root-point` vs actual root

`root-point` defined in `init-7.scm` is NOT the actual root of tein's `%dk` stack — tein's env setup creates a fresh root point with `#f` thunks (not error-raising procedures). using `root-point` directly causes "non procedure application" errors when `travel-to-point!` tries to call `(%point-out root-point)`. the fix is to walk the `%dk` chain to find the actual root at module load time.

## changes

### 1. rust: rename trampoline registration

`register_process_module()` in `context.rs` currently registers the trampoline as `"exit"`. rename to `"emergency-exit"`. this makes the rust trampoline the r7rs `emergency-exit` (immediate abort, no cleanup).

### 2. scheme: `(tein process)` module

**`process.sld`**: add `(import (chibi))`, add `emergency-exit` to exports.

**`process.scm`**: define scheme-level `exit` that:
1. captures actual `%dk` root at module load time via `find-root`
2. on `(exit . args)`:
   - calls `(travel-to-point! (%dk) %exit-root)` — runs all "after" thunks
   - resets `(%dk %exit-root)`
   - flushes current output and error ports
   - closes current output and error ports (per r7rs "closes all open ports")
   - calls `(apply emergency-exit args)` — rust trampoline halts VM

### 3. sandbox: `(scheme process-context)` shadow

no change needed — both `exit` and `emergency-exit` are already stubs returning `#f` in sandboxed contexts.

### 4. tests

- **update** `test_tein_process_exit_skips_dynamic_wind`: rename, assert thunks DO run now
- **add** `test_exit_runs_dynamic_wind_after_thunks`: nested dynamic-wind, verify "after" thunks execute
- **add** `test_exit_nested_dynamic_wind_order`: verify innermost-first unwind order
- **add** `test_emergency_exit_skips_dynamic_wind`: verify emergency-exit does NOT run thunks
- **add** `test_exit_flushes_ports`: verify output port flushed before exit
- **keep** existing exit value tests (they test `emergency-exit` semantics now via `exit` delegation)

### 5. docs

- AGENTS.md: update exit escape hatch flow, remove r7rs deviation note for `exit`
- ARCHITECTURE.md: update exit flow description
- docstrings: update `exit_trampoline` (now `emergency-exit`), `check_exit`

## edge cases

### "after" thunk raises an error

`travel-to-point!` propagates the error normally. `emergency-exit` never runs. the VM returns `Err(EvalError(...))` instead of `Value::Exit`. this is reasonable — if cleanup code errors, the embedder should know.

### port flushing

`exit` calls `(flush-output-port (current-output-port))` and `(flush-output-port (current-error-port))` before `emergency-exit`. also closes them per r7rs. if flushing fails, the error propagates (same as "after" thunk error case).

### `evaluate()` vs `evaluate_port()` vs `call()`

all three check `check_exit()` after each eval step. the scheme-level `exit` calls `emergency-exit` which sets the same thread-locals and throws the same exception. no changes needed to the rust eval loop.

### sandboxed contexts

`(tein process)` exit is a real scheme procedure in unsandboxed contexts. in sandboxed contexts, `(scheme process-context)` shadow stubs both `exit` and `emergency-exit` as `(lambda args #f)`. the `(tein process)` module itself is gated by allowlist. no change needed.

## non-goals

- r7rs `exit` running `dynamic-wind` thunks established in *other* `evaluate()` calls (each eval is independent)
- closing arbitrary user-opened ports (only current-output and current-error flushed/closed)
