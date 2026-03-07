# set_current_port API + REPL TrackingWriter — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** expose chibi's `sexp_set_parameter` to rust, add `Context::set_current_{output,input,error}_port`, use it in the REPL to eliminate blank lines (#121) and enable streaming flush (#120).

**Architecture:** four layers — C shim wrapper, FFI bindings, Context API methods, REPL TrackingWriter. TDD: tests before implementation at each layer. the library provides the general primitive; the REPL is the first consumer.

**Tech Stack:** C (tein_shim.c in chibi fork), rust unsafe FFI, rust `std::io::Write`, rustyline REPL.

**Design doc:** `docs/plans/2026-03-05-set-current-port-design.md`

**Branch:** create with `just bugfix repl-set-port-2603`

---

### Task 1: C shim — expose `sexp_set_parameter` + symbol getters

**Files:**
- Modify: `~/forks/chibi-scheme/tein_shim.c` (after custom port section, ~line 476)

**Step 1: add the four shim functions**

after the `tein_make_custom_output_port` function (line 476), add:

```c
// --- parameter setting ---
//
// wraps sexp_set_parameter to allow setting current-output-port,
// current-input-port, current-error-port from rust.

void tein_sexp_set_parameter(sexp ctx, sexp env, sexp name, sexp value) {
    sexp_set_parameter(ctx, env, name, value);
}

sexp tein_sexp_global_cur_in_symbol(sexp ctx) {
    return sexp_global(ctx, SEXP_G_CUR_IN_SYMBOL);
}

sexp tein_sexp_global_cur_out_symbol(sexp ctx) {
    return sexp_global(ctx, SEXP_G_CUR_OUT_SYMBOL);
}

sexp tein_sexp_global_cur_err_symbol(sexp ctx) {
    return sexp_global(ctx, SEXP_G_CUR_ERR_SYMBOL);
}
```

**Step 2: push chibi fork**

```bash
cd ~/forks/chibi-scheme
git add tein_shim.c
git commit -m "feat(shim): expose sexp_set_parameter + port symbol getters for rust FFI"
git push
```

**Step 3: rebuild tein to pull the fork change**

```bash
cd ~/projects/tein
just clean && cargo build
```

expected: builds successfully. the new symbols are available for linking.

**Step 4: commit (empty, just to mark progress if needed)**

no tein-side commit yet — nothing changed in tein's repo.

---

### Task 2: FFI bindings for set_parameter + symbol getters

**Files:**
- Modify: `tein/src/ffi.rs` — extern block (~line 218) + safe wrappers (~line 866)

**Step 1: write the failing test**

add to `tein/src/context.rs` test section (after `test_output_port_write` ~line 6820):

```rust
#[test]
fn test_set_current_output_port() {
    use std::sync::{Arc, Mutex};

    struct SharedWriter(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let ctx = Context::new_standard().expect("context");
    let port = ctx
        .open_output_port(SharedWriter(buf.clone()))
        .expect("open port");

    ctx.set_current_output_port(&port).expect("set port");

    // display without explicit port arg — should go to our custom port
    ctx.evaluate("(display \"hello\")").expect("display");
    ctx.evaluate("(flush-output (current-output-port))")
        .expect("flush");

    let output = buf.lock().unwrap();
    assert_eq!(&*output, b"hello");
}
```

**Step 2: run the test to verify it fails**

```bash
cargo test -p tein test_set_current_output_port -- --nocapture
```

expected: FAIL — `set_current_output_port` doesn't exist.

**Step 3: add extern declarations**

in `tein/src/ffi.rs`, inside the `extern "C"` block, after the custom port declarations (~line 218):

```rust
    // parameter setting (for current-output-port etc.)
    pub fn tein_sexp_set_parameter(ctx: sexp, env: sexp, name: sexp, value: sexp);

    // global symbol accessors for standard port parameters
    pub fn tein_sexp_global_cur_in_symbol(ctx: sexp) -> sexp;
    pub fn tein_sexp_global_cur_out_symbol(ctx: sexp) -> sexp;
    pub fn tein_sexp_global_cur_err_symbol(ctx: sexp) -> sexp;
```

**Step 4: add safe wrappers**

after `make_custom_output_port` (~line 866):

