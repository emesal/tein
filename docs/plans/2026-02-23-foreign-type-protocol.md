# foreign type protocol — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** enable rust types to be exposed as first-class scheme objects with method dispatch, introspection, and LLM-friendly error messages — zero C changes.

**Architecture:** handle-map (`ForeignStore`) inside `Context` keyed by `u64` IDs, with a `(tein foreign)` VFS module providing the scheme-side record type and protocol. rust `ForeignType` trait defines type name + method table. auto-generated convenience procs (`type-method`, `type?`) plus universal `foreign-call` dispatch. `Value::Foreign` variant for clean round-tripping. see `docs/plans/2026-02-23-foreign-type-protocol-design.md` for full design rationale.

**Tech Stack:** rust (new `foreign.rs` module, changes to `context.rs`, `value.rs`, `lib.rs`), scheme (new VFS files `lib/tein/foreign.sld` + `lib/tein/foreign.scm`), build.rs (add VFS entries)

---

### task 1: ForeignStore and ForeignType trait

**files:**
- create: `tein/src/foreign.rs`
- modify: `tein/src/lib.rs`

**step 1: create `tein/src/foreign.rs` with core types**

```rust
//! foreign type protocol — typed rust objects accessible from scheme
//!
//! enables rust types to be exposed as first-class scheme objects with
//! method dispatch, introspection, and clear error messages.
//!
//! # architecture
//!
//! foreign objects are stored in a per-context `ForeignStore` keyed by
//! monotonically increasing `u64` handle IDs. scheme sees them as
//! `<foreign>` records (from the `(tein foreign)` module) containing
//! a type name string and integer handle ID. the actual data lives
//! rust-side — scheme never touches it directly.
//!
//! # usage
//!
//! ```ignore
//! use tein::foreign::{ForeignType, MethodFn};
//!
//! struct Counter { n: i64 }
//!
//! impl ForeignType for Counter {
//!     fn type_name() -> &'static str { "counter" }
//!     fn methods() -> &'static [(&'static str, MethodFn)] {
//!         &[("get", |obj, _ctx, _args| {
//!             let c = obj.downcast_ref::<Counter>().unwrap();
//!             Ok(Value::Integer(c.n))
//!         })]
//!     }
//! }
//! ```

use crate::error::{Error, Result};
use crate::Value;
use std::any::Any;
use std::collections::HashMap;

/// a method on a foreign type, callable from scheme.
///
/// receives a mutable reference to the object (downcast inside the body),
/// the context for creating return values, and the remaining arguments.
///
/// # example
///
/// ```ignore
/// let method: MethodFn = |obj, _ctx, args| {
///     let counter = obj.downcast_mut::<Counter>().unwrap();
///     counter.n += 1;
///     Ok(Value::Integer(counter.n))
/// };
/// ```
pub type MethodFn = fn(&mut dyn Any, &MethodContext, &[Value]) -> Result<Value>;

/// limited context passed to foreign methods.
///
/// provides only what methods need: creating foreign values and
/// evaluating scheme. prevents methods from accessing ForeignStore
/// internals directly (which would cause borrow conflicts).
pub struct MethodContext {
    /// raw chibi context pointer — for Value::to_raw/from_raw
    pub(crate) ctx: crate::ffi::sexp,
}

/// a rust type that can be exposed to scheme as a foreign object.
///
/// implement this on your types, then register with
/// `ctx.register_foreign_type::<T>()`. the type name appears in
/// scheme as the record's type-name field and in error messages.
///
/// # naming
///
/// `type_name()` should be a kebab-case identifier suitable for
/// scheme: `"http-client"`, `"counter"`, `"file-writer"`.
/// auto-generated convenience procs use this name:
/// `http-client?`, `http-client-get`, etc.
pub trait ForeignType: Any + 'static {
    /// scheme-visible type name
    fn type_name() -> &'static str;

    /// method table — maps scheme symbol names to handler functions
    fn methods() -> &'static [(&'static str, MethodFn)];
}

/// a live foreign object instance
struct ForeignObject {
    /// the actual rust value
    data: Box<dyn Any>,
    /// type name (from ForeignType::type_name)
    type_name: &'static str,
}

/// type registration entry
struct TypeEntry {
    /// method table for this type
    methods: &'static [(&'static str, MethodFn)],
}

/// per-context storage for foreign type registrations and live instances.
///
/// lives inside `Context` as `RefCell<ForeignStore>`. drops all instances
/// when the context drops — no GC integration needed.
pub(crate) struct ForeignStore {
    /// registered types: name → method table
    types: HashMap<&'static str, TypeEntry>,
    /// live instances: handle ID → object
    instances: HashMap<u64, ForeignObject>,
    /// next handle ID
    next_id: u64,
}

impl ForeignStore {
    /// create an empty store
    pub(crate) fn new() -> Self {
        Self {
            types: HashMap::new(),
            instances: HashMap::new(),
            next_id: 1,
        }
    }

