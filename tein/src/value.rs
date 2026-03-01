//! Scheme value representation.
//!
//! [`Value`] is the safe Rust representation of a Chibi-Scheme sexp.
//! Most variants own their data; `Procedure`, `Port`, and `HashTable`
//! hold raw sexp pointers valid only within the originating
//! [`crate::Context`].
//!
//! # Variants
//!
//! | variant | scheme type | rust extraction |
//! |---------|------------|-----------------|
//! | `Integer(i64)` | fixnum | `as_integer()` |
//! | `Float(f64)` | flonum | `as_float()` |
//! | `Bignum(String)` | bignum (arbitrary precision) | `as_bignum()` |
//! | `Rational(Box, Box)` | exact ratio `n/d` | `as_rational()` |
//! | `Complex(Box, Box)` | complex `a+bi` | `as_complex()` |
//! | `String(String)` | string | `as_str()` |
//! | `Symbol(String)` | symbol | `as_symbol()` |
//! | `Boolean(bool)` | `#t` / `#f` | `as_bool()` |
//! | `List(Vec<Value>)` | proper list | `as_list()` |
//! | `Pair(Box, Box)` | dotted pair | `as_pair()` |
//! | `Vector(Vec<Value>)` | `#(...)` | `as_vector()` |
//! | `Char(char)` | character | `as_char()` |
//! | `Bytevector(Vec<u8>)` | `#u8(...)` | `as_bytevector()` |
//! | `Port(sexp)` | port | `as_port()` |
//! | `HashTable(sexp)` | hash-table | `as_hash_table()` |
//! | `Nil` | `'()` | — |
//! | `Unspecified` | void | — |
//! | `Procedure(sexp)` | lambda/opcode | `as_procedure()` |
//! | `Foreign { .. }` | foreign object | `ctx.foreign_ref::<T>()` |
//! | `Other(String)` | unhandled type | — |
//!
//! # Conversion
//!
//! `Value::from_raw()` converts Chibi sexps to safe values. Type checking
//! order matters: `complex → ratio → bignum → flonum → integer` (broadest first).
//! Chibi's integer predicate matches flonums like `4.0`, so flonum must come
//! before integer. Complex/ratio/bignum must precede flonum for similar reasons.
//!
//! `Value::to_raw()` converts back to Chibi sexps for calling into Scheme.

use crate::{
    error::{Error, Result},
    ffi,
};
use std::fmt;
use std::os::raw::c_int;

/// Maximum nesting depth for recursive value conversion.
/// Prevents stack overflow on deeply nested or (theoretically) circular structures.
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

/// A Scheme value.
///
/// Represents the result of evaluating Scheme code.
/// This is a safe wrapper around Chibi's internal sexp type.
///
/// Most variants own their data, but `Procedure` holds a raw sexp pointer
/// that is only valid within the originating `Context` (enforced by !Send/!Sync).
#[derive(Debug, Clone)]
pub enum Value {
    /// Integer value.
    Integer(i64),

    /// Floating point value.
    Float(f64),

    /// Bignum (arbitrary-precision integer, stored as decimal string).
    ///
    /// Chibi-scheme bignums are converted to their decimal representation
    /// for safe transport across the FFI boundary. Use `to_raw()` to
    /// convert back to a chibi bignum via `string->number`.
    Bignum(String),

    /// Rational number (exact ratio of two integers).
    ///
    /// Components are exact integers (`Integer` or `Bignum`).
    /// Displayed as `n/d` (e.g. `1/3`).
    Rational(Box<Value>, Box<Value>),

    /// Complex number with real and imaginary parts.
    ///
    /// Components are real numbers (`Integer`, `Float`, `Bignum`, or `Rational`).
    /// Displayed as `a+bi` (e.g. `1+2i`).
    Complex(Box<Value>, Box<Value>),

    /// String value.
    String(String),

    /// Symbol value.
    Symbol(String),

    /// Boolean value.
    Boolean(bool),

    /// Proper list (converted to vec).
    List(Vec<Value>),

    /// Pair (improper list or dotted pair).
    Pair(Box<Value>, Box<Value>),

    /// Vector (Scheme `#(...)`).
    Vector(Vec<Value>),

