//! Managed context on a dedicated thread.
//!
//! [`ThreadLocalContext`] runs a [`crate::Context`] on a dedicated thread and
//! proxies evaluation requests over channels. The type is `Send + Sync`,
//! safe to share across threads via `Arc`.
//!
//! # Modes
//!
//! | | persistent | fresh |
//! |---|---|---|
//! | **state** | accumulates across calls | rebuilt each call |
//! | **init closure** | runs once at creation | runs before every evaluation |
//! | **`reset()`** | tears down + rebuilds | no-op |
//! | **use case** | REPL, stateful scripting | deterministic evaluation |
//!
//! # When to use
//!
//! - Need `Send + Sync` for Scheme evaluation → [`ThreadLocalContext`]
//! - Need wall-clock timeouts → [`crate::TimeoutContext`]
//! - Single-threaded use → [`crate::Context`] directly
//!
//! # Example
//!
//! ```
//! use tein::{Context, Value};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // persistent: state accumulates
//! let ctx = Context::builder()
//!     .step_limit(1_000_000)
//!     .build_managed(|ctx| {
//!         ctx.evaluate("(define counter 0)")?;
//!         Ok(())
//!     })?;
//!
//! ctx.evaluate("(set! counter (+ counter 1))")?;
//! ctx.evaluate("(set! counter (+ counter 1))")?;
//! assert_eq!(ctx.evaluate("counter")?, Value::Integer(2));
//!
//! ctx.reset()?; // rebuilds, re-runs init
//! assert_eq!(ctx.evaluate("counter")?, Value::Integer(0));
//! # Ok(())
//! # }
//! ```

use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;

use crate::Value;
use crate::context::ContextBuilder;
use crate::error::{Error, Result};
use crate::thread::{ForeignFnPtr, Request, Response, SendableValue};

/// Operating mode for a [`ThreadLocalContext`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Context persists across evaluations, accumulating state.
    /// `reset()` tears down and rebuilds from the init closure.
    Persistent,
    /// Context is rebuilt from the init closure before every evaluation.
    /// `reset()` is a no-op.
    Fresh,
}

/// A managed Scheme context on a dedicated thread.
///
/// Wraps a [`Context`](crate::Context) running on a dedicated thread.
/// Requests are proxied over channels, making this type `Send + Sync`
/// and safe to share across threads via `Arc`.
///
/// # Modes
///
/// - **Persistent**: context lives across evaluations. State accumulates.
///   `reset()` tears down and rebuilds from the init closure.
/// - **Fresh**: context is rebuilt before every evaluation. No state leakage.
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
    /// Create a managed context on a dedicated thread.
    ///
    /// The init closure runs once after context creation. In fresh mode,
    /// it also runs before every evaluation.
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

    /// Build a context from a builder clone and run the init closure.
    fn build_and_init(
        builder: &ContextBuilder,
        init: &impl Fn(&crate::Context) -> Result<()>,
    ) -> Result<crate::Context> {
        let ctx = builder.clone().build()?;
        init(&ctx)?;
        Ok(ctx)
    }

    /// Evaluate Scheme code.
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

    /// Call a Scheme procedure with arguments.
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

    /// Register a variadic foreign function.
    pub fn define_fn_variadic(&self, name: &str, f: ForeignFnPtr) -> Result<()> {
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

    /// Rebuild the context from the init closure.
    ///
    /// In persistent mode, tears down the current context and rebuilds
    /// from the stored builder config + init closure. Live foreign objects
    /// are dropped. In fresh mode, this is a no-op (already rebuilds each call).
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

    /// Which mode this context is running in.
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