```rust
/// set a parameter value in the given environment.
///
/// used to override `current-output-port`, `current-input-port`,
/// `current-error-port`. `name` must be the global symbol for the
/// parameter (obtained via `sexp_global_cur_*_symbol`).
#[inline]
pub unsafe fn sexp_set_parameter(ctx: sexp, env: sexp, name: sexp, value: sexp) {
    unsafe { tein_sexp_set_parameter(ctx, env, name, value) }
}

/// return the global symbol for `current-input-port`.
#[inline]
pub unsafe fn sexp_global_cur_in_symbol(ctx: sexp) -> sexp {
    unsafe { tein_sexp_global_cur_in_symbol(ctx) }
}

/// return the global symbol for `current-output-port`.
#[inline]
pub unsafe fn sexp_global_cur_out_symbol(ctx: sexp) -> sexp {
    unsafe { tein_sexp_global_cur_out_symbol(ctx) }
}

/// return the global symbol for `current-error-port`.
#[inline]
pub unsafe fn sexp_global_cur_err_symbol(ctx: sexp) -> sexp {
    unsafe { tein_sexp_global_cur_err_symbol(ctx) }
}
```

**Step 5: verify it compiles**

```bash
cargo build -p tein
```

expected: compiles. test still fails (no `set_current_output_port` on `Context` yet).

**Step 6: commit**

```bash
git add tein/src/ffi.rs tein/src/context.rs
git commit -m "feat(ffi): add sexp_set_parameter + port symbol getter bindings"
```

---

### Task 3: Context API — `set_current_{output,input,error}_port`

**Files:**
- Modify: `tein/src/context.rs` — add private helper + three public methods (near `open_output_port` ~line 2919)

**Step 1: add the private helper + three public methods**

after `open_output_port` (after line ~2919):

```rust
    /// set a standard port parameter to the given port value.
    ///
    /// `symbol_fn` returns the global symbol for the parameter
    /// (e.g. `ffi::sexp_global_cur_out_symbol`).
    fn set_port_parameter(
        &self,
        port: &Value,
        symbol_fn: unsafe fn(ffi::sexp) -> ffi::sexp,
    ) -> Result<()> {
        let raw_port = port
            .as_port()
            .ok_or_else(|| Error::TypeError(format!("expected port, got {}", port)))?;
        unsafe {
            let env = ffi::sexp_context_env(self.ctx);
            let sym = symbol_fn(self.ctx);
            ffi::sexp_set_parameter(self.ctx, env, sym, raw_port);
        }
        Ok(())
    }

    /// Set the current output port for this context.
    ///
    /// Replaces the port that `(current-output-port)` returns in Scheme code.
    /// All output operations (`display`, `write`, `newline`, `write-char`)
    /// that default to `(current-output-port)` will use this port.
    ///
    /// # Examples
    ///
    /// ```
    /// use tein::{Context, Value};
    /// use std::sync::{Arc, Mutex};
    ///
    /// struct SharedWriter(Arc<Mutex<Vec<u8>>>);
    /// impl std::io::Write for SharedWriter {
    ///     fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
    ///         self.0.lock().unwrap().extend_from_slice(buf);
    ///         Ok(buf.len())
    ///     }
    ///     fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    /// }
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    /// let ctx = Context::new_standard()?;
    /// let port = ctx.open_output_port(SharedWriter(buf.clone()))?;
    /// ctx.set_current_output_port(&port)?;
    /// ctx.evaluate("(display \"hello\")")?;
    /// ctx.evaluate("(flush-output (current-output-port))")?;
    /// assert_eq!(&*buf.lock().unwrap(), b"hello");
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_current_output_port(&self, port: &Value) -> Result<()> {
        self.set_port_parameter(port, ffi::sexp_global_cur_out_symbol)
    }

    /// Set the current input port for this context.
    ///
    /// Replaces the port that `(current-input-port)` returns in Scheme code.
    ///
    /// # Examples
    ///
    /// ```
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new_standard()?;
    /// let port = ctx.open_input_port(std::io::Cursor::new(b"42"))?;
    /// ctx.set_current_input_port(&port)?;
    /// let val = ctx.evaluate("(read)")?;
    /// assert_eq!(val, Value::Integer(42));
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_current_input_port(&self, port: &Value) -> Result<()> {
        self.set_port_parameter(port, ffi::sexp_global_cur_in_symbol)
    }

    /// Set the current error port for this context.
    ///
    /// Replaces the port that `(current-error-port)` returns in Scheme code.
    ///
    /// # Examples
    ///
    /// ```
    /// use tein::{Context, Value};
    /// use std::sync::{Arc, Mutex};
    ///
    /// struct SharedWriter(Arc<Mutex<Vec<u8>>>);
    /// impl std::io::Write for SharedWriter {
    ///     fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
    ///         self.0.lock().unwrap().extend_from_slice(buf);
    ///         Ok(buf.len())
    ///     }
    ///     fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    /// }
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    /// let ctx = Context::new_standard()?;
    /// let port = ctx.open_output_port(SharedWriter(buf.clone()))?;
    /// ctx.set_current_error_port(&port)?;
    /// ctx.evaluate("(display \"oops\" (current-error-port))")?;
    /// ctx.evaluate("(flush-output (current-error-port))")?;
    /// assert_eq!(&*buf.lock().unwrap(), b"oops");
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_current_error_port(&self, port: &Value) -> Result<()> {
        self.set_port_parameter(port, ffi::sexp_global_cur_err_symbol)
    }
