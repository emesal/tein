//! Foreign type protocol — typed Rust objects accessible from Scheme.
//!
//! Enables Rust types to be exposed as first-class Scheme objects with
//! method dispatch, introspection, and clear error messages.
//!
//! # Architecture
//!
//! Foreign objects are stored in a per-context `ForeignStore` keyed by
//! unpredictable `u64` handle IDs generated via xorshift64 seeded from
//! `SystemTime`. Scheme sees them as tagged lists
//! `(__tein-foreign "type-name" handle-id)`. The actual data lives
//! Rust-side — Scheme never touches it directly.
//!
//! This Rust-side storage design is also **safety-critical**: it bypasses
//! chibi's C-level `sexp_register_type` + finaliser system, which has
//! known GC bugs (resurrection → use-after-free, re-entrant GC from
//! allocating finalisers, unordered finalisation of referenced objects).
//! See findings M19-M21 in `docs/plans/2026-02-25-chibi-scheme-review.md`.
//! Do NOT migrate to chibi-native type registration without first fixing
//! the upstream GC finaliser model.
//!
//! Unpredictable IDs prevent a Scheme program from enumerating sequential
//! values to access foreign objects it doesn't hold a reference to.
//!
//! # Dispatch chain
//!
//! When Scheme calls e.g. `(counter-get obj)`:
//!
//! 1. auto-generated convenience proc calls `(apply foreign-call obj 'get args)`
//! 2. `foreign-call` (native fn) reads `FOREIGN_STORE_PTR` from thread-local storage
//! 3. `dispatch_foreign_call` extracts handle ID, looks up type + method
//! 4. method's [`MethodFn`] is called with `&mut dyn Any` + [`MethodContext`] + args
//! 5. returned [`crate::Value`] is converted back to a scheme sexp
//!
//! `FOREIGN_STORE_PTR` is set by [`crate::Context::evaluate()`] /
//! [`crate::Context::call()`] via an RAII guard and cleared on return,
//! so foreign dispatch is only active during evaluation.
//!
//! # Complete example
//!
//! ```
//! use tein::{Context, ForeignType, MethodFn, Value};
//!
//! struct Counter { n: i64 }
//!
//! impl ForeignType for Counter {
//!     fn type_name() -> &'static str { "counter" }
//!     fn methods() -> &'static [(&'static str, MethodFn)] {
//!         &[
//!             ("increment", |obj, _ctx, _args| {
//!                 let c = obj.downcast_mut::<Counter>().unwrap();
//!                 c.n += 1;
//!                 Ok(Value::Integer(c.n))
//!             }),
//!             ("get", |obj, _ctx, _args| {
//!                 let c = obj.downcast_ref::<Counter>().unwrap();
//!                 Ok(Value::Integer(c.n))
//!             }),
//!         ]
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let ctx = Context::new_standard()?;
//!
//! // register type — auto-generates counter?, counter-increment, counter-get
//! ctx.register_foreign_type::<Counter>()?;
//!
//! // create a foreign value and call methods via ctx.call
//! let c = ctx.foreign_value(Counter { n: 0 })?;
//! let inc = ctx.evaluate("counter-increment")?;
//! let get = ctx.evaluate("counter-get")?;
//!
//! ctx.call(&inc, std::slice::from_ref(&c))?;
//! ctx.call(&inc, std::slice::from_ref(&c))?;
//! let result = ctx.call(&get, std::slice::from_ref(&c))?;
//! assert_eq!(result, Value::Integer(2));
//! # Ok(())
//! # }
//! ```

use crate::Value;
use crate::error::{Error, Result};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ── ext-type support ─────────────────────────────────────────────────────────

/// A method entry for dynamically registered ext types.
///
/// Unlike [`MethodFn`] (which takes `&mut dyn Any` through rust dispatch),
/// ext methods take `*mut c_void` and the API table — operating at the C ABI
/// level so no rust types need to cross the `.so` boundary.
pub(crate) struct ExtMethodEntry {
    /// scheme method name (e.g. `"get"`, `"increment"`)
    pub name: String,
    /// the method function pointer (matches `TeinMethodFn`)
    pub func: tein_ext::TeinMethodFn,
    /// whether the method requires mutable access (`&mut self` in the ext)
    pub is_mut: bool,
}