    /// register a type's name and method table. returns error if already registered.
    pub(crate) fn register_type<T: ForeignType>(&mut self) -> Result<()> {
        let name = T::type_name();
        if self.types.contains_key(name) {
            return Err(Error::EvalError(format!(
                "foreign type '{}' already registered",
                name
            )));
        }
        self.types.insert(name, TypeEntry {
            methods: T::methods(),
        });
        Ok(())
    }

    /// store a value and return its handle ID
    pub(crate) fn insert<T: ForeignType>(&mut self, value: T) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.instances.insert(id, ForeignObject {
            data: Box::new(value),
            type_name: T::type_name(),
        });
        id
    }

    /// look up an instance by handle ID
    pub(crate) fn get(&self, id: u64) -> Option<(&dyn Any, &'static str)> {
        self.instances.get(&id).map(|obj| (obj.data.as_ref(), obj.type_name))
    }

    /// look up an instance mutably by handle ID
    pub(crate) fn get_mut(&mut self, id: u64) -> Option<(&mut dyn Any, &'static str)> {
        self.instances.get_mut(&id).map(|obj| (obj.data.as_mut(), obj.type_name))
    }

    /// look up a method by type name and method name
    pub(crate) fn find_method(&self, type_name: &str, method_name: &str) -> Option<MethodFn> {
        self.types.get(type_name).and_then(|entry| {
            entry.methods.iter()
                .find(|(name, _)| *name == method_name)
                .map(|(_, f)| *f)
        })
    }

    /// list method names for a type
    pub(crate) fn method_names(&self, type_name: &str) -> Option<Vec<&'static str>> {
        self.types.get(type_name).map(|entry| {
            entry.methods.iter().map(|(name, _)| *name).collect()
        })
    }

    /// list all registered type names
    pub(crate) fn type_names(&self) -> Vec<&'static str> {
        self.types.keys().copied().collect()
    }

    /// check if a type is registered
    pub(crate) fn has_type(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }
}
```

**step 2: add module declaration to `tein/src/lib.rs`**

add `mod foreign;` after `mod ffi;` and add to the public re-exports:

```rust
pub mod foreign;
```

also add to the `pub use` section:

```rust
pub use foreign::{ForeignType, MethodFn, MethodContext};
```

**step 3: verify build**

run: `cargo build -p tein 2>&1 | tail -5`
expected: compiles (MethodContext references ffi::sexp which is pub(crate), all fine since foreign.rs is in the same crate)

**step 4: commit**

```bash
git add tein/src/foreign.rs tein/src/lib.rs
git commit -m "foreign: add ForeignType trait and ForeignStore"
```

---

### task 2: Value::Foreign variant

**files:**
- modify: `tein/src/value.rs`

**step 1: add the Foreign variant to the Value enum**

add after the `Other` variant:

```rust
    /// a foreign object managed by the Context's ForeignStore
    ///
    /// holds the handle ID and type name. the actual data lives rust-side
    /// in the ForeignStore — use `ctx.foreign_ref::<T>(value)` to access.
    Foreign {
        /// handle ID in the ForeignStore
        handle_id: u64,
        /// type name (from ForeignType::type_name)
        type_name: String,
    },
```

**step 2: add PartialEq arm**

in the `PartialEq` impl, add before the `_ => false` fallthrough:

```rust
            (
                Value::Foreign { handle_id: a, type_name: ta },
                Value::Foreign { handle_id: b, type_name: tb },
            ) => a == b && ta == tb,
```

**step 3: add Display arm**

in the `Display` impl, add before the `Other` arm:

```rust
            Value::Foreign { handle_id, type_name } => {
                write!(f, "#<foreign {}:{}>", type_name, handle_id)
            }
```

**step 4: add extraction helpers**

in the typed extraction helpers impl block, add:

```rust
    /// extract foreign object handle ID and type name
    pub fn as_foreign(&self) -> Option<(u64, &str)> {
        match self {
            Value::Foreign { handle_id, type_name } => Some((*handle_id, type_name.as_str())),
            _ => None,
        }
    }

    /// returns the type name if this value is a `Foreign` object
    pub fn foreign_type_name(&self) -> Option<&str> {
        match self {
            Value::Foreign { type_name, .. } => Some(type_name.as_str()),
            _ => None,
        }
    }

    /// returns true if this value is a `Foreign` object
    pub fn is_foreign(&self) -> bool {
        matches!(self, Value::Foreign { .. })
    }
```

**step 5: add to_raw conversion for Foreign**

in `to_raw_depth`, add a match arm before the `Other` arm:

```rust
                Value::Foreign { handle_id, type_name } => {
                    // construct the scheme-side <foreign> record as a list: (type-name . handle-id)
                    // the (tein foreign) module's procedures recognise this shape.
                    let name_c = std::ffi::CString::new(type_name.as_str())
                        .map_err(|_| Error::TypeError("type name contains null bytes".to_string()))?;
                    let name_sexp = ffi::sexp_c_str(ctx, name_c.as_ptr(), type_name.len() as ffi::sexp_sint_t);
                    let _name_root = ffi::GcRoot::new(ctx, name_sexp);
                    let id_sexp = ffi::sexp_make_fixnum(*handle_id as ffi::sexp_sint_t);
                    let tag = ffi::sexp_intern(ctx, b"__tein-foreign\0".as_ptr() as *const std::os::raw::c_char, 14);
                    let _tag_root = ffi::GcRoot::new(ctx, tag);
                    // build tagged list: (__tein-foreign type-name handle-id)
                    let inner = ffi::sexp_cons(ctx, id_sexp, ffi::get_null());
                    let _inner_root = ffi::GcRoot::new(ctx, inner);
                    let mid = ffi::sexp_cons(ctx, name_sexp, inner);
                    let _mid_root = ffi::GcRoot::new(ctx, mid);
                    Ok(ffi::sexp_cons(ctx, tag, mid))
                }