```

**Step 2: run the test**

```bash
cargo test -p tein test_set_current_output_port -- --nocapture
```

expected: PASS.

**Step 3: add remaining tests**

add after `test_set_current_output_port`:

```rust
#[test]
fn test_set_current_input_port() {
    let ctx = Context::new_standard().expect("context");
    let port = ctx
        .open_input_port(std::io::Cursor::new(b"42"))
        .expect("open port");
    ctx.set_current_input_port(&port).expect("set port");
    let val = ctx.evaluate("(read)").expect("read");
    assert_eq!(val, Value::Integer(42));
}

#[test]
fn test_set_current_error_port() {
    use std::sync::{Arc, Mutex};

    struct SharedWriter(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let ctx = Context::new_standard().expect("context");
    let port = ctx
        .open_output_port(SharedWriter(buf.clone()))
        .expect("open port");
    ctx.set_current_error_port(&port).expect("set port");
    ctx.evaluate("(display \"oops\" (current-error-port))")
        .expect("display");
    ctx.evaluate("(flush-output (current-error-port))")
        .expect("flush");
    let output = buf.lock().unwrap();
    assert_eq!(&*output, b"oops");
}

#[test]
fn test_set_port_rejects_non_port() {
    let ctx = Context::new_standard().expect("context");
    let err = ctx
        .set_current_output_port(&Value::Integer(42))
        .unwrap_err();
    assert!(
        matches!(err, Error::TypeError(_)),
        "expected TypeError, got {:?}",
        err
    );
}
```

**Step 4: run all port tests**

```bash
cargo test -p tein test_set_current -- --nocapture
```

expected: all 4 tests PASS.

**Step 5: commit**

```bash
git add tein/src/context.rs
git commit -m "feat(context): add set_current_{output,input,error}_port API

exposes chibi's sexp_set_parameter to let embedders redirect the
standard port parameters. any custom port from open_output_port /
open_input_port can be installed as the default."
```

---

### Task 4: REPL TrackingWriter + eval loop fix

**Files:**
- Modify: `tein-bin/src/main.rs`

**Step 1: write TrackingWriter unit tests**

add to the `#[cfg(test)] mod tests` section in `tein-bin/src/main.rs`:

```rust
#[test]
fn tracking_writer_tracks_newline() {
    use std::io::Write;

    let tracker = Rc::new(TrackingWriter::new());
    let mut writer = SharedTrackingWriter(tracker.clone());

    assert!(tracker.last_was_newline());

    writer.write_all(b"hello").unwrap();
    assert!(!tracker.last_was_newline());

    writer.write_all(b"\n").unwrap();
    assert!(tracker.last_was_newline());

    writer.write_all(b"world\n").unwrap();
    assert!(tracker.last_was_newline());

    writer.write_all(b"no newline").unwrap();
    assert!(!tracker.last_was_newline());
}

#[test]
fn tracking_writer_empty_write() {
    use std::io::Write;

    let tracker = Rc::new(TrackingWriter::new());
    let mut writer = SharedTrackingWriter(tracker.clone());

    // initial state: true (as if at start of line)
    assert!(tracker.last_was_newline());

    // empty write shouldn't change state
    writer.write_all(b"").unwrap();
    assert!(tracker.last_was_newline());

    writer.write_all(b"x").unwrap();
    assert!(!tracker.last_was_newline());

    // empty write after non-newline shouldn't change state
    writer.write_all(b"").unwrap();
    assert!(!tracker.last_was_newline());
}
```

**Step 2: run tests to verify they fail**

```bash
cargo test -p tein-bin tracking_writer
```

expected: FAIL — `TrackingWriter` doesn't exist.

**Step 3: implement TrackingWriter + SharedTrackingWriter**

add before `run_repl` in `tein-bin/src/main.rs`:

```rust
use std::cell::Cell;
use std::rc::Rc;

/// tracks whether the last byte written to stdout was a newline.
///
/// used by the REPL to conditionally emit `\n` after eval — avoids
/// spurious blank lines when scheme output already ends with `\n`.
/// flushes stdout on every write for immediate streaming output.
struct TrackingWriter {
    last_newline: Cell<bool>,
}

impl TrackingWriter {
    fn new() -> Self {
        Self {
            last_newline: Cell::new(true),
        }
    }

    fn last_was_newline(&self) -> bool {
        self.last_newline.get()
    }
}

/// shared wrapper that delegates `Write` to stdout while updating
/// the `TrackingWriter` state. uses `Rc` because `Context` is `!Send`.
struct SharedTrackingWriter(Rc<TrackingWriter>);

impl std::io::Write for SharedTrackingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = std::io::stdout().write(buf)?;
        if n > 0 {
            self.0.last_newline.set(buf[n - 1] == b'\n');
        }
        std::io::stdout().flush()?;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stdout().flush()
    }
}
```

**Step 4: run TrackingWriter tests**

```bash
cargo test -p tein-bin tracking_writer
```

expected: PASS.

**Step 5: update `run_repl` to use TrackingWriter**

replace the eval section in `run_repl` (~lines 266-288). the new flow:

1. after building the context (~line 208), add port setup:

```rust
    let tracker = Rc::new(TrackingWriter::new());
    let port = ctx
        .open_output_port(SharedTrackingWriter(tracker.clone()))
        .expect("failed to create tracking output port");
    ctx.set_current_output_port(&port)
        .expect("failed to set output port");
```

2. replace the eval block (~lines 266-288) with:

```rust
                    if !input.is_empty() {
                        let _ = rl.add_history_entry(&input);
                        match ctx.evaluate(&input) {
                            Ok(tein::Value::Unspecified) => {}
                            Ok(tein::Value::Exit(n)) => {
                                if let Some(path) = history_path() {
                                    let _ = rl.save_history(&path);
                                }
                                std::process::exit(n);
                            }
                            Ok(value) => {
                                if !tracker.last_was_newline() {
                                    println!();
                                }
                                println!("{}", value);
                            }
                            Err(e) => {
                                if !tracker.last_was_newline() {
                                    println!();
                                }
                                eprintln!("error: {}", e);
                            }
                        }
                    }
```

this removes:
- the `flush-output` wrapping format string
- the unconditional `println!()` on line 287

**Step 6: run all tein-bin tests**

```bash
cargo test -p tein-bin
```

expected: all tests PASS.

**Step 7: manual smoke test**

```bash
cargo run -p tein-bin
```

verify:
- `(display "hello")` → `hello` with no blank line after
- `(display "hello\n")` → `hello\n` with no blank line after
- `'meow` → `meow` (return value on its own line, no extra blank)
- `(+ 1 2)` → `3` (no extra blank)
- `(display "hi") 42` → `hi` then `42` on new line (no extra blank)

**Step 8: commit**

```bash
git add tein-bin/src/main.rs
git commit -m "fix(repl): eliminate blank lines + enable streaming flush

install a TrackingWriter-backed custom port as current-output-port.
the writer flushes stdout eagerly (fixing mid-eval buffering) and
tracks whether the last byte was a newline (fixing spurious blanks).

closes #121, closes #120"
```

---

### Task 5: docs updates

**Files:**
- Modify: `AGENTS.md` — add `set_current_*_port` to custom port flow description
- Modify: `docs/embedding.md` — add "redirecting standard ports" subsection after "output ports"
- Modify: `docs/reference.md` — add three methods

**Step 1: update AGENTS.md**

in the custom port flow paragraph, after the sentence ending "...`ctx.evaluate_port(&port)` → loops read+eval.", add:

```
`ctx.set_current_output_port(&port)` / `set_current_input_port` / `set_current_error_port` replace the default port parameter so all subsequent scheme IO goes through the custom port. uses `sexp_set_parameter` under the hood.
```

**Step 2: update docs/embedding.md**

after the output ports section (~line 280), add a new subsection:

```markdown
### redirecting standard ports

By default, `(current-output-port)` writes to C stdout. You can redirect it to a custom port:

\```rust
let ctx = Context::new_standard()?;
let port = ctx.open_output_port(my_writer)?;
ctx.set_current_output_port(&port)?;

// all scheme output now goes through my_writer
ctx.evaluate("(display \"hello\")")?;
\```

The same works for input and error ports via `set_current_input_port` and `set_current_error_port`.
```

**Step 3: update docs/reference.md**

add the three methods to the Context methods table.

**Step 4: commit**

```bash
git add AGENTS.md docs/embedding.md docs/reference.md
git commit -m "docs: add set_current_*_port to embedding guide + reference"
```

---

### Task 6: lint + final verification

**Step 1: lint**

```bash
just lint
```

fix any issues.

**Step 2: full test suite**

```bash
just test
```

expected: all tests pass.

**Step 3: commit any lint fixes**

```bash
git add -u && git commit -m "chore: lint fixes"
```

(skip if nothing to fix)

---

### AGENTS.md notes to collect

- `sexp_set_parameter` exposed via `tein_sexp_set_parameter` shim + symbol getters
- `set_current_output_port` / `set_current_input_port` / `set_current_error_port` on `Context`
- no GC rooting needed for `sexp_set_parameter` — it mutates existing opcode data
- `TrackingWriter` is a REPL concern in `tein-bin`, not in the library