/// Entry for a dynamically registered ext type.
struct ExtTypeEntry {
    methods: Vec<ExtMethodEntry>,
}

/// Result of a method lookup — either a static (rust-native) or ext (cdylib) method.
pub(crate) enum MethodLookup {
    /// static rust method via MethodFn
    Static(MethodFn),
    /// dynamic ext method via TeinMethodFn
    Ext {
        func: tein_ext::TeinMethodFn,
        #[allow(dead_code)] // reserved for future read-only dispatch optimisation
        is_mut: bool,
    },
}

thread_local! {
    /// xorshift64 state for unpredictable handle ID generation.
    /// seeded from SystemTime on first use to prevent sequential ID guessing.
    static XOR_STATE: Cell<u64> = const { Cell::new(0) };
}

/// Generate the next unpredictable handle ID via xorshift64.
///
/// On first call the state is seeded from `SystemTime` (or a fixed fallback).
/// IDs are never 0 — if the PRNG produces 0, a fixed non-zero value is used.
fn next_handle_id() -> u64 {
    XOR_STATE.with(|state| {
        let mut s = state.get();
        if s == 0 {
            s = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xdead_beef_cafe_f00d);
            if s == 0 {
                s = 0xdead_beef_cafe_f00d;
            }
        }
        // xorshift64 — state must never be 0
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        if s == 0 {
            s = 1;
        }
        state.set(s);
        // mask to chibi fixnum range: SEXP_MAX_FIXNUM = 2^62 - 1 on 64-bit
        // (SEXP_FIXNUM_BITS = 1, so max positive fixnum is i64::MAX >> 1).
        // handle IDs travel through scheme as fixnum literals; values outside
        // this range would corrupt the encoding. ensure non-zero after masking.
        let id = s & (i64::MAX as u64 >> 1);
        if id == 0 { 1 } else { id }
    })
}

/// A method on a foreign type, callable from Scheme.
///
/// Receives a mutable reference to the object (downcast inside the body),
/// a limited context for creating return values, and the remaining arguments.
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

/// Limited context passed to foreign methods.
///
/// Provides only what methods need: creating foreign values and
/// evaluating Scheme. Prevents methods from accessing ForeignStore
/// internals directly (which would cause borrow conflicts).
pub struct MethodContext {
    /// raw chibi context pointer — for Value::to_raw/from_raw
    #[allow(dead_code)]
    pub(crate) ctx: crate::ffi::sexp,
}

/// A Rust type that can be exposed to Scheme as a foreign object.
///
/// Implement this on your types, then register with
/// `ctx.register_foreign_type::<T>()`. The type name appears in
/// Scheme as the tagged list's type-name field and in error messages.
///
/// # Naming
///
/// `type_name()` should be a kebab-case identifier suitable for
/// Scheme: `"http-client"`, `"counter"`, `"file-writer"`.
/// Auto-generated convenience procs use this name:
/// `http-client?`, `http-client-get`, etc.
pub trait ForeignType: Any + 'static {
    /// Scheme-visible type name (kebab-case).
    fn type_name() -> &'static str;

    /// Method table — maps Scheme symbol names to handler functions.
    fn methods() -> &'static [(&'static str, MethodFn)];
}

/// A live foreign object instance.
struct ForeignObject {
    /// The actual Rust value.
    data: Box<dyn Any>,
    /// Type name (from ForeignType::type_name).
    type_name: &'static str,
}

/// Type registration entry.
struct TypeEntry {
    /// Method table for this type.
    methods: &'static [(&'static str, MethodFn)],
}

/// Per-context storage for foreign type registrations and live instances.
///
/// Lives inside `Context` as `RefCell<ForeignStore>`. Drops all instances
/// when the context drops — no GC integration needed.
///
/// Supports two registration paths:
/// - **static** (`types`): rust types implementing `ForeignType`, registered via
///   [`register_type`](ForeignStore::register_type). Method dispatch goes through `MethodFn`.
/// - **ext** (`ext_types`): types registered across the cdylib boundary via `TeinTypeDesc`.
///   Method dispatch calls `TeinMethodFn` with `*mut c_void` — no rust trait needed.
pub(crate) struct ForeignStore {
    /// registered static rust types: name → method table
    types: HashMap<&'static str, TypeEntry>,
    /// registered ext types: name → ext method table
    ext_types: HashMap<String, ExtTypeEntry>,
    /// live instances: handle ID → object
    instances: HashMap<u64, ForeignObject>,
}

