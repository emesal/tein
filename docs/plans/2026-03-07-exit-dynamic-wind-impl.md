# exit dynamic-wind cleanup implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** make `exit` r7rs-compliant by running `dynamic-wind` "after" thunks and flushing/closing ports before halting; `emergency-exit` keeps immediate-abort semantics.

**Architecture:** rename the rust exit trampoline to `emergency-exit`, then define `exit` as a scheme procedure in `(tein process)` that calls `travel-to-point!` to unwind `%dk`, flushes/closes ports, and delegates to `emergency-exit`. see `docs/plans/2026-03-07-exit-dynamic-wind-design.md` for full design.

**Tech Stack:** rust (context.rs FFI), scheme (chibi-scheme fork process.sld/process.scm), cargo test

**Branch:** `just bugfix exit-dynamic-wind-101-2603`

---

### task 1: rename rust trampoline registration from "exit" to "emergency-exit"

**files:**
- modify: `tein/src/context.rs:4060` — change `"exit"` → `"emergency-exit"` in `register_process_module()`
- modify: `tein/src/context.rs:1833-1850` — update docstring on `exit_trampoline` to reflect it's now `emergency-exit`

**step 1: update the registration**

in `register_process_module()` at line 4060, change:

```rust
self.define_fn_variadic("exit", exit_trampoline)?;
```

to:

```rust
self.define_fn_variadic("emergency-exit", exit_trampoline)?;
```

**step 2: update the docstring**

replace the `exit_trampoline` docstring (lines 1833-1850) with:

```rust
/// `emergency-exit` trampoline: immediate VM halt without cleanup.
///
/// sets EXIT_REQUESTED + EXIT_VALUE thread-locals and returns a scheme
/// exception to immediately stop the VM. the eval loop intercepts this
/// via `check_exit()` and returns `Ok(value)` to the rust caller.
///
/// semantics: `(exit)` → 0, `(exit #t)` → 0, `(exit #f)` → 1, `(exit obj)` → obj
///
/// this is r7rs `emergency-exit` — no `dynamic-wind` "after" thunks run,
/// no ports flushed. r7rs `exit` (which does run cleaners) is implemented
/// as a scheme procedure in `(tein process)` that delegates here after cleanup.
```

**step 3: update `register_process_module` docstring**

at line 4052, change:

```rust
/// Register `get-environment-variable`, `get-environment-variables`,
/// `command-line`, and `exit` native functions.
```

to:

```rust
/// Register `get-environment-variable`, `get-environment-variables`,
/// `command-line`, and `emergency-exit` native functions.
```

**step 4: run tests to confirm existing exit tests now fail**

run: `cargo test -p tein test_tein_process_exit 2>&1 | tail -30`

expected: tests that use `(exit ...)` directly should still pass because `exit` is now defined in scheme (wait — the scheme module hasn't been updated yet, so `exit` is undefined). the tests should fail since `exit` is no longer registered as a native fn.

actually: the tests import `(tein process)` which re-exports `exit` from `process.scm`. the `.scm` body is currently empty (trampolines are free vars). so `exit` will be an unbound reference. tests SHOULD fail. this is expected — we fix it in task 2.

**step 5: commit**

```
fix(context): rename exit trampoline registration to emergency-exit (#101)

the rust trampoline is now registered as "emergency-exit" (r7rs immediate
halt, no cleanup). exit will become a scheme procedure in the next step.
```

---

### task 2: update chibi-scheme fork — process.sld and process.scm

**files:**
- modify: `~/forks/chibi-scheme/lib/tein/process.sld`
- modify: `~/forks/chibi-scheme/lib/tein/process.scm`

**important:** changes go in `~/forks/chibi-scheme/` (the fork repo), NOT `target/chibi-scheme/` which is hard-reset on build.

**step 1: update process.sld**

replace contents of `~/forks/chibi-scheme/lib/tein/process.sld` with:

```scheme
(define-library (tein process)
  (import (scheme base) (chibi))
  (export get-environment-variable get-environment-variables
          command-line exit emergency-exit)
  (include "process.scm"))
```

changes: added `(chibi)` to imports, added `emergency-exit` to exports.

**step 2: update process.scm**

replace contents of `~/forks/chibi-scheme/lib/tein/process.scm` with:

```scheme
;;; (tein process) — process context access
;;;
;;; get-environment-variable, get-environment-variables, command-line,
;;; and emergency-exit are rust trampolines registered by the runtime.
;;;
;;; exit: r7rs-compliant — unwinds dynamic-wind "after" thunks via
;;; travel-to-point!, flushes and closes current output and error ports,
;;; then delegates to emergency-exit (rust trampoline, immediate VM halt).
;;;
;;; emergency-exit: immediate halt — no dynamic-wind cleanup, no port
;;; flushing. r7rs semantics.
;;;
;;; in sandboxed contexts, get-environment-variable returns #f,
;;; get-environment-variables returns '(), and command-line returns '("tein").

;;; walk %dk chain to find the actual root point.
;;; root-point from init-7.scm is NOT the same object as the actual %dk root
;;; in tein's context (tein's env setup creates a fresh root with #f thunks).
(define (%find-root point)
  (let ((parent (vector-ref point 3)))
    (if parent (%find-root parent) point)))

(define %exit-root (%find-root (%dk)))

(define (exit . args)
  ;; unwind dynamic-wind "after" thunks (innermost first)
  (travel-to-point! (%dk) %exit-root)
  (%dk %exit-root)
  ;; flush and close ports (r7rs: "flushes all ports ... then exits")
  (flush-output-port (current-output-port))
  (flush-output-port (current-error-port))
  (close-output-port (current-output-port))
  (close-output-port (current-error-port))
  ;; delegate to rust trampoline for actual VM halt
  (apply emergency-exit args))
```

**step 3: push the fork**

```bash
cd ~/forks/chibi-scheme && git add lib/tein/process.sld lib/tein/process.scm && git commit -m "feat(tein): r7rs-compliant exit with dynamic-wind cleanup (#101)

exit now unwinds %dk via travel-to-point!, flushes/closes ports,
then delegates to emergency-exit. emergency-exit is unchanged
(immediate halt, no cleanup)." && git push
```

**step 4: rebuild tein to pull fork changes**

```bash
cd ~/projects/tein && just clean && cargo build
```

**step 5: run exit tests to verify they pass**

run: `cargo test -p tein test_tein_process_exit 2>&1 | tail -30`

expected: all existing exit value tests pass. the `test_tein_process_exit_skips_dynamic_wind` test will FAIL because exit now DOES run dynamic-wind thunks (the "after" thunk calls `(error ...)` which interrupts exit). this is expected — we fix it in task 3.

**step 6: commit tein side**

```
chore: rebuild against updated chibi fork for exit changes (#101)
```

note: there may be nothing to commit on the tein side here (build artefacts aren't tracked). that's fine.

---

### task 3: update VFS entry deps for tein/process

**files:**
- modify: `tein/src/vfs_registry.rs:232` — update deps to include `chibi`

**step 1: update deps**

at line 232, change:

```rust
deps: &["scheme/base"],
```

to:

```rust
deps: &["scheme/base", "chibi"],
```

this ensures sandboxed contexts that allow `tein/process` also pull in `(chibi)` transitively.

**step 2: run tests**

run: `cargo test -p tein test_tein_process_exit 2>&1 | tail -30`

expected: same as task 2 step 5 — value tests pass, dynamic-wind test fails.

**step 3: commit**

```
fix(vfs): add chibi dep to tein/process for dynamic-wind access (#101)
```

---

### task 4: update tests

**files:**
- modify: `tein/src/context.rs` — update/add tests in the exit test section (around line 5683)

**step 1: rewrite `test_tein_process_exit_skips_dynamic_wind`**

replace the test at lines 5724-5746 with:

```rust
    #[test]
    fn test_exit_runs_dynamic_wind_after_thunks() {
        // r7rs: exit must run dynamic-wind "after" thunks before halting.
        // the after thunk mutates a flag; we check it ran by reading it
        // from a separate evaluate() call before exit halts.
        //
        // approach: use a custom output port to capture the after thunk's
        // side effect (writing to stdout), since exit halts the VM.
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate(
            "(let ((log '())) \
               (dynamic-wind \
                 (lambda () (set! log (cons 'in log))) \
                 (lambda () \
                   (dynamic-wind \
                     (lambda () (set! log (cons 'in2 log))) \
                     (lambda () \
                       (display (reverse log)) \
                       (exit 42)) \
                     (lambda () (set! log (cons 'out2 log))))) \
                 (lambda () (set! log (cons 'out log)))))",
        );
        // exit runs after thunks (out2, out) then halts — we get Exit(42)
        assert_eq!(r.unwrap(), Value::Exit(42));
    }

    #[test]
    fn test_exit_nested_dynamic_wind_order() {
        // verify innermost-first unwind order via output port capture
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let out = ctx.open_output_port();
        ctx.set_current_output_port(&out);
        let r = ctx.evaluate(
            "(dynamic-wind \
               (lambda () #f) \
               (lambda () \
                 (dynamic-wind \
                   (lambda () #f) \
                   (lambda () \
                     (dynamic-wind \
                       (lambda () #f) \
                       (lambda () (exit 0)) \
                       (lambda () (display \"c\")))) \
                   (lambda () (display \"b\")))) \
               (lambda () (display \"a\")))",
        );
        assert_eq!(r.unwrap(), Value::Exit(0));
        let output = out.get_string();
        assert_eq!(output, "cba", "after thunks run innermost-first");
    }

    #[test]
    fn test_emergency_exit_skips_dynamic_wind() {
        // emergency-exit must NOT run dynamic-wind "after" thunks
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let r = ctx.evaluate(
            "(dynamic-wind \
               (lambda () #f) \
               (lambda () (emergency-exit 42)) \
               (lambda () (error \"after thunk ran — unexpected\")))",
        );
        assert_eq!(
            r.unwrap(),
            Value::Exit(42),
            "emergency-exit bypasses dynamic-wind after thunks"
        );
    }

    #[test]
    fn test_exit_flushes_output_port() {
        // exit should flush current-output-port before halting
        let ctx = Context::new_standard().unwrap();
        ctx.evaluate("(import (tein process))").unwrap();
        let out = ctx.open_output_port();
        ctx.set_current_output_port(&out);
        let r = ctx.evaluate("(display \"hello\") (exit 0)");
        assert_eq!(r.unwrap(), Value::Exit(0));
        let output = out.get_string();
        assert_eq!(output, "hello", "output port flushed before exit");
    }
```

note on the output port API: check what API `tein` actually provides for custom output ports. the tests above assume `ctx.open_output_port()` and `out.get_string()`. if the API differs, adapt. look at existing port tests for the correct pattern:

run: `cargo test -p tein --lib -- --list 2>&1 | grep -i port | head -20`

adjust the test code to match whatever API exists. the key assertions are:
- `test_exit_runs_dynamic_wind_after_thunks`: `exit` returns `Exit(42)` and after thunks ran
- `test_exit_nested_dynamic_wind_order`: after thunks run innermost-first
- `test_emergency_exit_skips_dynamic_wind`: `emergency-exit` returns `Exit(42)`, no after thunks
- `test_exit_flushes_output_port`: output visible after exit

**step 2: run all exit tests**

run: `cargo test -p tein test_tein_process_exit test_exit test_emergency_exit 2>&1 | tail -30`

expected: all pass.

**step 3: run full test suite**

run: `just test 2>&1 | tail -10`

expected: all pass.

**step 4: commit**

```
test: add exit dynamic-wind and emergency-exit tests (#101)

- test_exit_runs_dynamic_wind_after_thunks: after thunks run on exit
- test_exit_nested_dynamic_wind_order: innermost-first unwind
- test_emergency_exit_skips_dynamic_wind: emergency-exit skips thunks
- test_exit_flushes_output_port: port flushed before exit
- removed test_tein_process_exit_skips_dynamic_wind (no longer applicable)
```

---

### task 5: update docs — AGENTS.md

**files:**
- modify: `AGENTS.md:91` — update exit escape hatch flow paragraph

**step 1: replace the exit escape hatch flow paragraph**

replace the paragraph at line 91 (the one starting with `**exit escape hatch flow**:`) with:

```
**exit escape hatch flow**: `(import (tein process))` → `(exit)` / `(exit obj)` unwinds the `%dk` dynamic-wind stack via `travel-to-point!` (runs all "after" thunks innermost-first), flushes and closes current output/error ports, then calls `emergency-exit` (rust trampoline). `emergency-exit` sets EXIT_REQUESTED + EXIT_VALUE thread-locals + returns exception to stop VM immediately → eval loop (`evaluate`/`evaluate_port`/`call`) intercepts via `check_exit()` → clears flags → converts EXIT_VALUE to `Value` → returns `Ok(Value::Exit(n))` to rust caller. `(exit)` → 0, `(exit #t)` → 0, `(exit #f)` → 1, `(exit obj)` → obj. EXIT_REQUESTED/EXIT_VALUE cleared on Context::drop(). `emergency-exit` is r7rs-compliant: immediate halt, no cleanup. `exit` is r7rs-compliant: runs `dynamic-wind` "after" thunks and flushes ports before halting.
```

also: find and remove the `**r7rs deviation**` sentence about exit/dynamic-wind (the "a future standalone interpreter host..." part). it's no longer accurate.

**step 2: commit**

```
docs(agents): update exit escape hatch flow for r7rs compliance (#101)
```

---

### task 6: update docs — ARCHITECTURE.md and reference.md

**files:**
- modify: `ARCHITECTURE.md:223-237` — update exit escape hatch flow
- modify: `docs/reference.md:227-233` — update r7rs deviations section

**step 1: update ARCHITECTURE.md**

replace lines 223-237 with:

```markdown
### Exit escape hatch flow

```
Scheme code calls (exit) or (exit obj) via (tein process):
  1. exit (scheme proc) unwinds %dk dynamic-wind stack via travel-to-point!
  2. flushes and closes current output/error ports
  3. calls emergency-exit (rust trampoline)
  4. emergency-exit sets EXIT_REQUESTED + EXIT_VALUE thread-locals
  5. returns an exception sexp to stop VM immediately
  6. evaluate() / evaluate_port() / call() intercepts via check_exit()
  7. check_exit(): reads EXIT_REQUESTED → clears flags → converts EXIT_VALUE → returns Ok(Value::Exit(n))
  8. (exit) → 0, (exit #t) → 0, (exit #f) → 1, (exit obj) → obj
  9. EXIT_REQUESTED + EXIT_VALUE cleared on Context::drop()

Scheme code calls (emergency-exit) or (emergency-exit obj):
  - skips steps 1-2 (no dynamic-wind cleanup, no port flushing)
  - goes directly to step 4
```
```

**step 2: update reference.md**

replace lines 227-233 (the "exit and dynamic-wind" deviation section) with:

```markdown
### exit and dynamic-wind

`exit` in `(tein process)` is r7rs-compliant — it runs all `dynamic-wind` "after" thunks
(innermost-first) and flushes/closes current output and error ports before halting.

`emergency-exit` skips all cleanup (r7rs-compliant immediate halt).
```

**step 3: commit**

```
docs: update architecture and reference for r7rs exit compliance (#101)
```

---

### task 7: update context.rs docstring for `check_exit`

**files:**
- modify: `tein/src/context.rs:2755-2770` — minor docstring update

**step 1: update `check_exit` docstring**

the existing docstring is fine but add a note that `exit` now runs dynamic-wind cleanup before reaching this point:

```rust
/// If the exit flag is set, clears it, releases the GC root on the
/// stashed value, and returns `Some(Ok(Value::Exit(n)))`.
/// Returns `None` if no exit was requested.
///
/// Called after scheme `emergency-exit` (direct) or `exit` (after
/// dynamic-wind cleanup and port flushing in scheme).
```

**step 2: run lint**

run: `just lint`

**step 3: run full test suite**

run: `just test 2>&1 | tail -10`

expected: all pass.

**step 4: commit**

```
fix: r7rs-compliant exit with dynamic-wind cleanup

closes #101
```

---

### task 8: collect AGENTS.md notes

**files:**
- modify: `AGENTS.md` — review and update any remaining references

**step 1: search for stale exit references**

run: `grep -n "GH #101\|r7rs deviation.*exit\|emergency-exit semantics.*exit" AGENTS.md`

remove or update any remaining references to the old deviation. the exit escape hatch flow was already updated in task 5. check that:
- the `**exit escape hatch flow**` paragraph (task 5) is the only place exit semantics are described
- no other paragraph still says "both exit and emergency-exit have emergency-exit semantics"
- the `critical gotchas` section's `Value::Exit(i32)` note is still accurate (it is — no changes needed there)

**step 2: commit if changes made**

```
chore(agents): remove stale exit r7rs deviation references (#101)
```

---

## notes for executing agent

- the chibi fork changes (task 2) MUST be pushed before rebuilding tein. `target/chibi-scheme` is hard-reset from remote on build.
- `(chibi)` dep is critical — without it, `travel-to-point!`, `%dk`, and `root-point` are invisible to `process.scm`.
- the output port test (task 4) needs the actual port API — check existing port tests for the right pattern before writing.
- `just lint` before final commit to catch formatting issues.
- after all tasks: run `just test` to confirm nothing regressed.
