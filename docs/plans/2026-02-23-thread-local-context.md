# ThreadLocalContext implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** add `ThreadLocalContext` — a `Send + Sync` managed context on a dedicated thread with persistent/fresh modes, init closures, and reset support.

**Architecture:** dedicated thread per logical context (channel-based), generalizing the `TimeoutContext` pattern. shared protocol types extracted into `thread.rs`. `ContextBuilder` gains `Clone` and two new builder methods. see `docs/plans/2026-02-23-thread-local-context-design.md` for full design.

**Tech Stack:** rust std (`mpsc`, `thread`, `Mutex`), existing tein crate internals

---

### Task 1: derive Clone on ContextBuilder

**Files:**
- Modify: `tein/src/context.rs:393` (the struct definition)

**Step 1: add Clone derive**

add `#[derive(Clone)]` to `ContextBuilder`:

```rust
#[derive(Clone)]
pub struct ContextBuilder {
    heap_size: usize,
    heap_max: usize,
    step_limit: Option<u64>,
    standard_env: bool,
    allowed_primitives: Option<Vec<&'static str>>,
    file_read_prefixes: Option<Vec<String>>,
    file_write_prefixes: Option<Vec<String>>,
}
```

**Step 2: verify it compiles**

Run: `cargo build -p tein`
Expected: compiles cleanly (all fields are already Clone)

**Step 3: commit**

```
feat: derive Clone on ContextBuilder
```

---

### Task 2: extract shared thread protocol into thread.rs

**Files:**
- Create: `tein/src/thread.rs`
- Modify: `tein/src/timeout.rs` (remove duplicated types, import from thread)
- Modify: `tein/src/lib.rs` (add `mod thread`)

**Step 1: write the test — verify TimeoutContext still works after refactor**

no new test file needed. existing `test_timeout_basic` et al in `tein/src/context.rs` serve as the regression suite.

Run: `cargo test -p tein test_timeout`
Expected: all 6 timeout tests pass (baseline)

**Step 2: create `tein/src/thread.rs` with shared protocol types**

extract from `timeout.rs`: `Request`, `Response`, `SendableValue`, their `Send` impls, and a `ForeignFnPtr` type alias. add `Reset` variant to `Request` and `Reset` variant to `Response` (needed by task 4).

```rust
//! shared protocol types for thread-based context wrappers
//!
//! both [`TimeoutContext`](crate::TimeoutContext) and
//! [`ThreadLocalContext`](crate::managed::ThreadLocalContext) run a
//! [`Context`](crate::Context) on a dedicated thread and communicate
//! via channels. this module contains the shared request/response
//! protocol and the `SendableValue` wrapper.

use crate::Value;
use crate::error::Result;

/// function pointer type for variadic foreign functions
pub(crate) type ForeignFnPtr = unsafe extern "C" fn(
    crate::ffi::sexp,
    crate::ffi::sexp,
    crate::ffi::sexp_sint_t,
    crate::ffi::sexp,
) -> crate::ffi::sexp;

/// request sent to a context thread
pub(crate) enum Request {
    /// evaluate a string of scheme code
    Evaluate(String),
    /// call a procedure with arguments
    Call(SendableValue, Vec<SendableValue>),
    /// register a variadic foreign function
    DefineFnVariadic {
        /// scheme name for the function
        name: String,
        /// the raw function pointer
        f: ForeignFnPtr,
    },
    /// rebuild the context from the stored builder + init closure
    Reset,
    /// shut down the context thread
    Shutdown,
}

// SAFETY: Request contains SendableValue which wraps Value (may hold raw
// sexp pointers). safe because values only travel to the context thread
// where the context that created them lives.
unsafe impl Send for Request {}

/// response from a context thread
pub(crate) enum Response {
    /// result of evaluate or call
    Value(Result<Value>),
    /// result of define_fn_variadic
    Defined(Result<()>),
    /// result of reset
    Reset(Result<()>),
}

// SAFETY: Response contains Result<Value> which may hold Value::Procedure
// (a raw *mut c_void). safe because values only travel between the caller
// and the single context thread — Procedure pointers are only
// dereferenced on the context thread where the context lives.
unsafe impl Send for Response {}

/// wrapper allowing a Value to be sent across threads
///
/// # safety
/// safe because values are only ever sent *back* to the thread that owns
/// the context. Procedure values contain raw sexp pointers that are only
/// valid on the context thread — this wrapper ensures they travel back
/// to where they came from.
pub(crate) struct SendableValue(pub(crate) Value);

// SAFETY: see struct-level doc. values only travel between the caller
// and the single context thread, and Procedure pointers are only
// dereferenced on the context thread.
unsafe impl Send for SendableValue {}
```