    /// Character value (unicode scalar value).
    Char(char),

    /// Bytevector (Scheme `#u8(...)`).
    Bytevector(Vec<u8>),

    /// An opaque input or output port.
    ///
    /// Holds a raw sexp pointer — only valid within the originating Context.
    Port(ffi::sexp),

    /// An opaque hash table (SRFI-69).
    ///
    /// Holds a raw sexp pointer — only valid within the originating Context.
    HashTable(ffi::sexp),

    /// Nil/empty list.
    Nil,

    /// Unspecified value (like void in C).
    Unspecified,

    /// A callable Scheme procedure or opcode (builtin like `+`).
    ///
    /// Holds a raw sexp pointer — only valid within the originating Context.
    /// Thread safety is enforced by Context being !Send + !Sync.
    Procedure(ffi::sexp),

    /// Other Scheme values we don't yet handle.
    Other(String),

    /// A foreign object managed by the Context's ForeignStore.
    ///
    /// Holds the handle ID and type name. The actual data lives Rust-side
    /// in the ForeignStore — use `ctx.foreign_ref::<T>(value)` to access.
    Foreign {
        /// Handle ID in the ForeignStore.
        handle_id: u64,
        /// Type name (from ForeignType::type_name).
        type_name: String,
    },
}

impl Value {
    /// Convert from raw Chibi sexp to safe value.
    ///
    /// # Safety
    /// ctx and raw must be valid pointers from Chibi-Scheme.
    ///
    /// `#[doc(hidden)]` — not part of the stable public API; exposed for proc-macro generated code.
    #[doc(hidden)]
    pub unsafe fn from_raw(ctx: ffi::sexp, raw: ffi::sexp) -> Result<Self> {
        unsafe { Self::from_raw_depth(ctx, raw, 0) }
    }

    /// Recursive inner conversion with depth tracking to prevent stack overflow.
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

            // --- numeric tower: check broadest first ---

            // complex numbers (real + imaginary)
            if ffi::sexp_complexp(raw) != 0 {
                // root raw — recursive from_raw_depth calls may allocate
                let _root = ffi::GcRoot::new(ctx, raw);
                let real_part = ffi::sexp_complex_real(raw);
                let imag_part = ffi::sexp_complex_imag(raw);
                let real = Value::from_raw_depth(ctx, real_part, depth + 1)?;
                let imag = Value::from_raw_depth(ctx, imag_part, depth + 1)?;
                return Ok(Value::Complex(Box::new(real), Box::new(imag)));
            }

            // rational numbers (numerator / denominator)
            if ffi::sexp_ratiop(raw) != 0 {
                // root raw — recursive from_raw_depth calls may allocate
                let _root = ffi::GcRoot::new(ctx, raw);
                let num = ffi::sexp_ratio_numerator(raw);
                let den = ffi::sexp_ratio_denominator(raw);
                let numerator = Value::from_raw_depth(ctx, num, depth + 1)?;
                let denominator = Value::from_raw_depth(ctx, den, depth + 1)?;
                return Ok(Value::Rational(Box::new(numerator), Box::new(denominator)));
            }

