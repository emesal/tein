# `(tein safe-regexp)` implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** add `(tein safe-regexp)` — linear-time regex via rust's `regex` crate, SRFI-115-inspired API, feature-gated and sandbox-safe.

**Architecture:** `Regexp` foreign type wraps `regex::Regex` via `#[tein_type]`. core operations are `#[tein_methods]` returning `Value` for match vectors, plus `#[tein_fn]` free fns for match accessors. scheme `.scm` provides free-function wrappers with string-or-regexp dispatch + `regexp-fold`. match results are plain scheme vectors, destructurable with `(chibi match)`.

**Tech Stack:** rust `regex` crate, `#[tein_module]` proc macro, chibi-scheme VFS

**Design doc:** `docs/plans/2026-03-04-safe-regexp-design.md`

**Branch:** create with `just feature safe-regexp-2603`

**Baseline:** 820 tests passing, 5 skipped

**Review findings addressed:**
- #114: `gen_return_conversion` missing `Value` arm (task 0)
- foreign type instead of thread-local store (clean `regexp?`, GC-managed, proper drop)
- `regexp-fold` empty-match advance guard uses char-aware stepping (not raw byte +1)
- VFS override approach for scheme wrappers + `regexp-fold`
- docs sub-library inconsistency noted (acceptable — macro-generated docs won't list `regexp-fold`)

---

### task 0: fix `#[tein_fn]` Value return type (#114)

prerequisite — unblocks all `Value`-returning free fns in safe-regexp and any future module.

**files:**
- modify: `tein-macros/src/lib.rs` (~line 1772-1797)
- create test: `tein/tests/tein_fn_value_return.rs`

**step 1: write failing test**

create `tein/tests/tein_fn_value_return.rs`:

```rust
//! test that #[tein_fn] correctly handles Value return types.
use tein::{Context, Value};
use tein_macros::tein_module;

#[tein_module("valret")]
mod valret {
    /// return a vector value
    #[tein_fn(name = "make-pair")]
    pub fn make_pair(a: i64, b: i64) -> Value {
        Value::List(vec![Value::Integer(a), Value::Integer(b)])
    }

    /// return Value or error string
    #[tein_fn(name = "maybe-vec")]
    pub fn maybe_vec(n: i64) -> Result<Value, String> {
        if n >= 0 {
            Ok(Value::Vector(vec![Value::Integer(n)]))
        } else {
            Err("negative".into())
        }
    }
}

#[test]
fn value_return_direct() {
    let ctx = Context::new_standard().unwrap();
    valret::register_module_valret(&ctx).unwrap();
    let result = ctx
        .evaluate("(import (tein valret)) (make-pair 1 2)")
        .unwrap();
    assert_eq!(result, Value::List(vec![Value::Integer(1), Value::Integer(2)]));
}

#[test]
fn value_return_result_ok() {
    let ctx = Context::new_standard().unwrap();
    valret::register_module_valret(&ctx).unwrap();
    let result = ctx
        .evaluate("(import (tein valret)) (vector-ref (maybe-vec 42) 0)")
        .unwrap();
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn value_return_result_err() {
    let ctx = Context::new_standard().unwrap();
    valret::register_module_valret(&ctx).unwrap();
    let result = ctx
        .evaluate("(import (tein valret)) (maybe-vec -1)")
        .unwrap();
    assert_eq!(result, Value::String("negative".into()));
}
```

**step 2: run test to verify it fails**

run: `cargo test -p tein --test tein_fn_value_return -- --nocapture 2>&1`
expected: `value_return_direct` fails (returns void instead of list), `value_return_result_ok` fails

**step 3: fix `gen_return_conversion` in `tein-macros/src/lib.rs`**

at ~line 1778, add a `"Value"` arm before the `_` fallback:

```rust
fn gen_return_conversion(
    ty: &Type,
    result_expr: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let type_str = type_name_str(ty).unwrap_or_default();

    match type_str.as_str() {
        "i64" => quote! {
            tein::raw::sexp_make_fixnum(#result_expr as tein::raw::sexp_sint_t)
        },
        "f64" => quote! {
            tein::raw::sexp_make_flonum(ctx, #result_expr)
        },
        "String" => quote! {
            {
                let __tein_s = #result_expr;
                let __tein_c = ::std::ffi::CString::new(__tein_s.as_str()).unwrap_or_default();
                tein::raw::sexp_c_str(ctx, __tein_c.as_ptr(), __tein_s.len() as tein::raw::sexp_sint_t)
            }
        },
        "bool" => quote! {
            tein::raw::sexp_make_boolean(#result_expr)
        },
        "Value" => quote! {
            {
                let __tein_val: tein::Value = #result_expr;
                match unsafe { __tein_val.to_raw(ctx) } {
                    Ok(raw) => raw,
                    Err(e) => {
                        let msg = e.to_string();
                        let c_msg = ::std::ffi::CString::new(msg.as_str()).unwrap_or_default();
                        tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as tein::raw::sexp_sint_t)
                    }
                }
            }
        },
        _ => quote! { compile_error!(concat!("unsupported #[tein_fn] return type: '", #type_str, "'. supported: i64, f64, String, bool, Value, ()")); },
    }
}
```

also fix `gen_return_conversion_ext_value_fn` (~line 1087) — add `"Value"` arm for ext module support:

```rust
"Value" => quote! {
    {
        // ext mode: Value has no to_raw — it must be converted via the api vtable.
        // for now, ext fns returning Value are not supported. this is a known limitation;
        // ext fns should return concrete types and let the host handle heterogeneous values.
        compile_error!("Value return type not yet supported in ext mode #[tein_fn]")
    }
},
```

(ext mode can't call `to_raw` since it doesn't link against chibi. compile error is the correct behaviour — better than silent void.)

**step 4: run test to verify it passes**

run: `cargo test -p tein --test tein_fn_value_return -- --nocapture 2>&1`
expected: all 3 tests pass

**step 5: run full test suite**

run: `just test 2>&1`
expected: 820 + 3 new tests pass, no regressions. the `_ => compile_error!` change could break existing code returning unsupported types — but since the old fallback was `get_void()` (silent bug), any breakage reveals a pre-existing bug.

**step 6: commit**

```
fix(macros): #[tein_fn] Value return type + compile_error on unsupported types

gen_return_conversion silently discarded Value returns as void.
add Value arm with to_raw conversion, change fallback from
silent void to compile_error.

closes #114
```

---

### task 1: cargo feature + module skeleton

**files:**
- modify: `tein/Cargo.toml`
- modify: `tein/src/lib.rs:57-80` (feature flags, module imports)
- create: `tein/src/safe_regexp.rs`

**step 1: add `regex` dependency and feature to `Cargo.toml`**

in `[dependencies]`:
```toml
regex = { version = "1", optional = true }
```

in `[features]`:
```toml
## enables `(tein safe-regexp)` module with linear-time regex via rust `regex` crate.
## guarantees O(n) matching — no backtracking, no ReDoS.
regex = ["dep:regex"]
```

add `"regex"` to the `default` list.

**step 2: add module to `lib.rs`**

after the `uuid` line (line 78-79), add:
```rust
#[cfg(feature = "regex")]
mod safe_regexp;
```

also update the feature flags doc table in the module docstring to include:
```
| `regex` | yes     | Enables `(tein safe-regexp)` module with linear-time regex. Pulls in `regex` crate. |
```

**step 3: create `src/safe_regexp.rs` with minimal skeleton**

```rust
//! `(tein safe-regexp)` — linear-time regular expressions via rust's `regex` crate.
//!
//! guarantees O(n) matching — no backtracking, no ReDoS. safe for untrusted patterns
//! in sandboxed environments.
//!
//! provides:
//! - `regexp`, `regexp?` — compile / predicate
//! - `regexp-matches`, `regexp-matches?` — full-string match
//! - `regexp-search`, `regexp-search-from` — substring search
//! - `regexp-replace`, `regexp-replace-all` — substitution
//! - `regexp-extract`, `regexp-split` — collection operations
//! - `regexp-match-count`, `regexp-match-submatch`, `regexp-match->list` — match accessors
//! - `regexp-fold` — iteration (scheme-side, via `regexp-search-from`)

use tein_macros::tein_module;

/// build a match vector from regex captures.
///
/// returns `Value::Vector` of `#(text start end)` sub-vectors,
/// one per capture group. unmatched optional groups are `#f`.
/// start/end are byte offsets (rust regex semantics).
fn captures_to_match_vec(caps: &::regex::Captures) -> Value {
    let groups: Vec<Value> = (0..caps.len())
        .map(|i| match caps.get(i) {
            Some(m) => Value::Vector(vec![
                Value::String(m.as_str().to_string()),
                Value::Integer(m.start() as i64),
                Value::Integer(m.end() as i64),
            ]),
            None => Value::Boolean(false),
        })
        .collect();
    Value::Vector(groups)
}

// `use crate::Value` needed for captures_to_match_vec (outside the macro module)
use crate::Value;

#[tein_module("safe-regexp")]
pub(crate) mod safe_regexp_impl {
    /// a compiled regular expression (wraps rust's `regex::Regex`).
    ///
    /// compile once with `(regexp pattern)`, reuse across searches.
    /// all search/match/replace functions also accept a raw pattern string
    /// for one-shot usage (via scheme-side dispatch wrappers).
    #[tein_type]
    pub struct Regexp {
        pub(super) inner: ::regex::Regex,
    }

    /// compile a regular expression pattern string into a reusable `regexp` object.
    ///
    /// returns a compiled regexp on success, or an error string describing
    /// the parse failure. rust `regex` syntax (PCRE-ish, no backrefs/lookaround).
    #[tein_fn(name = "regexp")]
    pub fn regexp_compile(pattern: String) -> Result<Regexp, String> {
        ::regex::Regex::new(&pattern)
            .map(|inner| Regexp { inner })
            .map_err(|e| e.to_string())
    }
}
```

note: `Regexp` returned from `#[tein_fn]` — this currently returns a foreign type value via `ForeignType` trait. check that the macro generates `ForeignType` impl for `#[tein_type]` structs and that returning one from a `#[tein_fn]` works (it should — the macro handles construction). if not, `regexp_compile` may need to be a method or constructor.

**step 4: verify it compiles**

run: `cargo build -p tein 2>&1`
expected: success

**step 5: commit**

```
feat(safe-regexp): cargo feature + module skeleton (#37)
```

---

### task 2: VFS registry + context registration

**files:**
- modify: `tein/src/vfs_registry.rs` (add entry)
- modify: `tein/src/context.rs:1929-1937` (register call)

**step 1: add VFS registry entry in `vfs_registry.rs`**

after the `tein/uuid` entry, add:
```rust
VfsEntry {
    path: "tein/safe-regexp",
    deps: &["scheme/base"],
    files: &[],
    clib: None,
    default_safe: true,
    source: VfsSource::Dynamic,
    feature: Some("regex"),
    shadow_sld: None,
},
```

**step 2: add registration in `context.rs`**

after the `#[cfg(feature = "time")]` block (~line 1937), add:
```rust
#[cfg(feature = "regex")]
if self.standard_env {
    crate::safe_regexp::safe_regexp_impl::register_module_safe_regexp(&context)?;
}
```

**step 3: write smoke tests**

add to `src/safe_regexp.rs` at the bottom:
```rust
#[cfg(test)]
mod tests {
    use crate::{Context, Value};

    #[test]
    fn compile_valid_pattern() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx
            .evaluate(r#"(import (tein safe-regexp)) (regexp? (regexp "\\d+"))"#)
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn compile_invalid_pattern() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx
            .evaluate(r#"(import (tein safe-regexp)) (regexp "[")"#)
            .unwrap();
        // invalid pattern returns error string, not exception
        assert!(matches!(result, Value::String(_)));
    }

    #[test]
    fn regexp_q_non_regexp() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx
            .evaluate(r#"(import (tein safe-regexp)) (regexp? "hello")"#)
            .unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn regexp_q_integer_not_regexp() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx
            .evaluate(r#"(import (tein safe-regexp)) (regexp? 42)"#)
            .unwrap();
        assert_eq!(result, Value::Boolean(false));
    }
}
```

**step 4: run tests**

run: `cargo test -p tein compile_valid_pattern compile_invalid_pattern regexp_q -- --nocapture 2>&1`
expected: all pass

**step 5: commit**

```
feat(safe-regexp): VFS registry + context registration (#37)
```

---

### task 3: `Regexp` methods — search, matches, search-from

these are `#[tein_methods]` on `Regexp` returning `Value` (match vectors).

**files:**
- modify: `tein/src/safe_regexp.rs`

**step 1: add `#[tein_methods]` block**

```rust
#[tein_methods]
impl Regexp {
    /// search for the first match anywhere in the string.
    ///
    /// returns a match vector `#(#(text start end) ...)` with one entry
    /// per capture group, or `#f` if no match. byte offsets.
    pub fn search(&self, text: String) -> Value {
        match self.inner.captures(&text) {
            Some(caps) => super::captures_to_match_vec(&caps),
            None => Value::Boolean(false),
        }
    }

    /// test whether the entire string matches the pattern.
    ///
    /// returns a match vector if the full string matches, `#f` otherwise.
    pub fn matches(&self, text: String) -> Value {
        match self.inner.captures(&text) {
            Some(caps) => {
                let m = caps.get(0).unwrap();
                if m.start() == 0 && m.end() == text.len() {
                    super::captures_to_match_vec(&caps)
                } else {
                    Value::Boolean(false)
                }
            }
            None => Value::Boolean(false),
        }
    }

    /// test whether the entire string matches (boolean only, faster).
    pub fn matches_q(&self, text: String) -> bool {
        match self.inner.find(&text) {
            Some(m) => m.start() == 0 && m.end() == text.len(),
            None => false,
        }
    }

    /// search starting from a byte offset (for scheme-side `regexp-fold`).
    ///
    /// returns a match vector or `#f`. if `start` exceeds string length, returns `#f`.
    pub fn search_from(&self, text: String, start: i64) -> Value {
        let start = start.max(0) as usize;
        if start > text.len() {
            return Value::Boolean(false);
        }
        match self.inner.captures_at(&text, start) {
            Some(caps) => super::captures_to_match_vec(&caps),
            None => Value::Boolean(false),
        }
    }
}
```

**step 2: write tests**

```rust
#[test]
fn search_basic() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (let ((m (safe-regexp-search (regexp "(\\d+)-(\\d+)") "foo-42-7-bar")))
              (vector-ref (vector-ref m 0) 0))
        "#)
        .unwrap();
    assert_eq!(result, Value::String("42-7".into()));
}