**Step 3: update `tein/src/lib.rs` — add `mod thread`**

add `mod thread;` after `mod timeout;` (line 27):

```rust
mod thread;
```

**Step 4: refactor `tein/src/timeout.rs` to use shared types**

replace the local `Request`, `Response`, `SendableValue` definitions and their `Send` impls with imports from `crate::thread`. the `TimeoutContext` struct, its `impl` blocks, `Debug`, and `Drop` stay unchanged except for import paths.

the new top of `timeout.rs`:

```rust
//! wall-clock timeout wrapper for scheme contexts
//!
//! [`TimeoutContext`] runs a [`Context`] on a dedicated thread and enforces
//! wall-clock deadlines on evaluation calls. the underlying context never
//! crosses thread boundaries (satisfying !Send).
//!
//! requires `step_limit` to be set on the builder so the context thread
//! is guaranteed to eventually terminate after a timeout fires.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::Value;
use crate::context::ContextBuilder;
use crate::error::{Error, Result};
use crate::thread::{ForeignFnPtr, Request, Response, SendableValue};
```

then remove lines 18–71 (the old type definitions) and keep everything from `pub struct TimeoutContext` onwards, updating any references as needed (e.g. the `DefineFnVariadic` field `f` type becomes `ForeignFnPtr`).

**Step 5: verify all timeout tests still pass**

Run: `cargo test -p tein test_timeout`
Expected: all 6 pass, no regressions

**Step 6: commit**

```
refactor: extract shared thread protocol into thread.rs
```

---

### Task 3: write ThreadLocalContext tests (persistent mode)

**Files:**
- Modify: `tein/src/context.rs` (add tests at end of `mod tests`)

**Step 1: write persistent mode tests**

add to the bottom of `mod tests` in `tein/src/context.rs`:

```rust
// --- managed context (persistent mode) ---

#[test]
fn test_managed_persistent_evaluate() {
    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed(|_ctx| Ok(()))
        .unwrap();
    let result = ctx.evaluate("(+ 1 2 3)").unwrap();
    assert_eq!(result, Value::Integer(6));
}

#[test]
fn test_managed_persistent_state_accumulates() {
    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed(|_ctx| Ok(()))
        .unwrap();
    ctx.evaluate("(define x 42)").unwrap();
    let result = ctx.evaluate("x").unwrap();
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn test_managed_persistent_init_closure() {
    let ctx = Context::builder()
        .standard_env()
        .step_limit(1_000_000)
        .build_managed(|ctx| {
            ctx.evaluate("(define greeting \"hello from init\")")?;
            Ok(())
        })
        .unwrap();
    let result = ctx.evaluate("greeting").unwrap();
    assert_eq!(result, Value::String("hello from init".to_string()));
}

#[test]
fn test_managed_persistent_call() {
    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed(|_ctx| Ok(()))
        .unwrap();
    let proc = ctx.evaluate("+").unwrap();
    let result = ctx.call(&proc, &[Value::Integer(10), Value::Integer(20)]).unwrap();
    assert_eq!(result, Value::Integer(30));
}

#[test]
fn test_managed_persistent_define_fn_variadic() {
    use crate::raw;

    unsafe extern "C" fn always_42(
        ctx: raw::sexp, _self: raw::sexp,
        _n: raw::sexp_sint_t, _args: raw::sexp,
    ) -> raw::sexp {
        unsafe { raw::sexp_make_fixnum(42) }
    }

    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed(|_ctx| Ok(()))
        .unwrap();
    ctx.define_fn_variadic("always-42", always_42).unwrap();
    let result = ctx.evaluate("(always-42)").unwrap();
    assert_eq!(result, Value::Integer(42));
}
```