impl ForeignStore {
    /// create an empty store
    pub(crate) fn new() -> Self {
        Self {
            types: HashMap::new(),
            ext_types: HashMap::new(),
            instances: HashMap::new(),
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
        self.types.insert(
            name,
            TypeEntry {
                methods: T::methods(),
            },
        );
        Ok(())
    }

    /// store a value and return its handle ID
    pub(crate) fn insert<T: ForeignType>(&mut self, value: T) -> u64 {
        let id = next_handle_id();
        self.instances.insert(
            id,
            ForeignObject {
                data: Box::new(value),
                type_name: T::type_name(),
            },
        );
        id
    }

    /// look up an instance by handle ID
    pub(crate) fn get(&self, id: u64) -> Option<(&dyn Any, &'static str)> {
        self.instances
            .get(&id)
            .map(|obj| (obj.data.as_ref(), obj.type_name))
    }

    /// look up an instance mutably by handle ID
    pub(crate) fn get_mut(&mut self, id: u64) -> Option<(&mut dyn Any, &'static str)> {
        self.instances
            .get_mut(&id)
            .map(|obj| (obj.data.as_mut(), obj.type_name))
    }

    /// look up a method by type name and method name (static types only).
    #[allow(dead_code)]
    pub(crate) fn find_method(&self, type_name: &str, method_name: &str) -> Option<MethodFn> {
        self.types.get(type_name).and_then(|entry| {
            entry
                .methods
                .iter()
                .find(|(name, _)| *name == method_name)
                .map(|(_, f)| *f)
        })
    }

    /// look up a method by type name and method name, checking both static and ext types.
    pub(crate) fn find_method_any(
        &self,
        type_name: &str,
        method_name: &str,
    ) -> Option<MethodLookup> {
        // check static types first
        if let Some(entry) = self.types.get(type_name) {
            for (name, func) in entry.methods {
                if *name == method_name {
                    return Some(MethodLookup::Static(*func));
                }
            }
        }
        // check ext types
        if let Some(entry) = self.ext_types.get(type_name) {
            for m in &entry.methods {
                if m.name == method_name {
                    return Some(MethodLookup::Ext {
                        func: m.func,
                        is_mut: m.is_mut,
                    });
                }
            }
        }
        None
    }

    /// register a dynamically-defined ext type with its method table.
    ///
    /// returns `Err` if the type name is already registered (static or ext).
    pub(crate) fn register_ext_type(
        &mut self,
        type_name: String,
        methods: Vec<ExtMethodEntry>,
    ) -> Result<()> {
        if self.types.contains_key(type_name.as_str()) || self.ext_types.contains_key(&type_name) {
            return Err(Error::EvalError(format!(
                "foreign type '{}' already registered",
                type_name
            )));
        }
        self.ext_types.insert(type_name, ExtTypeEntry { methods });
        Ok(())
    }

    /// list method names for a type (static types only).
    pub(crate) fn method_names(&self, type_name: &str) -> Option<Vec<&'static str>> {
        self.types
            .get(type_name)
            .map(|entry| entry.methods.iter().map(|(name, _)| *name).collect())
    }

    /// list method names for an ext type.
    pub(crate) fn ext_method_names(&self, type_name: &str) -> Option<Vec<&str>> {
        self.ext_types
            .get(type_name)
            .map(|entry| entry.methods.iter().map(|m| m.name.as_str()).collect())
    }

    /// list all registered type names (static + ext).
    pub(crate) fn type_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.types.keys().map(|s| s.to_string()).collect();
        names.extend(self.ext_types.keys().cloned());
        names
    }

    /// check if a type is registered
    #[allow(dead_code)]
    pub(crate) fn has_type(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }
}

