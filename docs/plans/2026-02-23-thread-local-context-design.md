# ThreadLocalContext design

> managed contexts on dedicated threads — `Send + Sync`, persistent or fresh

## motivation

tein's `Context` is `!Send + !Sync` (chibi isn't thread-safe). for parallel workloads — e.g. an LLM harness dispatching concurrent tool calls — each thread needs its own context. currently users must manage this manually: spawning threads, building contexts, wiring channels, handling init failures. `ThreadLocalContext` makes the safe path the easy path.

## approach

dedicated thread per logical context (channel-based), generalizing the proven `TimeoutContext` pattern. each `ThreadLocalContext` spawns a thread, builds a `Context` on it, runs an init closure, and proxies requests over channels. the type is `Send + Sync` — safe to wrap in `Arc` and share across an async runtime.

### alternatives considered

- **thin thread-local wrapper** (`thread_local!` + `RefCell`): simpler but `Value` is `!Send`, so results can't leave the thread-local closure. awkward composition with async runtimes.
- **context pool**: bounded concurrency via N dedicated threads with checkout/return. more complex, can be trivially built on top of this design later as `Vec<ThreadLocalContext>`.

## design

### modes

- **persistent** — context lives across evaluations, accumulates state. `reset()` tears down and rebuilds from the init closure.
- **fresh** — context is rebuilt from the init closure before every evaluation. `reset()` is a no-op.

### construction

```rust
// persistent (default) — context accumulates state across calls
let ctx = Context::builder()
    .standard_env()
    .step_limit(1_000_000)
    .build_managed(|ctx| {
        ctx.register_foreign_type::<MyTool>()?;
        ctx.define_fn_variadic("fetch-url", __tein_fetch_url)?;
        Ok(())
    })?;

// fresh — rebuilt before every evaluation
let ctx = Context::builder()
    .standard_env()
    .step_limit(1_000_000)
    .build_managed_fresh(|ctx| {
        ctx.register_foreign_type::<MyTool>()?;
        Ok(())
    })?;
```

init closure signature: `Fn(&Context) -> Result<()> + Send + 'static`. `Fn` (not `FnOnce`) because fresh mode and reset both re-invoke it.

### API surface

```rust
impl ThreadLocalContext {
    pub fn evaluate(&self, code: &str) -> Result<Value>;
    pub fn call(&self, proc: &Value, args: &[Value]) -> Result<Value>;
    pub fn define_fn_variadic(&self, name: &str, f: ForeignFnPtr) -> Result<()>;
    pub fn reset(&self) -> Result<()>;
    pub fn mode(&self) -> Mode;
}

pub enum Mode { Persistent, Fresh }
```

`define_fn_variadic` called after construction does NOT persist across resets/rebuilds — the init closure is the single source of truth.

### request/response protocol

```rust
enum Request {
    Evaluate(String),
    Call(SendableValue, Vec<SendableValue>),
    DefineFnVariadic { name: String, f: ForeignFnPtr },
    Reset,
    Shutdown,
}

enum Response {
    Value(Result<Value>),
    Defined(Result<()>),
    Reset(Result<()>),
}
```

shared with `TimeoutContext` — extract into `thread.rs`.

### thread lifecycle

```
build_managed(init_fn):
  1. spawn dedicated thread
  2. build Context from stored builder config
  3. run init_fn(&ctx)
  4. signal ready (or error)
  5. enter message loop

Reset (persistent):
  1. drop current Context
  2. rebuild from cloned builder config
  3. re-run init_fn(&ctx)
  4. signal success/error

Fresh mode evaluate:
  1. drop current Context
  2. rebuild + re-run init_fn
  3. evaluate code
  4. send response
```

### error handling

- **init closure fails**: `build_managed` returns the error, no thread left running.
- **reset fails**: `reset()` returns the error. context errors on subsequent calls until a successful reset.
- **thread panics**: channel disconnects → `Error::InitError("context thread died")`.
- **fresh mode init fails**: that `evaluate` returns the init error. next call tries again.

### Drop

sends `Shutdown`, joins the dedicated thread. foreign objects dropped when context drops on the dedicated thread.

### code organization

| file | role |
|------|------|
| `thread.rs` (new) | shared protocol: `Request`, `Response`, `SendableValue`, `Send` impls, dispatch helper |
| `timeout.rs` (refactored) | `TimeoutContext` uses shared protocol (no API change) |
| `managed.rs` (new) | `ThreadLocalContext`, `Mode` |
| `context.rs` | `ContextBuilder` gets `Clone` + `build_managed` / `build_managed_fresh` |
| `lib.rs` | re-export `ThreadLocalContext`, `Mode` |

### ContextBuilder changes

- add `#[derive(Clone)]`
- add `build_managed(init_fn) -> Result<ThreadLocalContext>`
- add `build_managed_fresh(init_fn) -> Result<ThreadLocalContext>`

## future work

- wall-clock timeout support ([#29](https://github.com/emesal/tein/issues/29))
- `ContextPool` as `Vec<ThreadLocalContext>` with checkout/return semantics

## tests

- persistent mode: evaluate, state accumulates across calls
- fresh mode: evaluate, state does NOT accumulate
- reset: state cleared, init closure re-runs, foreign objects dropped
- init failure: returns error, no leaked thread
- thread death: channel disconnect surfaces as error
- define_fn_variadic: works via channel proxy
- mode(): returns correct variant
