# foreign type protocol — design

**goal:** enable rust types to be exposed as first-class scheme objects with method dispatch, introspection, and error messages designed for LLM self-correction.

**primary use case:** LLM agents use sandboxed tein to write scheme logic. side-effects (file IO, HTTP, database) are performed via pre-compiled rust code exposed as typed foreign objects. the agent writes scheme, calls methods on these objects, and gets clear feedback when something goes wrong.

**approach:** handle-map with scheme-level protocol module (approach C + VFS module). no C-level changes.

---

## architecture overview

```
rust side                          scheme side
─────────                          ───────────
ForeignType trait                  (tein foreign) VFS module
  type_name()                        <foreign> record type
  methods()                          foreign? predicate
       │                             foreign-call dispatch
       ▼                             foreign-methods introspection
ForeignStore (in Context)              │
  type_registry: {name → methods}      │
  instances: {id → (Any, name)}        │
       │                               │
       └───── rust-registered fns ─────┘
              (dispatch, introspect)
```

**data flow:**
1. host calls `ctx.register_foreign_type::<HttpClient>()` — registers type name + method table
2. host registers a constructor like `make-http-client` via `define_fn_variadic`
3. agent's scheme code calls `(define c (make-http-client "https://..."))`
4. rust constructor creates `HttpClient`, calls `ctx.foreign_value(client)` → allocates handle ID, wraps in `<foreign>` record, returns to scheme
5. agent calls `(http-client-get c "/endpoint")` (auto-generated convenience proc) or `(foreign-call c 'get "/endpoint")`
6. rust dispatch extracts handle ID from record, looks up in `ForeignStore`, finds method "get", calls it with remaining args
7. method runs, returns `Value`, delivered back to scheme

**lifetime:** `ForeignStore` is a field on `Context`. all handles drop when the context drops. no GC integration needed — contexts are short-lived in the agent use case.

---

## rust API

### ForeignType trait

```rust
/// a rust type that can be exposed to scheme as a foreign object.
///
/// implement this trait on your types, then register with
/// `ctx.register_foreign_type::<T>()`.
pub trait ForeignType: Any + 'static {
    /// scheme-visible type name (used in predicates, error messages, Display)
    fn type_name() -> &'static str;

    /// method table — maps scheme symbol names to handler functions
    fn methods() -> &'static [(&'static str, MethodFn)];
}
```

### MethodFn type

```rust
/// a method on a foreign type, called from scheme via `foreign-call` or
/// auto-generated convenience procedures.
///
/// receives: mutable ref to the object (as `dyn Any` — downcast inside),
/// the context (for creating return values or calling scheme), and
/// the remaining arguments as `Value` slices.
pub type MethodFn = fn(&mut dyn Any, &Context, &[Value]) -> Result<Value>;
```

`&mut dyn Any` because methods may mutate state (advance a cursor, mark a connection used, etc.). the downcast is infallible when dispatch is correct — we match type name before calling.

### ForeignStore (internal)

```rust
/// per-context storage for foreign type registrations and live instances.
struct ForeignStore {
    /// registered types: name → method table
    type_registry: HashMap<&'static str, &'static [(&'static str, MethodFn)]>,

    /// live instances: handle ID → (boxed value, type name)
    instances: HashMap<u64, (&'static str, Box<dyn Any>)>,

    /// next handle ID (monotonically increasing)
    next_id: u64,
}
```

### Context methods

```rust
impl Context {
    /// register a foreign type's name and method table.
    /// also auto-registers scheme convenience procedures:
    /// - `type-name?` predicate
    /// - `type-name-method` for each method
    pub fn register_foreign_type<T: ForeignType>(&self) -> Result<()>;

    /// wrap a rust value as a scheme foreign object.
    /// stores it in the ForeignStore and returns a Value representing
    /// the `<foreign>` record that scheme code can use.
    pub fn foreign_value<T: ForeignType>(&self, value: T) -> Result<Value>;

    /// extract a foreign object reference from a Value, if it is a
    /// `Foreign` variant with the correct type.
    pub fn foreign_ref<T: ForeignType>(&self, value: &Value) -> Result<&T>;

    /// extract a mutable foreign object reference.
    pub fn foreign_mut<T: ForeignType>(&self, value: &Value) -> Result<&mut T>;
}
```