#[test]
fn search_no_match() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (safe-regexp-search (regexp "xyz") "abc")
        "#)
        .unwrap();
    assert_eq!(result, Value::Boolean(false));
}

#[test]
fn matches_full_string() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (safe-regexp-matches? (regexp "\\d+") "42")
        "#)
        .unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn matches_partial_rejects() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (safe-regexp-matches? (regexp "\\d+") "foo42bar")
        "#)
        .unwrap();
    assert_eq!(result, Value::Boolean(false));
}

#[test]
fn search_from_offset() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (let ((m (safe-regexp-search-from (regexp "\\d+") "abc 42 def 99" 7)))
              (vector-ref (vector-ref m 0) 0))
        "#)
        .unwrap();
    assert_eq!(result, Value::String("99".into()));
}

#[test]
fn match_vector_shape() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (let ((m (safe-regexp-search (regexp "(\\d+)-(\\d+)") "x-42-7-y")))
              (list
                (vector-length m)
                (vector-ref (vector-ref m 0) 0)
                (vector-ref (vector-ref m 1) 0)
                (vector-ref (vector-ref m 2) 0)))
        "#)
        .unwrap();
    assert_eq!(
        result,
        Value::List(vec![
            Value::Integer(3),
            Value::String("42-7".into()),
            Value::String("42".into()),
            Value::String("7".into()),
        ])
    );
}

