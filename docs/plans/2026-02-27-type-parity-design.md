# close tein-sexp ↔ chibi-scheme type parity gap

**issue**: #71
**date**: 2026-02-27
**status**: design approved
**blocks**: #36 `(tein json)`, #72 format module survey

## overview

`tein-sexp::SexpKind` and `tein::Value` must represent identical sets of scheme data
types for the `Value ↔ Sexp` bridge. this issue closes every gap: the full r7rs
numeric tower (bignum, rational, complex) and bytevector in `SexpKind`.

## new variants

### `tein::Value`

```rust
Bignum(String),                          // arbitrary-precision integer, decimal string
Rational(Box<Value>, Box<Value>),        // numerator/denominator (exact integers)
Complex(Box<Value>, Box<Value>),         // real/imaginary (any real numbers)
```

### `tein_sexp::SexpKind`

```rust
Bignum(String),                          // decimal string, no num-bigint dep
Rational(Box<Sexp>, Box<Sexp>),          // numerator/denominator
Complex(Box<Sexp>, Box<Sexp>),           // real/imag
Bytevector(Vec<u8>),                     // r7rs #u8(...)
```

## layer-by-layer design

### 1. `tein_shim.c` — predicates, extractors, constructors

new exports (all thin wrappers around chibi macros/functions):

| function | signature | notes |
|----------|-----------|-------|
| `tein_sexp_bignump` | `int (sexp x)` | predicate |
| `tein_sexp_ratiop` | `int (sexp x)` | predicate |
| `tein_sexp_complexp` | `int (sexp x)` | predicate |
| `tein_sexp_bignum_to_string` | `sexp (sexp ctx, sexp x)` | opens string port, calls `sexp_write_bignum`, returns scheme string |
| `tein_sexp_bignum_sign` | `int (sexp x)` | returns sign (1 or -1) |
| `tein_sexp_ratio_numerator` | `sexp (sexp x)` | component (fixnum or bignum) |
| `tein_sexp_ratio_denominator` | `sexp (sexp x)` | component (fixnum or bignum) |
| `tein_sexp_complex_real` | `sexp (sexp x)` | component (any real) |
| `tein_sexp_complex_imag` | `sexp (sexp x)` | component (any real) |
| `tein_sexp_string_to_number` | `sexp (sexp ctx, sexp str, int base)` | chibi's reader-based number parser |
| `tein_sexp_make_ratio` | `sexp (sexp ctx, sexp num, sexp den)` | constructor |
| `tein_sexp_make_complex` | `sexp (sexp ctx, sexp real, sexp imag)` | constructor |

`bignum_to_string` is the only non-trivial shim — it opens a string output port,
calls `sexp_write_bignum(ctx, x, port, 10)`, and returns `sexp_get_output_string(ctx, port)`.
all port plumbing stays in C.

### 2. `ffi.rs` — safe wrappers

one `extern "C"` declaration + safe wrapper per shim function. follows existing
pattern (unsafe block, thin delegation). no new abstractions needed.

### 3. `Value` — `from_raw` type check ordering

broadest-first to avoid false matches:

```
complex → ratio → bignum → flonum → integer → (rest unchanged)
```

rationale:
- complex subsumes all reals (chibi stores reals in complex slots)
- ratio components can be bignums
- bignum must precede integer (integer predicate only handles fixnums)
- flonum before integer (existing invariant — `sexp_integerp` matches some floats)

implementation:
- `Bignum`: call `bignum_to_string(ctx, raw)`, extract scheme string → `Value::Bignum(s)`
- `Rational`: extract numerator/denominator via shim, recursive `from_raw_depth` on each
- `Complex`: extract real/imag via shim, recursive `from_raw_depth` on each

gc safety: `bignum_to_string` allocates (string port + output string). root the bignum
if needed. ratio/complex component extraction is non-allocating (field access), but
the recursive `from_raw_depth` calls may allocate — root the parent sexp.

### 4. `Value` — `to_raw` conversions

