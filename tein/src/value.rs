//! scheme value representation

use crate::{
    error::{Error, Result},
    ffi,
};
use std::fmt;
use std::os::raw::c_int;

/// maximum nesting depth for recursive value conversion.
/// prevents stack overflow on deeply nested or (theoretically) circular structures.
const MAX_DEPTH: usize = 10_000;

// gc safety:
//
// chibi's conservative stack scanning is DISABLED in our build (no boehm,
// SEXP_USE_CONSERVATIVE_SCAN=0). the GC does NOT see rust locals — only
// objects reachable from the context's heap roots are safe from collection.
//
// any `sexp` held as a rust local across an allocation point (a call into
// chibi that may trigger GC) must be rooted via `ffi::GcRoot`. GcRoot is
// an RAII guard that calls sexp_preserve_object/sexp_release_object, so
// early returns and panics are handled automatically.
//
// allocation points in this module:
//   - to_raw_depth: sexp_make_flonum, sexp_c_str, sexp_intern,
//     sexp_cons, sexp_make_vector, sexp_make_bytes, recursive to_raw_depth calls
//   - from_raw_depth: sexp_symbol_to_string, recursive from_raw_depth
//     calls (which may hit sexp_symbol_to_string)
//
// safe (non-allocating) calls:
//   - type predicates (sexp_integerp, sexp_flonump, etc.)
//   - value extractors (sexp_unbox_fixnum, sexp_flonum_value,
//     sexp_string_data, sexp_car, sexp_cdr, sexp_vector_data)
//   - immediate constructors (sexp_make_fixnum, sexp_make_boolean,
//     get_null, get_void, get_true, get_false)
//   - sexp_vector_set (writes to existing vector, no allocation)
//
// note on structural sharing:
// scheme objects can form DAGs (e.g. (make-vector 2 x) shares x in both slots).
// from_raw traverses the full tree, visiting shared substructures multiple times.
// this is fine for typical values but exponential for deeply nested shared structures.
// a visited-set could fix this but would require representing sharing in Value.

/// a scheme value
///
/// represents the result of evaluating scheme code.
/// this is a safe wrapper around chibi's internal sexp type.
///
/// most variants own their data, but `Procedure` holds a raw sexp pointer
/// that is only valid within the originating `Context` (enforced by !Send/!Sync).
#[derive(Debug, Clone)]
pub enum Value {
    /// integer value
    Integer(i64),

    /// floating point value
    Float(f64),

    /// string value
    String(String),

    /// symbol value
    Symbol(String),

    /// boolean value
    Boolean(bool),

    /// proper list (converted to vec)
    List(Vec<Value>),

    /// pair (improper list or dotted pair)
    Pair(Box<Value>, Box<Value>),

    /// vector (scheme `#(...)`)
    Vector(Vec<Value>),

    /// character value (unicode scalar value)
    Char(char),

    /// bytevector (scheme `#u8(...)`)
    Bytevector(Vec<u8>),

    /// an opaque input or output port
    ///
    /// holds a raw sexp pointer — only valid within the originating Context.
    Port(ffi::sexp),

    /// an opaque hash table (srfi-69)
    ///
    /// holds a raw sexp pointer — only valid within the originating Context.
    HashTable(ffi::sexp),

    /// nil/empty list
    Nil,

    /// unspecified value (like void in c)
    Unspecified,

    /// a callable scheme procedure or opcode (builtin like `+`)
    ///
    /// holds a raw sexp pointer — only valid within the originating Context.
    /// thread safety is enforced by Context being !Send + !Sync.
    Procedure(ffi::sexp),

    /// other scheme values we don't yet handle
    Other(String),

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
}

impl Value {
    /// convert from raw chibi sexp to safe value
    ///
    /// # safety
    /// ctx and raw must be valid pointers from chibi-scheme
    pub(crate) unsafe fn from_raw(ctx: ffi::sexp, raw: ffi::sexp) -> Result<Self> {
        unsafe { Self::from_raw_depth(ctx, raw, 0) }
    }

