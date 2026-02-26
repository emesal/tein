# tein-sexp serde hardening implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** fix the alist round-trip bug, add missing serde API surface (`Sexp` as value type, IO functions), and close testing gaps so the serde data format is production-ready.

**Architecture:** pure rust changes in `tein-sexp/src/serde/` and `tein-sexp/src/ast.rs`. no C/FFI changes. the serializer/deserializer foundation is solid — this is a hardening pass adding correctness fixes, missing API, and comprehensive tests.

**Tech Stack:** rust, serde 1.x, tein-sexp crate (feature-gated behind `serde`)

---

### Task 1: fix `is_alist` to accept string keys

the `is_alist` heuristic in `de.rs` only recognises dotted pairs with symbol keys as alist entries. maps serialised with string keys (e.g. `BTreeMap<String, V>`) fail to round-trip through `deserialize_any` because they get classified as sequences instead of maps.

**Files:**
- Modify: `tein-sexp/src/serde/de.rs` — `is_alist` function (~line 232)
- Test: `tein-sexp/src/serde/de.rs` — tests module

**Step 1: write the failing test**

add to `de.rs` tests:

```rust
#[test]
fn round_trip_btreemap_string_keys() {
    use std::collections::BTreeMap;
    let mut m = BTreeMap::new();
    m.insert("name".to_string(), "alice".to_string());
    m.insert("role".to_string(), "admin".to_string());
    let text = crate::serde::to_string(&m).unwrap();
    let restored: BTreeMap<String, String> = from_str(&text).unwrap();
    assert_eq!(m, restored);
}
```

**Step 2: run test to verify it fails**

Run: `cargo test -p tein-sexp --features serde round_trip_btreemap_string_keys`
Expected: FAIL — deserialize_any classifies the alist as a sequence

**Step 3: fix `is_alist` to accept string keys**

in `de.rs`, change `is_alist`:

```rust
fn is_alist(items: &[Sexp]) -> bool {
    if items.is_empty() {
        return false;
    }
    items.iter().all(|item| match &item.kind {
        SexpKind::DottedList(keys, _) if keys.len() == 1 => {
            matches!(&keys[0].kind, SexpKind::Symbol(_) | SexpKind::String(_))
        }
        _ => false,
    })
}
```

**Step 4: run test to verify it passes**

Run: `cargo test -p tein-sexp --features serde round_trip_btreemap`
Expected: PASS

**Step 5: run full test suite**

Run: `cargo test -p tein-sexp --features serde`
Expected: all pass — no regressions

**Step 6: commit**

```bash
git add tein-sexp/src/serde/de.rs
git commit -m "fix(serde): accept string keys in alist heuristic

is_alist only recognised symbol keys, breaking BTreeMap<String, V>
round-trips through deserialize_any. now accepts both symbol and
string keys as valid alist entries."
```

---

### Task 2: error on `u64` overflow instead of silent precision loss

`serialize_u64` silently converts values > `i64::MAX` to `f64`, losing precision. this should be an explicit error — data corruption is worse than a clear failure.

**Files:**
- Modify: `tein-sexp/src/serde/ser.rs` — `serialize_u64` method
- Test: `tein-sexp/src/serde/ser.rs` — tests module

**Step 1: write the failing test**

add to `ser.rs` tests:

```rust
#[test]
fn serialize_u64_max_errors() {
    let result = to_sexp(&u64::MAX);
    assert!(result.is_err(), "u64::MAX should error, not silently lose precision");
}

#[test]
fn serialize_u64_fits_i64() {
    // values that fit in i64 should work fine
    let sexp = to_sexp(&(i64::MAX as u64)).unwrap();
    assert_eq!(sexp.to_string(), i64::MAX.to_string());
}
```

**Step 2: run test to verify `serialize_u64_max_errors` fails**

Run: `cargo test -p tein-sexp --features serde serialize_u64_max`
Expected: FAIL — currently returns Ok with lossy float

**Step 3: fix `serialize_u64` to error on overflow**

in `ser.rs`, replace the `serialize_u64` method:

```rust
fn serialize_u64(self, v: u64) -> Result<Sexp, ParseError> {
    if v <= i64::MAX as u64 {
        self.serialize_i64(v as i64)
    } else {
        Err(ParseError::no_span(format!(
            "u64 value {v} exceeds i64::MAX and cannot be represented losslessly"
        )))
    }
}
```

