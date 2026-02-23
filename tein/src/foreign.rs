//! foreign type protocol — typed rust objects accessible from scheme
//!
//! enables rust types to be exposed as first-class scheme objects with
//! method dispatch, introspection, and clear error messages.
//!
//! # architecture
//!
//! foreign objects are stored in a per-context `ForeignStore` keyed by
//! monotonically increasing `u64` handle IDs. scheme sees them as
//! tagged lists `(__tein-foreign "type-name" handle-id)`. the actual data
//! lives rust-side — scheme never touches it directly.
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

use crate::Value;
use crate::error::{Error, Result};
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;

/// a method on a foreign type, callable from scheme.
///
/// receives a mutable reference to the object (downcast inside the body),
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

/// limited context passed to foreign methods.
///
/// provides only what methods need: creating foreign values and
/// evaluating scheme. prevents methods from accessing ForeignStore
/// internals directly (which would cause borrow conflicts).
pub struct MethodContext {
    /// raw chibi context pointer — for Value::to_raw/from_raw
    #[allow(dead_code)]
    pub(crate) ctx: crate::ffi::sexp,
}

/// a rust type that can be exposed to scheme as a foreign object.
///
/// implement this on your types, then register with
/// `ctx.register_foreign_type::<T>()`. the type name appears in
/// scheme as the tagged list's type-name field and in error messages.
///
/// # naming
///
/// `type_name()` should be a kebab-case identifier suitable for
/// scheme: `"http-client"`, `"counter"`, `"file-writer"`.
/// auto-generated convenience procs use this name:
/// `http-client?`, `http-client-get`, etc.
pub trait ForeignType: Any + 'static {
    /// scheme-visible type name (kebab-case)
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
    /// next handle ID (monotonically increasing, starts at 1)
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
        let id = self.next_id;
        self.next_id += 1;
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

    /// look up a method by type name and method name
    pub(crate) fn find_method(&self, type_name: &str, method_name: &str) -> Option<MethodFn> {
        self.types.get(type_name).and_then(|entry| {
            entry
                .methods
                .iter()
                .find(|(name, _)| *name == method_name)
                .map(|(_, f)| *f)
        })
    }

    /// list method names for a type
    pub(crate) fn method_names(&self, type_name: &str) -> Option<Vec<&'static str>> {
        self.types
            .get(type_name)
            .map(|entry| entry.methods.iter().map(|(name, _)| *name).collect())
    }

    /// list all registered type names
    pub(crate) fn type_names(&self) -> Vec<&'static str> {
        self.types.keys().copied().collect()
    }

    /// check if a type is registered
    #[allow(dead_code)]
    pub(crate) fn has_type(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }
}

/// dispatch a method call on a foreign object.
///
/// called from the `foreign-call` native function. args is the raw sexp argument list:
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

        // look up method (drop store_ref before borrow_mut below)
        let method_fn = {
            let store_ref = store.borrow();
            store_ref
                .find_method(type_name, method_name)
                .ok_or_else(|| {
                    let available = store_ref
                        .method_names(type_name)
                        .map(|names| names.join(", "))
                        .unwrap_or_else(|| "none".to_string());
                    format!(
                        "foreign-call: {} has no method '{}' \u{2014} available: {}",
                        type_name, method_name, available
                    )
                })?
        };

        // call method with mutable access to the object
        let mut store_mut = store.borrow_mut();
        let (data, _) = store_mut
            .get_mut(handle_id)
            .ok_or_else(|| format!("foreign-call: stale handle {} ({})", handle_id, type_name))?;

        let method_ctx = MethodContext { ctx };
        method_fn(data, &method_ctx, &call_args)
            .map_err(|e| format!("{}.{}: {}", type_name, method_name, e))
    }
}