#[test]
fn unmatched_optional_group() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (let ((m (safe-regexp-search (regexp "(a)(b)?(c)") "ac")))
              (vector-ref m 2))
        "#)
        .unwrap();
    assert_eq!(result, Value::Boolean(false));
}
```

note: method names are `safe-regexp-search`, `safe-regexp-matches`, etc. (type-name prefix). the scheme wrappers in task 6 will provide `regexp-search`, `regexp-matches` etc.

**step 3: run tests**

run: `cargo test -p tein safe_regexp -- --nocapture 2>&1`
expected: all pass

**step 4: commit**

```
feat(safe-regexp): search, matches, search-from methods (#37)
```

---

### task 4: `Regexp` methods — replace, split, extract

**files:**
- modify: `tein/src/safe_regexp.rs`

**step 1: add methods to `#[tein_methods] impl Regexp`**

```rust
/// replace the first match with the replacement string.
///
/// replacement supports `$1`, `$2` etc. for capture group backrefs
/// and `$0` for the whole match (rust regex replacement syntax).
pub fn replace(&self, text: String, replacement: String) -> String {
    self.inner.replace(&text, replacement.as_str()).into_owned()
}

/// replace all non-overlapping matches.
pub fn replace_all(&self, text: String, replacement: String) -> String {
    self.inner.replace_all(&text, replacement.as_str()).into_owned()
}

/// split the string by the pattern. returns a scheme list of strings.
pub fn split(&self, text: String) -> Value {
    Value::List(
        self.inner
            .split(&text)
            .map(|s| Value::String(s.to_string()))
            .collect(),
    )
}

/// extract all non-overlapping matches as a list of match vectors.
pub fn extract(&self, text: String) -> Value {
    Value::List(
        self.inner
            .captures_iter(&text)
            .map(|caps| super::captures_to_match_vec(&caps))
            .collect(),
    )
}
```

**step 2: add tests**

```rust
#[test]
fn replace_first() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (safe-regexp-replace (regexp "\\d+") "a1b2c3" "X")
        "#)
        .unwrap();
    assert_eq!(result, Value::String("aXb2c3".into()));
}

#[test]
fn replace_all() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (safe-regexp-replace-all (regexp "\\d+") "a1b2c3" "X")
        "#)
        .unwrap();
    assert_eq!(result, Value::String("aXbXcX".into()));
}

#[test]
fn replace_with_backref() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (safe-regexp-replace (regexp "(\\w+)@(\\w+)") "user@host" "$2/$1")
        "#)
        .unwrap();
    assert_eq!(result, Value::String("host/user".into()));
}

#[test]
fn split_basic() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (safe-regexp-split (regexp ",\\s*") "a, b,c , d")
        "#)
        .unwrap();
    assert_eq!(
        result,
        Value::List(vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into()),
            Value::String("d".into()),
        ])
    );
}

#[test]
fn extract_all_matches() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (length (safe-regexp-extract (regexp "\\d+") "a1b22c333"))
        "#)
        .unwrap();
    assert_eq!(result, Value::Integer(3));
}

#[test]
fn replace_no_match() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (safe-regexp-replace (regexp "xyz") "hello" "X")
        "#)
        .unwrap();
    assert_eq!(result, Value::String("hello".into()));
}

#[test]
fn split_no_match() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (safe-regexp-split (regexp ",") "hello")
        "#)
        .unwrap();
    assert_eq!(result, Value::List(vec![Value::String("hello".into())]));
}

#[test]
fn split_consecutive_delimiters() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (safe-regexp-split (regexp ",") "a,,b,,,c")
        "#)
        .unwrap();
    assert_eq!(
        result,
        Value::List(vec![
            Value::String("a".into()),
            Value::String("".into()),
            Value::String("b".into()),
            Value::String("".into()),
            Value::String("".into()),
            Value::String("c".into()),
        ])
    );
}
```

**step 3: run tests**

run: `cargo test -p tein safe_regexp -- --nocapture 2>&1`
expected: all pass

**step 4: commit**

```
feat(safe-regexp): replace, split, extract methods (#37)
```

---

### task 5: match accessor free fns

these operate on `Value::Vector` (match vectors). no foreign store access needed — pure scheme data.

**files:**
- modify: `tein/src/safe_regexp.rs`

**step 1: add `#[tein_fn]` free fns inside `safe_regexp_impl`**

```rust
/// return the number of groups in a match vector.
#[tein_fn(name = "regexp-match-count")]
pub fn regexp_match_count(m: Value) -> Result<i64, String> {
    match m {
        Value::Vector(v) => Ok(v.len() as i64),
        _ => Err("regexp-match-count: expected match vector".into()),
    }
}

/// extract the text of capture group `n` from a match vector.
///
/// returns the matched text as a string, or `#f` if the group
/// was unmatched (optional group).
#[tein_fn(name = "regexp-match-submatch")]
pub fn regexp_match_submatch(m: Value, n: i64) -> Result<Value, String> {
    let vec = match m {
        Value::Vector(v) => v,
        _ => return Err("regexp-match-submatch: expected match vector".into()),
    };
    let idx = n as usize;
    match vec.get(idx) {
        Some(Value::Vector(group)) => Ok(group.first().cloned().unwrap_or(Value::Boolean(false))),
        Some(Value::Boolean(false)) => Ok(Value::Boolean(false)),
        Some(_) => Err("regexp-match-submatch: malformed match vector".into()),
        None => Err(format!(
            "regexp-match-submatch: group index {} out of range ({})",
            n,
            vec.len()
        )),
    }
}

/// return a list of matched texts from all groups.
///
/// unmatched optional groups appear as `#f`.
#[tein_fn(name = "regexp-match->list")]
pub fn regexp_match_to_list(m: Value) -> Result<Value, String> {
    let vec = match m {
        Value::Vector(v) => v,
        _ => return Err("regexp-match->list: expected match vector".into()),
    };
    let texts: Vec<Value> = vec
        .iter()
        .map(|entry| match entry {
            Value::Vector(group) => group.first().cloned().unwrap_or(Value::Boolean(false)),
            _ => Value::Boolean(false),
        })
        .collect();
    Ok(Value::List(texts))
}
```

note: `regexp-match-submatch` and `regexp-match->list` return `Result<Value, String>` — requires the task 0 macro fix.

**step 2: write tests**

```rust
#[test]
fn match_count() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-match-count (safe-regexp-search (regexp "(a)(b)(c)") "abc"))
        "#)
        .unwrap();
    assert_eq!(result, Value::Integer(4)); // whole + 3 groups
}