- `Bignum(s)` → create scheme string via `sexp_c_str`, pass to `sexp_string_to_number(ctx, str, 10)`
- `Rational(n, d)` → recursive `to_raw_depth` on components → `sexp_make_ratio(ctx, num, den)`
- `Complex(r, i)` → recursive `to_raw_depth` on components → `sexp_make_complex(ctx, real, imag)`

gc safety: `sexp_string_to_number` allocates. for rational/complex, the first component's
`to_raw` result must be rooted before converting the second component.

### 5. `Value` — `Display`, `PartialEq`

display:
- `Bignum(s)` → `{s}` (the string is already decimal)
- `Rational(n, d)` → `{n}/{d}`
- `Complex(r, i)` → `{r}+{i}i` (handle negative imag: `{r}{i}i`, no double sign)

`PartialEq`: derive-compatible — `Bignum` compares strings, `Rational`/`Complex` compare
components structurally. note: `"01"` ≠ `"1"` — the string comes from chibi's printer
which always normalises, so this is fine in practice.

### 6. `tein-sexp` — `SexpKind` additions

four new variants with constructors, accessors, `Display`, `PartialEq`, serde impls.

display formats (scheme-compatible):
- `Bignum("123456")` → `123456`
- `Rational(3, 4)` → `3/4`
- `Complex(1, 2)` → `1+2i` (negative imag: `1-2i`)
- `Bytevector([1, 2, 3])` → `#u8(1 2 3)`

convenience constructors on `Sexp`:
- `Sexp::bignum(s: impl Into<String>)`
- `Sexp::rational(num: Sexp, den: Sexp)`
- `Sexp::complex(real: Sexp, imag: Sexp)`
- `Sexp::bytevector(bytes: Vec<u8>)`

### 7. `tein-sexp` — lexer/parser additions

- **bignums**: integers that overflow `i64` → `SexpKind::Bignum(String)` (currently these would fail or truncate)
- **rationals**: `3/4`, `-1/2` — integer `/` integer with no whitespace
- **complex**: `1+2i`, `3-4i`, `+inf.0+nan.0i`, pure imaginary `+2i`
- **bytevectors**: `#u8(` prefix → parse unsigned bytes → `SexpKind::Bytevector`

printer: emit scheme-compatible output for each new variant.

### 8. `tein-sexp` — serde additions

serialise/deserialise the new variants. representation choices for serde formats:
- `Bignum` → string (lossless, since serde doesn't have arbitrary-precision integers natively)
- `Rational` → map `{"numerator": ..., "denominator": ...}` or tagged
- `Complex` → map `{"real": ..., "imag": ...}` or tagged
- `Bytevector` → byte array / base64 depending on format

these can be refined when `(tein json)` is implemented — the serde layer is internal
to the bridge, not a public API commitment.

## `Pair` ↔ `DottedList` — no new work

`Value::Pair(a, b)` maps to `SexpKind::DottedList(vec, tail)` structurally. the
bridge (future #36 work) handles flattening/nesting. no changes needed in this issue.

## testing

### chibi layer (tein crate)
- `from_raw` / `to_raw` round-trips for each new type
- `(expt 2 100)` → `Value::Bignum`
- `(/ 1 3)` → `Value::Rational`
- `(make-rectangular 1 2)` → `Value::Complex`
- edge cases: negative bignums, `0+0i`, ratio with bignum components
- display format correctness

### tein-sexp layer
- parse/print round-trips for bignum literals, rationals, complex, bytevectors
- overflow: `99999999999999999999` → `Bignum` not `Integer`
- serde round-trips (if serde feature enabled)

### scheme-level (integration)
- scheme test file exercising numeric tower through evaluate/return cycle

## AGENTS.md updates

after implementation:
- add `Bignum`, `Rational`, `Complex` to the `Value` enum listing
- update `from_raw` type check ordering documentation
- update "adding a new scheme type" checklist if the process changes
- document `sexp_string_to_number` in the shim exports
