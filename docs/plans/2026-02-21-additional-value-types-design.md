# additional value types — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** add Char, Bytevector, Port, and HashTable variants to the Value enum, completing milestone 4b.

**Architecture:** layer-by-layer bottom-up — C shim functions first, then rust FFI bindings, then Value enum changes (from_raw, to_raw, helpers, Display, PartialEq), then tests. hash tables use a dynamically registered type tag (srfi-69 define-record-type), so detection requires runtime lookup — deferred to last with graceful fallback to Other.

**Tech Stack:** C (tein_shim.c), rust (ffi.rs, value.rs, context.rs), chibi-scheme internals

---

### task 1: char shim functions

**files:**
- modify: `tein/vendor/chibi-scheme/tein_shim.c`

**step 1: add char shim functions**

add after the existing type check block (after line 14 `tein_sexp_pairp`):

```c
// character operations
int tein_sexp_charp(sexp x) { return sexp_charp(x); }
int tein_sexp_unbox_character(sexp x) { return sexp_unbox_character(x); }
sexp tein_sexp_make_character(int n) { return sexp_make_character(n); }
```

**step 2: verify build**

run: `cargo build -p tein 2>&1 | tail -5`
expected: compiles successfully (shim functions are compiled by build.rs)

---

### task 2: bytevector shim functions

**files:**
- modify: `tein/vendor/chibi-scheme/tein_shim.c`

**step 1: add bytevector shim functions**

add after the char block:

```c
// bytevector operations
int tein_sexp_bytesp(sexp x) { return sexp_bytesp(x); }
char* tein_sexp_bytes_data(sexp x) { return sexp_bytes_data(x); }
sexp_uint_t tein_sexp_bytes_length(sexp x) { return sexp_bytes_length(x); }
sexp tein_sexp_make_bytes(sexp ctx, sexp_uint_t len, unsigned char init) {
    return sexp_make_bytes(ctx, sexp_make_fixnum(len), sexp_make_fixnum(init));
}
```

note: `sexp_make_bytes` takes tagged fixnums, so we wrap the raw values.

**step 2: verify build**

run: `cargo build -p tein 2>&1 | tail -5`
expected: compiles successfully

---

### task 3: port shim functions

**files:**
- modify: `tein/vendor/chibi-scheme/tein_shim.c`

**step 1: add port shim functions**

add after the bytevector block:

```c
// port operations
int tein_sexp_portp(sexp x) { return sexp_portp(x); }
int tein_sexp_iportp(sexp x) { return sexp_iportp(x); }
int tein_sexp_oportp(sexp x) { return sexp_oportp(x); }
```

**step 2: verify build**

run: `cargo build -p tein 2>&1 | tail -5`
expected: compiles successfully

**step 3: commit shim changes**

```bash
git add tein/vendor/chibi-scheme/tein_shim.c
git commit -m "shim: add char, bytevector, port functions"
```

---

### task 4: FFI bindings for new types

**files:**
- modify: `tein/src/ffi.rs`

**step 1: add extern declarations**

add to the `unsafe extern "C"` block, after the existing type check declarations:

```rust
    // character operations (via tein shim)
    pub fn tein_sexp_charp(x: sexp) -> c_int;
    pub fn tein_sexp_unbox_character(x: sexp) -> c_int;
    pub fn tein_sexp_make_character(n: c_int) -> sexp;

    // bytevector operations (via tein shim)
    pub fn tein_sexp_bytesp(x: sexp) -> c_int;
    pub fn tein_sexp_bytes_data(x: sexp) -> *mut c_char;
    pub fn tein_sexp_bytes_length(x: sexp) -> sexp_uint_t;
    pub fn tein_sexp_make_bytes(ctx: sexp, len: sexp_uint_t, init: c_uchar) -> sexp;

    // port operations (via tein shim)
    pub fn tein_sexp_portp(x: sexp) -> c_int;
    pub fn tein_sexp_iportp(x: sexp) -> c_int;
    pub fn tein_sexp_oportp(x: sexp) -> c_int;
```

note: `c_uchar` needs to be added to the use statement: `use std::os::raw::{c_char, c_int, c_long, c_uchar, c_ulong, c_void};`

**step 2: add safe wrapper functions**

add after the existing wrapper functions, following the same pattern:

```rust
// character operations
#[inline]
pub unsafe fn sexp_charp(x: sexp) -> c_int {
    unsafe { tein_sexp_charp(x) }
}

#[inline]
pub unsafe fn sexp_unbox_character(x: sexp) -> c_int {
    unsafe { tein_sexp_unbox_character(x) }
}

#[inline]
pub unsafe fn sexp_make_character(n: c_int) -> sexp {
    unsafe { tein_sexp_make_character(n) }
}

// bytevector operations
#[inline]
pub unsafe fn sexp_bytesp(x: sexp) -> c_int {
    unsafe { tein_sexp_bytesp(x) }
}

#[inline]
pub unsafe fn sexp_bytes_data(x: sexp) -> *mut c_char {
    unsafe { tein_sexp_bytes_data(x) }
}

#[inline]
pub unsafe fn sexp_bytes_length(x: sexp) -> sexp_uint_t {
    unsafe { tein_sexp_bytes_length(x) }
}

#[inline]
pub unsafe fn sexp_make_bytes(ctx: sexp, len: sexp_uint_t, init: u8) -> sexp {
    unsafe { tein_sexp_make_bytes(ctx, len, init as c_uchar) }
}

// port operations
#[inline]
pub unsafe fn sexp_portp(x: sexp) -> c_int {
    unsafe { tein_sexp_portp(x) }
}

#[inline]
pub unsafe fn sexp_iportp(x: sexp) -> c_int {
    unsafe { tein_sexp_iportp(x) }
}

#[inline]
pub unsafe fn sexp_oportp(x: sexp) -> c_int {
    unsafe { tein_sexp_oportp(x) }
}
```

**step 3: update GC safety comment in value.rs**

in `value.rs`, add to the allocating list comment near the top:
- `sexp_make_bytes` to the allocating list

**step 4: update raw module re-exports in lib.rs**

add new symbols to the `pub mod raw` block:

```rust
pub use crate::ffi::{
    get_false, get_null, get_true, get_void, sexp_booleanp, sexp_bytesp,
    sexp_bytes_data, sexp_bytes_length, sexp_c_str, sexp_car, sexp_cdr,
    sexp_charp, sexp_cons, sexp_exceptionp, sexp_flonum_value, sexp_flonump,
    sexp_integerp, sexp_make_boolean, sexp_make_bytes, sexp_make_character,
    sexp_make_fixnum, sexp_make_flonum, sexp_nullp, sexp_pairp, sexp_portp,
    sexp_string_data, sexp_string_size, sexp_stringp, sexp_symbolp,
    sexp_unbox_character, sexp_unbox_fixnum, sexp_vectorp,
};
```

**step 5: verify build**

run: `cargo build -p tein 2>&1 | tail -5`
expected: compiles successfully

**step 6: commit**

```bash
git add tein/src/ffi.rs tein/src/lib.rs
git commit -m "ffi: add char, bytevector, port bindings"
```

---

### task 5: Value enum — new variants + from_raw

**files:**
- modify: `tein/src/value.rs`

**step 1: add new variants to the Value enum**

add after `Vector(Vec<Value>)`:

```rust
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
```

**step 2: add from_raw_depth detection**

in `from_raw_depth`, add char check after the boolean check (before `sexp_nullp`):

```rust
            if ffi::sexp_charp(raw) != 0 {
                let code = ffi::sexp_unbox_character(raw) as u32;
                let c = char::from_u32(code).ok_or_else(|| {
                    Error::TypeError(format!("invalid unicode codepoint: {:#x}", code))
                })?;
                return Ok(Value::Char(c));
            }
```

add bytevector check after the string check (before `sexp_vectorp`):

```rust
            if ffi::sexp_bytesp(raw) != 0 {
                let data = ffi::sexp_bytes_data(raw);
                let len = ffi::sexp_bytes_length(raw) as usize;
                let bytes = std::slice::from_raw_parts(data as *const u8, len).to_vec();
                return Ok(Value::Bytevector(bytes));
            }
```

add port check after the vector check (before `sexp_pairp`):

```rust
            if ffi::sexp_portp(raw) != 0 {
                return Ok(Value::Port(raw));
            }
```

note: hash table detection deferred to task 9.

**step 3: add to_raw_depth conversions**

in `to_raw_depth`, add match arms (before the Procedure arm):

```rust
                Value::Char(c) => Ok(ffi::sexp_make_character(*c as c_int)),
                Value::Bytevector(bytes) => {
                    let bv = ffi::sexp_make_bytes(ctx, bytes.len() as ffi::sexp_uint_t, 0);
                    // root bv across the memcpy (not strictly needed since
                    // no allocation happens, but defensive)
                    let _bv = ffi::GcRoot::new(ctx, bv);
                    let dst = ffi::sexp_bytes_data(bv) as *mut u8;
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
                    Ok(bv)
                }
                Value::Port(raw) => Ok(*raw),
                Value::HashTable(raw) => Ok(*raw),
```

note: need `use std::os::raw::c_int;` at the top of value.rs (or use the ffi re-export path).

