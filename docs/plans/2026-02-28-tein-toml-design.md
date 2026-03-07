# `(tein toml)` — TOML parsing and serialisation

closes #77

## summary

add `(tein toml)` as a built-in format module, following the pattern established by
`(tein json)`. TOML tables become alists, arrays become lists, datetimes use a tagged
representation for round-trip fidelity.

## representation

| TOML            | scheme                              | notes                              |
|-----------------|-------------------------------------|------------------------------------|
| table `{}`      | alist `((key . val) ...)`           | keys are always strings            |
| array `[]`      | list `(...)`                        |                                    |
| empty table     | `'()`                               | same ambiguity as json — accepted  |
| empty array     | `'()`                               |                                    |
| string          | string                              |                                    |
| integer         | integer                             | TOML integers are i64              |
| float           | flonum                              | includes inf, nan                  |
| boolean         | `#t / #f`                           |                                    |
| offset datetime | `(toml-datetime "1979-05-27T07:32:00Z")` | tagged list                   |
| local datetime  | `(toml-datetime "1979-05-27T07:32:00")`  | no offset                     |
| local date      | `(toml-datetime "1979-05-27")`           |                               |
| local time      | `(toml-datetime "07:32:00")`             |                               |

`toml-datetime` is a symbol tag. all four TOML datetime variants use the same tag —
the string content distinguishes them. `toml::Datetime::to_string()` produces the
canonical form; `toml::Datetime::from_str()` parses it back.

### inf/nan

TOML supports `inf`, `-inf`, `nan`. scheme has `+inf.0`, `-inf.0`, `+nan.0`.
these map directly via `Value::Float(f64)` — no special handling at the Value level.
stringify emits TOML syntax (`inf` not `+inf.0`).

### future: SRFI-19 dates

when #84 lands, `(tein toml)` could gain optional SRFI-19 date parsing. the tagged
representation is forward-compatible — a future version could accept either tagged
strings or SRFI-19 date objects for stringify, and offer a parse mode that returns
date objects instead of tagged strings.

## architecture

mirrors `(tein json)` exactly:

```
tein/src/toml.rs          — toml_parse + toml_stringify_raw
tein/src/context.rs       — trampolines + register_toml_module()
target/chibi-scheme/
  lib/tein/toml.sld       — module definition
  lib/tein/toml.scm       — module documentation
tein/build.rs             — VFS gating behind "toml" feature
```

### parse path

```
TOML string
  → toml::Value          (toml crate, via Value::from_str)
  → tein::Value          (recursive mapping)
```

`toml::Value` variant mapping:
- `String(s)` → `Value::String(s)`
- `Integer(i)` → `Value::Integer(i)`
- `Float(f)` → `Value::Float(f)`
- `Boolean(b)` → `Value::Boolean(b)`
- `Datetime(dt)` → `Value::List([Value::Symbol("toml-datetime"), Value::String(dt.to_string())])`
- `Array(arr)` → `Value::List(items)` or `Value::Nil` if empty
- `Table(map)` → alist or `Value::Nil` if empty

### stringify path

```
raw chibi sexp
  → toml::Value          (recursive mapping, detect alist/datetime/etc.)
  → toml::to_string()    (toml crate handles formatting)
```

unlike json (where we hand-built the output string), TOML output goes through
`toml::to_string()` because TOML formatting is complex (nested tables, key escaping,
inline tables vs sections). building a `toml::Value` tree from raw sexps then
delegating to the crate is safer and more correct.

datetime detection in raw sexps: a pair whose car is the symbol `toml-datetime` and
whose cadr is a string → parse via `toml::Datetime::from_str()` → `toml::Value::Datetime`.

alist detection: same logic as json — proper list where every element is a pair with
a string key.

### cargo feature

```toml
[features]
default = ["json", "toml"]
toml = ["dep:toml_crate"]

[dependencies]
toml_crate = { package = "toml", version = "1.0", default-features = false, features = ["parse", "display"], optional = true }
```

`default-features = false` with `parse` + `display` avoids pulling in serde — we use
`toml::Value::from_str()` and `toml::to_string()` which don't need it. the dep is
renamed to `toml_crate` to avoid collision with the `toml` feature name.

### VFS module

`lib/tein/toml.sld`:
```scheme
(define-library (tein toml)
  (import (scheme base))
  (export toml-parse toml-stringify)
  (include "toml.scm"))
```

`lib/tein/toml.scm`:
```scheme
;;; (tein toml) — bidirectional TOML <-> scheme value conversion
;;;
;;; toml-parse and toml-stringify are registered by the rust runtime
;;; via define_fn_variadic when a standard-env context is built.
;;; this file is included by toml.sld for module definition.
```

### scheme API

```scheme
(import (tein toml))

(toml-parse "[server]\nhost = \"localhost\"\nport = 8080")
; → (("server" . (("host" . "localhost") ("port" . 8080))))

(toml-stringify '(("server" . (("host" . "localhost") ("port" . 8080)))))
; → "[server]\nhost = \"localhost\"\nport = 8080\n"

;; datetimes round-trip
(toml-parse "date = 1979-05-27T07:32:00Z")
; → (("date" . (toml-datetime "1979-05-27T07:32:00Z")))
```

## testing

- unit tests in `toml.rs`: parse + stringify for all TOML types
- round-trip tests: `toml-stringify(toml-parse(x))` preserves structure
- datetime round-trip: all 4 variants (offset, local datetime, local date, local time)
- inf/nan handling
- nested tables and arrays of tables
- error cases: invalid TOML, non-stringifiable values
- scheme integration test in `tests/scheme/toml.scm`
- feature-gate verification: `cargo build --no-default-features --features toml`
