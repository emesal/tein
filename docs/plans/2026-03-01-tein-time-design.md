# (tein time) — sandbox-safe time module

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** add `(tein time)` as a sandbox-safe r7rs-compatible time module, replacing `(scheme time)` for sandboxed contexts. closes #90.

**Architecture:** rust-backed `#[tein_module("time")]` using `std::time` only, following the `uuid.rs` pattern. feature-gated behind `time` (default on, zero external deps). registered in `VFS_MODULES_SAFE`.

**Tech Stack:** rust std::time (SystemTime, Instant, OnceLock), tein_macros

---

## context

`(scheme time)` transitively depends on unsafe modules (`scheme/process-context`,
`scheme/file`, `srfi/18`) making it unsuitable for `VFS_MODULES_SAFE`. this module
provides a self-contained alternative.

## design

### exports

| scheme procedure | r7rs return type | rust type | implementation |
|---|---|---|---|
| `current-second` | inexact number | `f64` | `SystemTime::now().duration_since(UNIX_EPOCH)` as fractional seconds |
| `current-jiffy` | exact integer | `i64` | `Instant::now()` nanos since process-relative epoch |
| `jiffies-per-second` | exact integer (constant) | `i64` | `1_000_000_000` |

### semantics

- **`current-second`**: wall-clock POSIX time with sub-second precision. r7rs specifies
  TAI but explicitly allows UTC with a constant offset; we follow the common implementation
  practice of returning UTC-based time. returns inexact per spec.
- **`current-jiffy`**: monotonic clock, nanosecond resolution. epoch set on first call via
  `std::sync::OnceLock<Instant>`, constant within a program run per r7rs. returns exact integer.
- **`jiffies-per-second`**: constant 10⁹ (nanoseconds). exact integer per spec.

### what's excluded