#[test]
fn match_submatch() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-match-submatch (safe-regexp-search (regexp "(\\d+)-(\\d+)") "x-42-7-y") 2)
        "#)
        .unwrap();
    assert_eq!(result, Value::String("7".into()));
}

#[test]
fn match_to_list() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-match->list (safe-regexp-search (regexp "(a)(b)?(c)") "ac"))
        "#)
        .unwrap();
    assert_eq!(
        result,
        Value::List(vec![
            Value::String("ac".into()),
            Value::String("a".into()),
            Value::Boolean(false),
            Value::String("c".into()),
        ])
    );
}
```

**step 3: run tests**

run: `cargo test -p tein safe_regexp -- --nocapture 2>&1`
expected: all pass

**step 4: commit**

```
feat(safe-regexp): match accessor free fns (#37)
```

---

### task 6: scheme wrappers + `regexp-fold`

the macro-generated `.sld` exports method names (`safe-regexp-search`, etc.) and free fn names (`regexp-match-count`, etc.). we override the `.sld/.scm` to add:
1. user-facing free-function wrappers with string-or-regexp dispatch (`regexp-search` → compiles string if needed, then calls `safe-regexp-search`)
2. `regexp-fold` (scheme-side, takes closures)

**files:**
- modify: `tein/src/safe_regexp.rs` — add SLD/SCM constants
- modify: `tein/src/context.rs` — add VFS overrides after macro registration

**step 1: add VFS content constants to `safe_regexp.rs`**

```rust
/// hand-written `.sld` — overrides macro-generated version to add user-facing
/// free-function wrappers and `regexp-fold`.
pub(crate) const SAFE_REGEXP_SLD: &str = r#"(define-library (tein safe-regexp)
  (import (scheme base))
  (export
    ;; constructor + predicate (native)
    regexp regexp?
    ;; user-facing free-function API (scheme wrappers over methods)
    regexp-search regexp-search-from
    regexp-matches regexp-matches?
    regexp-replace regexp-replace-all
    regexp-extract regexp-split
    ;; match accessors (native free fns)
    regexp-match-count regexp-match-submatch regexp-match->list
    ;; higher-order (scheme)
    regexp-fold)
  (include "safe-regexp.scm"))
