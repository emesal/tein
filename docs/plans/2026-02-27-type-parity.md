# type parity implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** close every gap between `tein-sexp::SexpKind` and `tein::Value` — full r7rs numeric tower (bignum, rational, complex) and bytevector in SexpKind.

**Architecture:** bottom-up through 4 layers: chibi shim → ffi.rs → Value enum → tein-sexp. each layer is independently testable. the chibi layers (shim, ffi, Value) handle runtime chibi values; the tein-sexp layer handles parsed text. both must agree on variant names and representations so the future bridge (#36) is trivial.

**Tech Stack:** C (tein_shim.c in emesal/chibi-scheme fork), rust (tein crate ffi/value, tein-sexp crate ast/lexer/parser/printer)

**Design doc:** `docs/plans/2026-02-27-type-parity-design.md`
**Issue:** #71
**Base branch:** dev
**Branch:** `feature/numeric-tower-2602`

**Progress:** tasks 1-8 done. tasks 9-10 remain.
**Notes:**
- tasks 3-5 combined in one commit (value.rs non-exhaustive match forced all three together). `test_complex_from_scheme` uses `Context::new_standard()` — `make-rectangular` requires standard env.
- tasks 6-8 done in session 2: tein-sexp ast variants, lexer/parser for bignum/rational/bytevector/complex. 139 tein-sexp tests passing.
- commits: `2dd42f3` (task 6), `603df68` (task 7), `ff9f5a1` (task 8)

---

### ~~Task 1: chibi shim — numeric tower predicates & extractors~~ ✓

> done: pushed to emesal/chibi-scheme emesal-tein, rebuilt clean.

**Files:**
- Modify: `target/chibi-scheme/tein_shim.c` (chibi fork, branch `emesal-tein`)

This task is done in the **chibi-scheme fork repo** (`emesal/chibi-scheme`, branch `emesal-tein`), not the tein repo. push to the fork, then `just clean && cargo build` in tein to pick it up.

**Step 1: add predicate wrappers**

after the existing `tein_sexp_bytesp` block (~line 39), add:

```c
// numeric tower predicates (via tein shim)
int tein_sexp_bignump(sexp x) { return sexp_bignump(x); }
int tein_sexp_ratiop(sexp x) { return sexp_ratiop(x); }
int tein_sexp_complexp(sexp x) { return sexp_complexp(x); }
```

**Step 2: add bignum extractors**

```c
// bignum operations
int tein_sexp_bignum_sign(sexp x) { return sexp_bignum_sign(x); }

sexp tein_sexp_bignum_to_string(sexp ctx, sexp x) {
    sexp out = sexp_open_output_string(ctx);
    sexp_write_bignum(ctx, x, out, 10);
    return sexp_get_output_string(ctx, out);
}
```

**Step 3: add ratio extractors**

```c
// ratio operations
sexp tein_sexp_ratio_numerator(sexp x) { return sexp_ratio_numerator(x); }
sexp tein_sexp_ratio_denominator(sexp x) { return sexp_ratio_denominator(x); }
```

**Step 4: add complex extractors**

```c
// complex operations
sexp tein_sexp_complex_real(sexp x) { return sexp_complex_real(x); }
sexp tein_sexp_complex_imag(sexp x) { return sexp_complex_imag(x); }
```

**Step 5: add constructors for to_raw path**

```c
// numeric tower constructors
sexp tein_sexp_string_to_number(sexp ctx, sexp str, int base) {
    return sexp_string_to_number(ctx, str, sexp_make_fixnum(base));
}

sexp tein_sexp_make_ratio(sexp ctx, sexp num, sexp den) {
    return sexp_make_ratio(ctx, num, den);
}

sexp tein_sexp_make_complex(sexp ctx, sexp real, sexp imag) {
    return sexp_make_complex(ctx, real, imag);
}
```

**Step 6: commit and push to fork**

```bash
cd target/chibi-scheme
git add tein_shim.c
git commit -m "feat: numeric tower predicates, extractors, and constructors for tein"
git push origin emesal-tein
```

**Step 7: rebuild tein to verify shim compiles**

```bash
just clean && cargo build
```

Expected: clean build, no errors.

---

### ~~Task 2: ffi.rs — safe wrappers for numeric tower~~ ✓

**Files:**
- Modify: `tein/src/ffi.rs`

**Step 1: add extern declarations**

in the `extern "C"` block, after the bytevector declarations (~line 75), add:

```rust
    // numeric tower operations (via tein shim)
    pub fn tein_sexp_bignump(x: sexp) -> c_int;
    pub fn tein_sexp_ratiop(x: sexp) -> c_int;
    pub fn tein_sexp_complexp(x: sexp) -> c_int;
    pub fn tein_sexp_bignum_sign(x: sexp) -> c_int;
    pub fn tein_sexp_bignum_to_string(ctx: sexp, x: sexp) -> sexp;
    pub fn tein_sexp_ratio_numerator(x: sexp) -> sexp;
    pub fn tein_sexp_ratio_denominator(x: sexp) -> sexp;
    pub fn tein_sexp_complex_real(x: sexp) -> sexp;
    pub fn tein_sexp_complex_imag(x: sexp) -> sexp;
    pub fn tein_sexp_string_to_number(ctx: sexp, str: sexp, base: c_int) -> sexp;
    pub fn tein_sexp_make_ratio(ctx: sexp, num: sexp, den: sexp) -> sexp;
    pub fn tein_sexp_make_complex(ctx: sexp, real: sexp, imag: sexp) -> sexp;
```

**Step 2: add safe wrappers**

after the bytevector wrappers (~line 347), add a new section:

```rust
// numeric tower operations

#[inline]
pub unsafe fn sexp_bignump(x: sexp) -> c_int {
    unsafe { tein_sexp_bignump(x) }
}

#[inline]
pub unsafe fn sexp_ratiop(x: sexp) -> c_int {
    unsafe { tein_sexp_ratiop(x) }
}

#[inline]
pub unsafe fn sexp_complexp(x: sexp) -> c_int {
    unsafe { tein_sexp_complexp(x) }
}

#[inline]
pub unsafe fn sexp_bignum_sign(x: sexp) -> c_int {
    unsafe { tein_sexp_bignum_sign(x) }
}

/// converts a bignum to a decimal string sexp. allocates (opens string port).
#[inline]
pub unsafe fn sexp_bignum_to_string(ctx: sexp, x: sexp) -> sexp {
    unsafe { tein_sexp_bignum_to_string(ctx, x) }
}

#[inline]
pub unsafe fn sexp_ratio_numerator(x: sexp) -> sexp {
    unsafe { tein_sexp_ratio_numerator(x) }
}

#[inline]
pub unsafe fn sexp_ratio_denominator(x: sexp) -> sexp {
    unsafe { tein_sexp_ratio_denominator(x) }
}

#[inline]
pub unsafe fn sexp_complex_real(x: sexp) -> sexp {
    unsafe { tein_sexp_complex_real(x) }
}

#[inline]
pub unsafe fn sexp_complex_imag(x: sexp) -> sexp {
    unsafe { tein_sexp_complex_imag(x) }
}

/// parses a string sexp as a number in the given base. allocates.
#[inline]
pub unsafe fn sexp_string_to_number(ctx: sexp, s: sexp, base: c_int) -> sexp {
    unsafe { tein_sexp_string_to_number(ctx, s, base) }
}

#[inline]
pub unsafe fn sexp_make_ratio(ctx: sexp, num: sexp, den: sexp) -> sexp {
    unsafe { tein_sexp_make_ratio(ctx, num, den) }
}

#[inline]
pub unsafe fn sexp_make_complex(ctx: sexp, real: sexp, imag: sexp) -> sexp {
    unsafe { tein_sexp_make_complex(ctx, real, imag) }
}
```

**Step 3: verify build**

Run: `cargo build`
Expected: compiles cleanly.

**Step 4: lint**

Run: `just lint`
Expected: no warnings.

**Step 5: commit**

```
feat(ffi): safe wrappers for numeric tower — bignum, rational, complex (#71)
```

---

### ~~Task 3: Value enum — new variants + Display + PartialEq + accessors~~ ✓

**Files:**
- Modify: `tein/src/value.rs`

**Step 1: add variants to the Value enum**

in `pub enum Value` (value.rs:88), after `Float(f64)`:

```rust
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
```

**Step 2: add Display arms**

in the `fmt::Display` impl for Value, add arms (before the `Other` arm):

```rust
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
```

**Step 3: update PartialEq**

check existing PartialEq impl — if it's derived, the new variants are handled automatically. if manual, add arms. (current impl uses `#[derive(Clone)]` and manual PartialEq — check and update accordingly.)

**Step 4: add accessors**

after `as_bytevector` (~line 637):

```rust
    /// extract as bignum string, if this is a `Bignum`
    pub fn as_bignum(&self) -> Option<&str> {
        match self {
            Value::Bignum(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// extract rational components, if this is a `Rational`
    pub fn as_rational(&self) -> Option<(&Value, &Value)> {
        match self {
            Value::Rational(n, d) => Some((n.as_ref(), d.as_ref())),
            _ => None,
        }
    }

    /// extract complex components, if this is a `Complex`
    pub fn as_complex(&self) -> Option<(&Value, &Value)> {
        match self {
            Value::Complex(r, i) => Some((r.as_ref(), i.as_ref())),
            _ => None,
        }
    }
```

**Step 5: verify build + lint**

Run: `cargo build && just lint`
Expected: may have warnings about unhandled match arms in from_raw/to_raw — that's expected, fixed in tasks 4-5.

**Step 6: commit**

```
feat(value): add Bignum, Rational, Complex variants with Display + accessors (#71)
```

---

### ~~Task 4: Value — from_raw for numeric tower~~ ✓

**Files:**
- Modify: `tein/src/value.rs` (the `from_raw_depth` function)

**Step 1: reorder type checks and add numeric tower branches**

in `from_raw_depth` (value.rs:166), the new order is:

1. exception check (unchanged)
2. **complex** check (new — broadest numeric type)
3. **ratio** check (new)
4. **bignum** check (new)
5. flonum check (existing)
6. integer/fixnum check (existing)
7. everything else (existing, unchanged)

insert **before** the flonum check (~line 179):

```rust
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
    let str_sexp = ffi::sexp_bignum_to_string(ctx, raw);
    let str_ptr = ffi::sexp_string_data(str_sexp);
    let str_len = ffi::sexp_string_size(str_sexp);
    let bytes = std::slice::from_raw_parts(str_ptr as *const u8, str_len as usize);
    let s = String::from_utf8(bytes.to_vec())?;
    return Ok(Value::Bignum(s));
}
```

**Step 2: write tests**

in the `#[cfg(test)]` section of `tein/src/context.rs`:

```rust
#[test]
fn test_bignum_from_scheme() {
    let ctx = Context::new().expect("failed to create context");
    let result = ctx.evaluate("(expt 2 100)").expect("evaluation failed");
    match &result {
        Value::Bignum(s) => assert_eq!(s, "1267650600228229401496703205376"),
        other => panic!("expected Bignum, got {:?}", other),
    }
}

#[test]
fn test_bignum_negative() {
    let ctx = Context::new().expect("failed to create context");
    let result = ctx.evaluate("(- (expt 2 100))").expect("evaluation failed");
    match &result {
        Value::Bignum(s) => assert!(s.starts_with('-'), "expected negative, got {s}"),
        other => panic!("expected Bignum, got {:?}", other),
    }
}

#[test]
fn test_rational_from_scheme() {
    let ctx = Context::new().expect("failed to create context");
    let result = ctx.evaluate("(/ 1 3)").expect("evaluation failed");
    match &result {
        Value::Rational(n, d) => {
            assert_eq!(**n, Value::Integer(1));
            assert_eq!(**d, Value::Integer(3));
        }
        other => panic!("expected Rational, got {:?}", other),
    }
}

#[test]
fn test_rational_display() {
    let v = Value::Rational(Box::new(Value::Integer(1)), Box::new(Value::Integer(3)));
    assert_eq!(v.to_string(), "1/3");
}

#[test]
fn test_complex_from_scheme() {
    let ctx = Context::new().expect("failed to create context");
    let result = ctx
        .evaluate("(make-rectangular 1 2)")
        .expect("evaluation failed");
    match &result {
        Value::Complex(r, i) => {
            assert_eq!(**r, Value::Integer(1));
            assert_eq!(**i, Value::Integer(2));
        }
        other => panic!("expected Complex, got {:?}", other),
    }
}

#[test]
fn test_complex_display() {
    let v = Value::Complex(Box::new(Value::Integer(1)), Box::new(Value::Integer(2)));
    assert_eq!(v.to_string(), "1+2i");
}

#[test]
fn test_complex_negative_imag_display() {
    let v = Value::Complex(Box::new(Value::Integer(1)), Box::new(Value::Integer(-2)));
    assert_eq!(v.to_string(), "1-2i");
}

#[test]
fn test_bignum_display() {
    let v = Value::Bignum("1267650600228229401496703205376".to_string());
    assert_eq!(v.to_string(), "1267650600228229401496703205376");
}

#[test]
fn test_rational_with_bignum_components() {
    let ctx = Context::new().expect("failed to create context");
    let result = ctx
        .evaluate("(/ (expt 2 100) (expt 3 50))")
        .expect("evaluation failed");
    match &result {
        Value::Rational(_, _) => {} // just verify it parses as rational
        other => panic!("expected Rational, got {:?}", other),
    }
}
```

**Step 3: run tests**

Run: `cargo test --lib -- test_bignum test_rational test_complex`
Expected: all pass.

**Step 4: lint**

Run: `just lint`

**Step 5: commit**

```
feat(value): from_raw for bignum, rational, complex — broadest-first type check ordering (#71)
```

---

### ~~Task 5: Value — to_raw for numeric tower~~ ✓

**Files:**
- Modify: `tein/src/value.rs` (the `to_raw_depth` function)

**Step 1: add to_raw arms**

in `to_raw_depth`, after the `Value::Float` arm, add:

```rust
Value::Bignum(s) => {
    let c_str = std::ffi::CString::new(s.as_str())
        .map_err(|_| Error::TypeError("bignum string contains null bytes".to_string()))?;
    let str_sexp = ffi::sexp_c_str(ctx, c_str.as_ptr(), s.len() as ffi::sexp_sint_t);
    let result = ffi::sexp_string_to_number(ctx, str_sexp, 10);
    if ffi::sexp_exceptionp(result) != 0 {
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
```

**Step 2: write round-trip tests**

in `tein/src/context.rs` tests:

```rust
#[test]
fn test_bignum_to_raw_roundtrip() {
    unsafe extern "C" fn get_bignum(
        ctx_ptr: crate::ffi::sexp,
        _self: crate::ffi::sexp,
        _n: crate::ffi::sexp_sint_t,
        _args: crate::ffi::sexp,
    ) -> crate::ffi::sexp {
        unsafe {
            let val = Value::Bignum("1267650600228229401496703205376".to_string());
            val.to_raw(ctx_ptr)
                .unwrap_or_else(|_| crate::ffi::get_void())
        }
    }

    let ctx = Context::new().expect("failed to create context");
    ctx.define_fn_variadic("get-bignum", get_bignum)
        .expect("failed to define fn");
    let result = ctx.evaluate("(get-bignum)").expect("evaluation failed");
    match &result {
        Value::Bignum(s) => assert_eq!(s, "1267650600228229401496703205376"),
        other => panic!("expected Bignum, got {:?}", other),
    }
}

#[test]
fn test_rational_to_raw_roundtrip() {
    unsafe extern "C" fn get_rational(
        ctx_ptr: crate::ffi::sexp,
        _self: crate::ffi::sexp,
        _n: crate::ffi::sexp_sint_t,
        _args: crate::ffi::sexp,
    ) -> crate::ffi::sexp {
        unsafe {
            let val = Value::Rational(
                Box::new(Value::Integer(1)),
                Box::new(Value::Integer(3)),
            );
            val.to_raw(ctx_ptr)
                .unwrap_or_else(|_| crate::ffi::get_void())
        }
    }

    let ctx = Context::new().expect("failed to create context");
    ctx.define_fn_variadic("get-rational", get_rational)
        .expect("failed to define fn");
    let result = ctx.evaluate("(get-rational)").expect("evaluation failed");
    match &result {
        Value::Rational(n, d) => {
            assert_eq!(**n, Value::Integer(1));
            assert_eq!(**d, Value::Integer(3));
        }
        other => panic!("expected Rational, got {:?}", other),
    }
}

#[test]
fn test_complex_to_raw_roundtrip() {
    unsafe extern "C" fn get_complex(
        ctx_ptr: crate::ffi::sexp,
        _self: crate::ffi::sexp,
        _n: crate::ffi::sexp_sint_t,
        _args: crate::ffi::sexp,
    ) -> crate::ffi::sexp {
        unsafe {
            let val = Value::Complex(
                Box::new(Value::Integer(1)),
                Box::new(Value::Integer(2)),
            );
            val.to_raw(ctx_ptr)
                .unwrap_or_else(|_| crate::ffi::get_void())
        }
    }

    let ctx = Context::new().expect("failed to create context");
    ctx.define_fn_variadic("get-complex", get_complex)
        .expect("failed to define fn");
    let result = ctx.evaluate("(get-complex)").expect("evaluation failed");
    match &result {
        Value::Complex(r, i) => {
            assert_eq!(**r, Value::Integer(1));
            assert_eq!(**i, Value::Integer(2));
        }
        other => panic!("expected Complex, got {:?}", other),
    }
}
```

**Step 3: run tests**

Run: `cargo test --lib -- test_bignum test_rational test_complex`
Expected: all pass.

**Step 4: run full test suite to check for regressions**

Run: `just test`
Expected: all existing tests still pass. values that were previously `Other` for bignums/rationals/complex will now be properly typed — this is desired, not a regression. if any test asserted `Value::Other(...)` for these types, update it.

**Step 5: lint**

Run: `just lint`

**Step 6: commit**

```
feat(value): to_raw for bignum, rational, complex with GC-safe rooting (#71)
```

---

### ~~Task 6: tein-sexp — SexpKind new variants (ast, constructors, accessors, Display, PartialEq)~~ ✓

**Files:**
- Modify: `tein-sexp/src/ast.rs`

**Step 1: add variants to SexpKind**

in `pub enum SexpKind` (ast.rs:74), after `Char(char)`:

```rust
    /// bignum (arbitrary-precision integer, decimal string)
    Bignum(String),
    /// rational number `n/d`
    Rational(Box<Sexp>, Box<Sexp>),
    /// complex number `a+bi`
    Complex(Box<Sexp>, Box<Sexp>),
    /// bytevector `#u8(1 2 3)`
    Bytevector(Vec<u8>),
```

**Step 2: add constructors**

after `Sexp::nil()`:

```rust
    /// bignum (arbitrary-precision integer as decimal string)
    pub fn bignum(s: impl Into<String>) -> Self {
        Self::new(SexpKind::Bignum(s.into()))
    }

    /// rational number
    pub fn rational(numerator: Sexp, denominator: Sexp) -> Self {
        Self::new(SexpKind::Rational(Box::new(numerator), Box::new(denominator)))
    }

    /// complex number
    pub fn complex(real: Sexp, imag: Sexp) -> Self {
        Self::new(SexpKind::Complex(Box::new(real), Box::new(imag)))
    }

    /// bytevector
    pub fn bytevector(bytes: Vec<u8>) -> Self {
        Self::new(SexpKind::Bytevector(bytes))
    }
```

**Step 3: add accessors**

after `is_nil()`:

```rust
    /// extract as bignum string, if this is a `Bignum`
    pub fn as_bignum(&self) -> Option<&str> {
        match &self.kind {
            SexpKind::Bignum(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// extract rational components, if this is a `Rational`
    pub fn as_rational(&self) -> Option<(&Sexp, &Sexp)> {
        match &self.kind {
            SexpKind::Rational(n, d) => Some((n.as_ref(), d.as_ref())),
            _ => None,
        }
    }

    /// extract complex components, if this is a `Complex`
    pub fn as_complex(&self) -> Option<(&Sexp, &Sexp)> {
        match &self.kind {
            SexpKind::Complex(r, i) => Some((r.as_ref(), i.as_ref())),
            _ => None,
        }
    }

    /// extract as bytevector slice, if this is a `Bytevector`
    pub fn as_bytevector(&self) -> Option<&[u8]> {
        match &self.kind {
            SexpKind::Bytevector(b) => Some(b.as_slice()),
            _ => None,
        }
    }
```

**Step 4: add Display arms**

in `impl fmt::Display for Sexp`, before the `SexpKind::Nil` arm:

```rust
SexpKind::Bignum(s) => write!(f, "{s}"),
SexpKind::Rational(n, d) => write!(f, "{n}/{d}"),
SexpKind::Complex(r, i) => {
    write!(f, "{r}")?;
    let imag_str = format!("{i}");
    if imag_str.starts_with('-') || imag_str.starts_with('+') {
        write!(f, "{imag_str}i")
    } else {
        write!(f, "+{imag_str}i")
    }
}
SexpKind::Bytevector(bytes) => {
    write!(f, "#u8(")?;
    for (idx, b) in bytes.iter().enumerate() {
        if idx > 0 {
            write!(f, " ")?;
        }
        write!(f, "{b}")?;
    }
    write!(f, ")")
}
```

**Step 5: add Serialize arms**

in the `serde::Serialize` impl, before `SexpKind::Nil`:

```rust
SexpKind::Bignum(s) => serializer.serialize_str(s),
SexpKind::Rational(n, d) => {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("numerator", n.as_ref())?;
    map.serialize_entry("denominator", d.as_ref())?;
    map.end()
}
SexpKind::Complex(r, i) => {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("real", r.as_ref())?;
    map.serialize_entry("imag", i.as_ref())?;
    map.end()
}
SexpKind::Bytevector(bytes) => {
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(bytes.len()))?;
    for b in bytes {
        seq.serialize_element(b)?;
    }
    seq.end()
}
```

**Step 6: add tests**

in the `#[cfg(test)]` module of ast.rs:

```rust
#[test]
fn display_bignum() {
    assert_eq!(Sexp::bignum("12345678901234567890").to_string(), "12345678901234567890");
    assert_eq!(Sexp::bignum("-99999").to_string(), "-99999");
}

#[test]
fn display_rational() {
    let r = Sexp::rational(Sexp::integer(1), Sexp::integer(3));
    assert_eq!(r.to_string(), "1/3");
}

#[test]
fn display_complex() {
    let c = Sexp::complex(Sexp::integer(1), Sexp::integer(2));
    assert_eq!(c.to_string(), "1+2i");

    let c_neg = Sexp::complex(Sexp::integer(1), Sexp::integer(-2));
    assert_eq!(c_neg.to_string(), "1-2i");
}

#[test]
fn display_bytevector() {
    assert_eq!(Sexp::bytevector(vec![1, 2, 3]).to_string(), "#u8(1 2 3)");
    assert_eq!(Sexp::bytevector(vec![]).to_string(), "#u8()");
}

#[test]
fn accessors_new_types() {
    assert_eq!(Sexp::bignum("42").as_bignum(), Some("42"));
    assert!(Sexp::rational(Sexp::integer(1), Sexp::integer(2)).as_rational().is_some());
    assert!(Sexp::complex(Sexp::integer(1), Sexp::integer(2)).as_complex().is_some());
    assert_eq!(Sexp::bytevector(vec![1, 2]).as_bytevector(), Some([1u8, 2].as_slice()));
}

#[test]
fn equality_new_types() {
    assert_eq!(Sexp::bignum("123"), Sexp::bignum("123"));
    assert_ne!(Sexp::bignum("123"), Sexp::bignum("456"));
    assert_eq!(
        Sexp::rational(Sexp::integer(1), Sexp::integer(3)),
        Sexp::rational(Sexp::integer(1), Sexp::integer(3)),
    );
    assert_eq!(Sexp::bytevector(vec![1, 2]), Sexp::bytevector(vec![1, 2]));
}
```

**Step 7: run tests + lint**

Run: `cargo test -p tein-sexp && just lint`
Expected: all pass.

**Step 8: commit**

```
feat(tein-sexp): add Bignum, Rational, Complex, Bytevector to SexpKind (#71)
```

---

### ~~Task 7: tein-sexp — lexer/parser for bignum, rational, and bytevector literals~~ ✓

**Files:**
- Modify: `tein-sexp/src/lexer.rs`
- Modify: `tein-sexp/src/parser.rs`

**Step 1: add new token variants to TokenKind**

in `pub enum TokenKind` (lexer.rs:22):

```rust
    /// bignum literal (integer that overflows i64)
    Bignum(String),
    /// rational literal (numerator string, denominator string)
    Rational(String, String),
    /// `#u8(`
    HashU8Paren,
```

**Step 2: update `lex_number` for bignum overflow**

in `lex_number` (lexer.rs:670), the integer branch currently does:

```rust
let val: i64 = text.parse().map_err(|_| { ... })?;
Ok(TokenKind::Integer(val))
```

change to:

```rust
match text.parse::<i64>() {
    Ok(val) => Ok(TokenKind::Integer(val)),
    Err(_) => Ok(TokenKind::Bignum(text.to_string())),
}
```

**Step 3: update `lex_number` for rationals**

after parsing the integer part, before returning, check for `/` followed by digits:

```rust
// check for rational: integer `/` integer (no whitespace)
if !is_float && self.peek_char() == Some('/') {
    let slash_pos = self.pos;
    self.advance(); // consume /
    let den_start = self.pos;
    // optional sign on denominator
    if self.peek_char() == Some('+') || self.peek_char() == Some('-') {
        self.advance();
    }
    while let Some(c) = self.peek_char() {
        if c.is_ascii_digit() {
            self.advance();
        } else {
            break;
        }
    }
    if self.pos > den_start {
        let num_str = self.input[start..slash_pos].to_string();
        let den_str = self.input[den_start..self.pos].to_string();
        return Ok(TokenKind::Rational(num_str, den_str));
    } else {
        // no digits after / — backtrack, treat / as part of next token
        self.pos = slash_pos;
    }
}
```

this must come before the integer/bignum parse at the end of the function.

**Step 4: add `#u8(` to hash-prefix dispatch**

find where `#(` produces `HashParen` in the lexer (in the `#` handler). add before or after:

```rust
'u' => {
    // check for #u8(
    if self.peek_char2() == Some('8') {
        self.advance(); // consume u
        self.advance(); // consume 8
        if self.peek_char() == Some('(') {
            self.advance(); // consume (
            return Ok(TokenKind::HashU8Paren);
        }
        // not #u8( — fall through to symbol or error
    }
    // fall through
}
```

**Step 5: update parser for new tokens**

in `parser.rs`, in the main `parse_expr` or equivalent match:

```rust
TokenKind::Bignum(s) => SexpKind::Bignum(s),
TokenKind::Rational(n, d) => {
    // parse numerator and denominator as integer or bignum
    let num = match n.parse::<i64>() {
        Ok(v) => Sexp::integer(v),
        Err(_) => Sexp::bignum(n),
    };
    let den = match d.parse::<i64>() {
        Ok(v) => Sexp::integer(v),
        Err(_) => Sexp::bignum(d),
    };
    SexpKind::Rational(Box::new(num), Box::new(den))
}
TokenKind::HashU8Paren => {
    // parse bytes until )
    let mut bytes = Vec::new();
    loop {
        match self.peek_token()?.kind {
            TokenKind::RightParen => { self.advance(); break; }
            TokenKind::Integer(n) => {
                if n < 0 || n > 255 {
                    return Err(ParseError::new(
                        format!("bytevector element out of range: {n}"),
                        self.current_span(),
                    ));
                }
                bytes.push(n as u8);
                self.advance();
            }
            _ => return Err(ParseError::new(
                "expected integer or ) in bytevector".to_string(),
                self.current_span(),
            )),
        }
    }
    SexpKind::Bytevector(bytes)
}
```

**Step 6: write tests**

lexer tests:
```rust
#[test]
fn lex_bignum() {
    let tokens = tokenize("99999999999999999999999999");
    assert!(matches!(&tokens[0].kind, TokenKind::Bignum(s) if s == "99999999999999999999999999"));
}

#[test]
fn lex_negative_bignum() {
    let tokens = tokenize("-99999999999999999999999999");
    assert!(matches!(&tokens[0].kind, TokenKind::Bignum(s) if s == "-99999999999999999999999999"));
}

#[test]
fn lex_rational() {
    let tokens = tokenize("3/4");
    assert!(matches!(&tokens[0].kind, TokenKind::Rational(n, d) if n == "3" && d == "4"));
}

#[test]
fn lex_negative_rational() {
    let tokens = tokenize("-1/2");
    assert!(matches!(&tokens[0].kind, TokenKind::Rational(n, d) if n == "-1" && d == "2"));
}

#[test]
fn lex_bytevector_prefix() {
    let tokens = tokenize("#u8(1 2 3)");
    assert!(matches!(tokens[0].kind, TokenKind::HashU8Paren));
    assert!(matches!(tokens[1].kind, TokenKind::Integer(1)));
}
```

parser tests:
```rust
#[test]
fn parse_bignum() {
    let sexp = parse_one("99999999999999999999999999");
    assert_eq!(sexp.as_bignum(), Some("99999999999999999999999999"));
}

#[test]
fn parse_rational() {
    let sexp = parse_one("3/4");
    let (n, d) = sexp.as_rational().unwrap();
    assert_eq!(n.as_integer(), Some(3));
    assert_eq!(d.as_integer(), Some(4));
}

#[test]
fn parse_bytevector() {
    let sexp = parse_one("#u8(1 2 3)");
    assert_eq!(sexp.as_bytevector(), Some([1u8, 2, 3].as_slice()));
}

#[test]
fn parse_bytevector_empty() {
    let sexp = parse_one("#u8()");
    assert_eq!(sexp.as_bytevector(), Some([].as_slice()));
}

#[test]
fn roundtrip_bignum() {
    assert_eq!(parse_one("99999999999999999999999999").to_string(), "99999999999999999999999999");
}

#[test]
fn roundtrip_rational() {
    assert_eq!(parse_one("3/4").to_string(), "3/4");
}

#[test]
fn roundtrip_bytevector() {
    assert_eq!(parse_one("#u8(1 2 3)").to_string(), "#u8(1 2 3)");
}
```

**Step 7: run tests + lint**

Run: `cargo test -p tein-sexp && just lint`

**Step 8: commit**

```
feat(tein-sexp): lexer/parser for bignum, rational, bytevector literals (#71)
```

---

### ~~Task 8: tein-sexp — complex number lexing/parsing~~ ✓

**Files:**
- Modify: `tein-sexp/src/lexer.rs`
- Modify: `tein-sexp/src/parser.rs`

separated from task 7 because complex number syntax is the most intricate part of r7rs number grammar.

**Step 1: extend lex_number for complex suffix**

after parsing a real number (integer, float, or bignum), check for `+`/`-` followed by another number and `i`:

patterns to handle:
- `1+2i` — integer real + integer imag
- `1-2i` — integer real + negative integer imag
- `1.0+2.0i` — float real + float imag
- `+2i` — pure imaginary (real = 0)
- `-3i` — pure negative imaginary (real = 0)
- `+i` is `0+1i`, `-i` is `0-1i`

add a new token variant:

```rust
    /// complex literal (real part string, imaginary part string including sign)
    Complex(String, String),
```

in `lex_number`, after determining the real part text, check:
1. if next char is `+` or `-` and not at delimiter
2. scan the imaginary part (digits, optional `.`, optional `e`)
3. if it ends with `i`, emit `TokenKind::Complex(real_str, imag_str)`
4. special case: if just `+i` or `-i` after real, imag is `1` or `-1`
5. special case: if no real part before `+Ni`, real is `"0"`

**Step 2: parser creates SexpKind::Complex**

```rust
TokenKind::Complex(real_str, imag_str) => {
    let real = parse_number_string(&real_str)?;
    let imag = parse_number_string(&imag_str)?;
    SexpKind::Complex(Box::new(real), Box::new(imag))
}
```

where `parse_number_string` tries i64 → bignum → f64 in order.

**Step 3: write tests**

```rust
#[test]
fn parse_complex_integers() {
    let sexp = parse_one("1+2i");
    let (r, i) = sexp.as_complex().unwrap();
    assert_eq!(r.as_integer(), Some(1));
    assert_eq!(i.as_integer(), Some(2));
}

#[test]
fn parse_complex_negative_imag() {
    let sexp = parse_one("1-2i");
    let (r, i) = sexp.as_complex().unwrap();
    assert_eq!(r.as_integer(), Some(1));
    assert_eq!(i.as_integer(), Some(-2));
}

#[test]
fn parse_pure_imaginary() {
    let sexp = parse_one("+2i");
    let (r, i) = sexp.as_complex().unwrap();
    assert_eq!(r.as_integer(), Some(0));
    assert_eq!(i.as_integer(), Some(2));
}

#[test]
fn parse_plus_i() {
    let sexp = parse_one("+i");
    let (r, i) = sexp.as_complex().unwrap();
    assert_eq!(r.as_integer(), Some(0));
    assert_eq!(i.as_integer(), Some(1));
}

#[test]
fn parse_minus_i() {
    let sexp = parse_one("-i");
    let (r, i) = sexp.as_complex().unwrap();
    assert_eq!(r.as_integer(), Some(0));
    assert_eq!(i.as_integer(), Some(-1));
}

#[test]
fn roundtrip_complex() {
    assert_eq!(parse_one("1+2i").to_string(), "1+2i");
    assert_eq!(parse_one("1-2i").to_string(), "1-2i");
}
```

**Step 4: run tests + lint**

Run: `cargo test -p tein-sexp && just lint`

**Step 5: commit**

```
feat(tein-sexp): complex number lexing and parsing (#71)
```

---

### Task 9: scheme-level integration tests

**Files:**
- Create: `tein/tests/scheme/numeric_tower.scm`
- Possibly modify: `tein/tests/scheme_tests.rs` (if .scm files aren't auto-discovered)

**Step 1: check how scheme tests are discovered**

read `tein/tests/scheme_tests.rs` to understand whether new `.scm` files are auto-discovered or must be listed explicitly.

**Step 2: write scheme test file**

```scheme
;; numeric tower tests

;; bignums
(test-equal (expt 2 100) 1267650600228229401496703205376)
(test-true (integer? (expt 2 100)))
(test-true (exact? (expt 2 100)))

;; rationals
(test-equal (/ 1 3) 1/3)
(test-true (rational? (/ 1 3)))
(test-true (exact? (/ 1 3)))

;; complex
(test-equal (make-rectangular 1 2) 1+2i)
(test-true (complex? (make-rectangular 1 2)))

;; arithmetic with bignums
(test-equal (+ (expt 2 100) 1) 1267650600228229401496703205377)

;; rational simplification
(test-equal (/ 2 4) 1/2)
```

**Step 3: run scheme tests**

Run: `cargo test -p tein -- scheme`
Expected: all pass.

**Step 4: commit**

```
test: scheme-level numeric tower integration tests (#71)
```

---

### Task 10: full test suite + AGENTS.md updates + final commit

**Files:**
- Modify: `AGENTS.md`

**Step 1: run full test suite**

Run: `just test`
Expected: all tests pass, no regressions.

**Step 2: lint**

Run: `just lint`

**Step 3: update AGENTS.md**

- add `Bignum`, `Rational`, `Complex` to the Value enum listing in the architecture section
- update the `from_raw` type check ordering note: `complex → ratio → bignum → flonum → integer`
- update "adding a new scheme type" checklist if the process changed
- add note about `sexp_string_to_number` / `sexp_bignum_to_string` shim functions
- note the chibi safety invariant: type check ordering matters for numeric tower (complex > ratio > bignum > flonum > integer)

**Step 4: commit**

```
docs: update AGENTS.md with numeric tower variants and type check ordering (#71)

closes #71
```

---

### summary

| task | layer | what |
|------|-------|------|
| 1 | C shim (fork) | predicates, extractors, constructors |
| 2 | ffi.rs | safe wrappers |
| 3 | value.rs | new variants, Display, accessors |
| 4 | value.rs | from_raw (broadest-first ordering) |
| 5 | value.rs | to_raw (with GC rooting) |
| 6 | tein-sexp ast.rs | SexpKind variants, constructors, Display, serde |
| 7 | tein-sexp lexer/parser | bignum, rational, bytevector literals |
| 8 | tein-sexp lexer/parser | complex number literals |
| 9 | scheme tests | integration tests |
| 10 | docs | AGENTS.md updates, final validation |

### notes for AGENTS.md (collected during planning)

- **numeric tower type check ordering**: `from_raw` must check `complex → ratio → bignum → flonum → integer` (broadest first). document alongside existing flonum-before-integer note.
- **`sexp_string_to_number`**: used for `Bignum::to_raw`. takes a scheme string + base, returns the parsed number. allocates.
- **`sexp_bignum_to_string`**: shim function that opens a string port and writes via `sexp_write_bignum`. allocates.
- **gc rooting in to_raw for composite types**: when converting `Rational` or `Complex`, the first component must be GC-rooted before converting the second (both `sexp_make_ratio` and `sexp_make_complex` may allocate).