### usage example

```rust
struct Counter { n: i64 }

impl ForeignType for Counter {
    fn type_name() -> &'static str { "counter" }
    fn methods() -> &'static [(&'static str, MethodFn)] {
        &[
            ("increment", |obj, _ctx, _args| {
                let c = obj.downcast_mut::<Counter>().unwrap();
                c.n += 1;
                Ok(Value::Integer(c.n))
            }),
            ("get", |obj, _ctx, _args| {
                let c = obj.downcast_ref::<Counter>().unwrap();
                Ok(Value::Integer(c.n))
            }),
            ("reset", |obj, _ctx, _args| {
                let c = obj.downcast_mut::<Counter>().unwrap();
                c.n = 0;
                Ok(Value::Unspecified)
            }),
        ]
    }
}

// constructor — registered as a scheme function
#[scheme_fn]
fn make_counter() -> ??? {
    // needs ctx access — see open question below
}
```

---

## scheme surface

### `(tein foreign)` VFS module

```scheme
;;; (tein foreign) — foreign object protocol
;;;
;;; provides predicates and dispatch for rust-backed typed objects.
;;; foreign objects are created by rust-registered constructor functions
;;; and manipulated via method dispatch.

(define-record-type <foreign>
  (%make-foreign type-name handle-id)
  foreign?
  (type-name foreign-type)
  (handle-id foreign-handle-id))
```

`%make-foreign` is internal — not exported. only rust code creates instances.

### exported procedures

all implemented as rust-registered variadic functions (they need access to `ForeignStore`):

- **`(foreign? x)`** — is `x` a foreign object? (delegates to the record predicate)
- **`(foreign-type x)`** — returns the type name as a string
- **`(foreign-call obj 'method arg ...)`** — universal dispatch
- **`(foreign-methods obj)`** — list of method names for this object's type
- **`(foreign-types)`** — list of all registered type names
- **`(foreign-type-methods type-name)`** — method names for a type by name

### auto-generated convenience procedures

when `register_foreign_type::<HttpClient>()` is called, tein auto-registers:

- **`(http-client? x)`** — `#t` if `x` is a foreign object with type name "http-client"
- **`(http-client-get client arg ...)`** — equivalent to `(foreign-call client 'get arg ...)`
- **`(http-client-post client arg ...)`** — etc. for each method
- **`(http-client-close client)`** — etc.

these are thin wrappers that call the same dispatch path but provide better error messages (they know the expected type at registration time).

### the scheme programmer's experience

```scheme
(import (tein foreign))

;; create
(define c (make-counter))
c                             ; => #<foreign counter:1>

;; use
(counter-increment c)         ; => 1
(counter-increment c)         ; => 2
(counter-get c)               ; => 2

;; introspect
(foreign? c)                  ; => #t
(foreign-type c)              ; => "counter"
(foreign-methods c)           ; => (increment get reset)
(foreign-types)               ; => ("counter")

;; universal dispatch (same thing, different syntax)
(foreign-call c 'get)         ; => 2
```

---

## error messages

designed for LLM self-correction. every error under our control follows: **name the thing, name the problem, name the alternatives.**