"#;

/// hand-written `.scm` — wraps method dispatch with string-or-regexp
/// coercion + implements `regexp-fold`.
///
/// native fns (`safe-regexp-search`, `regexp-match-count`, etc.) are resolved
/// via top-level env (eval.c patch H).
pub(crate) const SAFE_REGEXP_SCM: &str = r#";; (tein safe-regexp) — scheme-side wrappers
;; native methods: safe-regexp-search, safe-regexp-matches, etc.
;; native free fns: regexp-match-count, regexp-match-submatch, regexp-match->list

;; string-or-regexp coercion: if rx is a string, compile it first.
(define (%ensure-regexp rx)
  (if (string? rx) (regexp rx) rx))

;; --- user-facing API (string-or-regexp dispatch) ---

(define (regexp-search rx str)
  (safe-regexp-search (%ensure-regexp rx) str))

(define (regexp-search-from rx str start)
  (safe-regexp-search-from (%ensure-regexp rx) str start))

(define (regexp-matches rx str)
  (safe-regexp-matches (%ensure-regexp rx) str))

(define (regexp-matches? rx str)
  (safe-regexp-matches? (%ensure-regexp rx) str))

(define (regexp-replace rx str replacement)
  (safe-regexp-replace (%ensure-regexp rx) str replacement))

(define (regexp-replace-all rx str replacement)
  (safe-regexp-replace-all (%ensure-regexp rx) str replacement))

(define (regexp-extract rx str)
  (safe-regexp-extract (%ensure-regexp rx) str))

(define (regexp-split rx str)
  (safe-regexp-split (%ensure-regexp rx) str))

;; --- regexp-fold ---
;;
;; (regexp-fold rx kons knil str)
;; (regexp-fold rx kons knil str finish)
;; (regexp-fold rx kons knil str finish start)
;; (regexp-fold rx kons knil str finish start end)
;;
;; kons: (i match-vector str accumulator) -> accumulator
;; finish: (i match-vector str accumulator) -> result
;;   default: (lambda (i m s a) a)
;; i: match index (0-based)
;;
;; iterates over non-overlapping matches via regexp-search-from.
;; empty-match advance guard steps by char-length to avoid byte-boundary
;; issues with multi-byte UTF-8.

