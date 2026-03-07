# `(tein json)` — built-in JSON module

**issue**: #36
**date**: 2026-02-27
**status**: implemented

## overview

bidirectional JSON ↔ scheme conversion as a built-in `tein` module. the implementation
lives in the `tein` crate, using `serde_json` for parsing/serialising and a new
`Value ↔ tein_sexp::Sexp` bridge for type conversion.

## architecture

```
json string                          chibi sexp
    ↓ serde_json::from_str               ↑ Value::to_raw()
serde_json::Value                    tein::Value
    ↓ serde (Deserialize)                ↑ bridge::sexp_to_value()
tein_sexp::Sexp ──────────────────→ tein_sexp::Sexp
```

reverse path for stringify. the bridge is reusable — future format modules
(`(tein toml)`, `(tein yaml)`, etc.) plug in at the serde layer and share
everything below it.

## decisions

### representation choices

| JSON type | scheme representation | rationale |
|-----------|----------------------|-----------|
| object `{}` | alist `((key . val) ...)` | keys are strings. buildable with cons/null, idiomatic scheme, O(n) lookup acceptable for typical JSON sizes |
| array `[]` | list `(...)` | natural mapping |
| string | string | direct |
| number (integer) | integer (fixnum or bignum) | full numeric tower via #71 |
| number (float) | flonum | direct |
| `true`/`false` | `#t`/`#f` | direct |
| `null` | `'null` symbol | distinguishes from `'()` (empty array). standard approach in scheme JSON libraries |

### no `json?` predicate

there's no meaningful type to test — scheme values *are* the intermediate
representation. "can this round-trip through json?" is a capability check better
expressed as try/catch around `json-stringify`.

### no `(tein serde)` scheme module

the shared layer is rust-internal only (the `Value ↔ Sexp` bridge). each format
module exposes its own scheme API directly. this avoids committing to a generic
dispatch protocol and keeps the scheme API surface minimal.

### built-in, not cdylib

the `Value ↔ Sexp` bridge needs access to both `tein::Value` and `tein_sexp::Sexp`,
which are only co-available in the `tein` crate. making this a cdylib would require
either duplicating the bridge in the ext API vtable or bypassing it entirely.
built-in is simpler, and json is high-value enough to justify the dependency.

### sandbox safety

pure data conversion — no IO, no filesystem, no side effects. safe for all sandbox
presets including the most restrictive.

## scheme API

```scheme
(import (tein json))

(json-parse "{\"name\": \"tein\", \"version\": 1}")
;; => (("name" . "tein") ("version" . 1))

(json-stringify '(("name" . "tein") ("version" . 1)))
;; => "{\"name\":\"tein\",\"version\":1}"

(json-parse "null")   ;; => null (symbol)
(json-parse "[]")     ;; => ()
(json-parse "[1,2]")  ;; => (1 2)
```

## components

### 1. prerequisite: type parity (#71)

extend `tein-sexp::SexpKind` with `Bignum(String)`, `Rational`, `Complex`,
`Bytevector`. extend `tein::Value` with matching variants. add chibi shim
predicates/extractors. see #71 for full scope.

### 2. `Value ↔ Sexp` bridge (`src/sexp_bridge.rs`)

new module in the `tein` crate. two public functions:

- `value_to_sexp(value: &Value) -> Result<Sexp>`
- `sexp_to_value(sexp: &Sexp) -> Result<Value>`

mapping:

| `Value` | `SexpKind` | notes |
|---------|------------|-------|
| `Integer` ↔ `Integer` | direct |
| `Float` ↔ `Float` | direct |
| `Bignum` ↔ `Bignum` | direct (string repr) |
| `Rational` ↔ `Rational` | recursive on components |
| `Complex` ↔ `Complex` | recursive on components |
| `String` ↔ `String` | direct |
| `Symbol` ↔ `Symbol` | direct |
| `Boolean` ↔ `Boolean` | direct |
| `Char` ↔ `Char` | direct |
| `List` ↔ `List` | recursive |
| `Vector` ↔ `Vector` | recursive |
| `Bytevector` ↔ `Bytevector` | direct |
| `Nil` ↔ `Nil` | direct |
| `Pair(a, b)` → `DottedList` | flatten right-recursive pairs into head + tail |
| `DottedList` → `Pair` | nest into right-recursive pairs |
| `Unspecified`, `Procedure`, `Port`, `HashTable`, `Foreign`, `Other` | → error |

depth limit: reuse existing `MAX_DEPTH` constant.

### 3. `(tein json)` module

**new dependencies for `tein` crate:**
- `serde = { version = "1", features = ["derive"] }`
- `serde_json = "1"`
- `tein-sexp = { path = "../tein-sexp", features = ["serde"] }`

**implementation** (in `src/json.rs` or similar):

`json_parse(input: &str) -> Result<Value>`:
1. `serde_json::from_str::<tein_sexp::Sexp>(input)?` — json → sexp via tein-sexp's serde visitor
2. custom handling: remap `Sexp::Nil` from json null → `Sexp::Symbol("null")`
3. `sexp_to_value(&sexp)?` — sexp → value via bridge

`json_stringify(value: &Value) -> Result<String>`:
1. `value_to_sexp(value)?` — value → sexp via bridge
2. custom handling: remap `Sexp::Symbol("null")` → json null
3. `serde_json::to_string(&sexp)?` — sexp → json via tein-sexp's serde Serialize

**registration**: VFS module `(tein json)` with `json-parse` and `json-stringify`
as native fns, registered in `context.rs` alongside existing built-in modules.

### 4. error handling

- `json-parse` on invalid JSON → scheme string with error message (per existing tein `Result::Err` convention)
- `json-stringify` on unconvertible value (procedure, port, etc.) → scheme string with error message
- depth limit on bridge conversion → error

### 5. testing

- round-trip tests: json → scheme → json for all JSON types
- `'null` vs `'()` distinction preserved through round-trip
- nested objects/arrays, unicode strings, empty containers
- large integers (bignum round-trip once #71 lands)
- error cases: invalid JSON, unconvertible scheme values
- integration tests via `tests/scheme_tests.rs` pattern with `.scm` test files
- scheme-level tests in `tests/scheme/json.scm`

## dependency chain

```
#71 (type parity) → bridge (src/sexp_bridge.rs) → (tein json) #36
                                                 → (tein toml) future
                                                 → (tein yaml) future
                                                 → #72 (format survey)
```

## future considerations

- `(tein json)` could gain `json-parse-port` for streaming if needed
- pretty-printing option for `json-stringify` (optional indent parameter)
- the bridge enables any serde format with ~20 lines of glue per format