**Step 4: run tests to verify both pass**

Run: `cargo test -p tein-sexp --features serde serialize_u64`
Expected: PASS

**Step 5: commit**

```bash
git add tein-sexp/src/serde/ser.rs
git commit -m "fix(serde): error on u64 overflow instead of silent f64 truncation"
```

---

### Task 3: explicit `i128`/`u128` error messages

serde's default `serialize_i128`/`serialize_u128` produce an unhelpful "i128 is not supported" error. override them with clear messages.

**Files:**
- Modify: `tein-sexp/src/serde/ser.rs` — add i128/u128 overrides
- Modify: `tein-sexp/src/serde/de.rs` — add i128/u128 overrides
- Test: both files

**Step 1: write the failing tests**

add to `ser.rs` tests:

```rust
#[test]
fn serialize_i128_error_message() {
    let err = to_sexp(&42i128).unwrap_err();
    assert!(
        err.to_string().contains("i128"),
        "error should mention i128: {err}"
    );
}

#[test]
fn serialize_u128_error_message() {
    let err = to_sexp(&42u128).unwrap_err();
    assert!(
        err.to_string().contains("u128"),
        "error should mention u128: {err}"
    );
}
```

add to `de.rs` tests:

```rust
#[test]
fn deserialize_i128_error_message() {
    let err = from_str::<i128>("42").unwrap_err();
    assert!(
        err.to_string().contains("i128"),
        "error should mention i128: {err}"
    );
}

#[test]
fn deserialize_u128_error_message() {
    let err = from_str::<u128>("42").unwrap_err();
    assert!(
        err.to_string().contains("u128"),
        "error should mention u128: {err}"
    );
}
```

**Step 2: run tests to verify they fail (default error messages are unhelpful)**

Run: `cargo test -p tein-sexp --features serde 128`
Expected: FAIL — default messages say "i128 is not supported" without context

**Step 3: add explicit overrides**

in `ser.rs`, add to the `Serializer` impl:

```rust
fn serialize_i128(self, _v: i128) -> Result<Sexp, ParseError> {
    Err(ParseError::no_span(
        "i128 cannot be represented in s-expressions (i64 max)",
    ))
}

fn serialize_u128(self, _v: u128) -> Result<Sexp, ParseError> {
    Err(ParseError::no_span(
        "u128 cannot be represented in s-expressions (i64 max)",
    ))
}
```

in `de.rs`, add to the `SexpDeserializer` impl:

```rust
fn deserialize_i128<V: de::Visitor<'de>>(self, _visitor: V) -> Result<V::Value, ParseError> {
    Err(self.error("i128 cannot be deserialized from s-expressions (i64 max)"))
}

fn deserialize_u128<V: de::Visitor<'de>>(self, _visitor: V) -> Result<V::Value, ParseError> {
    Err(self.error("u128 cannot be deserialized from s-expressions (i64 max)"))
}
```

**Step 4: run tests to verify they pass**

Run: `cargo test -p tein-sexp --features serde 128`
Expected: PASS — error messages now mention the specific type

**Step 5: commit**

```bash
git add tein-sexp/src/serde/ser.rs tein-sexp/src/serde/de.rs
git commit -m "fix(serde): explicit error messages for i128/u128"
```

---

### Task 4: implement `Serialize` and `Deserialize` for `Sexp`

this enables `Sexp` as a generic value type (like `serde_json::Value`), allowing catch-all fields, dynamic data, and interop with other serde formats.

**Files:**
- Modify: `tein-sexp/src/ast.rs` — add `Serialize`/`Deserialize` impls (feature-gated)
- Test: `tein-sexp/src/ast.rs` or `tein-sexp/src/serde/de.rs`

**Step 1: write the failing tests**

add a new test block in `de.rs` tests (or `ser.rs` — pick one, keep them together):