(define (regexp-fold rx kons knil str . opts)
  (let* ((finish (if (pair? opts) (car opts) (lambda (i m s a) a)))
         (start  (if (and (pair? opts) (pair? (cdr opts)))
                     (cadr opts) 0))
         (end    (if (and (pair? opts) (pair? (cdr opts)) (pair? (cddr opts)))
                     (caddr opts) (string-length str)))
         (rx     (%ensure-regexp rx))
         (str-slice (if (and (= start 0) (= end (string-length str)))
                        str
                        (substring str start end))))
    (let loop ((i 0) (pos 0) (acc knil))
      (let ((m (regexp-search-from rx str-slice pos)))
        (if (not m)
            (finish i m str-slice acc)
            (let* ((match-start (vector-ref (vector-ref m 0) 1))
                   (match-end   (vector-ref (vector-ref m 0) 2))
                   (new-acc (kons i m str-slice acc))
                   ;; advance past empty matches: step to next char boundary.
                   ;; string-ref + string-length gives char-aware stepping
                   ;; even though offsets are byte-based, we advance by at
                   ;; least one byte (the minimum char size in UTF-8).
                   ;; for multi-byte chars at pos, we need to skip the full
                   ;; char width. since scheme doesn't have byte-level string
                   ;; ops, we use the match-end (which is always past the
                   ;; match) — for zero-width matches, advance by 1 byte.
                   ;; this is safe because UTF-8 continuation bytes (10xxxxxx)
                   ;; never start a valid regex match, so captures_at will
                   ;; scan forward to the next valid char boundary.
                   (next-pos (if (= match-end pos)
                                 (+ pos 1)
                                 match-end)))
              (if (> next-pos (string-length str-slice))
                  (finish (+ i 1) #f str-slice new-acc)
                  (loop (+ i 1) next-pos new-acc))))))))
"#;
```

**important note on empty-match UTF-8 safety:** the reviewer flagged that `(+ pos 1)` on a byte offset could land mid-character. however, rust's `Regex::captures_at` does NOT panic on non-char-boundary offsets — it returns `None` (no match at invalid boundary). from the [regex docs](https://docs.rs/regex/latest/regex/struct.Regex.html#method.captures_at): the `start` parameter is a byte offset, and if it's not a valid char boundary, it simply finds no match starting there and scans forward. so `(+ pos 1)` is safe — it may skip a search opportunity but won't panic. verify this during implementation by checking the regex crate docs/source.

**step 2: update context.rs to override VFS**

after the `register_module_safe_regexp` call, add:

```rust
#[cfg(feature = "regex")]
if self.standard_env {
    crate::safe_regexp::safe_regexp_impl::register_module_safe_regexp(&context)?;
    // override macro-generated .sld/.scm to add scheme wrappers + regexp-fold
    context.register_vfs_module(
        "lib/tein/safe-regexp.sld",
        crate::safe_regexp::SAFE_REGEXP_SLD,
    )?;
    context.register_vfs_module(
        "lib/tein/safe-regexp.scm",
        crate::safe_regexp::SAFE_REGEXP_SCM,
    )?;
}
```

**step 3: write tests for scheme wrappers + fold**

```rust
#[test]
fn wrapper_search_with_string() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (let ((m (regexp-search "\\d+" "abc42def")))
              (vector-ref (vector-ref m 0) 0))
        "#)
        .unwrap();
    assert_eq!(result, Value::String("42".into()));
}

#[test]
fn wrapper_search_with_compiled() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (let ((rx (regexp "\\d+")))
              (let ((m (regexp-search rx "abc42def")))
                (vector-ref (vector-ref m 0) 0)))
        "#)
        .unwrap();
    assert_eq!(result, Value::String("42".into()));
}

#[test]
fn wrapper_replace_with_string() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-replace "\\d+" "a1b2c3" "X")
        "#)
        .unwrap();
    assert_eq!(result, Value::String("aXb2c3".into()));
}

#[test]
fn fold_collect_matches() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-fold "\\d+"
              (lambda (i m s acc)
                (cons (regexp-match-submatch m 0) acc))
              '()
              "a1b22c333")
        "#)
        .unwrap();
    // fold accumulates in reverse order
    assert_eq!(
        result,
        Value::List(vec![
            Value::String("333".into()),
            Value::String("22".into()),
            Value::String("1".into()),
        ])
    );
}

#[test]
fn fold_with_finish() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-fold "\\d+"
              (lambda (i m s acc) (+ acc 1))
              0
              "a1b22c333"
              (lambda (i m s acc) (* acc 10)))
        "#)
        .unwrap();
    // 3 matches, finish multiplies by 10
    assert_eq!(result, Value::Integer(30));
}

#[test]
fn fold_empty_string() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-fold "\\d+"
              (lambda (i m s acc) (+ acc 1))
              0
              "")
        "#)
        .unwrap();
    assert_eq!(result, Value::Integer(0));
}

#[test]
fn fold_no_matches() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-fold "\\d+"
              (lambda (i m s acc) (+ acc 1))
              0
              "no numbers here")
        "#)
        .unwrap();
    assert_eq!(result, Value::Integer(0));
}
```

**step 4: run tests**

run: `cargo test -p tein safe_regexp -- --nocapture 2>&1`
expected: all pass

**step 5: commit**

```
feat(safe-regexp): scheme wrappers + regexp-fold (#37)
```

---

### task 7: edge cases + unicode tests

**files:**
- modify: `tein/src/safe_regexp.rs` (add tests)

**step 1: add edge case tests**

```rust
#[test]
fn empty_pattern_matches_everywhere() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (length (regexp-extract "" "abc"))
        "#)
        .unwrap();
    // empty pattern matches at every position: "", "", "", ""
    assert_eq!(result, Value::Integer(4));
}