| scheme code | error message |
|---|---|
| `(foreign-call 42 'get)` | `foreign-call: expected foreign object, got 42` |
| `(foreign-call c 'delete)` | `foreign-call: counter has no method 'delete' — available: increment, get, reset` |
| `(counter-get 42)` | `counter-get: expected counter, got 42` |
| `(counter-get some-http-client)` | `counter-get: expected counter, got http-client` |
| `(foreign-call c 'get 1 2 3)` | `counter.get: expected 0 arguments, got 3` (if method validates) |
| `(+ c 1)` | `+: expected number, got #<foreign counter:1>` (chibi's error — Display is informative) |

---

## Value::Foreign variant

new variant in the `Value` enum:

```rust
/// a foreign object managed by the ForeignStore
///
/// holds the handle ID and type name. the actual data lives in
/// the Context's ForeignStore — use `ctx.foreign_ref::<T>(value)`
/// to access it.
Foreign {
    handle_id: u64,
    type_name: String,
},
```

**from_raw recognition:** during `Value::from_raw`, after checking all native types, check if the sexp is a record matching the `<foreign>` structure (type-name string + integer handle-id). this requires knowing the record type descriptor, which we'd stash in `Context` at `(tein foreign)` module load time.

**to_raw conversion:** `Value::Foreign` reconstructs the scheme record from the handle ID and type name.

**extraction helpers:**

```rust
impl Value {
    pub fn as_foreign(&self) -> Option<(u64, &str)> { ... }
    pub fn is_foreign(&self) -> bool { ... }
    pub fn foreign_type_name(&self) -> Option<&str> { ... }
}
```

---

## implementation files

| file | change |
|---|---|
| `tein/src/foreign.rs` (new) | `ForeignType` trait, `MethodFn`, `ForeignStore`, `ForeignObject`, dispatch logic |
| `tein/src/context.rs` | `ForeignStore` field, `register_foreign_type()`, `foreign_value()`, `foreign_ref()`, `foreign_mut()`, auto-register convenience procs, load `(tein foreign)` on first use |
| `tein/src/value.rs` | `Value::Foreign` variant, `from_raw` recognition, `to_raw` conversion, helpers, Display, PartialEq |
| `tein/src/lib.rs` | re-export `ForeignType`, `MethodFn` |
| `tein/vendor/chibi-scheme/lib/tein/foreign.sld` (new) | `(tein foreign)` library definition |
| `tein/vendor/chibi-scheme/lib/tein/foreign.scm` (new) | record type + exported scheme procedures |
| `tein/examples/foreign_types.rs` (new) | Counter example |

**no C changes.** tein_shim.c, eval.c, vm.c untouched.

---

## open questions to resolve during implementation

1. **constructor ergonomics:** `#[scheme_fn]` currently doesn't have access to `Context`. constructors need `ctx` to call `foreign_value()`. options: (a) constructors are always `define_fn_variadic` (manual, but explicit), (b) extend `#[scheme_fn]` to support a `ctx: &Context` parameter. (b) is better long-term but adds scope. recommend (a) for now, (b) as follow-up.

2. **`from_raw` record recognition:** we need to identify the `<foreign>` record type descriptor at runtime. options: (a) stash the descriptor in Context when `(tein foreign)` is loaded, (b) use a magic symbol/tag convention. (a) is cleaner.

3. **interior mutability:** `foreign_mut` needs `&mut` access to ForeignStore from `&self` Context. this means `RefCell<ForeignStore>` or similar interior mutability. fits the existing pattern (Context already uses interior mutability for fuel, etc.).

4. **`#[scheme_fn]` support for foreign types as arguments:** currently supports `i64`, `f64`, `String`, `bool`. adding `Foreign<T>` as an argument type in the proc macro would be very ergonomic but adds macro complexity. recommend as follow-up milestone.

---

## testing

```
context.rs tests:
  test_foreign_type_register         register type, verify method list via introspection
  test_foreign_value_roundtrip       create in rust → pass to scheme → return to rust → downcast
  test_foreign_call_dispatch         scheme calls method, correct result
  test_foreign_call_wrong_method     error lists available methods
  test_foreign_call_not_foreign      error on non-foreign argument
  test_foreign_call_wrong_type       error names both expected and actual type
  test_foreign_convenience_procs     auto-generated type-method procs work
  test_foreign_predicate             auto-generated type? predicate
  test_foreign_introspection         foreign-types, foreign-methods, foreign-type-methods
  test_foreign_display               #<foreign type:id> format
  test_foreign_in_sandbox            works with preset-based sandboxing
  test_foreign_cleanup_on_drop       Context drop deallocates all handles
  test_foreign_stale_handle          clear error on expired handle
  test_foreign_mutable_state         method mutates, next call sees change
```

---

## non-goals (explicit)

- **GC-integrated lifetimes.** handles are scoped to Context, cleaned up on drop. no chibi GC interaction.
- **scheme-defined foreign types.** the agent *uses* foreign types, it doesn't *define* them. type definitions come from the rust host.
- **#[scheme_fn] integration.** proc macro support for foreign types as args/returns is a follow-up.
- **thread safety.** Context is !Send + !Sync. ForeignStore follows suit.

## related

- #27: VFS-embedded documentation for LLM schemers (depends on this landing)