```rust
#[test]
fn sexp_as_deserialize_target() {
    let sexp: Sexp = from_str("(1 2 3)").unwrap();
    assert_eq!(sexp, Sexp::list(vec![Sexp::integer(1), Sexp::integer(2), Sexp::integer(3)]));
}

#[test]
fn sexp_as_serialize_source() {
    let sexp = Sexp::list(vec![Sexp::symbol("hello"), Sexp::integer(42)]);
    let text = crate::serde::to_string(&sexp).unwrap();
    assert_eq!(text, "(hello 42)");
}

#[test]
fn sexp_round_trip_nested() {
    let original = Sexp::list(vec![
        Sexp::symbol("config"),
        Sexp::dotted_list(vec![Sexp::symbol("name")], Sexp::string("test")),
        Sexp::boolean(true),
    ]);
    let text = crate::serde::to_string(&original).unwrap();
    let restored: Sexp = from_str(&text).unwrap();
    assert_eq!(original, restored);
}

#[test]
fn sexp_in_struct_field() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Wrapper {
        tag: String,
        data: Sexp,
    }
    let w = Wrapper {
        tag: "test".to_string(),
        data: Sexp::list(vec![Sexp::integer(1), Sexp::integer(2)]),
    };
    let text = crate::serde::to_string(&w).unwrap();
    let restored: Wrapper = from_str(&text).unwrap();
    assert_eq!(w, restored);
}
```

**Step 2: run tests to verify they fail**

Run: `cargo test -p tein-sexp --features serde sexp_as`
Expected: FAIL — Sexp doesn't impl Serialize/Deserialize

**Step 3: implement `Serialize` for `Sexp`**

in `ast.rs`, add a feature-gated impl. the key insight: serialize `Sexp` by producing the same output as if we hand-called the serializer for each variant. this means `Sexp` serializes as itself — the identity transform through the serde data model.

```rust
#[cfg(feature = "serde")]
impl serde::Serialize for Sexp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        match &self.kind {
            SexpKind::Integer(n) => serializer.serialize_i64(*n),
            SexpKind::Float(f) => serializer.serialize_f64(*f),
            SexpKind::String(s) => serializer.serialize_str(s),
            SexpKind::Symbol(s) => {
                // symbols serialize as a newtype struct to distinguish from strings
                serializer.serialize_newtype_struct("@@sexp-symbol", s)
            }
            SexpKind::Boolean(b) => serializer.serialize_bool(*b),
            SexpKind::Char(c) => serializer.serialize_char(*c),
            SexpKind::List(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            SexpKind::DottedList(items, tail) => {
                // serialize as tagged: ("@@dotted" items... tail)
                serializer.serialize_newtype_struct("@@sexp-dotted", &DottedListHelper(items, tail))
            }
            SexpKind::Vector(items) => {
                // serialize as tagged to distinguish from list
                serializer.serialize_newtype_struct("@@sexp-vector", items)
            }
            SexpKind::Nil => serializer.serialize_unit(),
        }
    }
}
```

wait — this approach gets complex fast. the simpler and more correct approach: when serializing `Sexp` *through our own serializer* (which produces `Sexp`), it's the identity. when serializing to *another* format, we want a reasonable representation.

actually, the cleanest design: **`Sexp` serializes into our `Serializer` by just returning a clone of itself**, and for foreign serializers we map to the natural serde data model. but `serde::Serialize` is generic over `S: Serializer`, so we can't specialise.

let me reconsider. the pragmatic approach used by `serde_json::Value`:
- `Value::serialize` maps each variant to the corresponding serde call
- `Value::deserialize` uses `deserialize_any` to self-describe

this is exactly right for `Sexp` too. the tricky parts are symbols (no serde equivalent — serialize as string, lose the distinction) and dotted lists (no serde equivalent — flatten into sequence). this is acceptable for cross-format interop, and for `Sexp`→`Sexp` round-trips we use `to_sexp`/`from_sexp` directly.

revised implementation:

```rust
#[cfg(feature = "serde")]
impl serde::Serialize for Sexp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        match &self.kind {
            SexpKind::Integer(n) => serializer.serialize_i64(*n),
            SexpKind::Float(f) => serializer.serialize_f64(*f),
            SexpKind::String(s) => serializer.serialize_str(s),
            SexpKind::Symbol(s) => serializer.serialize_str(s),
            SexpKind::Boolean(b) => serializer.serialize_bool(*b),
            SexpKind::Char(c) => serializer.serialize_char(*c),
            SexpKind::List(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            SexpKind::DottedList(items, tail) => {
                let mut seq = serializer.serialize_seq(Some(items.len() + 1))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.serialize_element(tail.as_ref())?;
                seq.end()
            }
            SexpKind::Vector(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            SexpKind::Nil => serializer.serialize_unit(),
        }
    }
}
```

for `Deserialize`, use `deserialize_any` with a visitor that reconstructs `Sexp`:

