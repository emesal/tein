# quickstart

Get tein running in five minutes.

---

## dependency

Add tein to your `Cargo.toml`. All optional format modules are on by default:

```toml
[dependencies]
tein = { git = "https://github.com/emesal/tein" }
```

To strip out the format-module dependencies (`serde`, `serde_json`, `toml`, `uuid`):

```toml
[dependencies]
tein = { git = "https://github.com/emesal/tein", default-features = false }
```

Available features (all default-on):

| feature | what it adds |
|---------|-------------|
| `json`  | `(tein json)` — `json-parse`, `json-stringify`. pulls in `serde` + `serde_json`. |
| `toml`  | `(tein toml)` — `toml-parse`, `toml-stringify`. pulls in the `toml` crate. |
| `uuid`  | `(tein uuid)` — `make-uuid`, `uuid?`, `uuid-nil`. pulls in the `uuid` crate. |
| `time`  | `(tein time)` — `current-second`, `current-jiffy`, `jiffies-per-second`. pure `std::time`, no extra deps. |

Re-enable individual features selectively:

```toml
tein = { git = "https://github.com/emesal/tein", default-features = false, features = ["json"] }
```

---

## first evaluation

Two constructors cover most use cases:

```rust
use tein::{Context, Value};

// primitive environment — all built-in opcodes (arithmetic, car/cdr, etc.) and syntax,
// but no standard library: no map, for-each, string-split, call/cc, etc.
let ctx = Context::new()?;
let result = ctx.evaluate("(+ 1 2 3)")?;
assert_eq!(result, Value::Integer(6));

// full R7RS standard library — map, for-each, string-split, call/cc, dynamic-wind, etc.
let ctx = Context::new_standard()?;
let result = ctx.evaluate("(map (lambda (x) (* x x)) (list 1 2 3 4 5))")?;
assert_eq!(result, Value::List(vec![
    Value::Integer(1), Value::Integer(4), Value::Integer(9),
    Value::Integer(16), Value::Integer(25),
]));
```

`Context::new()` is faster to build and appropriate when you control the Scheme code and
only need the core language. `Context::new_standard()` loads the full r7rs-small stdlib
from tein's embedded VFS — adds a few milliseconds at startup, pays off if you need
higher-order functions, string processing, or `call/cc`.

State persists across `evaluate()` calls on the same context:

```rust
let ctx = Context::new()?;
ctx.evaluate("(define x 42)")?;
let result = ctx.evaluate("x")?;
assert_eq!(result, Value::Integer(42));
```

---

## working with values

`evaluate()` returns `tein::Value`, a Rust enum that covers every Scheme type. Pattern
match directly:

```rust
let ctx = Context::new()?;

match ctx.evaluate("(list 1 2 3)")? {
    Value::List(items) => println!("got {} items", items.len()),
    Value::Nil         => println!("empty list"),
    other              => println!("unexpected: {other}"),
}
```

For the common cases, extraction helpers return `Option<T>`:

```rust
let ctx = Context::new()?;

let n: i64  = ctx.evaluate("42")?.as_integer().unwrap();
let f: f64  = ctx.evaluate("3.14")?.as_float().unwrap();
let b: bool = ctx.evaluate("#t")?.as_bool().unwrap();

// as_string() and as_list() borrow from the Value — bind it first
let val = ctx.evaluate(r#""hello""#)?;
let s: &str = val.as_string().unwrap();

let val = ctx.evaluate("(list 10 20 30)")?;
let v: &[Value] = val.as_list().unwrap();
```

All helpers return `None` when the value is the wrong type — no panics.

The full variant table (including `Pair`, `Vector`, `Char`, `Bytevector`, `Procedure`,
`Foreign`, and the numeric tower variants) is in [embedding.md](embedding.md).

---

## calling rust from scheme

Annotate a Rust function with `#[tein_fn]`, then register it by name using
`define_fn_variadic`. The macro generates a wrapper named `__tein_{fn_name}`:

```rust
use tein::{Context, Value, tein_fn};

#[tein_fn]
fn square(n: i64) -> i64 {
    n * n
}

let ctx = Context::new()?;
ctx.define_fn_variadic("square", __tein_square)?;

let result = ctx.evaluate("(square 7)")?;
assert_eq!(result, Value::Integer(49));
```

Supported argument types: `i64`, `f64`, `String`, `bool`. Functions can take any number
of typed arguments. To accept any Scheme value, use `value: Value` in the signature.

Return `Result<T, E: Display>` to surface errors back to Scheme:

```rust
#[tein_fn]
fn safe_div(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 {
        Err("division by zero".to_string())
    } else {
        Ok(a / b)
    }
}

ctx.define_fn_variadic("safe-div", __tein_safe_div)?;

// scheme error propagated as a string result:
let result = ctx.evaluate("(safe-div 10 0)")?;
// result is Value::String("division by zero")
```

---

## sandboxing in one step

Build a sandboxed context with `ContextBuilder`:

```rust
use tein::{Context, sandbox::Modules};

let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .step_limit(50_000)
    .build()?;
```

`Modules::Safe` is the conservative preset: `scheme/base`, `scheme/write`,
`scheme/read`, most `srfi/*`, and `tein/*` (excluding eval, repl, and process). File IO
is blocked by default — no `open-input-file` access unless you explicitly grant it:

```rust
// no file_read() policy → open-input-file is denied
match ctx.evaluate(r#"(open-input-file "/etc/passwd")"#) {
    Err(e) => println!("blocked: {e}"),
    Ok(_)  => unreachable!(),
}
```

The step limit bounds total VM operations, terminating infinite loops before they
consume resources. Combine with a wall-clock timeout via `build_timeout` for hard
deadlines.

For the full sandboxing API — `Modules::Only`, `Modules::None`, `file_read`,
`file_write`, `allow_module`, `build_timeout` — see [sandboxing.md](sandboxing.md).