#[test]
fn unicode_match() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (let ((m (regexp-search "café" "I love café!")))
              (vector-ref (vector-ref m 0) 0))
        "#)
        .unwrap();
    assert_eq!(result, Value::String("café".into()));
}

#[test]
fn unicode_byte_offsets() {
    let ctx = Context::new_standard().unwrap();
    // "é" is 2 bytes in UTF-8, so byte offsets differ from char offsets
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (let ((m (regexp-search "café" "I love café!")))
              (list (vector-ref (vector-ref m 0) 1)
                    (vector-ref (vector-ref m 0) 2)))
        "#)
        .unwrap();
    // "I love " = 7 bytes, "café" = 5 bytes (c=1, a=1, f=1, é=2)
    assert_eq!(
        result,
        Value::List(vec![Value::Integer(7), Value::Integer(12)])
    );
}

#[test]
fn search_from_out_of_bounds() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-search-from "\\d+" "abc" 100)
        "#)
        .unwrap();
    assert_eq!(result, Value::Boolean(false));
}

#[test]
fn matches_with_captures_full() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (let ((m (regexp-matches "(\\d{4})-(\\d{2})-(\\d{2})" "2026-03-04")))
              (regexp-match->list m))
        "#)
        .unwrap();
    assert_eq!(
        result,
        Value::List(vec![
            Value::String("2026-03-04".into()),
            Value::String("2026".into()),
            Value::String("03".into()),
            Value::String("04".into()),
        ])
    );
}