```rust
#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Sexp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SexpVisitor)
    }
}

#[cfg(feature = "serde")]
struct SexpVisitor;

#[cfg(feature = "serde")]
impl<'de> serde::de::Visitor<'de> for SexpVisitor {
    type Value = Sexp;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "any s-expression value")
    }

    fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Sexp, E> {
        Ok(Sexp::boolean(v))
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Sexp, E> {
        Ok(Sexp::integer(v))
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Sexp, E> {
        if v <= i64::MAX as u64 {
            Ok(Sexp::integer(v as i64))
        } else {
            Err(E::custom(format!("u64 value {v} exceeds i64::MAX")))
        }
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Sexp, E> {
        Ok(Sexp::float(v))
    }

    fn visit_char<E: serde::de::Error>(self, v: char) -> Result<Sexp, E> {
        Ok(Sexp::char(v))
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Sexp, E> {
        Ok(Sexp::string(v))
    }

    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Sexp, E> {
        Ok(Sexp::string(v))
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Sexp, E> {
        Ok(Sexp::nil())
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Sexp, E> {
        Ok(Sexp::nil())
    }

    fn visit_some<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Sexp, D::Error> {
        Sexp::deserialize(deserializer)
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Sexp, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Sexp::list(items))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Sexp, A::Error> {
        let mut entries = Vec::with_capacity(map.size_hint().unwrap_or(0));
        while let Some((key, val)) = map.next_entry::<Sexp, Sexp>()? {
            entries.push(Sexp::dotted_list(vec![key], val));
        }
        Ok(Sexp::list(entries))
    }
}
```

note: when deserializing `Sexp` from our own format, symbols come through `visit_string` and become `Sexp::String` — the symbol/string distinction is lost. this is inherent to the serde data model (it has no "symbol" concept). for lossless `Sexp`→`Sexp`, users should use `to_sexp`/`from_sexp` directly. document this.

**Step 4: run tests to verify they pass**

Run: `cargo test -p tein-sexp --features serde sexp_`
Expected: PASS

**Step 5: commit**

```bash
git add tein-sexp/src/ast.rs tein-sexp/src/serde/de.rs
git commit -m "feat(serde): implement Serialize/Deserialize for Sexp

enables Sexp as a generic value type (like serde_json::Value).
symbols round-trip as strings through foreign serializers — use
to_sexp/from_sexp for lossless Sexp↔Sexp conversion."
```

---

### Task 5: add `from_reader` and `to_writer`