- TAI/NTP/leap-second handling (chibi's elaborate `scheme/time/tai` machinery)
- `SEXP_CLOCK_TYPE` / `SEXP_CLOCK_EPOCH_OFFSET` env var config
- dependency on `scheme/process-context`, `scheme/file`, `srfi/18`

---

## implementation plan

### task 1: create `tein/src/time.rs` with `#[tein_module]`

**files:**
- create: `tein/src/time.rs`

**step 1: write the module**

```rust
//! `(tein time)` — sandbox-safe r7rs time procedures.
//!
//! provides:
//! - `current-second` — wall-clock POSIX time as inexact seconds since epoch
//! - `current-jiffy` — monotonic nanosecond counter (exact integer)
//! - `jiffies-per-second` — constant 10⁹

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tein_macros::tein_module;

/// monotonic epoch for jiffies — set on first `current-jiffy` call,
/// constant for the rest of the program run (per r7rs).
static JIFFY_EPOCH: OnceLock<Instant> = OnceLock::new();

#[tein_module("time")]
pub(crate) mod time_impl {
    /// nanoseconds per second — the jiffy resolution constant.
    #[allow(dead_code)]
    #[tein_const]
    pub const JIFFIES_PER_SECOND: i64 = 1_000_000_000;

    /// return current wall-clock time as inexact seconds since the POSIX epoch.
    ///
    /// r7rs specifies TAI but explicitly allows UTC with a constant offset.
    /// we return UTC-based time, matching common implementation practice.
    #[tein_fn(name = "current-second")]
    pub fn current_second() -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_secs_f64()
    }

    /// return elapsed nanoseconds since a process-relative epoch (exact integer).
    ///
    /// the epoch is set on first call and remains constant within a program run,
    /// per r7rs. uses `Instant` for monotonic timing.
    #[tein_fn(name = "current-jiffy")]
    pub fn current_jiffy() -> i64 {
        let epoch = JIFFY_EPOCH.get_or_init(Instant::now);
        epoch.elapsed().as_nanos() as i64
    }
}
```

**step 2: commit**

```
feat(time): add (tein time) module implementation (#90)
```

---

### task 2: wire up feature gate and registration

**files:**
- modify: `tein/Cargo.toml` — add `time` feature
- modify: `tein/src/lib.rs` — add `mod time` + feature flag + doc table row
- modify: `tein/src/context.rs` — register module in builder

**step 1: add feature to `tein/Cargo.toml`**

in `[features]`, add `time` feature (no deps — std only). add to `default` list.

```toml
## enables `(tein time)` module with `current-second`, `current-jiffy`, and `jiffies-per-second`.
## pure std::time — no external dependencies.
time = []
```

default becomes:
```toml
default = ["json", "toml", "uuid", "time"]
```

**step 2: add module to `tein/src/lib.rs`**

after the uuid module declaration:
```rust
#[cfg(feature = "time")]
mod time;
```

add row to the feature flags doc table:
```
| `time`  | yes     | Enables `(tein time)` module with `current-second`, `current-jiffy`, and `jiffies-per-second`. Pure `std::time` — no external deps. |
```

**step 3: register in `tein/src/context.rs`**

after the uuid registration block (around line 1976), add:
```rust
#[cfg(feature = "time")]
if self.standard_env {
    crate::time::time_impl::register_module_time(&context)?;
}
```

update the comment on line 1961 to mention time:
```rust
// register feature-gated module trampolines for standard-env contexts.
// these are pure data operations (format conversion, uuid generation, time),
// no IO — always safe and cheap to register.
```

**step 4: verify it compiles**

run: `cargo build`

**step 5: commit**

```
feat(time): wire up feature gate and context registration (#90)
```

---

### task 3: add to `VFS_MODULES_SAFE`

**files:**
- modify: `tein/src/sandbox.rs` — add VfsModule entry

**step 1: add entry to `VFS_MODULES_SAFE`**

add after the `tein/uuid` entry (around line 263):
```rust
VfsModule {
    path: "tein/time",
    deps: &[],
},
```

**step 2: verify sandbox tests pass**

run: `cargo test --lib -- sandbox`

**step 3: commit**

```
feat(time): add (tein time) to VFS_MODULES_SAFE (#90)
```

---

### task 4: write rust integration tests

**files:**
- create: `tein/tests/tein_time.rs`

**step 1: write integration tests**

```rust
//! integration tests for `(tein time)`.

use tein::{Context, Value};

fn ctx() -> Context {
    Context::new_standard().expect("context")
}

#[test]
fn test_current_second_returns_flonum() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let val = ctx.evaluate("(current-second)").expect("current-second");
    assert!(
        matches!(val, Value::Float(_)),
        "expected float, got {:?}",
        val
    );
}

#[test]
fn test_current_second_is_positive() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let val = ctx.evaluate("(current-second)").expect("current-second");
    if let Value::Float(f) = val {
        assert!(f > 0.0, "current-second should be positive, got {}", f);
    } else {
        panic!("expected float");
    }
}

#[test]
fn test_current_second_is_recent() {
    // sanity check: should be after 2025-01-01 (~1735689600)
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let val = ctx.evaluate("(current-second)").expect("current-second");
    if let Value::Float(f) = val {
        assert!(f > 1_735_689_600.0, "timestamp too old: {}", f);
    } else {
        panic!("expected float");
    }
}

#[test]
fn test_current_jiffy_returns_integer() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let val = ctx.evaluate("(current-jiffy)").expect("current-jiffy");
    assert!(
        matches!(val, Value::Integer(_)),
        "expected integer, got {:?}",
        val
    );
}

#[test]
fn test_current_jiffy_is_non_negative() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let val = ctx.evaluate("(current-jiffy)").expect("current-jiffy");
    if let Value::Integer(n) = val {
        assert!(n >= 0, "jiffies should be non-negative, got {}", n);
    } else {
        panic!("expected integer");
    }
}

#[test]
fn test_current_jiffy_is_monotonic() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    let a = ctx.evaluate("(current-jiffy)").expect("jiffy a");
    let b = ctx.evaluate("(current-jiffy)").expect("jiffy b");
    if let (Value::Integer(a), Value::Integer(b)) = (a, b) {
        assert!(b >= a, "jiffies not monotonic: {} then {}", a, b);
    } else {
        panic!("expected integers");
    }
}

#[test]
fn test_jiffies_per_second_value() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import");
    assert_eq!(
        ctx.evaluate("jiffies-per-second").unwrap(),
        Value::Integer(1_000_000_000)
    );
}

#[test]
fn test_time_docs() {
    let ctx = ctx();
    ctx.evaluate("(import (tein time))").expect("import time");
    ctx.evaluate("(import (tein time docs))")
        .expect("import time docs");
    ctx.evaluate("(import (tein docs))").expect("import docs");
    let desc = ctx.evaluate("(describe time-docs)").expect("describe");
    if let Value::String(s) = desc {
        assert!(s.contains("current-second"), "docs missing current-second: {}", s);
        assert!(s.contains("current-jiffy"), "docs missing current-jiffy: {}", s);
        assert!(s.contains("jiffies-per-second"), "docs missing jiffies-per-second: {}", s);
    } else {
        panic!("describe returned non-string: {:?}", desc);
    }
}

#[test]
fn test_time_in_sandbox() {
    let ctx = Context::builder()
        .standard_env()
        .safe()
        .allow(&["import"])
        .build()
        .expect("sandboxed context");
    ctx.evaluate("(import (tein time))")
        .expect("import in sandbox");

    // current-second works
    let val = ctx.evaluate("(current-second)").expect("current-second in sandbox");
    assert!(
        matches!(val, Value::Float(_)),
        "expected float, got {:?}",
        val
    );

    // current-jiffy works
    let val = ctx.evaluate("(current-jiffy)").expect("current-jiffy in sandbox");
    assert!(
        matches!(val, Value::Integer(_)),
        "expected integer, got {:?}",
        val
    );

    // jiffies-per-second is correct
    assert_eq!(
        ctx.evaluate("jiffies-per-second").unwrap(),
        Value::Integer(1_000_000_000)
    );
}
```

**step 2: run tests**

run: `cargo test -p tein -- tein_time`
expected: all pass

**step 3: commit**

```
test(time): add rust integration tests for (tein time) (#90)
```

---

### task 5: write scheme-level tests

**files:**
- create: `tein/tests/scheme/tein_time.scm`
- modify: `tein/tests/scheme_tests.rs` — add test entry

**step 1: write scheme test file**

```scheme
;;; (tein time) scheme-level tests

(import (tein time))

;; current-second returns an inexact number
(test-true "time/current-second-inexact" (inexact? (current-second)))

;; current-second is positive
(test-true "time/current-second-positive" (> (current-second) 0))

;; current-second is a reasonable recent timestamp (after 2025-01-01)
(test-true "time/current-second-recent" (> (current-second) 1735689600))

;; current-jiffy returns an exact integer
(test-true "time/current-jiffy-exact" (exact? (current-jiffy)))
(test-true "time/current-jiffy-integer" (integer? (current-jiffy)))

;; current-jiffy is non-negative
(test-true "time/current-jiffy-non-negative" (>= (current-jiffy) 0))

;; current-jiffy is monotonic
(test-true "time/current-jiffy-monotonic"
  (let ((a (current-jiffy))
        (b (current-jiffy)))
    (>= b a)))

;; jiffies-per-second is 10^9
(test-equal "time/jiffies-per-second" 1000000000 jiffies-per-second)

;; elapsed time via jiffies is consistent with jiffies-per-second
(test-true "time/elapsed-seconds"
  (let ((start (current-jiffy))
        (end (current-jiffy)))
    (>= (/ (- end start) jiffies-per-second) 0)))
```

**step 2: add test entry to `scheme_tests.rs`**

after the uuid test entry:
```rust
#[cfg(feature = "time")]
#[test]
fn test_scheme_tein_time() {
    run_scheme_test(include_str!("scheme/tein_time.scm"));
}
```

**step 3: run tests**

run: `cargo test -p tein -- tein_time`
expected: all pass

**step 4: commit**

```
test(time): add scheme-level tests for (tein time) (#90)
```

---

### task 6: update docs and AGENTS.md

**files:**
- modify: `AGENTS.md` — add time.rs to architecture, add to test count, note in VFS_MODULES_SAFE
- modify: `tein/src/sandbox.rs` — update doc comment about scheme/time exclusion

**step 1: update AGENTS.md architecture**

add `time.rs` entry after `uuid.rs`:
```
  time.rs      — time_impl #[tein_module]: current-second (wall-clock via SystemTime),
                 current-jiffy (monotonic via Instant + OnceLock), jiffies-per-second (constant 10⁹).
                 feature-gated behind `time` cargo feature
```

add VFS entries to chibi-scheme section:
```
  lib/tein/time.sld  — (tein time) library definition (generated by #[tein_module])
  lib/tein/time.scm  — module documentation (generated by #[tein_module])
```

update test count in commands section to reflect new tests.

**step 2: update sandbox.rs doc comment**

in the `VFS_MODULES_SAFE` doc comment (around line 227-229), update the sentence about `scheme/time`:
```rust
/// `scheme/time` is excluded because it transitively depends on unvetted modules
/// (`scheme/process-context`, `scheme/file`). use `(tein time)` instead (see #90).
```

**step 3: run lint**

run: `just lint`

**step 4: commit**

```
docs: update AGENTS.md and sandbox docs for (tein time), closes #90
```

---

### task 7: final verification

**step 1: full test suite**

run: `just test`
expected: all pass, new test count reflects tein_time tests

**step 2: verify sandbox works end to end**

run: `cargo test -p tein -- test_time_in_sandbox`

**step 3: collect AGENTS.md notes**

review any caveats discovered during implementation for AGENTS.md.
- `time` feature has no external deps (unlike `uuid`, `json`, `toml`)
- `JIFFY_EPOCH` is a process-global `OnceLock`, shared across all Context instances — jiffies are process-relative, not context-relative. this matches r7rs ("constant within a single run of the program").
