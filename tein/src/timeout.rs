//! Wall-clock timeout wrapper for Scheme contexts.
//!
//! [`TimeoutContext`] runs a [`crate::Context`] on a dedicated thread and enforces
//! wall-clock deadlines on evaluation calls. The underlying context never
//! crosses thread boundaries (satisfying `!Send`).
//!
//! Requires `step_limit` to be set on the builder so the context thread
//! is guaranteed to eventually terminate after a timeout fires.
//!
//! # When to use
//!
//! - **`TimeoutContext`** — single-owner, wall-clock deadlines, state persists
//! - **[`crate::managed::ThreadLocalContext`]** — `Send + Sync`, persistent or
//!   fresh modes, no built-in timeout
//! - **[`crate::Context`]** — single-threaded, no timeout, maximum control
//!
//! # examples
//!
//! ```
//! use tein::{Context, Value, Error};
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ctx = Context::builder()
//!     .step_limit(1_000_000)
//!     .build_timeout(Duration::from_secs(5))?;
//!
//! let result = ctx.evaluate("(+ 1 2 3)")?;
//! assert_eq!(result, Value::Integer(6));
//!
//! // state persists between evaluations
//! ctx.evaluate("(define x 42)")?;
//! assert_eq!(ctx.evaluate("x")?, Value::Integer(42));
//! # Ok(())
//! # }
//! ```

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::Value;
use crate::context::ContextBuilder;
use crate::error::{Error, Result};
use crate::thread::{ForeignFnPtr, Request, Response, SendableValue};

/// A Scheme context with wall-clock timeout enforcement.
///
/// Wraps a [`crate::Context`] running on a dedicated thread. Each evaluation
/// call has a wall-clock deadline; if exceeded, `Error::Timeout` is returned.
///
/// The context thread is bounded by the step limit — after timeout fires,
/// the thread will eventually halt when fuel runs out.
///
/// # examples
///
/// ```
/// use tein::{Context, Value};
/// use std::time::Duration;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let ctx = Context::builder()
///     .step_limit(1_000_000)
///     .build_timeout(Duration::from_secs(5))?;
///
/// let result = ctx.evaluate("(+ 1 2 3)")?;
/// assert_eq!(result, Value::Integer(6));
/// # Ok(())
/// # }
/// ```
pub struct TimeoutContext {
    tx: mpsc::Sender<Request>,
    rx: mpsc::Receiver<Response>,
    timeout: Duration,
    handle: Option<thread::JoinHandle<()>>,
}

impl ContextBuilder {
    /// Build a timeout-wrapped context on a dedicated thread.
    ///
    /// Requires `step_limit` to be set (ensures the context thread terminates
    /// after a timeout). Returns `Error::InitError` if step_limit is missing.
    pub fn build_timeout(self, timeout: Duration) -> Result<TimeoutContext> {
        if !self.has_step_limit() {
            return Err(Error::InitError(
                "step_limit is required when using build_timeout (ensures thread termination)"
                    .to_string(),
            ));
        }

        let (req_tx, req_rx) = mpsc::channel::<Request>();
        let (resp_tx, resp_rx) = mpsc::channel::<Response>();

        let handle = thread::spawn(move || {
            // context is created on this thread and never leaves
            let ctx = match self.build() {
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
                        let result = ctx.evaluate(&code);
                        if resp_tx.send(Response::Value(result)).is_err() {
                            break;
                        }
                    }
                    Request::Call(proc, args) => {
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
                    // TimeoutContext doesn't support reset — treat as shutdown
                    Request::Reset | Request::Shutdown => break,
                }
            }
        });

        // wait for init response
        let init = resp_rx
            .recv()
            .map_err(|_| Error::InitError("context thread died during init".to_string()))?;

        match init {
            Response::Value(Err(e)) => return Err(e),
            Response::Value(Ok(_)) => {}
            _ => {
                return Err(Error::InitError(
                    "unexpected init response from context thread".to_string(),
                ));
            }
        }

        Ok(TimeoutContext {
            tx: req_tx,
            rx: resp_rx,
            timeout,
            handle: Some(handle),
        })
    }
}

impl TimeoutContext {
    /// Evaluate Scheme code with wall-clock timeout.
    pub fn evaluate(&self, code: &str) -> Result<Value> {
        self.tx
            .send(Request::Evaluate(code.to_string()))
            .map_err(|_| Error::InitError("context thread is dead".to_string()))?;

        match self.rx.recv_timeout(self.timeout) {
            Ok(Response::Value(result)) => result,
            Ok(_) => Err(Error::InitError("unexpected response type".to_string())),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(Error::InitError("context thread died".to_string()))
            }
        }
    }

    /// Call a Scheme procedure with wall-clock timeout.
    pub fn call(&self, proc: &Value, args: &[Value]) -> Result<Value> {
        self.tx
            .send(Request::Call(
                SendableValue(proc.clone()),
                args.iter().map(|a| SendableValue(a.clone())).collect(),
            ))
            .map_err(|_| Error::InitError("context thread is dead".to_string()))?;

        match self.rx.recv_timeout(self.timeout) {
            Ok(Response::Value(result)) => result,
            Ok(_) => Err(Error::InitError("unexpected response type".to_string())),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(Error::InitError("context thread died".to_string()))
            }
        }
    }

    /// Register a variadic foreign function (with timeout on response).
    pub fn define_fn_variadic(&self, name: &str, f: ForeignFnPtr) -> Result<()> {
        self.tx
            .send(Request::DefineFnVariadic {
                name: name.to_string(),
                f,
            })
            .map_err(|_| Error::InitError("context thread is dead".to_string()))?;

        match self.rx.recv_timeout(self.timeout) {
            Ok(Response::Defined(result)) => result,
            Ok(_) => Err(Error::InitError("unexpected response type".to_string())),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(Error::InitError("context thread died".to_string()))
            }
        }
    }
}

impl std::fmt::Debug for TimeoutContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimeoutContext")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl Drop for TimeoutContext {
    fn drop(&mut self) {
        let _ = self.tx.send(Request::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