            // bignums (arbitrary-precision integers)
            if ffi::sexp_bignump(raw) != 0 {
                // root raw — sexp_bignum_to_string allocates (opens a string port)
                let _root = ffi::GcRoot::new(ctx, raw);
                let str_sexp = ffi::sexp_bignum_to_string(ctx, raw);
                let str_ptr = ffi::sexp_string_data(str_sexp);
                let str_len = ffi::sexp_string_size(str_sexp);
                let bytes = std::slice::from_raw_parts(str_ptr as *const u8, str_len as usize);
                let s = String::from_utf8(bytes.to_vec())?;
                return Ok(Value::Bignum(s));
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
            // must run before generic pair/list handling below.
            // root `raw` across the sexp_symbol_to_string call — it allocates
            // (via sexp_c_string) and can trigger GC. without rooting, the pair
            // accessed by sexp_cdr(raw) below could be collected.
            if ffi::sexp_pairp(raw) != 0 {
                let car = ffi::sexp_car(raw);
                if ffi::sexp_symbolp(car) != 0 {
                    let _pair_root = ffi::GcRoot::new(ctx, raw);
                    let sym_str = ffi::sexp_symbol_to_string(ctx, car);
                    let sym_ptr = ffi::sexp_string_data(sym_str);
                    let sym_len = ffi::sexp_string_size(sym_str) as usize;
                    let sym_bytes = std::slice::from_raw_parts(sym_ptr as *const u8, sym_len);
                    if sym_bytes == b"__tein-foreign" {
                        let rest = ffi::sexp_cdr(raw);
                        if ffi::sexp_pairp(rest) != 0 {
                            let name_sexp = ffi::sexp_car(rest);
                            let id_rest = ffi::sexp_cdr(rest);
                            if ffi::sexp_stringp(name_sexp) != 0 && ffi::sexp_pairp(id_rest) != 0 {
                                let id_sexp = ffi::sexp_car(id_rest);
                                if ffi::sexp_integerp(id_sexp) != 0 {
                                    let name_ptr = ffi::sexp_string_data(name_sexp);
                                    let name_len = ffi::sexp_string_size(name_sexp) as usize;
                                    let name_bytes =
                                        std::slice::from_raw_parts(name_ptr as *const u8, name_len);
                                    let type_name = String::from_utf8(name_bytes.to_vec())?;
                                    let handle_id = ffi::sexp_unbox_fixnum(id_sexp) as u64;
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

    /// Check if a sexp is a proper list (ends in nil).
    ///
    /// Uses tortoise-and-hare cycle detection to avoid infinite loops
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

    /// Extract a structured error from a Chibi exception.
    ///
    /// Detects sandbox sentinel prefixes (`[sandbox:file]`, `[sandbox:binding]`)
    /// and module policy violations, returning `SandboxViolation` for those cases
    /// and `EvalError` for everything else.
    unsafe fn extract_exception_error(ctx: ffi::sexp, exn: ffi::sexp) -> Error {
        unsafe {
            let msg_sexp = ffi::sexp_exception_message(exn);
            let message = if ffi::sexp_stringp(msg_sexp) != 0 {
                let ptr = ffi::sexp_string_data(msg_sexp);
                let len = ffi::sexp_string_size(msg_sexp) as usize;
                let bytes = std::slice::from_raw_parts(ptr as *const u8, len);
                // use strict from_utf8 rather than lossy — lossy replacement could
                // corrupt sentinel prefix matching ([sandbox:file], [sandbox:binding]),
                // allowing an attacker to bypass sandbox detection via invalid UTF-8.
                match std::string::String::from_utf8(bytes.to_vec()) {
                    Ok(s) => s,
                    Err(_) => {
                        return Error::EvalError("exception with non-UTF-8 message".to_string());
                    }
                }
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

            // VFS gate: detect import failures when sandboxed.
            // chibi emits "couldn't find import" from meta-7.scm (scheme level)
            // or "couldn't find file in module path" from eval.c (C level).
            if message == "couldn't find import" || message == "couldn't find file in module path" {
                use crate::sandbox::VFS_GATE;
                let is_gated = VFS_GATE.with(|cell| cell.get() != crate::sandbox::GATE_OFF);
                if is_gated {
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

    /// Convert a Rust value to a raw Chibi sexp.
    ///
    /// Useful for returning values from foreign functions registered
    /// with [`crate::Context::define_fn_variadic`] or `#[tein_fn]`.
    ///
    /// Supports all value types except `Other`.
    ///
    /// # Safety
    ///
    /// `ctx` must be a valid, live Chibi-Scheme context pointer.
    pub unsafe fn to_raw(&self, ctx: ffi::sexp) -> Result<ffi::sexp> {
        unsafe { self.to_raw_depth(ctx, 0) }
    }

    /// Recursive inner conversion with depth tracking to prevent stack overflow.
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
                Value::Bignum(s) => {
                    let c_str = std::ffi::CString::new(s.as_str()).map_err(|_| {
                        Error::TypeError("bignum string contains null bytes".to_string())
                    })?;
                    let str_sexp =
                        ffi::sexp_c_str(ctx, c_str.as_ptr(), s.len() as ffi::sexp_sint_t);
                    // root str_sexp — sexp_string_to_number allocates internally
                    let _str_root = ffi::GcRoot::new(ctx, str_sexp);
                    let result = ffi::sexp_string_to_number(ctx, str_sexp, 10);
                    // sexp_string_to_number returns SEXP_FALSE on parse failure, not an exception
                    if ffi::sexp_booleanp(result) != 0 {
                        return Err(Error::TypeError(format!("invalid bignum string: {s}")));
                    }
                    Ok(result)
                }
                Value::Rational(n, d) => {
                    let num = n.to_raw_depth(ctx, depth + 1)?;
                    // root num — converting denominator may allocate
                    let _num_root = ffi::GcRoot::new(ctx, num);
                    let den = d.to_raw_depth(ctx, depth + 1)?;
                    Ok(ffi::sexp_make_ratio(ctx, num, den))
                }
                Value::Complex(r, i) => {
                    let real = r.to_raw_depth(ctx, depth + 1)?;
                    // root real — converting imag may allocate
                    let _real_root = ffi::GcRoot::new(ctx, real);
                    let imag = i.to_raw_depth(ctx, depth + 1)?;
                    Ok(ffi::sexp_make_complex(ctx, real, imag))
                }
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
                Value::Foreign {
                    handle_id,
                    type_name,
                } => {
                    // build tagged list: (__tein-foreign "type-name" handle-id)
                    // scheme predicates and accessors in (tein foreign) recognise this shape.
                    let name_c = std::ffi::CString::new(type_name.as_str()).map_err(|_| {
                        Error::TypeError("type name contains null bytes".to_string())
                    })?;
                    let name_sexp =
                        ffi::sexp_c_str(ctx, name_c.as_ptr(), type_name.len() as ffi::sexp_sint_t);
                    let _name_root = ffi::GcRoot::new(ctx, name_sexp);
                    let id_sexp = ffi::sexp_make_fixnum(*handle_id as ffi::sexp_sint_t);
                    let tag = ffi::sexp_intern(ctx, c"__tein-foreign".as_ptr(), 14);
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
    /// Extract as integer, if this value is an `Integer`.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Value::Integer(n) => Some(*n),
            _ => None,
        }
    }

    /// Extract as float, if this value is a `Float`.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Extract as string slice, if this value is a `String`.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Extract as symbol name, if this value is a `Symbol`.
    pub fn as_symbol(&self) -> Option<&str> {
        match self {
            Value::Symbol(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Extract as boolean, if this value is a `Boolean`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Extract as list slice, if this value is a `List`.
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// Extract as pair references, if this value is a `Pair`.
    pub fn as_pair(&self) -> Option<(&Value, &Value)> {
        match self {
            Value::Pair(car, cdr) => Some((car.as_ref(), cdr.as_ref())),
            _ => None,
        }
    }

    /// Extract as vector slice, if this value is a `Vector`.
    pub fn as_vector(&self) -> Option<&[Value]> {
        match self {
            Value::Vector(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// Extract the raw sexp pointer, if this value is a `Procedure`.
    ///
    /// The returned pointer is opaque — pass it to [`crate::Context::call`] to invoke.
    pub fn as_procedure(&self) -> Option<ffi::sexp> {
        match self {
            Value::Procedure(raw) => Some(*raw),
            _ => None,
        }
    }

    /// Extract as char, if this value is a `Char`.
    pub fn as_char(&self) -> Option<char> {
        match self {
            Value::Char(c) => Some(*c),
            _ => None,
        }
    }

    /// Extract as byte slice, if this value is a `Bytevector`.
    pub fn as_bytevector(&self) -> Option<&[u8]> {
        match self {
            Value::Bytevector(bytes) => Some(bytes.as_slice()),
            _ => None,
        }
    }

    /// Extract as bignum string, if this is a `Bignum`.
    pub fn as_bignum(&self) -> Option<&str> {
        match self {
            Value::Bignum(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Extract rational components, if this is a `Rational`.
    pub fn as_rational(&self) -> Option<(&Value, &Value)> {
        match self {
            Value::Rational(n, d) => Some((n.as_ref(), d.as_ref())),
            _ => None,
        }
    }

    /// Extract complex components, if this is a `Complex`.
    pub fn as_complex(&self) -> Option<(&Value, &Value)> {
        match self {
            Value::Complex(r, i) => Some((r.as_ref(), i.as_ref())),
            _ => None,
        }
    }

    /// Extract the raw sexp pointer, if this value is a `Port`.
    ///
    /// The returned pointer is opaque — pass it back to Scheme via [`crate::Context::call`].
    pub fn as_port(&self) -> Option<ffi::sexp> {
        match self {
            Value::Port(raw) => Some(*raw),
            _ => None,
        }
    }

    /// Extract the raw sexp pointer, if this value is a `HashTable`.
    ///
    /// The returned pointer is opaque — pass it back to Scheme via [`crate::Context::call`].
    pub fn as_hash_table(&self) -> Option<ffi::sexp> {
        match self {
            Value::HashTable(raw) => Some(*raw),
            _ => None,
        }
    }

    /// Extract foreign object handle ID and type name.
    pub fn as_foreign(&self) -> Option<(u64, &str)> {
        match self {
            Value::Foreign {
                handle_id,
                type_name,
            } => Some((*handle_id, type_name.as_str())),
            _ => None,
        }
    }

    /// Returns the type name if this value is a `Foreign` object.
    pub fn foreign_type_name(&self) -> Option<&str> {
        match self {
            Value::Foreign { type_name, .. } => Some(type_name.as_str()),
            _ => None,
        }
    }

    /// Returns true if this value is a `Foreign` object.
    pub fn is_foreign(&self) -> bool {
        matches!(self, Value::Foreign { .. })
    }

    /// Returns true if this value is a `Procedure`.
    pub fn is_procedure(&self) -> bool {
        matches!(self, Value::Procedure(_))
    }

    /// Returns true if this value is `Nil`.
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    /// Returns true if this value is `Unspecified`.
    pub fn is_unspecified(&self) -> bool {
        matches!(self, Value::Unspecified)
    }

    /// Returns true if this value is a `Char`.
    pub fn is_char(&self) -> bool {
        matches!(self, Value::Char(_))
    }

    /// Returns true if this value is a `Bytevector`.
    pub fn is_bytevector(&self) -> bool {
        matches!(self, Value::Bytevector(_))
    }

    /// Returns true if this value is a `Port`.
    pub fn is_port(&self) -> bool {
        matches!(self, Value::Port(_))
    }

    /// Returns true if this value is a `HashTable`.
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
            (Value::Bignum(a), Value::Bignum(b)) => a == b,
            (Value::Rational(an, ad), Value::Rational(bn, bd)) => an == bn && ad == bd,
            (Value::Complex(ar, ai), Value::Complex(br, bi)) => ar == br && ai == bi,
            (Value::Port(a), Value::Port(b)) => std::ptr::eq(*a, *b),
            (Value::HashTable(a), Value::HashTable(b)) => std::ptr::eq(*a, *b),
            // procedure equality is raw pointer identity (same scheme object)
            (Value::Procedure(a), Value::Procedure(b)) => std::ptr::eq(*a, *b),
            (Value::Other(a), Value::Other(b)) => a == b,
            (
                Value::Foreign {
                    handle_id: a,
                    type_name: ta,
                },
                Value::Foreign {
                    handle_id: b,
                    type_name: tb,
                },
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
            Value::Bignum(s) => write!(f, "{s}"),
            Value::Rational(n, d) => write!(f, "{n}/{d}"),
            Value::Complex(r, i) => {
                write!(f, "{r}")?;
                // check if imaginary part displays with a leading sign
                let imag_str = format!("{i}");
                if imag_str.starts_with('-') || imag_str.starts_with('+') {
                    write!(f, "{imag_str}i")
                } else {
                    write!(f, "+{imag_str}i")
                }
            }
            Value::Port(_) => write!(f, "#<port>"),
            Value::HashTable(_) => write!(f, "#<hash-table>"),
            Value::Nil => write!(f, "()"),
            Value::Unspecified => write!(f, "#<unspecified>"),
            Value::Procedure(_) => write!(f, "#<procedure>"),
            Value::Other(s) => write!(f, "#<{}>", s),
            Value::Foreign {
                handle_id,
                type_name,
            } => {
                write!(f, "#<foreign {}:{}>", type_name, handle_id)
            }
        }
    }
}
