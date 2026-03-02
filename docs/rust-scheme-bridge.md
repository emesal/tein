# rust–scheme bridge

expose Rust functions, types, and constants to Scheme with zero FFI boilerplate.

## #[tein_fn] — standalone functions

```rust
use tein::tein_fn;

#[tein_fn]
fn square(n: i64) -> i64 { n * n }

ctx.define_fn_variadic("square", __tein_square)?;
assert_eq!(ctx.evaluate("(square 7)")?, Value::Integer(49));
```

Generates `__tein_{fn_name}` with chibi's variadic FFI signature.

### supported types

| rust type | scheme type |
|-----------|-------------|
| `i64` | integer |
| `f64` | float |
| `String` | string |
| `bool` | boolean |
| `Value` | any scheme value |

Return `Result<T, E: Display>` — `Err` becomes a scheme error string. Return `()` for void.

### the Value argument type

Use `value: Value` to accept any scheme value (e.g. for predicates):

```rust
#[tein_fn]
fn my_pred(value: Value) -> bool {
    matches!(value, Value::Integer(_))
}
```

## #[tein_module] — full module pattern

Groups functions, types, and constants into an importable `(tein module-name)` VFS module.

```rust
use tein_macros::{tein_module, tein_fn, tein_type, tein_methods, tein_const};

#[tein_module("mymod")]
pub(crate) mod mymod_impl {
    /// greet someone
    #[tein_fn]
    pub fn greet(name: String) -> String {
        format!("hello, {name}!")
    }

    /// the answer to everything
    #[tein_const]
    pub const ANSWER: i64 = 42;

    /// a counter type
    #[tein_type]
    pub struct Counter { pub n: i64 }

    #[tein_methods]
    impl Counter {
        pub fn get(&self) -> i64 { self.n }
        pub fn increment(&mut self) -> i64 { self.n += 1; self.n }
    }
}

// register before importing — call once per context
mymod_impl::register_module_mymod(&ctx)?;

// scheme:
// (import (tein mymod))
// (mymod-greet "alice")     => "hello, alice!"
// answer                    => 42
// (counter? x)              => #t or #f
// (counter-get c)           => current value
// (counter-increment c)     => new value
```

`#[tein_module]` also scrapes doc comments (`///`) from each item and stores them in
a doc alist that `(tein docs)` can query at runtime — see [modules.md](modules.md).

### naming conventions

| rust name | scheme name | note |
|-----------|-------------|------|
| `greet` (free fn) | `mymod-greet` | module prefix added |
| `ANSWER` (const) | `answer` | no module prefix |
| `Counter` (type) | `counter` | kebab-case |
| `get` (method) | `counter-get` | type prefix added |
| `is_valid_q` | `mymod-is-valid?` | `_q` → `?` |
| `set_bang` | `mymod-set!` | `_bang` → `!` |
| `_` in name | `-` | underscores → hyphens |

Override with `#[tein_fn(name = "scheme-name")]` or `#[tein_type(name = "scheme-name")]`.

### #[tein_const] naming note

Constants get **no** module prefix — `#[tein_const] pub const MY_VALUE` in a module
`"mymod"` → scheme name `my-value`. Free fns do get the prefix (`mymod-my-fn`).

## ForeignType — manual implementation

Alternative to `#[tein_module]` when you need more control:

```rust
use tein::{Context, ForeignType, MethodFn, Value};

struct Counter { n: i64 }

impl ForeignType for Counter {
    fn type_name() -> &'static str { "counter" }
    fn methods() -> &'static [(&'static str, MethodFn)] {
        &[
            ("get", |obj, _ctx, _args| {
                Ok(Value::Integer(obj.downcast_ref::<Counter>().unwrap().n))
            }),
            ("increment", |obj, _ctx, _args| {
                let c = obj.downcast_mut::<Counter>().unwrap();
                c.n += 1;
                Ok(Value::Integer(c.n))
            }),
        ]
    }
}

ctx.register_foreign_type::<Counter>()?;
// auto-generated: counter?, counter-get, counter-increment
```

### creating and using foreign values

```rust
let c = ctx.foreign_value(Counter { n: 0 })?;

// call from rust
let inc = ctx.evaluate("counter-increment")?;
ctx.call(&inc, std::slice::from_ref(&c))?;

// get a typed reference back
let c_ref = ctx.foreign_ref::<Counter>(&c)?;  // &Counter
println!("{}", c_ref.n);
```

Or pass to scheme for evaluation:

```scheme
(counter-get my-counter)     ; => integer
(counter-increment c)        ; => incremented value
```

### introspection

```scheme
(foreign-types)                ; => ("counter") — all registered type names
(foreign-methods "counter")    ; => (get increment)
(foreign-type my-counter)      ; => "counter"
(foreign-handle-id my-counter) ; => 1  (monotonic handle ID)
```

Error messages list available methods on wrong-method call — designed to be useful in
LLM tool errors.

## ctx.call() — calling scheme from rust

Retrieve a scheme procedure and call it:

```rust
ctx.evaluate("(define (add a b) (+ a b))")?;
let add_fn = ctx.evaluate("add")?;
let result = ctx.call(&add_fn, &[Value::Integer(3), Value::Integer(4)])?;
assert_eq!(result, Value::Integer(7));
```

## reader extensions

Register custom `#` dispatch characters.

### from rust

```rust
let handler = ctx.evaluate("(lambda (port) 42)")?;
ctx.register_reader(b'j', &handler)?;
assert_eq!(ctx.evaluate("#j")?, Value::Integer(42));
```

### from scheme

```scheme
(import (tein reader))
(set-reader! #\j (lambda (port) (list 'json (read port))))
;; #j(1 2 3) → (json (1 2 3))
```

Other exports: `unset-reader!`, `reader-dispatch-chars`

Reserved characters (cannot override): `t`, `f`, `\`, `(`, numeric prefixes.

The dispatch table is thread-local and cleared on `Context` drop.

## macro expansion hooks

Intercept every macro expansion at analysis time. The return value replaces the
expansion and is re-analysed (replace-and-reanalyse semantics).

### from scheme

```scheme
(import (tein macro))

;; observe without changing
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    expanded))

;; transform: log every 'when expansion
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    (when (eq? name 'when)
      (display (list 'expanding name unexpanded)))
    expanded))
```

### from rust

```rust
let hook = ctx.evaluate("(lambda (name pre post env) post)")?;
ctx.set_macro_expand_hook(&hook)?;
// later:
ctx.unset_macro_expand_hook();
```

Hook args: `name` (symbol), `unexpanded` form, `expanded` form, syntactic environment.
Recursion guard prevents the hook triggering on its own macro usage.
Hook cleared on `Context` drop.

Other exports: `unset-macro-expand-hook!`, `macro-expand-hook` (returns current hook or `#f`).