**Step 2: run tests — verify they fail (type doesn't exist yet)**

Run: `cargo test -p tein test_managed_persistent -- --no-run 2>&1 | head -5`
Expected: compile error — `build_managed` doesn't exist

**Step 3: commit**

```
test: add persistent mode tests for ThreadLocalContext (red)
```

---

### Task 4: write ThreadLocalContext tests (fresh mode + reset)

**Files:**
- Modify: `tein/src/context.rs` (add tests at end of `mod tests`)

**Step 1: write fresh mode and reset tests**

```rust
// --- managed context (fresh mode) ---

#[test]
fn test_managed_fresh_evaluate() {
    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed_fresh(|_ctx| Ok(()))
        .unwrap();
    let result = ctx.evaluate("(+ 10 20)").unwrap();
    assert_eq!(result, Value::Integer(30));
}

#[test]
fn test_managed_fresh_state_does_not_persist() {
    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed_fresh(|_ctx| Ok(()))
        .unwrap();
    ctx.evaluate("(define x 42)").unwrap();
    // fresh mode rebuilds context, so x should not exist
    let result = ctx.evaluate("x");
    assert!(result.is_err());
}

#[test]
fn test_managed_fresh_init_closure_runs_each_time() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = counter.clone();

    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed_fresh(move |_ctx| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();

    ctx.evaluate("(+ 1 1)").unwrap();
    ctx.evaluate("(+ 2 2)").unwrap();
    ctx.evaluate("(+ 3 3)").unwrap();

    // init ran once during build + once per evaluate = 4
    assert_eq!(counter.load(Ordering::SeqCst), 4);
}

// --- managed context (reset) ---

#[test]
fn test_managed_persistent_reset_clears_state() {
    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed(|_ctx| Ok(()))
        .unwrap();
    ctx.evaluate("(define x 99)").unwrap();
    assert_eq!(ctx.evaluate("x").unwrap(), Value::Integer(99));

    ctx.reset().unwrap();

    // after reset, x should not exist
    let result = ctx.evaluate("x");
    assert!(result.is_err());
}

#[test]
fn test_managed_persistent_reset_reruns_init() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = counter.clone();

    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed(move |_ctx| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();

    // init ran once during build
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    ctx.reset().unwrap();

    // init ran again during reset
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[test]
fn test_managed_fresh_reset_is_noop() {
    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed_fresh(|_ctx| Ok(()))
        .unwrap();
    // should not error
    ctx.reset().unwrap();
}

// --- managed context (error handling) ---

#[test]
fn test_managed_init_failure_returns_error() {
    let result = Context::builder()
        .step_limit(1_000_000)
        .build_managed(|_ctx| {
            Err(Error::InitError("intentional init failure".to_string()))
        });
    assert!(result.is_err());
    match result.unwrap_err() {
        Error::InitError(msg) => assert!(msg.contains("intentional init failure")),
        other => panic!("expected InitError, got {:?}", other),
    }
}

#[test]
fn test_managed_mode() {
    use crate::managed::Mode;

    let persistent = Context::builder()
        .step_limit(1_000_000)
        .build_managed(|_ctx| Ok(()))
        .unwrap();
    assert_eq!(persistent.mode(), Mode::Persistent);

    let fresh = Context::builder()
        .step_limit(1_000_000)
        .build_managed_fresh(|_ctx| Ok(()))
        .unwrap();
    assert_eq!(fresh.mode(), Mode::Fresh);
}

#[test]
fn test_managed_drop_cleans_up() {
    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed(|_ctx| Ok(()))
        .unwrap();
    drop(ctx);
    // no panic, no leaked thread — success
}
```

**Step 2: run tests — verify they fail (type doesn't exist yet)**

Run: `cargo test -p tein test_managed -- --no-run 2>&1 | head -5`
Expected: compile error

**Step 3: commit**

```
test: add fresh mode, reset, and error tests for ThreadLocalContext (red)
```

---

### Task 5: implement ThreadLocalContext in managed.rs

**Files:**
- Create: `tein/src/managed.rs`
- Modify: `tein/src/lib.rs` (add `pub mod managed`, re-export types)
- Modify: `tein/src/context.rs` (add `build_managed` and `build_managed_fresh` to `ContextBuilder`)

**Step 1: create `tein/src/managed.rs`**

```rust
//! managed context on a dedicated thread
//!
//! [`ThreadLocalContext`] runs a [`Context`] on a dedicated thread and
//! proxies evaluation requests over channels. supports persistent mode
//! (state accumulates) and fresh mode (context rebuilt each call).
//! the type is `Send + Sync`, safe to share across threads via `Arc`.

use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;

use crate::Value;
use crate::context::ContextBuilder;
use crate::error::{Error, Result};
use crate::thread::{ForeignFnPtr, Request, Response, SendableValue};

/// operating mode for a [`ThreadLocalContext`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// context persists across evaluations, accumulating state.
    /// `reset()` tears down and rebuilds from the init closure.
    Persistent,
    /// context is rebuilt from the init closure before every evaluation.
    /// `reset()` is a no-op.
    Fresh,
}

/// a managed scheme context on a dedicated thread
///
/// wraps a [`Context`](crate::Context) running on a dedicated thread.
/// requests are proxied over channels, making this type `Send + Sync`
/// and safe to share across threads via `Arc`.
///
/// # modes
///
/// - **persistent**: context lives across evaluations. state accumulates.
///   `reset()` tears down and rebuilds from the init closure.
/// - **fresh**: context is rebuilt before every evaluation. no state leakage.
///   `reset()` is a no-op.
///
/// # examples
///
/// ```
/// use tein::{Context, Value};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let ctx = Context::builder()
///     .step_limit(1_000_000)
///     .build_managed(|ctx| {
///         ctx.evaluate("(define x 42)")?;
///         Ok(())
///     })?;
///
/// let result = ctx.evaluate("(+ x 1)")?;
/// assert_eq!(result, Value::Integer(43));
/// # Ok(())
/// # }
/// ```
pub struct ThreadLocalContext {
    tx: mpsc::Sender<Request>,
    rx: Mutex<mpsc::Receiver<Response>>,
    mode: Mode,
    handle: Option<thread::JoinHandle<()>>,
}

// SAFETY: the inner Context never leaves its dedicated thread.
// tx is Send, rx is wrapped in Mutex (Sync). all values crossing
// the channel boundary use SendableValue with the same safety
// argument as TimeoutContext.
unsafe impl Send for ThreadLocalContext {}
unsafe impl Sync for ThreadLocalContext {}

impl ThreadLocalContext {
    /// create a managed context on a dedicated thread
    ///
    /// the init closure runs once after context creation. in fresh mode,
    /// it also runs before every evaluation and on reset.
    pub(crate) fn new(
        builder: ContextBuilder,
        mode: Mode,
        init: impl Fn(&crate::Context) -> Result<()> + Send + 'static,
    ) -> Result<Self> {
        let (req_tx, req_rx) = mpsc::channel::<Request>();
        let (resp_tx, resp_rx) = mpsc::channel::<Response>();

        let handle = thread::spawn(move || {
            // build and init the context on this thread
            let mut ctx = match Self::build_and_init(&builder, &init) {
                Ok(ctx) => ctx,
                Err(e) => {
                    let _ = resp_tx.send(Response::Value(Err(e)));
                    return;
                }
            };

            // signal successful init
            let _ = resp_tx.send(Response::Value(Ok(Value::Unspecified)));

            // message loop
            for req in req_rx {
                match req {
                    Request::Evaluate(code) => {
                        if mode == Mode::Fresh {
                            match Self::build_and_init(&builder, &init) {
                                Ok(new_ctx) => ctx = new_ctx,
                                Err(e) => {
                                    if resp_tx.send(Response::Value(Err(e))).is_err() {
                                        break;
                                    }
                                    continue;
                                }
                            }
                        }
                        let result = ctx.evaluate(&code);
                        if resp_tx.send(Response::Value(result)).is_err() {
                            break;
                        }
                    }
                    Request::Call(proc, args) => {
                        if mode == Mode::Fresh {
                            match Self::build_and_init(&builder, &init) {
                                Ok(new_ctx) => ctx = new_ctx,
                                Err(e) => {
                                    if resp_tx.send(Response::Value(Err(e))).is_err() {
                                        break;
                                    }
                                    continue;
                                }
                            }
                        }
                        let args: Vec<Value> = args.into_iter().map(|s| s.0).collect();
                        let result = ctx.call(&proc.0, &args);
                        if resp_tx.send(Response::Value(result)).is_err() {
                            break;
                        }
                    }
                    Request::DefineFnVariadic { name, f } => {
                        let result = ctx.define_fn_variadic(&name, f);
                        if resp_tx.send(Response::Defined(result)).is_err() {
                            break;
                        }
                    }
                    Request::Reset => {
                        if mode == Mode::Fresh {
                            // fresh mode already rebuilds each call — reset is a no-op
                            if resp_tx.send(Response::Reset(Ok(()))).is_err() {
                                break;
                            }
                        } else {
                            match Self::build_and_init(&builder, &init) {
                                Ok(new_ctx) => {
                                    ctx = new_ctx;
                                    if resp_tx.send(Response::Reset(Ok(()))).is_err() {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    if resp_tx.send(Response::Reset(Err(e))).is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Request::Shutdown => break,
                }
            }
        });

        // wait for init response
        let init_resp = resp_rx
            .recv()
            .map_err(|_| Error::InitError("context thread died during init".to_string()))?;

        match init_resp {
            Response::Value(Err(e)) => return Err(e),
            Response::Value(Ok(_)) => {}
            _ => {
                return Err(Error::InitError(
                    "unexpected init response from context thread".to_string(),
                ));
            }
        }

        Ok(ThreadLocalContext {
            tx: req_tx,
            rx: Mutex::new(resp_rx),
            mode,
            handle: Some(handle),
        })
    }

    /// build a context from a builder clone and run the init closure
    fn build_and_init(
        builder: &ContextBuilder,
        init: &(impl Fn(&crate::Context) -> Result<()> + Send + 'static),
    ) -> Result<crate::Context> {
        let ctx = builder.clone().build()?;
        init(&ctx)?;
        Ok(ctx)
    }

    /// evaluate scheme code
    pub fn evaluate(&self, code: &str) -> Result<Value> {
        self.tx
            .send(Request::Evaluate(code.to_string()))
            .map_err(|_| Error::InitError("context thread is dead".to_string()))?;

        let rx = self.rx.lock().unwrap();
        match rx.recv() {
            Ok(Response::Value(result)) => result,
            Ok(_) => Err(Error::InitError("unexpected response type".to_string())),
            Err(_) => Err(Error::InitError("context thread died".to_string())),
        }
    }

    /// call a scheme procedure with arguments
    pub fn call(&self, proc: &Value, args: &[Value]) -> Result<Value> {
        self.tx
            .send(Request::Call(
                SendableValue(proc.clone()),
                args.iter().map(|a| SendableValue(a.clone())).collect(),
            ))
            .map_err(|_| Error::InitError("context thread is dead".to_string()))?;

        let rx = self.rx.lock().unwrap();
        match rx.recv() {
            Ok(Response::Value(result)) => result,
            Ok(_) => Err(Error::InitError("unexpected response type".to_string())),
            Err(_) => Err(Error::InitError("context thread died".to_string())),
        }
    }

    /// register a variadic foreign function
    pub fn define_fn_variadic(
        &self,
        name: &str,
        f: ForeignFnPtr,
    ) -> Result<()> {
        self.tx
            .send(Request::DefineFnVariadic {
                name: name.to_string(),
                f,
            })
            .map_err(|_| Error::InitError("context thread is dead".to_string()))?;

        let rx = self.rx.lock().unwrap();
        match rx.recv() {
            Ok(Response::Defined(result)) => result,
            Ok(_) => Err(Error::InitError("unexpected response type".to_string())),
            Err(_) => Err(Error::InitError("context thread died".to_string())),
        }
    }

    /// rebuild the context from the init closure
    ///
    /// in persistent mode, tears down the current context and rebuilds
    /// from the stored builder config + init closure. live foreign objects
    /// are dropped. in fresh mode, this is a no-op (already rebuilds each call).
    pub fn reset(&self) -> Result<()> {
        self.tx
            .send(Request::Reset)
            .map_err(|_| Error::InitError("context thread is dead".to_string()))?;

        let rx = self.rx.lock().unwrap();
        match rx.recv() {
            Ok(Response::Reset(result)) => result,
            Ok(_) => Err(Error::InitError("unexpected response type".to_string())),
            Err(_) => Err(Error::InitError("context thread died".to_string())),
        }
    }

    /// which mode this context is running in
    pub fn mode(&self) -> Mode {
        self.mode
    }
}

impl std::fmt::Debug for ThreadLocalContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadLocalContext")
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

impl Drop for ThreadLocalContext {
    fn drop(&mut self) {
        let _ = self.tx.send(Request::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
```

**Step 2: add `mod managed` and re-exports to `tein/src/lib.rs`**

add after `mod thread;`:

```rust
pub mod managed;
```

and add to the re-exports:

```rust
pub use managed::{Mode, ThreadLocalContext};
```

**Step 3: add `build_managed` and `build_managed_fresh` to ContextBuilder**

add to `impl ContextBuilder` in `tein/src/context.rs`, after `build()`:

```rust
    /// build a managed context on a dedicated thread (persistent mode)
    ///
    /// the init closure runs once after context creation. state accumulates
    /// across evaluations. use `reset()` to tear down and rebuild.
    ///
    /// requires `step_limit` to be set (ensures the context thread can
    /// be bounded by fuel if needed).
    pub fn build_managed(
        self,
        init: impl Fn(&Context) -> Result<()> + Send + 'static,
    ) -> Result<crate::managed::ThreadLocalContext> {
        crate::managed::ThreadLocalContext::new(self, crate::managed::Mode::Persistent, init)
    }

    /// build a managed context on a dedicated thread (fresh mode)
    ///
    /// the init closure runs before every evaluation — context is rebuilt
    /// each time. no state persists between calls. `reset()` is a no-op.
    ///
    /// requires `step_limit` to be set (ensures the context thread can
    /// be bounded by fuel if needed).
    pub fn build_managed_fresh(
        self,
        init: impl Fn(&Context) -> Result<()> + Send + 'static,
    ) -> Result<crate::managed::ThreadLocalContext> {
        crate::managed::ThreadLocalContext::new(self, crate::managed::Mode::Fresh, init)
    }
```

**Step 4: run all tests**

Run: `cargo test -p tein`
Expected: all tests pass — existing tests green, new managed tests green

**Step 5: run clippy**

Run: `cargo clippy -p tein`
Expected: no warnings

**Step 6: commit**

```
feat: add ThreadLocalContext with persistent/fresh modes and reset
```

---

### Task 6: update documentation

**Files:**
- Modify: `AGENTS.md` (architecture section, lib.rs re-exports, data flow)
- Modify: `DEVELOPMENT.md` (add managed context section)
- Modify: `TODO.md` (mark context pooling/thread-local item done, add pool follow-on)

**Step 1: update `AGENTS.md`**

- add `managed.rs` and `thread.rs` to the architecture tree
- add `ThreadLocalContext`, `Mode` to the `lib.rs` re-exports list
- add "managed context flow" paragraph to data flow section

**Step 2: update `DEVELOPMENT.md`**

- add `managed.rs` and `thread.rs` to directory structure
- add "managed context" subsection under architecture
- update test count

**Step 3: update `TODO.md`**

- add `[x] **thread-local / managed contexts**` item under ideas or as a new milestone entry
- note pool as future follow-on

**Step 4: verify build + tests still pass**

Run: `cargo test -p tein && cargo clippy -p tein`
Expected: all green

**Step 5: commit**

```
docs: add ThreadLocalContext to architecture docs and roadmap
```

---

### Task 7: add managed context example

**Files:**
- Create: `tein/examples/managed.rs`

**Step 1: write the example**

```rust
//! managed context example — persistent and fresh modes
//!
//! demonstrates ThreadLocalContext with init closures, state
//! accumulation (persistent mode), and clean rebuilds (fresh mode).

use tein::{Context, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- persistent mode ---
    println!("=== persistent mode ===");

    let ctx = Context::builder()
        .standard_env()
        .step_limit(1_000_000)
        .build_managed(|ctx| {
            ctx.evaluate("(define counter 0)")?;
            Ok(())
        })?;

    // state accumulates across calls
    ctx.evaluate("(set! counter (+ counter 1))")?;
    ctx.evaluate("(set! counter (+ counter 1))")?;
    let result = ctx.evaluate("counter")?;
    println!("counter after 2 increments: {}", result);

    // reset clears state and re-runs init
    ctx.reset()?;
    let result = ctx.evaluate("counter")?;
    println!("counter after reset: {}", result);

    // --- fresh mode ---
    println!("\n=== fresh mode ===");

    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed_fresh(|_ctx| {
            println!("  (init closure running)");
            Ok(())
        })?;

    // each evaluate gets a fresh context
    let r1 = ctx.evaluate("(+ 1 2)")?;
    let r2 = ctx.evaluate("(* 3 4)")?;
    println!("1 + 2 = {}", r1);
    println!("3 * 4 = {}", r2);

    println!("\ndone!");
    Ok(())
}
```

**Step 2: run the example**

Run: `cargo run -p tein --example managed`
Expected: output showing counter increments, reset, and fresh mode init closure running multiple times

**Step 3: commit**

```
example: add managed context demo (persistent + fresh modes)
```