    /// recursive inner conversion with depth tracking to prevent stack overflow
    unsafe fn from_raw_depth(ctx: ffi::sexp, raw: ffi::sexp, depth: usize) -> Result<Self> {
        if depth > MAX_DEPTH {
            return Err(Error::EvalError(
                "value nesting depth exceeded maximum".to_string(),
            ));
        }

        unsafe {
            if ffi::sexp_exceptionp(raw) != 0 {
                return Err(Self::extract_exception_error(ctx, raw));
            }

            // check floats before integers because sexp_integerp matches some floats
            if ffi::sexp_flonump(raw) != 0 {
                let f = ffi::sexp_flonum_value(raw);
                return Ok(Value::Float(f));
            }

            if ffi::sexp_integerp(raw) != 0 {
                let i = ffi::sexp_unbox_fixnum(raw);
                return Ok(Value::Integer(i as i64));
            }

            if ffi::sexp_booleanp(raw) != 0 {
                let true_val = ffi::get_true();
                return Ok(Value::Boolean(raw == true_val));
            }

            if ffi::sexp_charp(raw) != 0 {
                let code = ffi::sexp_unbox_character(raw) as u32;
                let c = char::from_u32(code).ok_or_else(|| {
                    Error::TypeError(format!("invalid unicode codepoint: {:#x}", code))
                })?;
                return Ok(Value::Char(c));
            }

            if ffi::sexp_nullp(raw) != 0 {
                return Ok(Value::Nil);
            }

            if raw == ffi::get_void() {
                return Ok(Value::Unspecified);
            }

            if ffi::sexp_stringp(raw) != 0 {
                let str_ptr = ffi::sexp_string_data(raw);
                let str_len = ffi::sexp_string_size(raw);
                let bytes = std::slice::from_raw_parts(str_ptr as *const u8, str_len as usize);
                let s = String::from_utf8(bytes.to_vec())?;
                return Ok(Value::String(s));
            }

            if ffi::sexp_bytesp(raw) != 0 {
                let data = ffi::sexp_bytes_data(raw);
                let len = ffi::sexp_bytes_length(raw) as usize;
                let bytes = std::slice::from_raw_parts(data as *const u8, len).to_vec();
                return Ok(Value::Bytevector(bytes));
            }

            if ffi::sexp_symbolp(raw) != 0 {
                let str_sexp = ffi::sexp_symbol_to_string(ctx, raw);
                let str_ptr = ffi::sexp_string_data(str_sexp);
                let str_len = ffi::sexp_string_size(str_sexp);
                let bytes = std::slice::from_raw_parts(str_ptr as *const u8, str_len as usize);
                let s = String::from_utf8(bytes.to_vec())?;
                return Ok(Value::Symbol(s));
            }

            // check applicablep (procedures + builtins like +) before pairs/lists
            if ffi::sexp_applicablep(raw) != 0 {
                return Ok(Value::Procedure(raw));
            }

            if ffi::sexp_vectorp(raw) != 0 {
                let len = ffi::sexp_vector_length(raw) as usize;
                let data = ffi::sexp_vector_data(raw);
                // root the vector — recursive conversion may allocate
                // (e.g. sexp_symbol_to_string for symbol elements)
                let _vec = ffi::GcRoot::new(ctx, raw);
                let mut items = Vec::with_capacity(len);
                for i in 0..len {
                    let elem = *data.add(i);
                    items.push(Value::from_raw_depth(ctx, elem, depth + 1)?);
                }
                return Ok(Value::Vector(items));
            }

            if ffi::sexp_portp(raw) != 0 {
                return Ok(Value::Port(raw));
            }

            // check for foreign object tagged list: (__tein-foreign "type-name" handle-id)
            // must run before generic pair/list handling below
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
                                    let type_name = String::from_utf8(name_bytes.to_vec())?;
                                    let handle_id = ffi::sexp_unbox_fixnum(id_sexp) as u64;
                                    return Ok(Value::Foreign { handle_id, type_name });
                                }
                            }
                        }
                    }
                }
            }

            if ffi::sexp_pairp(raw) != 0 {
                if Self::is_proper_list(raw) {
                    let mut items = Vec::new();
                    let mut current = raw;

                    while ffi::sexp_pairp(current) != 0 {
                        // root current pair — recursive conversion may allocate
                        let _pair = ffi::GcRoot::new(ctx, current);
                        let car = ffi::sexp_car(current);
                        items.push(Value::from_raw_depth(ctx, car, depth + 1)?);
                        current = ffi::sexp_cdr(current);
                    }

                    return Ok(Value::List(items));
                } else {
                    // root raw across recursive conversions of car and cdr
                    let _pair = ffi::GcRoot::new(ctx, raw);
                    let car = ffi::sexp_car(raw);
                    let cdr = ffi::sexp_cdr(raw);
                    return Ok(Value::Pair(
                        Box::new(Value::from_raw_depth(ctx, car, depth + 1)?),
                        Box::new(Value::from_raw_depth(ctx, cdr, depth + 1)?),
                    ));
                }
            }

            // fallback for other types
            Ok(Value::Other("<unhandled-type>".to_string()))
        }
    }

    /// check if a sexp is a proper list (ends in nil)
    ///
    /// uses tortoise-and-hare cycle detection to avoid infinite loops
    /// on circular lists constructed via set-cdr!.
    unsafe fn is_proper_list(sexp: ffi::sexp) -> bool {
        unsafe {
            let mut tortoise = sexp;
            let mut hare = sexp;

            loop {
                if ffi::sexp_pairp(hare) == 0 {
                    return ffi::sexp_nullp(hare) != 0;
                }
                hare = ffi::sexp_cdr(hare);

                if ffi::sexp_pairp(hare) == 0 {
                    return ffi::sexp_nullp(hare) != 0;
                }
                hare = ffi::sexp_cdr(hare);

                tortoise = ffi::sexp_cdr(tortoise);

                if hare == tortoise {
                    return false; // cycle detected
                }
            }
        }
    }

    /// extract a structured error from a chibi exception
    ///
    /// detects sandbox sentinel prefixes (`[sandbox:file]`, `[sandbox:binding]`)
    /// and module policy violations, returning `SandboxViolation` for those cases
    /// and `EvalError` for everything else.
    unsafe fn extract_exception_error(ctx: ffi::sexp, exn: ffi::sexp) -> Error {
        unsafe {
            let msg_sexp = ffi::sexp_exception_message(exn);
            let message = if ffi::sexp_stringp(msg_sexp) != 0 {
                let ptr = ffi::sexp_string_data(msg_sexp);
                let len = ffi::sexp_string_size(msg_sexp) as usize;
                let bytes = std::slice::from_raw_parts(ptr as *const u8, len);
                std::string::String::from_utf8_lossy(bytes).into_owned()
            } else {
                "unknown error".to_owned()
            };

            // extract irritants for appending to messages
            let irritant_str = {
                let irritants = ffi::sexp_exception_irritants(exn);
                if ffi::sexp_pairp(irritants) != 0 {
                    Value::from_raw(ctx, irritants)
                        .ok()
                        .map(|v| format!("{}", v))
                } else {
                    None
                }
            };

            // sentinel: file IO policy denial
            if let Some(path) = message.strip_prefix("[sandbox:file] ") {
                return Error::SandboxViolation(format!("file access denied: {}", path));
            }

            // sentinel: binding stub
            if let Some(rest) = message.strip_prefix("[sandbox:binding] ") {
                return Error::SandboxViolation(rest.to_string());
            }

            // module policy: detect import failures when VfsOnly is active.
            // chibi emits "couldn't find import" from meta-7.scm (scheme level)
            // or "couldn't find file in module path" from eval.c (C level).
            if message == "couldn't find import" || message == "couldn't find file in module path" {
                use crate::sandbox::MODULE_POLICY;
                use crate::sandbox::ModulePolicy;
                let is_vfs_only = MODULE_POLICY.with(|cell| cell.get() == ModulePolicy::VfsOnly);
                if is_vfs_only {
                    let module = irritant_str.as_deref().unwrap_or("unknown");
                    return Error::SandboxViolation(format!(
                        "module import blocked: {} (not available in this sandbox)",
                        module
                    ));
                }
            }

            // default: ordinary eval error with irritants appended
            if let Some(irr) = irritant_str {
                Error::EvalError(format!("{}: {}", message, irr))
            } else {
                Error::EvalError(message)
            }
        }
    }

    /// convert a rust value to a raw chibi sexp
    ///
    /// useful for returning values from foreign functions registered
    /// with [`Context::define_fn_variadic`] or `#[scheme_fn]`.
    ///
    /// supports all value types except `Other`.
    ///
    /// # Safety
    ///
    /// `ctx` must be a valid, live chibi-scheme context pointer.
    pub unsafe fn to_raw(&self, ctx: ffi::sexp) -> Result<ffi::sexp> {
        unsafe { self.to_raw_depth(ctx, 0) }
    }

    /// recursive inner conversion with depth tracking to prevent stack overflow
    unsafe fn to_raw_depth(&self, ctx: ffi::sexp, depth: usize) -> Result<ffi::sexp> {
        if depth > MAX_DEPTH {
            return Err(Error::EvalError(
                "value nesting depth exceeded maximum".to_string(),
            ));
        }

        unsafe {
            match self {
                Value::Integer(n) => Ok(ffi::sexp_make_fixnum(*n as ffi::sexp_sint_t)),
                Value::Float(f) => Ok(ffi::sexp_make_flonum(ctx, *f)),
                Value::Boolean(b) => Ok(ffi::sexp_make_boolean(*b)),
                Value::String(s) => {
                    let c_str = std::ffi::CString::new(s.as_str())
                        .map_err(|_| Error::TypeError("string contains null bytes".to_string()))?;
                    Ok(ffi::sexp_c_str(
                        ctx,
                        c_str.as_ptr(),
                        s.len() as ffi::sexp_sint_t,
                    ))
                }
                Value::Symbol(s) => {
                    let c_str = std::ffi::CString::new(s.as_str())
                        .map_err(|_| Error::TypeError("symbol contains null bytes".to_string()))?;
                    Ok(ffi::sexp_intern(
                        ctx,
                        c_str.as_ptr(),
                        s.len() as ffi::sexp_sint_t,
                    ))
                }
                Value::Nil => Ok(ffi::get_null()),
                Value::Unspecified => Ok(ffi::get_void()),
                Value::List(items) => {
                    // build list from back to front: (cons last (cons ... (cons first nil)))
                    let mut result = ffi::get_null();
                    for item in items.iter().rev() {
                        // root accumulator across to_raw_depth + sexp_cons allocations
                        let _tail = ffi::GcRoot::new(ctx, result);
                        let raw_item = item.to_raw_depth(ctx, depth + 1)?;
                        // root raw_item across sexp_cons (which allocates a pair)
                        let _head = ffi::GcRoot::new(ctx, raw_item);
                        result = ffi::sexp_cons(ctx, raw_item, result);
                    }
                    Ok(result)
                }
                Value::Pair(car, cdr) => {
                    let raw_car = car.to_raw_depth(ctx, depth + 1)?;
                    // root raw_car across cdr conversion + sexp_cons
                    let _car = ffi::GcRoot::new(ctx, raw_car);
                    let raw_cdr = cdr.to_raw_depth(ctx, depth + 1)?;
                    // root raw_cdr across sexp_cons
                    let _cdr = ffi::GcRoot::new(ctx, raw_cdr);
                    Ok(ffi::sexp_cons(ctx, raw_car, raw_cdr))
                }
                Value::Vector(items) => {
                    let len = items.len();
                    let vec = ffi::sexp_make_vector(ctx, len as ffi::sexp_uint_t, ffi::get_void());
                    // root vec across element conversions (each may allocate)
                    let _vec = ffi::GcRoot::new(ctx, vec);
                    for (i, item) in items.iter().enumerate() {
                        let raw_item = item.to_raw_depth(ctx, depth + 1)?;
                        ffi::sexp_vector_set(vec, i as ffi::sexp_uint_t, raw_item);
                    }
                    Ok(vec)
                }
                Value::Char(c) => Ok(ffi::sexp_make_character(*c as c_int)),
                Value::Bytevector(bytes) => {
                    let bv = ffi::sexp_make_bytes(ctx, bytes.len() as ffi::sexp_uint_t, 0);
                    // root bv across the memcpy (defensive — no allocation happens here,
                    // but GcRoot ensures safety if chibi's impl changes)
                    let _bv = ffi::GcRoot::new(ctx, bv);
                    let dst = ffi::sexp_bytes_data(bv) as *mut u8;
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
                    Ok(bv)
                }
                Value::Port(raw) => Ok(*raw),
                Value::HashTable(raw) => Ok(*raw),
                Value::Procedure(raw) => Ok(*raw),
                Value::Other(desc) => Err(Error::TypeError(format!(
                    "cannot convert Other({}) to raw sexp",
                    desc,
                ))),
                Value::Foreign { handle_id, type_name } => {
                    // build tagged list: (__tein-foreign "type-name" handle-id)
                    // scheme predicates and accessors in (tein foreign) recognise this shape.
                    let name_c = std::ffi::CString::new(type_name.as_str())
                        .map_err(|_| Error::TypeError("type name contains null bytes".to_string()))?;
                    let name_sexp = ffi::sexp_c_str(ctx, name_c.as_ptr(), type_name.len() as ffi::sexp_sint_t);
                    let _name_root = ffi::GcRoot::new(ctx, name_sexp);
                    let id_sexp = ffi::sexp_make_fixnum(*handle_id as ffi::sexp_sint_t);
                    let tag = ffi::sexp_intern(ctx, b"__tein-foreign\0".as_ptr() as *const std::os::raw::c_char, 14);
                    let _tag_root = ffi::GcRoot::new(ctx, tag);
                    // cons from right to left: tag . (name . (id . ()))
                    let tail = ffi::sexp_cons(ctx, id_sexp, ffi::get_null());
                    let _tail_root = ffi::GcRoot::new(ctx, tail);
                    let mid = ffi::sexp_cons(ctx, name_sexp, tail);
                    let _mid_root = ffi::GcRoot::new(ctx, mid);
                    Ok(ffi::sexp_cons(ctx, tag, mid))
                }
            }
        }
    }
}