```

**step 6: add from_raw recognition for Foreign**

in `from_raw_depth`, add a check just before the pair/list handling (before `if ffi::sexp_pairp(raw) != 0`). this recognises the tagged list `(__tein-foreign "type-name" handle-id)`:

```rust
            // check for foreign object tagged list: (__tein-foreign "type-name" handle-id)
            if ffi::sexp_pairp(raw) != 0 {
                let car = ffi::sexp_car(raw);
                if ffi::sexp_symbolp(car) != 0 {
                    let sym_str = ffi::sexp_symbol_to_string(ctx, car);
                    let sym_ptr = ffi::sexp_string_data(sym_str);
                    let sym_len = ffi::sexp_string_size(sym_str) as usize;
                    let sym_bytes = std::slice::from_raw_parts(sym_ptr as *const u8, sym_len);
                    if sym_bytes == b"__tein-foreign" {
                        let rest = ffi::sexp_cdr(raw);
                        if ffi::sexp_pairp(rest) != 0 {
                            let name_sexp = ffi::sexp_car(rest);
                            let id_rest = ffi::sexp_cdr(rest);
                            if ffi::sexp_stringp(name_sexp) != 0
                                && ffi::sexp_pairp(id_rest) != 0
                            {
                                let id_sexp = ffi::sexp_car(id_rest);
                                if ffi::sexp_integerp(id_sexp) != 0 {
                                    let name_ptr = ffi::sexp_string_data(name_sexp);
                                    let name_len = ffi::sexp_string_size(name_sexp) as usize;
                                    let name_bytes = std::slice::from_raw_parts(
                                        name_ptr as *const u8,
                                        name_len,
                                    );
                                    let type_name =
                                        String::from_utf8(name_bytes.to_vec())?;
                                    let handle_id =
                                        ffi::sexp_unbox_fixnum(id_sexp) as u64;
                                    return Ok(Value::Foreign {
                                        handle_id,
                                        type_name,
                                    });
                                }
                            }
                        }
                    }
                }
            }
```

**step 7: verify build**

run: `cargo build -p tein 2>&1 | tail -5`
expected: compiles successfully

**step 8: commit**

```bash
git add tein/src/value.rs
git commit -m "value: add Foreign variant with tagged-list wire format"
```

---

### task 3: ForeignStore integration in Context

**files:**
- modify: `tein/src/context.rs`

**step 1: add ForeignStore field to Context**

add `use std::cell::RefCell;` to imports (Cell is already imported, add RefCell).

add `use crate::foreign::{ForeignStore, ForeignType, MethodContext, MethodFn};` to imports.

add field to `Context` struct:

```rust
pub struct Context {
    ctx: ffi::sexp,
    step_limit: Option<u64>,
    has_io_wrappers: bool,
    has_module_policy: bool,
    foreign_store: RefCell<ForeignStore>,
}
```

**step 2: initialise ForeignStore in ContextBuilder::build()**

in the `build()` method, where `Context` is constructed (the `Ok(Context { ... })` return), add:

```rust
    foreign_store: RefCell::new(ForeignStore::new()),
```

**step 3: add register_foreign_type method**

add to `impl Context`, after `define_fn_variadic`:

```rust
    /// register a foreign type, making its methods callable from scheme.
    ///
    /// auto-registers scheme convenience procedures for each method:
    /// - `type-name?` — predicate (is this a foreign object of this type?)
    /// - `type-name-method` — for each method in the type's method table
    ///
    /// requires `(tein foreign)` protocol functions to be available.
    /// call `register_foreign_protocol()` first if not already done.
    ///
    /// # example
    ///
    /// ```ignore
    /// ctx.register_foreign_type::<Counter>()?;
    /// // now scheme has: counter?, counter-increment, counter-get, counter-reset
    /// ```
    pub fn register_foreign_type<T: ForeignType>(&self) -> Result<()> {
        self.foreign_store.borrow_mut().register_type::<T>()?;
        // auto-register convenience procedures is deferred to task 6
        Ok(())
    }

    /// wrap a rust value as a scheme foreign object.
    ///
    /// stores it in the ForeignStore and returns a `Value::Foreign`
    /// that scheme code can pass around, inspect, and use with `foreign-call`.
    pub fn foreign_value<T: ForeignType>(&self, value: T) -> Result<Value> {
        let id = self.foreign_store.borrow_mut().insert(value);
        Ok(Value::Foreign {
            handle_id: id,
            type_name: T::type_name().to_string(),
        })
    }