standard serde data format API. `to_writer` avoids an intermediate string allocation; `from_reader` reads the whole input (s-expressions aren't streamable without a streaming parser).

**Files:**
- Modify: `tein-sexp/src/serde/mod.rs` — add `from_reader`, `to_writer`, `to_writer_pretty`

**Step 1: write the failing tests**

add to `mod.rs` (or a new test in de.rs/ser.rs — wherever the public API tests live):

```rust
#[test]
fn from_reader_api() {
    let input = b"((name . \"alice\") (age . 30))";
    let result: std::collections::BTreeMap<String, crate::serde::de::from_str::<serde_json::Value>> // no, keep it simple
    // actually just test from_reader works like from_str:
    use std::io::Cursor;
    let reader = Cursor::new(b"42");
    let result: i32 = crate::serde::from_reader(reader).unwrap();
    assert_eq!(result, 42);
}

#[test]
fn to_writer_api() {
    let mut buf = Vec::new();
    crate::serde::to_writer(&mut buf, &42).unwrap();
    assert_eq!(std::str::from_utf8(&buf).unwrap(), "42");
}

#[test]
fn to_writer_pretty_api() {
    let mut buf = Vec::new();
    crate::serde::to_writer_pretty(&mut buf, &vec![1, 2, 3]).unwrap();
    assert_eq!(std::str::from_utf8(&buf).unwrap(), "(1 2 3)");
}
```

**Step 2: run tests to verify they fail**

Run: `cargo test -p tein-sexp --features serde reader`
Expected: FAIL — functions don't exist

**Step 3: implement the functions**

in `serde/mod.rs`:

```rust
use std::io;

/// deserialize a value from a reader containing s-expression text
pub fn from_reader<R: io::Read, T: serde::de::DeserializeOwned>(
    mut reader: R,
) -> Result<T, ParseError> {
    let mut input = String::new();
    reader
        .read_to_string(&mut input)
        .map_err(|e| ParseError::no_span(format!("io error: {e}")))?;
    from_str(&input)
}

/// serialize a value to a writer as compact s-expression text
pub fn to_writer<W: io::Write, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), ParseError> {
    let sexp = to_sexp(value)?;
    write!(writer, "{}", printer::to_string(&sexp))
        .map_err(|e| ParseError::no_span(format!("io error: {e}")))
}

/// serialize a value to a writer as pretty-printed s-expression text
pub fn to_writer_pretty<W: io::Write, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), ParseError> {
    let sexp = to_sexp(value)?;
    write!(writer, "{}", printer::to_string_pretty(&sexp))
        .map_err(|e| ParseError::no_span(format!("io error: {e}")))
}
```

update the `pub use` exports:

```rust
pub use de::{from_reader, from_sexp, from_str};
```

wait — `from_reader` is defined in `mod.rs`, not `de.rs`. keep it in `mod.rs` alongside `to_string` and `to_string_pretty` since these are convenience wrappers. just add to the existing public API.

**Step 4: run tests to verify they pass**

Run: `cargo test -p tein-sexp --features serde reader writer`
Expected: PASS

**Step 5: commit**

```bash
git add tein-sexp/src/serde/mod.rs
git commit -m "feat(serde): add from_reader, to_writer, to_writer_pretty"
```

---

### Task 6: add doc-tests to serde public API

every public function in the serde module should have a `# Examples` section.

**Files:**
- Modify: `tein-sexp/src/serde/mod.rs` — add doc-tests to `to_string`, `to_string_pretty`, `from_reader`, `to_writer`, `to_writer_pretty`
- Modify: `tein-sexp/src/serde/ser.rs` — add doc-test to `to_sexp`
- Modify: `tein-sexp/src/serde/de.rs` — add doc-tests to `from_str`, `from_sexp`

**Step 1: add doc-tests**

for `to_string` in `mod.rs`:
~~~rust
/// serialize a value to compact s-expression text
///
/// # Examples
///
/// ```
/// use tein_sexp::serde::to_string;
///
/// assert_eq!(to_string(&42).unwrap(), "42");
/// assert_eq!(to_string(&vec![1, 2, 3]).unwrap(), "(1 2 3)");
/// ```
~~~

for `to_string_pretty` in `mod.rs`:
~~~rust
/// serialize a value to pretty-printed s-expression text
///
/// short forms stay compact; long lists break across lines with indentation.
///
/// # Examples
///
/// ```
/// use tein_sexp::serde::to_string_pretty;
///
/// assert_eq!(to_string_pretty(&(1, 2, 3)).unwrap(), "(1 2 3)");
/// ```
~~~

for `from_reader` in `mod.rs`:
~~~rust
/// deserialize a value from a reader containing s-expression text
///
/// # Examples
///
/// ```
/// use tein_sexp::serde::from_reader;
///
/// let input = b"((name . \"alice\") (age . 30))";
/// let map: std::collections::BTreeMap<String, tein_sexp::Sexp> =
///     from_reader(&input[..]).unwrap();
/// ```
~~~

for `to_writer` in `mod.rs`:
~~~rust
/// serialize a value to a writer as compact s-expression text
///
/// # Examples
///
/// ```
/// use tein_sexp::serde::to_writer;
///
/// let mut buf = Vec::new();
/// to_writer(&mut buf, &42).unwrap();
/// assert_eq!(buf, b"42");
/// ```
~~~

for `to_sexp` in `ser.rs`:
~~~rust
/// serialize a value to an s-expression AST node
///
/// # Examples
///
/// ```
/// use tein_sexp::serde::to_sexp;
///
/// let sexp = to_sexp(&true).unwrap();
/// assert_eq!(sexp.to_string(), "#t");
/// ```
~~~

for `from_str` in `de.rs`:
~~~rust
/// deserialize a value from s-expression text
///
/// # Examples
///
/// ```
/// use tein_sexp::serde::from_str;
///
/// let v: Vec<i32> = from_str("(1 2 3)").unwrap();
/// assert_eq!(v, vec![1, 2, 3]);
/// ```
~~~

for `from_sexp` in `de.rs`:
~~~rust
/// deserialize a value from an [`Sexp`] node
///
/// # Examples
///
/// ```
/// use tein_sexp::{Sexp, serde::from_sexp};
///
/// let sexp = Sexp::integer(42);
/// let n: i32 = from_sexp(&sexp).unwrap();
/// assert_eq!(n, 42);
/// ```
~~~

**Step 2: run doc-tests**

Run: `cargo test -p tein-sexp --features serde --doc`
Expected: all pass

**Step 3: commit**

```bash
git add tein-sexp/src/serde/mod.rs tein-sexp/src/serde/ser.rs tein-sexp/src/serde/de.rs
git commit -m "docs(serde): add doc-tests to all public serde API functions"
```

---

### Task 7: serde attribute compatibility tests

test that common `#[serde(...)]` attributes work correctly. these are *test-only* — no implementation changes expected, but if any fail we fix them.

**Files:**
- Test: `tein-sexp/src/serde/de.rs` — tests module

**Step 1: write the tests**

```rust
// --- serde attribute compatibility ---

#[test]
fn serde_rename_field() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Renamed {
        #[serde(rename = "full-name")]
        name: String,
    }
    let r = Renamed { name: "alice".to_string() };
    let text = crate::serde::to_string(&r).unwrap();
    assert!(text.contains("full-name"), "got: {text}");
    let restored: Renamed = from_str(&text).unwrap();
    assert_eq!(r, restored);
}

#[test]
fn serde_default_missing_field() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct WithDefault {
        name: String,
        #[serde(default)]
        count: i32,
    }
    let d: WithDefault = from_str("((name . \"alice\"))").unwrap();
    assert_eq!(d.name, "alice");
    assert_eq!(d.count, 0);
}

#[test]
fn serde_skip_serializing_if() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Sparse {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        email: Option<String>,
    }
    let s = Sparse { name: "alice".to_string(), email: None };
    let text = crate::serde::to_string(&s).unwrap();
    assert!(!text.contains("email"), "should skip None: {text}");
}

#[test]
fn serde_flatten() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Base {
        name: String,
    }
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Extended {
        #[serde(flatten)]
        base: Base,
        age: i32,
    }
    let e = Extended { base: Base { name: "alice".to_string() }, age: 30 };
    let text = crate::serde::to_string(&e).unwrap();
    let restored: Extended = from_str(&text).unwrap();
    assert_eq!(e, restored);
}

#[test]
fn serde_internally_tagged_enum() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "type")]
    enum Event {
        Click { x: i32, y: i32 },
        Keypress { key: String },
    }
    let e = Event::Click { x: 10, y: 20 };
    let text = crate::serde::to_string(&e).unwrap();
    let restored: Event = from_str(&text).unwrap();
    assert_eq!(e, restored);
}

#[test]
fn serde_adjacently_tagged_enum() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "t", content = "c")]
    enum Msg {
        Text(String),
        Number(i32),
    }
    let m = Msg::Text("hello".to_string());
    let text = crate::serde::to_string(&m).unwrap();
    let restored: Msg = from_str(&text).unwrap();
    assert_eq!(m, restored);
}

#[test]
fn serde_untagged_enum() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(untagged)]
    enum StringOrInt {
        Int(i32),
        Str(String),
    }
    let a = StringOrInt::Int(42);
    let text_a = crate::serde::to_string(&a).unwrap();
    let restored_a: StringOrInt = from_str(&text_a).unwrap();
    assert_eq!(a, restored_a);

    let b = StringOrInt::Str("hello".to_string());
    let text_b = crate::serde::to_string(&b).unwrap();
    let restored_b: StringOrInt = from_str(&text_b).unwrap();
    assert_eq!(b, restored_b);
}
```

**Step 2: run the tests**

Run: `cargo test -p tein-sexp --features serde serde_`
Expected: if any fail, investigate and fix before proceeding

**Step 3: fix any failures**

if `serde_flatten`, `serde_internally_tagged_enum`, or `serde_adjacently_tagged_enum` fail, the root cause is likely in `deserialize_any` / alist detection. fix accordingly.

**Step 4: commit**

```bash
git add tein-sexp/src/serde/de.rs
git commit -m "test(serde): attribute compatibility tests (rename, default, flatten, tag)"
```

---

### Task 8: edge case tests

round-trip coverage for tricky values: special floats, empty structs, unicode, newtype/tuple/unit structs, non-string map keys.

**Files:**
- Test: `tein-sexp/src/serde/de.rs` — tests module

**Step 1: write the tests**

```rust
// --- edge cases ---

#[test]
fn round_trip_special_floats() {
    // NaN
    let text = crate::serde::to_string(&f64::NAN).unwrap();
    let restored: f64 = from_str(&text).unwrap();
    assert!(restored.is_nan());

    // infinity
    let text = crate::serde::to_string(&f64::INFINITY).unwrap();
    assert_eq!(from_str::<f64>(&text).unwrap(), f64::INFINITY);

    let text = crate::serde::to_string(&f64::NEG_INFINITY).unwrap();
    assert_eq!(from_str::<f64>(&text).unwrap(), f64::NEG_INFINITY);
}

#[test]
fn round_trip_empty_struct() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Empty {}
    let e = Empty {};
    let text = crate::serde::to_string(&e).unwrap();
    let restored: Empty = from_str(&text).unwrap();
    assert_eq!(e, restored);
}

#[test]
fn round_trip_newtype_struct() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Wrapper(i32);
    let w = Wrapper(42);
    let text = crate::serde::to_string(&w).unwrap();
    let restored: Wrapper = from_str(&text).unwrap();
    assert_eq!(w, restored);
}

#[test]
fn round_trip_tuple_struct() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Pair(i32, String);
    let p = Pair(42, "hello".to_string());
    let text = crate::serde::to_string(&p).unwrap();
    let restored: Pair = from_str(&text).unwrap();
    assert_eq!(p, restored);
}

#[test]
fn round_trip_unit_struct() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Marker;
    let m = Marker;
    let text = crate::serde::to_string(&m).unwrap();
    let restored: Marker = from_str(&text).unwrap();
    assert_eq!(m, restored);
}

#[test]
fn round_trip_unicode_strings() {
    let text = crate::serde::to_string(&"héllo wörld 🌍").unwrap();
    let restored: String = from_str(&text).unwrap();
    assert_eq!(restored, "héllo wörld 🌍");
}

#[test]
fn round_trip_escaped_strings() {
    let s = "line1\nline2\ttab\\backslash\"quote";
    let text = crate::serde::to_string(&s).unwrap();
    let restored: String = from_str(&text).unwrap();
    assert_eq!(restored, s);
}

#[test]
fn round_trip_integer_map_keys() {
    use std::collections::BTreeMap;
    let mut m = BTreeMap::new();
    m.insert(1i32, "one".to_string());
    m.insert(2, "two".to_string());
    let text = crate::serde::to_string(&m).unwrap();
    let restored: BTreeMap<i32, String> = from_str(&text).unwrap();
    assert_eq!(m, restored);
}
```

**Step 2: run tests**

Run: `cargo test -p tein-sexp --features serde round_trip_`
Expected: all pass (some may need fixes — especially `round_trip_integer_map_keys` since `is_alist` may not handle integer keys through `deserialize_any`)

**Step 3: fix any failures and commit**

```bash
git add tein-sexp/src/serde/de.rs
git commit -m "test(serde): edge case round-trip tests (floats, unicode, struct variants, map keys)"
```

---

### Task 9: update TODO.md

mark the serde data format as complete (with the hardening note).

**Files:**
- Modify: `TODO.md`

**Step 1: update the roadmap**

change the serde line under milestone 5:

```
- [x] **serde data format** — s-expression ↔ rust structs via tein-sexp (hardened: alist fix, Sexp value type, IO API, attribute compat)
```

**Step 2: commit**

```bash
git add TODO.md
git commit -m "docs: mark serde data format complete in TODO"
```

---

## task summary

| # | task | type |
|---|------|------|
| 1 | fix `is_alist` string keys bug | bugfix |
| 2 | error on u64 overflow | bugfix |
| 3 | explicit i128/u128 errors | bugfix |
| 4 | `Serialize`/`Deserialize` for `Sexp` | feature |
| 5 | `from_reader`/`to_writer` API | feature |
| 6 | doc-tests on public API | docs |
| 7 | serde attribute compat tests | tests |
| 8 | edge case round-trip tests | tests |
| 9 | update TODO.md | docs |

## out of scope (deferred)

- **recursion depth limit** — tein-sexp is a config format, not untrusted-input parser. add when needed.
- **zero-copy deserialization** — performance optimisation, premature for 0.1.
- **rename `ParseError`** — breaking API change, defer to 0.2.
- **streaming deserializer** — s-expressions aren't naturally streamable. `parse_all` + `from_sexp` covers the multi-expression case.