// --- typed extraction helpers ---

impl Value {
    /// extract as integer, if this value is an `Integer`
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(n) => Some(*n),
            _ => None,
        }
    }

    /// extract as float, if this value is a `Float`
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// extract as string slice, if this value is a `String`
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// extract as symbol name, if this value is a `Symbol`
    pub fn as_symbol(&self) -> Option<&str> {
        match self {
            Value::Symbol(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// extract as boolean, if this value is a `Boolean`
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// extract as list slice, if this value is a `List`
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// extract as pair references, if this value is a `Pair`
    pub fn as_pair(&self) -> Option<(&Value, &Value)> {
        match self {
            Value::Pair(car, cdr) => Some((car.as_ref(), cdr.as_ref())),
            _ => None,
        }
    }

    /// extract as vector slice, if this value is a `Vector`
    pub fn as_vector(&self) -> Option<&[Value]> {
        match self {
            Value::Vector(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// extract the raw sexp pointer, if this value is a `Procedure`
    ///
    /// the returned pointer is opaque — pass it to [`Context::call`] to invoke.
    pub fn as_procedure(&self) -> Option<ffi::sexp> {
        match self {
            Value::Procedure(raw) => Some(*raw),
            _ => None,
        }
    }

    /// extract as char, if this value is a `Char`
    pub fn as_char(&self) -> Option<char> {
        match self {
            Value::Char(c) => Some(*c),
            _ => None,
        }
    }

    /// extract as byte slice, if this value is a `Bytevector`
    pub fn as_bytevector(&self) -> Option<&[u8]> {
        match self {
            Value::Bytevector(bytes) => Some(bytes.as_slice()),
            _ => None,
        }
    }

    /// extract the raw sexp pointer, if this value is a `Port`
    ///
    /// the returned pointer is opaque — pass it back to scheme via [`Context::call`].
    pub fn as_port(&self) -> Option<ffi::sexp> {
        match self {
            Value::Port(raw) => Some(*raw),
            _ => None,
        }
    }

    /// extract the raw sexp pointer, if this value is a `HashTable`
    ///
    /// the returned pointer is opaque — pass it back to scheme via [`Context::call`].
    pub fn as_hash_table(&self) -> Option<ffi::sexp> {
        match self {
            Value::HashTable(raw) => Some(*raw),
            _ => None,
        }
    }

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

    /// returns true if this value is a `Procedure`
    pub fn is_procedure(&self) -> bool {
        matches!(self, Value::Procedure(_))
    }

    /// returns true if this value is `Nil`
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    /// returns true if this value is `Unspecified`
    pub fn is_unspecified(&self) -> bool {
        matches!(self, Value::Unspecified)
    }

    /// returns true if this value is a `Char`
    pub fn is_char(&self) -> bool {
        matches!(self, Value::Char(_))
    }

    /// returns true if this value is a `Bytevector`
    pub fn is_bytevector(&self) -> bool {
        matches!(self, Value::Bytevector(_))
    }

    /// returns true if this value is a `Port`
    pub fn is_port(&self) -> bool {
        matches!(self, Value::Port(_))
    }

    /// returns true if this value is a `HashTable`
    pub fn is_hash_table(&self) -> bool {
        matches!(self, Value::HashTable(_))
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Symbol(a), Value::Symbol(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Pair(a1, a2), Value::Pair(b1, b2)) => a1 == b1 && a2 == b2,
            (Value::Vector(a), Value::Vector(b)) => a == b,
            (Value::Nil, Value::Nil) => true,
            (Value::Unspecified, Value::Unspecified) => true,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Bytevector(a), Value::Bytevector(b)) => a == b,
            (Value::Port(a), Value::Port(b)) => std::ptr::eq(*a, *b),
            (Value::HashTable(a), Value::HashTable(b)) => std::ptr::eq(*a, *b),
            // procedure equality is raw pointer identity (same scheme object)
            (Value::Procedure(a), Value::Procedure(b)) => std::ptr::eq(*a, *b),
            (Value::Other(a), Value::Other(b)) => a == b,
            (
                Value::Foreign { handle_id: a, type_name: ta },
                Value::Foreign { handle_id: b, type_name: tb },
            ) => a == b && ta == tb,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(i) => write!(f, "{}", i),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::String(s) => {
                write!(f, "\"")?;
                for ch in s.chars() {
                    match ch {
                        '"' => write!(f, "\\\"")?,
                        '\\' => write!(f, "\\\\")?,
                        '\n' => write!(f, "\\n")?,
                        '\r' => write!(f, "\\r")?,
                        '\t' => write!(f, "\\t")?,
                        c => write!(f, "{}", c)?,
                    }
                }
                write!(f, "\"")
            }
            Value::Symbol(s) => write!(f, "{}", s),
            Value::Boolean(b) => write!(f, "{}", if *b { "#t" } else { "#f" }),
            Value::List(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
            Value::Pair(car, cdr) => {
                write!(f, "({} . {})", car, cdr)
            }
            Value::Vector(items) => {
                write!(f, "#(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
            Value::Char(c) => match c {
                ' ' => write!(f, "#\\space"),
                '\n' => write!(f, "#\\newline"),
                '\t' => write!(f, "#\\tab"),
                '\r' => write!(f, "#\\return"),
                '\0' => write!(f, "#\\null"),
                _ if c.is_control() => write!(f, "#\\x{:x}", *c as u32),
                _ => write!(f, "#\\{}", c),
            },
            Value::Bytevector(bytes) => {
                write!(f, "#u8(")?;
                for (i, b) in bytes.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", b)?;
                }
                write!(f, ")")
            }
            Value::Port(_) => write!(f, "#<port>"),
            Value::HashTable(_) => write!(f, "#<hash-table>"),
            Value::Nil => write!(f, "()"),
            Value::Unspecified => write!(f, "#<unspecified>"),
            Value::Procedure(_) => write!(f, "#<procedure>"),
            Value::Other(s) => write!(f, "#<{}>", s),
            Value::Foreign { handle_id, type_name } => {
                write!(f, "#<foreign {}:{}>", type_name, handle_id)
            }
        }
    }
}