```

**step 4: add foreign_ref and foreign_mut**

```rust
    /// borrow a foreign object immutably.
    ///
    /// returns `None` if the value isn't a Foreign, the handle is stale,
    /// or the type doesn't match.
    pub fn foreign_ref<T: ForeignType + 'static>(&self, value: &Value) -> Result<std::cell::Ref<'_, T>> {
        let (id, actual_type) = value.as_foreign().ok_or_else(|| {
            Error::TypeError(format!("expected foreign object, got {}", value))
        })?;
        if actual_type != T::type_name() {
            return Err(Error::TypeError(format!(
                "expected {}, got {}",
                T::type_name(),
                actual_type
            )));
        }
        let store = self.foreign_store.borrow();
        if !store.get(id).is_some() {
            return Err(Error::EvalError(format!(
                "stale foreign handle: {} ({})",
                id, actual_type
            )));
        }
        // we need to return a Ref that borrows through the RefCell.
        // use Ref::map to project into the Any, then downcast.
        Ok(std::cell::Ref::map(store, |s| {
            let (data, _) = s.get(id).unwrap();
            data.downcast_ref::<T>().unwrap()
        }))
    }
```

note: `foreign_mut` has the same pattern but with `RefMut`. however, for the dispatch path (task 5) we'll access `ForeignStore` directly. `foreign_ref`/`foreign_mut` are convenience for the host rust code. if the `Ref::map` approach proves too complex, we can simplify to a `with_foreign` closure-based API instead. use judgement during implementation.

**step 5: verify build**

run: `cargo build -p tein 2>&1 | tail -5`
expected: compiles successfully

**step 6: commit**

```bash
git add tein/src/context.rs
git commit -m "context: integrate ForeignStore with register/value/ref API"
```

---

### task 4: (tein foreign) VFS module

**files:**
- create: `tein/vendor/chibi-scheme/lib/tein/foreign.sld`
- create: `tein/vendor/chibi-scheme/lib/tein/foreign.scm`
- modify: `tein/build.rs` (add VFS entries)

**step 1: create the library definition**

create `tein/vendor/chibi-scheme/lib/tein/foreign.sld`:

```scheme
(define-library (tein foreign)
  (import (scheme base))
  (export foreign? foreign-type foreign-handle-id
          foreign-call foreign-methods
          foreign-types foreign-type-methods)
  (include "foreign.scm"))
```

**step 2: create the scheme implementation**

create `tein/vendor/chibi-scheme/lib/tein/foreign.scm`:

```scheme
;;; (tein foreign) — foreign object protocol
;;;
;;; foreign objects are tagged lists: (__tein-foreign "type-name" handle-id)
;;; created by rust, manipulated via dispatch to rust-registered methods.

;; predicates and accessors for the tagged list representation
;; (__tein-foreign "type-name" handle-id)