**step 4: add PartialEq arms**

in the `PartialEq` impl, add:

```rust
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Bytevector(a), Value::Bytevector(b)) => a == b,
            (Value::Port(a), Value::Port(b)) => std::ptr::eq(*a, *b),
            (Value::HashTable(a), Value::HashTable(b)) => std::ptr::eq(*a, *b),
```

**step 5: add Display arms**

in the `Display` impl, add:

```rust
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
```

**step 6: verify build**

run: `cargo build -p tein 2>&1 | tail -5`
expected: compiles successfully (tests may fail until we add exhaustive match updates)

**step 7: commit**

```bash
git add tein/src/value.rs
git commit -m "value: add Char, Bytevector, Port, HashTable variants"
```

---

### task 6: Value helpers (extraction + predicates)

**files:**
- modify: `tein/src/value.rs`

**step 1: add extraction helpers**

add to the typed extraction helpers impl block, after `as_procedure`:

```rust
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
```

**step 2: add predicate helpers**

add after `is_unspecified`:

```rust
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
```

**step 3: verify build + clippy**

run: `cargo clippy -p tein 2>&1 | tail -10`
expected: no errors (warnings ok)

**step 4: commit**

```bash
git add tein/src/value.rs
git commit -m "value: add extraction helpers and predicates for new types"
```

---

### task 7: char tests

**files:**
- modify: `tein/src/context.rs`

**step 1: write char tests**

add in the test module, after the existing type tests:

```rust
    // --- characters ---

    #[test]
    fn test_char_value() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate(r"#\a").expect("failed to evaluate");
        assert_eq!(result, Value::Char('a'));
        assert_eq!(result.as_char(), Some('a'));
    }

    #[test]
    fn test_char_special() {
        let ctx = Context::new().expect("failed to create context");
        assert_eq!(
            ctx.evaluate(r"#\space").expect("space"),
            Value::Char(' ')
        );
        assert_eq!(
            ctx.evaluate(r"#\newline").expect("newline"),
            Value::Char('\n')
        );
        assert_eq!(
            ctx.evaluate(r"#\tab").expect("tab"),
            Value::Char('\t')
        );
    }

    #[test]
    fn test_char_unicode() {
        let ctx = Context::new().expect("failed to create context");
        // lambda character
        let result = ctx.evaluate(r"#\λ").expect("unicode char");
        assert_eq!(result, Value::Char('λ'));
    }

    #[test]
    fn test_char_display() {
        assert_eq!(format!("{}", Value::Char('a')), r"#\a");
        assert_eq!(format!("{}", Value::Char(' ')), r"#\space");
        assert_eq!(format!("{}", Value::Char('\n')), r"#\newline");
        assert_eq!(format!("{}", Value::Char('\t')), r"#\tab");
    }

    #[test]
    fn test_char_round_trip() {
        unsafe extern "C" fn return_char(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                Value::Char('λ')
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("context");
        ctx.define_fn_variadic("get-char", return_char)
            .expect("define");
        let result = ctx.evaluate("(get-char)").expect("call");
        assert_eq!(result, Value::Char('λ'));
    }
```

**step 2: run char tests**

run: `cargo test -p tein test_char -- --nocapture 2>&1 | tail -20`
expected: all pass

**step 3: commit**

```bash
git add tein/src/context.rs
git commit -m "test: char value extraction, display, round-trip"
```

---

### task 8: bytevector tests

**files:**
- modify: `tein/src/context.rs`

**step 1: write bytevector tests**

add after char tests:

```rust
    // --- bytevectors ---

    #[test]
    fn test_bytevector_value() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("#u8(1 2 3)").expect("failed to evaluate");
        assert_eq!(result, Value::Bytevector(vec![1, 2, 3]));
        assert_eq!(result.as_bytevector(), Some([1u8, 2, 3].as_slice()));
    }

    #[test]
    fn test_bytevector_empty() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("#u8()").expect("failed to evaluate");
        assert_eq!(result, Value::Bytevector(vec![]));
    }

    #[test]
    fn test_bytevector_display() {
        let bv = Value::Bytevector(vec![0, 127, 255]);
        assert_eq!(format!("{}", bv), "#u8(0 127 255)");
        assert_eq!(format!("{}", Value::Bytevector(vec![])), "#u8()");
    }

    #[test]
    fn test_bytevector_round_trip() {
        unsafe extern "C" fn return_bv(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                Value::Bytevector(vec![10, 20, 30])
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("context");
        ctx.define_fn_variadic("get-bv", return_bv).expect("define");
        let result = ctx.evaluate("(get-bv)").expect("call");
        assert_eq!(result, Value::Bytevector(vec![10, 20, 30]));
    }
```