/// Dispatch a method call on a foreign object.
///
/// Called from the `foreign-call` native function. args is the raw sexp argument list:
/// `(obj method-symbol arg1 arg2 ...)`
pub(crate) unsafe fn dispatch_foreign_call(
    store: &RefCell<ForeignStore>,
    ctx: crate::ffi::sexp,
    args: crate::ffi::sexp,
) -> std::result::Result<Value, String> {
    use crate::ffi;
    unsafe {
        // extract obj (first arg)
        if ffi::sexp_nullp(args) != 0 {
            return Err("foreign-call: expected at least 2 arguments, got 0".to_string());
        }
        let obj_sexp = ffi::sexp_car(args);
        let rest = ffi::sexp_cdr(args);

        // convert obj to Value to extract Foreign fields
        let obj_value =
            Value::from_raw(ctx, obj_sexp).map_err(|e| format!("foreign-call: {}", e))?;

        let (handle_id, type_name) = obj_value
            .as_foreign()
            .ok_or_else(|| format!("foreign-call: expected foreign object, got {}", obj_value))?;

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
        let method_name = std::str::from_utf8(std::slice::from_raw_parts(
            method_ptr as *const u8,
            method_len,
        ))
        .map_err(|_| "foreign-call: invalid utf-8 in method name".to_string())?;

        // look up method — check both static and ext types
        // (drop store_ref before any borrow_mut below)
        let method_lookup = {
            let store_ref = store.borrow();
            store_ref
                .find_method_any(type_name, method_name)
                .ok_or_else(|| {
                    // build available-methods error message from both tables
                    let mut available: Vec<String> = store_ref
                        .method_names(type_name)
                        .map(|ns| ns.iter().map(|n| n.to_string()).collect())
                        .or_else(|| {
                            store_ref
                                .ext_method_names(type_name)
                                .map(|ns| ns.iter().map(|n| n.to_string()).collect())
                        })
                        .unwrap_or_default();
                    available.sort();
                    format!(
                        "foreign-call: {} has no method '{}' \u{2014} available: {}",
                        type_name,
                        method_name,
                        if available.is_empty() {
                            "none".to_string()
                        } else {
                            available.join(", ")
                        }
                    )
                })?
        };

        match method_lookup {
            MethodLookup::Static(method_fn) => {
                // collect remaining args as Vec<Value>
                let mut call_args = Vec::new();
                let mut current = args_rest;
                while ffi::sexp_pairp(current) != 0 {
                    let arg = ffi::sexp_car(current);
                    call_args.push(
                        Value::from_raw(ctx, arg)
                            .map_err(|e| format!("foreign-call: argument error: {}", e))?,
                    );
                    current = ffi::sexp_cdr(current);
                }

                // call method with mutable access to the object
                let mut store_mut = store.borrow_mut();
                let (data, _) = store_mut.get_mut(handle_id).ok_or_else(|| {
                    format!("foreign-call: stale handle {} ({})", handle_id, type_name)
                })?;

                let method_ctx = MethodContext { ctx };
                method_fn(data, &method_ctx, &call_args)
                    .map_err(|e| format!("{}.{}: {}", type_name, method_name, e))
            }
            MethodLookup::Ext { func, .. } => {
                // ext methods receive raw sexp args — no Value conversion needed.
                // they use the API table to extract types themselves.
                let mut store_mut = store.borrow_mut();
                let (data, _) = store_mut.get_mut(handle_id).ok_or_else(|| {
                    format!("foreign-call: stale handle {} ({})", handle_id, type_name)
                })?;

                // build_ext_api() is in context.rs — access it via a thread-local
                let api = crate::context::EXT_API.with(|cell| cell.get());
                if api.is_null() {
                    return Err(
                        "foreign-call: ext API not available (not in load_extension context)"
                            .to_string(),
                    );
                }

                // count args for n parameter
                let mut n: std::ffi::c_long = 0;
                let mut cur = args_rest;
                while ffi::sexp_pairp(cur) != 0 {
                    n += 1;
                    cur = ffi::sexp_cdr(cur);
                }

                let result = func(
                    data as *mut dyn Any as *mut std::ffi::c_void,
                    ctx as *mut tein_ext::OpaqueCtx,
                    api,
                    n,
                    args_rest as *mut tein_ext::OpaqueVal,
                );

                Value::from_raw(ctx, result as ffi::sexp)
                    .map_err(|e| format!("{}.{}: {}", type_name, method_name, e))
            }
        }
    }
}