(define (foreign? x)
  (and (pair? x)
       (eq? (car x) '__tein-foreign)
       (pair? (cdr x))
       (string? (cadr x))
       (pair? (cddr x))
       (integer? (caddr x))))

(define (foreign-type x)
  (if (foreign? x)
      (cadr x)
      (error "foreign-type: expected foreign object, got" x)))

(define (foreign-handle-id x)
  (if (foreign? x)
      (caddr x)
      (error "foreign-handle-id: expected foreign object, got" x)))

;; foreign-call, foreign-methods, foreign-types, foreign-type-methods
;; are registered from rust as native functions (they need ForeignStore access).
;; the .sld exports them — rust injects them into the module env during
;; register_foreign_protocol().
```

note: `foreign-call`, `foreign-methods`, `foreign-types`, `foreign-type-methods` are *exported by the .sld* but *defined by rust* (registered as foreign functions into the module's environment). the .scm only defines the pure-scheme parts (predicates, accessors). this is the same pattern chibi uses for modules with native C components — the .sld declares exports, the native code provides the implementations.

**step 3: add VFS entries in build.rs**

add to the `VFS_FILES` array:

```rust
    // tein foreign type protocol
    "lib/tein/foreign.sld",
    "lib/tein/foreign.scm",
```

**step 4: create the directory**

run: `mkdir -p tein/vendor/chibi-scheme/lib/tein`

**step 5: verify build**

run: `cargo build -p tein 2>&1 | tail -10`
expected: compiles (VFS embeds the new files)

**step 6: commit**

```bash
git add tein/vendor/chibi-scheme/lib/tein/foreign.sld tein/vendor/chibi-scheme/lib/tein/foreign.scm tein/build.rs
git commit -m "vfs: add (tein foreign) module for foreign object protocol"
```

---

### task 5: foreign-call dispatch (rust-side)

**files:**
- modify: `tein/src/context.rs`
- modify: `tein/src/foreign.rs`

this is the core dispatch mechanism. we register `__tein-foreign-call`, `__tein-foreign-methods`, `__tein-foreign-types`, `__tein-foreign-type-methods` as native variadic functions, then expose them to scheme via the `(tein foreign)` module.

**step 1: add dispatch functions to `foreign.rs`**

add at the bottom of `foreign.rs`:

```rust
use crate::ffi;
use std::ffi::CString;

/// dispatch a method call on a foreign object.
///
/// called from scheme as: (foreign-call obj 'method arg ...)
/// args layout: (obj method-symbol arg1 arg2 ...)
pub(crate) unsafe fn dispatch_foreign_call(
    store: &RefCell<ForeignStore>,
    ctx: ffi::sexp,
    args: ffi::sexp,
) -> std::result::Result<Value, String> {
    unsafe {
        // extract obj (first arg)
        if ffi::sexp_nullp(args) != 0 {
            return Err("foreign-call: expected at least 2 arguments, got 0".to_string());
        }
        let obj_sexp = ffi::sexp_car(args);
        let rest = ffi::sexp_cdr(args);

        // convert obj to Value to extract Foreign fields
        let obj_value = Value::from_raw(ctx, obj_sexp)
            .map_err(|e| format!("foreign-call: {}", e))?;

        let (handle_id, type_name) = obj_value.as_foreign().ok_or_else(|| {
            format!("foreign-call: expected foreign object, got {}", obj_value)
        })?;

        // extract method name (second arg — must be a symbol)
        if ffi::sexp_nullp(rest) != 0 {
            return Err(format!(
                "foreign-call: expected method name after {} object",
                type_name
            ));
        }
        let method_sexp = ffi::sexp_car(rest);
        let args_rest = ffi::sexp_cdr(rest);

        if ffi::sexp_symbolp(method_sexp) == 0 {
            return Err("foreign-call: method name must be a symbol".to_string());
        }
        let method_str_sexp = ffi::sexp_symbol_to_string(ctx, method_sexp);
        let method_ptr = ffi::sexp_string_data(method_str_sexp);
        let method_len = ffi::sexp_string_size(method_str_sexp) as usize;
        let method_name = std::str::from_utf8(
            std::slice::from_raw_parts(method_ptr as *const u8, method_len)
        ).map_err(|_| "foreign-call: invalid utf-8 in method name".to_string())?;

        // collect remaining args as Vec<Value>
        let mut call_args = Vec::new();
        let mut current = args_rest;
        while ffi::sexp_pairp(current) != 0 {
            let arg = ffi::sexp_car(current);
            call_args.push(Value::from_raw(ctx, arg)
                .map_err(|e| format!("foreign-call: argument error: {}", e))?);
            current = ffi::sexp_cdr(current);
        }

        // look up method
        let store_ref = store.borrow();
        let method_fn = store_ref.find_method(type_name, method_name).ok_or_else(|| {
            let available = store_ref.method_names(type_name)
                .map(|names| names.join(", "))
                .unwrap_or_else(|| "none".to_string());
            format!(
                "foreign-call: {} has no method '{}' \u{2014} available: {}",
                type_name, method_name, available
            )
        })?;
        drop(store_ref);

        // call method with mutable access to the object
        let mut store_mut = store.borrow_mut();
        let (data, _) = store_mut.get_mut(handle_id).ok_or_else(|| {
            format!("foreign-call: stale handle {} ({})", handle_id, type_name)
        })?;

        let method_ctx = MethodContext { ctx };
        method_fn(data, &method_ctx, &call_args)
            .map_err(|e| format!("{}.{}: {}", type_name, method_name, e))
    }
}
```

add `use std::cell::RefCell;` to the imports at the top of `foreign.rs`.

**step 2: register protocol functions in context.rs**

add a method to Context for registering the protocol dispatch functions. these are the native functions that back the `(tein foreign)` module's exports:

```rust
    /// register the foreign object protocol dispatch functions.
    ///
    /// called automatically by `register_foreign_type` on first use.
    /// registers: foreign-call, foreign-methods, foreign-types, foreign-type-methods
    fn register_foreign_protocol(&self) -> Result<()> {
        // we use a thread-local to give the extern "C" functions access to the ForeignStore.
        // this is safe because Context is !Send + !Sync.
        // implementation: set the thread-local before evaluate, clear after.
        // for now, register the functions and they'll access via the thread-local.

        self.define_fn_variadic("foreign-call", foreign_call_wrapper)?;
        self.define_fn_variadic("foreign-methods", foreign_methods_wrapper)?;
        self.define_fn_variadic("foreign-types", foreign_types_wrapper)?;
        self.define_fn_variadic("foreign-type-methods", foreign_type_methods_wrapper)?;
        Ok(())
    }
```

the wrapper functions (`foreign_call_wrapper`, etc.) are `unsafe extern "C" fn` with the chibi signature. they access the `ForeignStore` via a thread-local pointer set by Context before each `evaluate`/`call`. this follows the same pattern as `ORIGINAL_PROCS` and `FS_POLICY`.

add the thread-local and wrapper functions:

```rust
thread_local! {
    static FOREIGN_STORE: Cell<*const RefCell<ForeignStore>> = const { Cell::new(std::ptr::null()) };
}

unsafe extern "C" fn foreign_call_wrapper(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let store_ptr = FOREIGN_STORE.with(|c| c.get());
        if store_ptr.is_null() {
            let msg = "foreign-call: no foreign store (internal error)";
            let c_msg = CString::new(msg).unwrap();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let store = &*store_ptr;

        match crate::foreign::dispatch_foreign_call(store, ctx, args) {
            Ok(value) => value.to_raw(ctx).unwrap_or_else(|_| ffi::get_void()),
            Err(msg) => {
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}
```

similarly implement `foreign_methods_wrapper`, `foreign_types_wrapper`, `foreign_type_methods_wrapper` — each reads from `ForeignStore` and returns scheme lists/strings.

**step 3: set/clear thread-local around evaluate and call**

in `Context::evaluate()` and `Context::call()`, wrap the evaluation with:

```rust
FOREIGN_STORE.with(|c| c.set(&self.foreign_store as *const _));
// ... existing evaluation code ...
FOREIGN_STORE.with(|c| c.set(std::ptr::null()));
```

use a scope guard pattern (or ensure the clear happens even on early return) to prevent dangling pointers.

**step 4: update register_foreign_type to call register_foreign_protocol on first use**

add a `has_foreign_protocol: Cell<bool>` field to Context. in `register_foreign_type`, check it and call `register_foreign_protocol()` on first use.

**step 5: verify build**

run: `cargo build -p tein 2>&1 | tail -10`
expected: compiles

**step 6: commit**

```bash
git add tein/src/context.rs tein/src/foreign.rs
git commit -m "foreign: implement dispatch, introspection, and thread-local bridge"
```

---

### task 6: auto-generated convenience procedures

**files:**
- modify: `tein/src/context.rs`

**step 1: generate convenience procs in register_foreign_type**

after registering the type in the store, auto-generate scheme procedures for each method and a predicate. these are defined as scheme lambdas that wrap `foreign-call`:

```rust
    // auto-register convenience procedures
    let type_name = T::type_name();

    // predicate: (type-name? x)
    let pred_code = format!(
        "(define ({tn}? x) (and (foreign? x) (equal? (foreign-type x) \"{tn}\")))",
        tn = type_name
    );
    self.evaluate(&pred_code)?;

    // method wrappers: (type-name-method obj arg ...)
    for (method_name, _) in T::methods() {
        let wrapper_code = format!(
            "(define ({tn}-{mn} obj . args) \
               (if (and (foreign? obj) (equal? (foreign-type obj) \"{tn}\")) \
                   (apply foreign-call obj '{mn} args) \
                   (error \"{tn}-{mn}: expected {tn}, got\" \
                          (if (foreign? obj) (foreign-type obj) obj))))",
            tn = type_name,
            mn = method_name
        );
        self.evaluate(&wrapper_code)?;
    }
```

this is elegant: the convenience procs are pure scheme, defined in terms of the protocol. they provide type-specific error messages because they know the expected type at definition time.

**step 2: verify build**

run: `cargo build -p tein 2>&1 | tail -5`
expected: compiles

**step 3: commit**

```bash
git add tein/src/context.rs
git commit -m "foreign: auto-generate convenience procs and predicates"
```

---

### task 7: tests — basic registration and round-trip

**files:**
- modify: `tein/src/context.rs` (test module)

**step 1: define a test foreign type**

add at the top of the test module:

```rust
    // --- foreign type protocol ---

    use crate::foreign::{ForeignType, MethodFn};

    struct TestCounter {
        n: i64,
    }

    impl ForeignType for TestCounter {
        fn type_name() -> &'static str { "test-counter" }
        fn methods() -> &'static [(&'static str, MethodFn)] {
            &[
                ("increment", |obj, _ctx, _args| {
                    let c = obj.downcast_mut::<TestCounter>().unwrap();
                    c.n += 1;
                    Ok(Value::Integer(c.n))
                }),
                ("get", |obj, _ctx, _args| {
                    let c = obj.downcast_ref::<TestCounter>().unwrap();
                    Ok(Value::Integer(c.n))
                }),
                ("reset", |obj, _ctx, _args| {
                    let c = obj.downcast_mut::<TestCounter>().unwrap();
                    c.n = 0;
                    Ok(Value::Unspecified)
                }),
            ]
        }
    }
```

**step 2: write registration test**

```rust
    #[test]
    fn test_foreign_type_register() {
        let ctx = Context::new_standard().expect("context");
        ctx.register_foreign_type::<TestCounter>().expect("register");
        // verify introspection
        let types = ctx.evaluate("(foreign-types)").expect("foreign-types");
        let list = types.as_list().expect("expected list");
        assert!(list.iter().any(|v| v.as_string() == Some("test-counter")));
    }
```

**step 3: write round-trip test**

```rust
    #[test]
    fn test_foreign_value_roundtrip() {
        let ctx = Context::new_standard().expect("context");
        ctx.register_foreign_type::<TestCounter>().expect("register");

        let val = ctx.foreign_value(TestCounter { n: 42 }).expect("create");
        assert!(val.is_foreign());
        assert_eq!(val.foreign_type_name(), Some("test-counter"));

        // pass to scheme and back
        // define a variable holding the foreign value, then return it
        let result = ctx.evaluate("(begin (define tc (make-dummy)) tc)");
        // actually — we need a constructor. let's use define_fn_variadic.
        // simpler: test to_raw → from_raw directly
        let raw = unsafe { val.to_raw(ctx.ctx_ptr()).unwrap() };
        let back = unsafe { Value::from_raw(ctx.ctx_ptr(), raw).unwrap() };
        assert_eq!(val, back);
    }
```

note: `ctx_ptr()` needs to be a pub(crate) accessor on Context for the raw ctx pointer. add if not present:

```rust
    /// raw context pointer for internal use
    pub(crate) fn ctx_ptr(&self) -> ffi::sexp {
        self.ctx
    }
```

**step 4: run tests**

run: `cargo test -p tein test_foreign -- --nocapture 2>&1 | tail -20`
expected: pass

**step 5: commit**

```bash
git add tein/src/context.rs
git commit -m "test: foreign type registration and value round-trip"
```

---

### task 8: tests — dispatch and error messages

**files:**
- modify: `tein/src/context.rs` (test module)

**step 1: add a constructor for TestCounter**

register a constructor function so scheme can create TestCounter instances:

```rust
    /// helper: register TestCounter with a scheme-callable constructor
    fn setup_test_counter(ctx: &Context) {
        ctx.register_foreign_type::<TestCounter>().expect("register");

        // register constructor using define_fn_variadic
        unsafe extern "C" fn make_test_counter(
            ctx_ptr: ffi::sexp,
            _self: ffi::sexp,
            _n: ffi::sexp_sint_t,
            _args: ffi::sexp,
        ) -> ffi::sexp {
            // access ForeignStore via thread-local (set by evaluate/call)
            // create counter with n=0, wrap as foreign value
            unsafe {
                let store_ptr = FOREIGN_STORE.with(|c| c.get());
                if store_ptr.is_null() {
                    let msg = "make-test-counter: no store";
                    let c_msg = CString::new(msg).unwrap();
                    return ffi::make_error(ctx_ptr, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
                }
                let store = &*store_ptr;
                let id = store.borrow_mut().insert(TestCounter { n: 0 });
                let val = Value::Foreign {
                    handle_id: id,
                    type_name: "test-counter".to_string(),
                };
                val.to_raw(ctx_ptr).unwrap_or_else(|_| ffi::get_void())
            }
        }

        ctx.define_fn_variadic("make-test-counter", make_test_counter)
            .expect("define constructor");
    }
```

note: this is test infrastructure. real users would do something similar. the exact pattern for constructors is one of our "open questions" from the design — we'll refine the ergonomics later.

**step 2: write dispatch tests**

```rust
    #[test]
    fn test_foreign_call_dispatch() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx.evaluate("
            (let ((c (make-test-counter)))
              (test-counter-increment c)
              (test-counter-increment c)
              (test-counter-get c))
        ").expect("dispatch");
        assert_eq!(result, Value::Integer(2));
    }

    #[test]
    fn test_foreign_call_universal_dispatch() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx.evaluate("
            (let ((c (make-test-counter)))
              (foreign-call c 'increment)
              (foreign-call c 'get))
        ").expect("foreign-call dispatch");
        assert_eq!(result, Value::Integer(1));
    }

    #[test]
    fn test_foreign_call_mutable_state() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx.evaluate("
            (let ((c (make-test-counter)))
              (test-counter-increment c)
              (test-counter-increment c)
              (test-counter-reset c)
              (test-counter-get c))
        ").expect("mutable state");
        assert_eq!(result, Value::Integer(0));
    }
```

**step 3: write error message tests**

```rust
    #[test]
    fn test_foreign_call_wrong_method() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let err = ctx.evaluate("
            (let ((c (make-test-counter)))
              (foreign-call c 'nonexistent))
        ").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("no method 'nonexistent'"), "got: {}", msg);
        assert!(msg.contains("increment"), "should list available methods, got: {}", msg);
    }

    #[test]
    fn test_foreign_call_not_foreign() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let err = ctx.evaluate("(foreign-call 42 'get)").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("expected foreign object"), "got: {}", msg);
    }

    #[test]
    fn test_foreign_convenience_wrong_type() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let err = ctx.evaluate("(test-counter-get 42)").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("expected test-counter"), "got: {}", msg);
    }
```

**step 4: run tests**

run: `cargo test -p tein test_foreign -- --nocapture 2>&1 | tail -30`
expected: all pass

**step 5: commit**

```bash
git add tein/src/context.rs
git commit -m "test: foreign dispatch, mutable state, and error messages"
```

---

### task 9: tests — introspection and predicates

**files:**
- modify: `tein/src/context.rs` (test module)

**step 1: write introspection tests**

```rust
    #[test]
    fn test_foreign_introspection_methods() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx.evaluate("
            (let ((c (make-test-counter)))
              (foreign-methods c))
        ").expect("foreign-methods");
        let methods = result.as_list().expect("expected list");
        let names: Vec<&str> = methods.iter()
            .filter_map(|v| v.as_symbol().or_else(|| v.as_string()))
            .collect();
        assert!(names.contains(&"increment"), "got: {:?}", names);
        assert!(names.contains(&"get"), "got: {:?}", names);
        assert!(names.contains(&"reset"), "got: {:?}", names);
    }

    #[test]
    fn test_foreign_introspection_type_methods() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx.evaluate("(foreign-type-methods \"test-counter\")")
            .expect("foreign-type-methods");
        let methods = result.as_list().expect("expected list");
        assert_eq!(methods.len(), 3);
    }

    #[test]
    fn test_foreign_predicate() {
        let ctx = Context::new_standard().expect("context");
        setup_test_counter(&ctx);

        let result = ctx.evaluate("
            (let ((c (make-test-counter)))
              (test-counter? c))
        ").expect("predicate");
        assert_eq!(result, Value::Boolean(true));

        let result = ctx.evaluate("(test-counter? 42)").expect("predicate false");
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_foreign_display() {
        let val = Value::Foreign {
            handle_id: 7,
            type_name: "http-client".to_string(),
        };
        assert_eq!(format!("{}", val), "#<foreign http-client:7>");
    }
```

**step 2: run tests**

run: `cargo test -p tein test_foreign -- --nocapture 2>&1 | tail -30`
expected: all pass

**step 3: commit**

```bash
git add tein/src/context.rs
git commit -m "test: foreign introspection, predicates, and display"
```

---

### task 10: tests — sandbox integration and cleanup

**files:**
- modify: `tein/src/context.rs` (test module)

**step 1: write sandbox integration test**

```rust
    #[test]
    fn test_foreign_in_sandbox() {
        use crate::sandbox::*;
        let ctx = Context::builder()
            .standard_env()
            .preset(&ARITHMETIC)
            .preset(&LISTS)
            .allow(&["import"])
            .build()
            .expect("sandboxed context");

        ctx.register_foreign_type::<TestCounter>().expect("register");
        // define constructor (same pattern as setup_test_counter but inline)
        // ... or call setup_test_counter if FOREIGN_STORE thread-local is compatible

        // verify foreign protocol works in sandboxed env
        let result = ctx.evaluate("
            (let ((c (make-test-counter)))
              (test-counter-increment c)
              (test-counter-get c))
        ").expect("sandboxed foreign call");
        assert_eq!(result, Value::Integer(1));
    }
```

**step 2: write cleanup test**

```rust
    #[test]
    fn test_foreign_cleanup_on_drop() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        // use a type that signals when dropped
        struct Canary(Arc<AtomicBool>);
        impl Drop for Canary {
            fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
        }
        impl ForeignType for Canary {
            fn type_name() -> &'static str { "canary" }
            fn methods() -> &'static [(&'static str, MethodFn)] { &[] }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        {
            let ctx = Context::new_standard().expect("context");
            ctx.register_foreign_type::<Canary>().expect("register");
            let _val = ctx.foreign_value(Canary(dropped.clone())).expect("create");
            assert!(!dropped.load(Ordering::SeqCst), "should not be dropped yet");
        }
        // Context dropped — ForeignStore dropped — Canary dropped
        assert!(dropped.load(Ordering::SeqCst), "canary should be dropped");
    }
```

**step 3: run full test suite**

run: `cargo test -p tein 2>&1 | tail -20`
expected: all pass

run: `cargo clippy -p tein 2>&1 | tail -10`
expected: no errors

**step 4: commit**

```bash
git add tein/src/context.rs
git commit -m "test: foreign sandbox integration and cleanup-on-drop"
```

---

### task 11: example and documentation

**files:**
- create: `tein/examples/foreign_types.rs`
- modify: `AGENTS.md` (architecture section)
- modify: `TODO.md` (roadmap)
- modify: `DEVELOPMENT.md`

**step 1: write the example**

create `tein/examples/foreign_types.rs` — a Counter demo showing the full lifecycle: registration, construction, method calls, introspection, error handling. follow the pattern from `examples/ffi.rs`.

**step 2: verify example runs**

run: `cargo run -p tein --example foreign_types 2>&1`
expected: clean output showing counter operations and introspection

**step 3: update AGENTS.md**

update the architecture section to mention `foreign.rs` and the `(tein foreign)` module. update the Value enum variant list to include `Foreign`. add a brief description of the foreign type protocol data flow.

**step 4: update TODO.md**

add a new milestone entry and check it off:

```markdown
- [x] **foreign type protocol**
  - `ForeignType` trait + `ForeignStore` handle map
  - `(tein foreign)` VFS module with record type and protocol
  - auto-generated convenience procs and predicates
  - `Value::Foreign` variant with tagged-list wire format
  - runtime introspection: foreign-types, foreign-methods
  - LLM-friendly error messages
```

**step 5: update DEVELOPMENT.md**

add a section describing the foreign type protocol architecture.

**step 6: run full test suite + clippy + fmt**

run: `cargo test -p tein 2>&1 | tail -5`
run: `cargo clippy -p tein 2>&1 | tail -5`
run: `cargo fmt -p tein --check 2>&1`
expected: all clean

**step 7: commit**

```bash
git add tein/examples/foreign_types.rs AGENTS.md TODO.md DEVELOPMENT.md
git commit -m "docs: foreign type protocol example, architecture docs, roadmap update"
```