**step 2: run bytevector tests**

run: `cargo test -p tein test_bytevector -- --nocapture 2>&1 | tail -20`
expected: all pass

**step 3: commit**

```bash
git add tein/src/context.rs
git commit -m "test: bytevector value extraction, display, round-trip"
```

---

### task 9: port tests + hash table investigation

**files:**
- modify: `tein/src/context.rs`
- possibly modify: `tein/vendor/chibi-scheme/tein_shim.c`, `tein/src/ffi.rs`, `tein/src/value.rs`

**step 1: write port test**

```rust
    // --- ports ---

    #[test]
    fn test_port_opaque() {
        let ctx = Context::new_standard().expect("standard context");
        let result = ctx.evaluate("(current-input-port)").expect("port");
        assert!(result.is_port(), "expected Port, got {:?}", result);
    }

    #[test]
    fn test_port_display() {
        // can't easily construct a Port without a context, just test Display for coverage
        assert_eq!(format!("{}", Value::Port(std::ptr::null_mut())), "#<port>");
    }
```

**step 2: run port tests**

run: `cargo test -p tein test_port -- --nocapture 2>&1 | tail -20`
expected: pass

**step 3: investigate hash table detection**

hash tables are `define-record-type` in srfi-69, so they get a dynamic type tag.
investigate whether we can detect them reliably. options:
- look up the type tag at runtime via `sexp_env_ref` for `Hash-Table`
- check `sexp_typep` + compare type name string
- accept that they fall through to `Other` until srfi-69 is loaded

if detection is feasible, add shim + ffi + from_raw detection.
if not feasible without significant complexity, document the limitation and skip — hash tables will appear as `Other` and can still be passed back to scheme code. add a TODO comment.

**step 4: write hash table test (conditional)**

if detection works:
```rust
    // --- hash tables ---

    #[test]
    fn test_hash_table_opaque() {
        let ctx = Context::new_standard().expect("standard context");
        ctx.evaluate("(import (srfi 69))").expect("import srfi-69");
        let result = ctx.evaluate("(make-hash-table)").expect("hash table");
        assert!(result.is_hash_table(), "expected HashTable, got {:?}", result);
    }
```

if detection is not feasible, write a documenting test:
```rust
    #[test]
    fn test_hash_table_falls_through_to_other() {
        // hash tables use a runtime-registered type tag from srfi-69
        // and cannot be reliably detected without module introspection.
        // they appear as Other and can still be passed back to scheme.
        let ctx = Context::new_standard().expect("standard context");
        ctx.evaluate("(import (srfi 69))").expect("import srfi-69");
        let result = ctx.evaluate("(make-hash-table)").expect("hash table");
        assert!(matches!(result, Value::Other(_)), "got {:?}", result);
    }
```

**step 5: write continuation documentation test**

```rust
    // --- continuations ---

    #[test]
    fn test_continuation_is_procedure() {
        // continuations in chibi are SEXP_PROCEDURE at the type level.
        // they're fully callable via Context::call, just like regular procedures.
        let ctx = Context::new_standard().expect("standard context");
        let result = ctx
            .evaluate("(call-with-current-continuation (lambda (k) k))")
            .expect("call/cc");
        assert!(result.is_procedure(), "expected Procedure, got {:?}", result);
    }
```

**step 6: run all tests**

run: `cargo test -p tein 2>&1 | tail -20`
expected: all pass

**step 7: commit**

```bash
git add tein/src/context.rs tein/vendor/chibi-scheme/tein_shim.c tein/src/ffi.rs tein/src/value.rs
git commit -m "test: port, hash table, continuation tests"
```

---

### task 10: update docs + GC safety comment

**files:**
- modify: `tein/src/value.rs` (GC safety comment)
- modify: `AGENTS.md` (Value enum description)
- modify: `TODO.md` (check off milestone item)

**step 1: update GC safety comment in value.rs**

add `sexp_make_bytes` to the allocating functions list in the comment block at the top of value.rs.

**step 2: update AGENTS.md**

update the `Value` enum description in the architecture section to mention the new types.

**step 3: update TODO.md**

check off `additional value types`:
```
- [x] **additional value types**
  - char, bytevector, port (opaque), hash table (opaque or Other fallback)
  - continuations already handled as Procedure (chibi uses same type tag)
```

**step 4: run full test suite**

run: `cargo test -p tein 2>&1 | tail -20`
expected: all pass

run: `cargo clippy -p tein 2>&1 | tail -10`
expected: no errors

**step 5: commit**

```bash
git add tein/src/value.rs AGENTS.md TODO.md
git commit -m "docs: update for additional value types, complete milestone 4b"
```