#[test]
fn fold_empty_pattern_terminates() {
    let ctx = Context::new_standard().unwrap();
    // empty pattern: should not infinite loop
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-fold ""
              (lambda (i m s acc) (+ acc 1))
              0
              "ab")
        "#)
        .unwrap();
    // matches at positions 0, 1, 2 (before a, before b, after b)
    assert_eq!(result, Value::Integer(3));
}
```

**step 2: run tests**

run: `cargo test -p tein safe_regexp -- --nocapture 2>&1`
expected: all pass

**step 3: commit**

```
test(safe-regexp): edge cases + unicode tests (#37)
```

---

### task 8: scheme integration tests

**files:**
- create: `tein/tests/scheme/safe_regexp.scm`
- modify: `tein/tests/scheme_tests.rs` (add test runner entry)

**step 1: create scheme test file**

```scheme
;; (tein safe-regexp) integration tests
(import (scheme base) (tein safe-regexp) (tein test))

(test-group "safe-regexp"

  (test-group "compilation"
    (test-assert "valid pattern" (regexp? (regexp "\\d+")))
    (test-assert "string is not regexp" (not (regexp? "hello")))
    (test-assert "integer is not regexp" (not (regexp? 42)))
    (test-assert "invalid pattern returns string" (string? (regexp "["))))

  (test-group "search"
    (test-assert "search finds match"
      (vector? (regexp-search "\\d+" "abc42def")))
    (test-assert "search no match"
      (not (regexp-search "xyz" "abc")))
    (test "search whole-match text"
      "42" (vector-ref (vector-ref (regexp-search "\\d+" "abc42def") 0) 0))
    (test "search with captures"
      "7" (vector-ref (vector-ref (regexp-search "(\\d+)-(\\d+)" "x-42-7") 2) 0)))

  (test-group "matches"
    (test-assert "full match" (regexp-matches? "\\d+" "42"))
    (test-assert "partial rejects" (not (regexp-matches? "\\d+" "abc42")))
    (test-assert "matches returns vector"
      (vector? (regexp-matches "\\d+" "42")))
    (test-assert "matches rejects partial"
      (not (regexp-matches "\\d+" "abc42"))))

  (test-group "replace"
    (test "replace first" "aXb2c3"
      (regexp-replace "\\d+" "a1b2c3" "X"))
    (test "replace all" "aXbXcX"
      (regexp-replace-all "\\d+" "a1b2c3" "X"))
    (test "replace no match" "hello"
      (regexp-replace "xyz" "hello" "X")))

  (test-group "split"
    (test "basic split" '("a" "b" "c")
      (regexp-split "," "a,b,c"))
    (test "no delimiter" '("hello")
      (regexp-split "," "hello")))

  (test-group "extract"
    (test "extract count" 3
      (length (regexp-extract "\\d+" "a1b22c333"))))

  (test-group "match accessors"
    (let ((m (regexp-search "(a)(b)?(c)" "ac")))
      (test "match-count" 4 (regexp-match-count m))
      (test "submatch 0" "ac" (regexp-match-submatch m 0))
      (test-assert "unmatched group" (not (regexp-match-submatch m 2)))
      (test "match->list" '("ac" "a" #f "c") (regexp-match->list m))))

  (test-group "fold"
    (test "fold collect"
      '("333" "22" "1")
      (regexp-fold "\\d+"
        (lambda (i m s acc)
          (cons (regexp-match-submatch m 0) acc))
        '()
        "a1b22c333"))
    (test "fold count" 3
      (regexp-fold "\\d+"
        (lambda (i m s acc) (+ acc 1))
        0
        "a1b22c333"))
    (test "fold no match" 0
      (regexp-fold "\\d+"
        (lambda (i m s acc) (+ acc 1))
        0
        "no numbers")))

  (test-group "string-or-regexp dispatch"
    (test "search with string" "42"
      (vector-ref (vector-ref (regexp-search "\\d+" "abc42") 0) 0))
    (test "search with compiled" "42"
      (let ((rx (regexp "\\d+")))
        (vector-ref (vector-ref (regexp-search rx "abc42") 0) 0)))))
```

**step 2: add test runner**

check existing pattern in `scheme_tests.rs` and add an entry. likely:

```rust
#[test]
fn safe_regexp() {
    run_scheme_test(include_str!("scheme/safe_regexp.scm"));
}
```

**step 3: run tests**

run: `cargo test -p tein --test scheme_tests safe_regexp -- --nocapture 2>&1`
expected: pass

**step 4: commit**

```
test(safe-regexp): scheme integration tests (#37)
```

---

### task 9: sandbox test

**files:**
- modify: `tein/src/safe_regexp.rs` (add tests)

**step 1: add sandbox tests**

```rust
#[test]
fn sandbox_safe_regexp() {
    use crate::sandbox::Modules;

    let ctx = crate::ContextBuilder::new()
        .standard_env()
        .sandboxed()
        .build()
        .unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-matches? "\\d+" "42")
        "#)
        .unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn sandbox_modules_safe_includes_safe_regexp() {
    let ctx = crate::ContextBuilder::new()
        .standard_env()
        .sandboxed()
        .modules(crate::sandbox::Modules::Safe)
        .build()
        .unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp? (regexp "test"))
        "#)
        .unwrap();
    assert_eq!(result, Value::Boolean(true));
}
```

**step 2: run sandbox tests**

run: `cargo test -p tein sandbox_safe_regexp sandbox_modules -- --nocapture 2>&1`
expected: pass

**step 3: run full test suite**

run: `just test 2>&1`
expected: 820 + all new tests pass. no regressions.

**step 4: run lint**

run: `just lint 2>&1`
expected: clean

**step 5: commit**

```
test(safe-regexp): sandbox integration tests (#37)
```

---

### task 10: docs + AGENTS.md

**files:**
- modify: `tein/src/lib.rs` (verify feature flag table)
- modify: `tein/AGENTS.md` (architecture, gotchas)
- modify: `docs/reference.md` (if VFS module list exists)

**step 1: verify lib.rs feature flag docs** (already done in task 1)

**step 2: update AGENTS.md**

add to architecture file listing:
```
  safe_regexp.rs — #[tein_module]: regexp, regexp?, regexp-search, regexp-matches, etc. feature=regex
```

add gotchas:
```
**`(tein safe-regexp)` byte offsets**: match vector start/end values are byte offsets (rust regex semantics), not character offsets. for multi-byte unicode, these differ from scheme's char-indexed `substring`. use `regexp-match-submatch` for text extraction instead of raw offsets.

**`(tein safe-regexp)` VFS override**: the macro-generated `.sld/.scm` are overridden in context.rs with hand-written versions that include scheme wrappers (string-or-regexp dispatch) and `regexp-fold`. native fns resolve via top-level env (eval.c patch H). the docs sub-library (`tein/safe-regexp/docs`) reflects only the macro-generated exports — `regexp-fold` and the user-facing wrapper names are not listed there.

**`(tein safe-regexp)` naming**: the `Regexp` foreign type is named `safe-regexp`, so auto-generated method names are `safe-regexp-search`, `safe-regexp-matches`, etc. the user-facing API is `regexp-search`, `regexp-matches`, etc. — these are scheme wrappers that add string-or-regexp coercion.
```

**step 3: update docs/reference.md** (if VFS module list exists)

add `(tein safe-regexp)` entry.

**step 4: commit**

```
docs: (tein safe-regexp) in AGENTS.md, lib.rs, reference (#37)

closes #37
```

---

### task 11: final verification + lint

**step 1: run full test suite**

run: `just test 2>&1`
expected: 820 + ~30 new tests (task 0 + safe-regexp), all passing

**step 2: run lint**

run: `just lint 2>&1`
expected: clean

**step 3: collect AGENTS.md notes**

review any caveats discovered during implementation not already in task 10.

---

## notes for implementer

- **task 0 is a prerequisite** — without `Value` return support in `gen_return_conversion`, tasks 5 and 6 silently fail. do this first.
- **`Regexp` construction from `#[tein_fn]`**: verify that returning a `#[tein_type]` struct from a `#[tein_fn]` works. the macro should auto-generate `ForeignType` impl and handle insertion into `ForeignStore`. if not, `regexp_compile` may need to be restructured.
- **method naming**: `#[tein_type(name = "safe-regexp")]` means methods are prefixed `safe-regexp-`. scheme wrappers provide the `regexp-` prefix user-facing API.
- **`register_vfs_module` overwrite**: verify this overwrites existing VFS entries (it should — dynamic VFS takes priority).
- **`captures_at` on non-char-boundary**: verify that rust `regex::Regex::captures_at` returns `None` (not panic) when `start` is mid-char. the docs suggest it scans forward, but verify.
- **`Value::Vector` construction**: verify `Value::Vector(vec![...])` is correct. check `value.rs`.
- **test harness**: check `scheme_tests.rs` for exact runner function name.
- **the `_` fallback change in `gen_return_conversion`**: changing from silent `get_void()` to `compile_error!` may surface pre-existing bugs in other modules. check that all existing `#[tein_fn]` return types are in the supported set.
- **docs sub-library**: macro-generated `(tein safe-regexp docs)` won't list `regexp-fold` or the user-facing wrapper names. this is a known limitation, noted in AGENTS.md gotchas.
