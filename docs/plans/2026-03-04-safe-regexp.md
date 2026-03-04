# `(tein safe-regexp)` implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** add `(tein safe-regexp)` — linear-time regex via rust's `regex` crate, SRFI-115-inspired API, feature-gated and sandbox-safe.

**Architecture:** `Regexp` foreign type wraps `regex::Regex` via `#[tein_type]`. core operations are `#[tein_methods]` returning `Value` for complex results (match vectors). scheme `.scm` provides free-function wrappers with string-or-regexp dispatch + `regexp-fold`. match results are plain scheme vectors, destructurable with `(chibi match)`.

**Tech Stack:** rust `regex` crate, `#[tein_module]` proc macro, chibi-scheme VFS

**Design doc:** `docs/plans/2026-03-04-safe-regexp-design.md`

**Branch:** create with `just feature safe-regexp-2603`

**Baseline:** 820 tests passing, 5 skipped

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

#[tein_module("safe-regexp")]
pub(crate) mod safe_regexp_impl {
    /// a compiled regular expression (wraps rust's `regex::Regex`).
    ///
    /// compile once with `(regexp pattern)`, reuse across searches.
    /// all search/match/replace functions also accept a raw pattern string
    /// for one-shot usage.
    #[tein_type]
    pub struct Regexp {
        inner: ::regex::Regex,
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

**step 4: verify it compiles**

run: `cargo build -p tein 2>&1`
expected: success (the module skeleton compiles but isn't wired into context yet)

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

**step 3: write a smoke test**

add to `src/safe_regexp.rs` at the bottom:
```rust
#[cfg(test)]
mod tests {
    use crate::{Context, Value};

    #[test]
    fn compile_valid_pattern() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx.evaluate(r#"(import (tein safe-regexp)) (regexp? (regexp "\\d+"))"#).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn compile_invalid_pattern() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx.evaluate(r#"(import (tein safe-regexp)) (regexp "[")"#).unwrap();
        // invalid pattern returns error string, not exception
        assert!(matches!(result, Value::String(_)));
    }
}
```

**step 4: run tests**

run: `cargo test -p tein compile_valid_pattern compile_invalid_pattern -- --nocapture 2>&1`
expected: both pass

**step 5: commit**

```
feat(safe-regexp): VFS registry + context registration (#37)
```

---

### task 3: `Regexp` methods — search, matches, search-from

these are `#[tein_methods]` on `Regexp` returning `Value` (match vectors).

**files:**
- modify: `tein/src/safe_regexp.rs`

**step 1: add helper for building match vectors**

inside the `safe_regexp_impl` module, add a helper that converts `regex::Captures` to our match vector shape:

```rust
/// build a match vector from regex captures.
///
/// returns `Value::Vector` of `#(text start end)` sub-vectors,
/// one per capture group. unmatched optional groups are `#f`.
/// byte offsets from rust regex — documented, not converted.
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
```

**step 2: add `#[tein_methods]` block**

```rust
#[tein_methods]
impl Regexp {
    /// search for the first match anywhere in the string.
    ///
    /// returns a match vector `#(#(text start end) ...)` with one entry
    /// per capture group, or `#f` if no match. byte offsets.
    pub fn search(&self, text: String) -> Value {
        match self.inner.captures(&text) {
            Some(caps) => captures_to_match_vec(&caps),
            None => Value::Boolean(false),
        }
    }

    /// test whether the entire string matches the pattern.
    ///
    /// returns a match vector if the full string matches, `#f` otherwise.
    /// internally anchors the pattern with `^` and `$`.
    pub fn matches(&self, text: String) -> Value {
        match self.inner.captures(&text) {
            Some(caps) => {
                let m = caps.get(0).unwrap();
                if m.start() == 0 && m.end() == text.len() {
                    captures_to_match_vec(&caps)
                } else {
                    Value::Boolean(false)
                }
            }
            None => Value::Boolean(false),
        }
    }

    /// test whether the entire string matches (boolean only).
    pub fn matches_q(&self, text: String) -> bool {
        match self.inner.find(&text) {
            Some(m) => m.start() == 0 && m.end() == text.len(),
            None => false,
        }
    }

    /// search starting from a byte offset.
    ///
    /// returns a match vector or `#f`. used by scheme-side `regexp-fold`.
    pub fn search_from(&self, text: String, start: i64) -> Value {
        let start = start as usize;
        if start > text.len() {
            return Value::Boolean(false);
        }
        match self.inner.captures_at(&text, start) {
            Some(caps) => captures_to_match_vec(&caps),
            None => Value::Boolean(false),
        }
    }
}
```

**step 3: write tests**

```rust
#[test]
fn search_basic() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (let ((m (regexp-search (regexp "(\d+)-(\d+)") "foo-42-7-bar")))
              (vector-ref (vector-ref m 0) 0))
        "#)
        .unwrap();
    assert_eq!(result, Value::String("42-7".into()));
}

#[test]
fn search_no_match() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"(import (tein safe-regexp)) (regexp-search (regexp "xyz") "abc")"#)
        .unwrap();
    assert_eq!(result, Value::Boolean(false));
}

#[test]
fn matches_full_string() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-matches? (regexp "\\d+") "42")
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
            (regexp-matches? (regexp "\\d+") "foo42bar")
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
            (let ((m (regexp-search-from (regexp "\\d+") "abc 42 def 99" 7)))
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
            (let ((m (regexp-search (regexp "(\\d+)-(\\d+)") "x-42-7-y")))
              (list
                (vector-length m)
                (vector-ref (vector-ref m 0) 0)
                (vector-ref (vector-ref m 1) 0)
                (vector-ref (vector-ref m 2) 0)))
        "#)
        .unwrap();
    // 3 groups (whole + 2 captures), texts: "42-7", "42", "7"
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
            (let ((m (regexp-search (regexp "(a)(b)?(c)") "ac")))
              (vector-ref m 2))
        "#)
        .unwrap();
    assert_eq!(result, Value::Boolean(false));
}
```

**step 4: run tests**

run: `cargo test -p tein safe_regexp -- --nocapture 2>&1`
expected: all pass

**step 5: commit**

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
/// replacement can use `$1`, `$2` etc. for backrefs to capture groups,
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
    let matches: Vec<Value> = self
        .inner
        .captures_iter(&text)
        .map(|caps| captures_to_match_vec(&caps))
        .collect();
    Value::List(matches)
}
```

**step 2: write tests**

```rust
#[test]
fn replace_first() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-replace (regexp "\\d+") "a1b2c3" "X")
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
            (regexp-replace-all (regexp "\\d+") "a1b2c3" "X")
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
            (regexp-replace (regexp "(\\w+)@(\\w+)") "user@host" "$2/$1")
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
            (regexp-split (regexp ",\\s*") "a, b,c , d")
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
            (length (regexp-extract (regexp "\\d+") "a1b22c333"))
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
            (regexp-replace (regexp "xyz") "hello" "X")
        "#)
        .unwrap();
    assert_eq!(result, Value::String("hello".into()));
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

### task 5: scheme wrappers — string-or-regexp dispatch + match accessors

the `#[tein_module]` macro generates an `.scm` file that is currently empty (or has only const defs). we need scheme-side wrappers that provide the user-facing free-function API, delegating to `Regexp` methods.

**files:**
- modify: `tein/src/safe_regexp.rs` — add `#[tein_fn]` free functions for match accessors, and figure out how scheme wrapper code gets into the `.scm`

**important discovery from research:** the `#[tein_module]` macro generates `.scm` content from `#[tein_const]` definitions only. for custom scheme code (like `regexp-fold`), we need the module to use `VfsSource::Embedded` with hand-written `.sld` and `.scm` files, OR register extra VFS content at registration time.

actually, re-checking: the `(tein time)` module uses `VfsSource::Embedded` with `.sld/.scm` files that contain stubs, and then `register_module_time` overwrites with native fns. for `(tein safe-regexp)` we need scheme-side code for `regexp-fold` and the string-or-regexp dispatch wrappers.

**revised approach:** use `VfsSource::Dynamic` (the macro handles VFS registration). the scheme wrapper code needs to be in a hand-written `.scm` that gets included. let me check if `#[tein_module]` supports custom scheme code...

the cleanest path: define the match accessor fns (`regexp-match-count`, `regexp-match-submatch`, `regexp-match->list`) as `#[tein_fn]` free fns (they work on plain `Value::Vector`, no foreign store access needed). string-or-regexp dispatch and `regexp-fold` go in a separate `.scm` file registered manually.

**step 1: add match accessor free fns**

these operate on `Value::Vector` (match vectors), no foreign type access needed:

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

**step 2: write tests for accessors**

```rust
#[test]
fn match_count() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-match-count (regexp-search (regexp "(a)(b)(c)") "abc"))
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
            (regexp-match-submatch (regexp-search (regexp "(\\d+)-(\\d+)") "x-42-7-y") 2)
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
            (regexp-match->list (regexp-search (regexp "(a)(b)?(c)") "ac"))
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

### task 6: scheme-side wrappers + `regexp-fold`

the macro-generated `.scm` only has const definitions. we need scheme code for:
1. free-function wrappers with string-or-regexp dispatch
2. `regexp-fold`

**approach:** register additional VFS content manually in the registration function. check how `(tein json)` does manual VFS + native fn registration. alternatively, we can put the scheme wrappers directly in the `.sld` body.

**files:**
- modify: `tein/src/safe_regexp.rs`
- modify: `tein/src/vfs_registry.rs` (change to `Embedded` if needed)

**step 1: investigate the macro-generated `.sld`**

the `#[tein_module("safe-regexp")]` macro generates a `.sld` that exports all `#[tein_fn]` names and `#[tein_methods]` method names (with type prefix). it also generates a `.scm` with const definitions.

we need to add scheme code to the `.scm`. the cleanest way: add a hand-written VFS file that gets registered alongside the macro-generated ones. add to the `register_module_safe_regexp` call site in `context.rs`:

```rust
#[cfg(feature = "regex")]
if self.standard_env {
    crate::safe_regexp::safe_regexp_impl::register_module_safe_regexp(&context)?;
    // register scheme wrappers (string-or-regexp dispatch + regexp-fold)
    context.register_vfs_module(
        "lib/tein/safe-regexp-extra.scm",
        include_str!("safe_regexp_extra.scm"),
    )?;
}
```

wait — actually this won't work because the `.sld` needs to `(include "safe-regexp-extra.scm")` and the macro generates the `.sld`.

**revised approach:** the simplest path is to NOT use scheme wrappers at all for string dispatch. instead, each `#[tein_fn]` free fn accepts `Value` and does the dispatch in rust:

```rust
/// helper: extract or compile a Regexp from a Value (string or foreign).
fn ensure_regexp(v: &Value) -> Result<::regex::Regex, String> {
    match v {
        Value::String(s) => ::regex::Regex::new(s).map_err(|e| e.to_string()),
        Value::Foreign { type_name, .. } if type_name == "regexp" => {
            // can't access foreign store from free fn — need another approach
            todo!()
        }
        _ => Err("expected regexp or string".into()),
    }
}
```

...and we're back to the problem. free fns can't access the foreign store.

**final clean approach:** make the user-facing API pure free functions that accept `Value`. for string args, compile on the fly. for `Regexp` foreign args, use the *method* dispatch internally from scheme. the `.scm` does:

```scheme
(define (regexp-search rx str)
  (if (string? rx)
      (regexp-search (regexp rx) str)      ;; compile, then recurse with foreign
      (safe-regexp-search rx str)))        ;; method dispatch
```

where `safe-regexp-search` is the method name generated by `#[tein_methods]`. the recursion compiles the string once, then dispatches to the method.

but wait — this means we need both the method names AND the wrapper names exported. the macro generates method names as `regexp-search` (type-name prefix + method name). if the type is `regexp` and the method is `search`, the scheme name is `regexp-search`. that collides with our wrapper!

we need the type to have a different name, or the methods to have a different prefix. options:
- name the type `safe-regexp` → methods become `safe-regexp-search`, wrappers are `regexp-search`
- name methods with an internal prefix: `search-impl` → `regexp-search-impl`, wrapper is `regexp-search`

**decision:** name the type `safe-regexp`. methods become `safe-regexp-search`, `safe-regexp-matches`, etc. scheme wrappers `regexp-search`, `regexp-matches` etc. handle string dispatch and delegate to the `safe-regexp-*` methods. this cleanly separates the internal (method-dispatch) API from the public (SRFI-115-style) API.

update: change `#[tein_type]` to `#[tein_type(name = "safe-regexp")]` so methods are prefixed `safe-regexp-`.

for the scheme code, we need it in the `.scm`. the macro generates `.scm` content from `#[tein_const]`. we can use a `#[tein_const]` trick — no, that's hacky.

better: register the scheme code as a separate VFS file that the `.sld` includes. since the macro generates the `.sld`, we need to register the extra `.scm` file AND modify the `.sld` to include it. this requires manual VFS registration.

**simplest path:** switch from `VfsSource::Dynamic` to `VfsSource::Embedded` with hand-written `.sld` and `.scm` files, same pattern as `(tein json)` and `(tein time)`. native fns are registered via `define_fn_variadic` in a manual registration function (not macro-generated). the `#[tein_module]` macro still handles `#[tein_type]` and `#[tein_methods]` code generation.

actually, re-examining the codebase: `(tein json)` doesn't use `#[tein_module]` at all — it has manual trampolines + embedded `.sld/.scm`. `(tein time)` uses `#[tein_module]` with `VfsSource::Embedded` — the macro generates the native registration, and the embedded `.sld/.scm` provide stubs that get overwritten.

**cleanest approach for safe-regexp:** use `#[tein_module]` for the type + methods + free fns (macro handles all the native fn registration). then in `context.rs`, after the macro registration, also register a hand-written scheme wrapper file as extra VFS content. the macro-generated `.sld` won't include it, but we can register a *second* module `(tein safe-regexp wrapper)` or override the `.sld` entirely.

okay, this is getting complicated. let me step back and think about what's actually simplest.

**THE SIMPLEST APPROACH:** don't use `#[tein_type]` at all. store compiled regexps in a module-level thread-local `HashMap<u64, Regex>`. the `regexp` constructor compiles and stores, returns an integer handle. all free fns accept `Value` — if it's an integer handle, look up the compiled regex; if it's a string, compile on the fly. `regexp?` checks if the handle exists. no foreign type, no method dispatch, no scheme wrappers needed. everything is `#[tein_fn]` free fns.

this is simpler, faster (no foreign dispatch overhead), and avoids all the `.sld` generation issues. the only downside: regexp objects don't print nicely (`42` vs `#<foreign regexp>`), and GC won't clean them up (but we can add a `regexp-free` or use weak refs, or just accept that compiled regexps live for the context's lifetime — they're small).

**actually**: compiled `Regex` objects ARE small (just an `Arc` internally). in realistic usage, an LLM agent compiles maybe 5-50 patterns per evaluation. leaking those is fine. and for printing, LLM agents don't inspect opaque objects — they care about match results.

this is the right call. let me restructure.

---

**REVISED PLAN — tasks 1-2 stay the same. tasks 3-8 below replace tasks 3-6 above.**

---

### task 3 (revised): thread-local regexp store + core search fns

**files:**
- modify: `tein/src/safe_regexp.rs` — rewrite to use thread-local store

**step 1: rewrite module with thread-local store**

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

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tein_macros::tein_module;

/// global counter for regexp handle IDs.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// compiled regexp store — maps handle IDs to compiled `Regex` objects.
    /// regexps live for the thread's lifetime (small, `Arc`-backed internally).
    static REGEXP_STORE: RefCell<HashMap<u64, regex::Regex>> = RefCell::new(HashMap::new());
}

/// look up a compiled regexp by handle, or compile from a string on the fly.
///
/// accepts `Value::Integer` (handle ID from `regexp`) or `Value::String` (pattern).
/// returns a `regex::Regex` or an error message.
fn with_regexp<F, R>(v: &Value, f: F) -> Result<R, String>
where
    F: FnOnce(&regex::Regex) -> R,
{
    match v {
        Value::Integer(id) => REGEXP_STORE.with(|store| {
            let store = store.borrow();
            match store.get(&(*id as u64)) {
                Some(re) => Ok(f(re)),
                None => Err(format!("regexp: invalid handle {}", id)),
            }
        }),
        Value::String(pattern) => {
            let re = regex::Regex::new(pattern).map_err(|e| e.to_string())?;
            Ok(f(&re))
        }
        _ => Err("expected regexp handle or pattern string".into()),
    }
}

/// build a match vector from regex captures.
///
/// returns `Value::Vector` of `#(text start end)` sub-vectors,
/// one per capture group. unmatched optional groups are `#f`.
/// start/end are byte offsets (rust regex semantics).
fn captures_to_match_vec(caps: &regex::Captures) -> Value {
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

#[tein_module("safe-regexp")]
pub(crate) mod safe_regexp_impl {

    /// compile a regular expression pattern into a reusable handle.
    ///
    /// returns an integer handle on success, or an error string on invalid pattern.
    /// rust `regex` syntax (PCRE-ish, no backrefs/lookaround).
    #[tein_fn(name = "regexp")]
    pub fn regexp_compile(pattern: String) -> Result<i64, String> {
        let re = ::regex::Regex::new(&pattern).map_err(|e| e.to_string())?;
        let id = super::NEXT_ID.fetch_add(1, super::Ordering::Relaxed);
        super::REGEXP_STORE.with(|store| store.borrow_mut().insert(id, re));
        Ok(id as i64)
    }

    /// test whether a value is a compiled regexp handle.
    #[tein_fn(name = "regexp?")]
    pub fn regexp_q(value: Value) -> bool {
        match value {
            Value::Integer(id) => super::REGEXP_STORE
                .with(|store| store.borrow().contains_key(&(id as u64))),
            _ => false,
        }
    }

    /// search for the first match anywhere in the string.
    ///
    /// `rx` can be a compiled regexp handle or a pattern string.
    /// returns a match vector `#(#(text start end) ...)` or `#f`.
    #[tein_fn(name = "regexp-search")]
    pub fn regexp_search(rx: Value, text: String) -> Result<Value, String> {
        super::with_regexp(&rx, |re| match re.captures(&text) {
            Some(caps) => super::captures_to_match_vec(&caps),
            None => Value::Boolean(false),
        })
    }

    /// search starting from a byte offset (for `regexp-fold`).
    ///
    /// returns a match vector or `#f`.
    #[tein_fn(name = "regexp-search-from")]
    pub fn regexp_search_from(rx: Value, text: String, start: i64) -> Result<Value, String> {
        let start_usize = start.max(0) as usize;
        super::with_regexp(&rx, |re| {
            if start_usize > text.len() {
                return Value::Boolean(false);
            }
            match re.captures_at(&text, start_usize) {
                Some(caps) => super::captures_to_match_vec(&caps),
                None => Value::Boolean(false),
            }
        })
    }

    /// test whether the entire string matches the pattern.
    ///
    /// returns a match vector if the full string matches, `#f` otherwise.
    #[tein_fn(name = "regexp-matches")]
    pub fn regexp_matches(rx: Value, text: String) -> Result<Value, String> {
        super::with_regexp(&rx, |re| match re.captures(&text) {
            Some(caps) => {
                let m = caps.get(0).unwrap();
                if m.start() == 0 && m.end() == text.len() {
                    super::captures_to_match_vec(&caps)
                } else {
                    Value::Boolean(false)
                }
            }
            None => Value::Boolean(false),
        })
    }

    /// test whether the entire string matches (boolean only, faster).
    #[tein_fn(name = "regexp-matches?")]
    pub fn regexp_matches_q(rx: Value, text: String) -> Result<bool, String> {
        super::with_regexp(&rx, |re| match re.find(&text) {
            Some(m) => m.start() == 0 && m.end() == text.len(),
            None => false,
        })
    }
}
```

**step 2: update tests for new handle-based API**

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
    fn search_basic() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx
            .evaluate(r#"
                (import (tein safe-regexp))
                (let ((m (regexp-search (regexp "(\\d+)-(\\d+)") "foo-42-7-bar")))
                  (vector-ref (vector-ref m 0) 0))
            "#)
            .unwrap();
        assert_eq!(result, Value::String("42-7".into()));
    }

    #[test]
    fn search_with_string_pattern() {
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
    fn search_no_match() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx
            .evaluate(r#"(import (tein safe-regexp)) (regexp-search "xyz" "abc")"#)
            .unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn matches_full_string() {
        let ctx = Context::new_standard().unwrap();
        let result = ctx
            .evaluate(r#"
                (import (tein safe-regexp))
                (regexp-matches? (regexp "\\d+") "42")
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
                (regexp-matches? (regexp "\\d+") "foo42bar")
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
                (let ((m (regexp-search-from (regexp "\\d+") "abc 42 def 99" 7)))
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
                (let ((m (regexp-search "(\\d+)-(\\d+)" "x-42-7-y")))
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
                (let ((m (regexp-search "(a)(b)?(c)" "ac")))
                  (vector-ref m 2))
            "#)
            .unwrap();
        assert_eq!(result, Value::Boolean(false));
    }
}
```

**step 3: run tests**

run: `cargo test -p tein safe_regexp -- --nocapture 2>&1`
expected: all pass

**step 4: commit**

```
feat(safe-regexp): thread-local store + search/matches fns (#37)
```

---

### task 4 (revised): replace, split, extract fns

**files:**
- modify: `tein/src/safe_regexp.rs`

**step 1: add fns inside `safe_regexp_impl` module**

```rust
/// replace the first match with the replacement string.
///
/// `rx` can be a compiled regexp handle or a pattern string.
/// replacement supports `$1`, `$2` etc. for capture group backrefs
/// and `$0` for the whole match (rust regex replacement syntax).
#[tein_fn(name = "regexp-replace")]
pub fn regexp_replace(rx: Value, text: String, replacement: String) -> Result<String, String> {
    super::with_regexp(&rx, |re| {
        re.replace(&text, replacement.as_str()).into_owned()
    })
}

/// replace all non-overlapping matches.
#[tein_fn(name = "regexp-replace-all")]
pub fn regexp_replace_all(rx: Value, text: String, replacement: String) -> Result<String, String> {
    super::with_regexp(&rx, |re| {
        re.replace_all(&text, replacement.as_str()).into_owned()
    })
}

/// split the string by the pattern. returns a list of strings.
#[tein_fn(name = "regexp-split")]
pub fn regexp_split(rx: Value, text: String) -> Result<Value, String> {
    super::with_regexp(&rx, |re| {
        Value::List(
            re.split(&text)
                .map(|s| Value::String(s.to_string()))
                .collect(),
        )
    })
}

/// extract all non-overlapping matches as a list of match vectors.
#[tein_fn(name = "regexp-extract")]
pub fn regexp_extract(rx: Value, text: String) -> Result<Value, String> {
    super::with_regexp(&rx, |re| {
        Value::List(
            re.captures_iter(&text)
                .map(|caps| super::captures_to_match_vec(&caps))
                .collect(),
        )
    })
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
            (regexp-replace "\\d+" "a1b2c3" "X")
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
            (regexp-replace-all "\\d+" "a1b2c3" "X")
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
            (regexp-replace "(\\w+)@(\\w+)" "user@host" "$2/$1")
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
            (regexp-split ",\\s*" "a, b,c , d")
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
            (length (regexp-extract "\\d+" "a1b22c333"))
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
            (regexp-replace "xyz" "hello" "X")
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
            (regexp-split "," "hello")
        "#)
        .unwrap();
    assert_eq!(result, Value::List(vec![Value::String("hello".into())]));
}
```

**step 3: run tests**

run: `cargo test -p tein safe_regexp -- --nocapture 2>&1`
expected: all pass

**step 4: commit**

```
feat(safe-regexp): replace, split, extract fns (#37)
```

---

### task 5 (revised): match accessor fns

**files:**
- modify: `tein/src/safe_regexp.rs`

**step 1: add match accessor fns** (same as original task 5 step 1 — these are the `regexp-match-count`, `regexp-match-submatch`, `regexp-match->list` free fns operating on `Value::Vector`)

see original task 5 step 1 code above — it works unchanged since match accessors don't touch the regexp store.

**step 2: add tests** (same as original task 5 step 2)

see original task 5 test code above.

**step 3: run tests**

run: `cargo test -p tein safe_regexp -- --nocapture 2>&1`
expected: all pass

**step 4: commit**

```
feat(safe-regexp): match accessor fns (#37)
```

---

### task 6 (revised): `regexp-fold` in scheme

`regexp-fold` is the one proc that must be in scheme (takes a scheme closure). it needs to be in the `.scm` file.

**challenge:** with `VfsSource::Dynamic`, the macro generates the `.sld` and `.scm`. the generated `.scm` only has const definitions. we need to add `regexp-fold` to the `.scm`.

**approach:** switch to manual VFS registration. keep `#[tein_module]` for native fn registration, but override the generated `.sld` with hand-written content that includes our extra scheme file. register additional VFS content in `context.rs` after the macro registration.

actually, let me check: does `register_vfs_module` called *after* the macro registration override the macro-generated files? checking:

the macro calls `register_vfs_module("lib/tein/safe-regexp.sld", ...)` and `register_vfs_module("lib/tein/safe-regexp.scm", ...)`. if we call `register_vfs_module` again with the same path, it should overwrite. so we can:

1. let the macro register its generated `.sld` and `.scm`
2. immediately after, re-register the `.sld` with an updated version that also exports `regexp-fold` and includes both the generated `.scm` and our extra scheme code
3. register the extra scheme file

but re-registering the `.sld` is fragile — we'd need to duplicate the export list.

**simpler:** create a separate scheme file `lib/tein/safe-regexp-fold.scm` registered as VFS content, and have the `.sld` include it. since we can't modify the generated `.sld`...

**simplest working approach:** don't put `regexp-fold` in the module at all. provide it as a separate module `(tein safe-regexp fold)`:

no, that's ugly. users expect `(import (tein safe-regexp))` to get everything.

**actual simplest approach:** switch the module to `VfsSource::Embedded` with hand-written `.sld` and `.scm` files, exactly like `(tein json)` does. the `#[tein_module]` macro handles native fn registration + type registration. the `.sld/.scm` files are hand-written and embedded at build time.

wait — `(tein json)` does NOT use `#[tein_module]`. it uses manual trampolines. the `(tein time)` module uses `#[tein_module]` with embedded files — it has stub `.sld/.scm` in the VFS that the module resolver sees, and the macro's `register_module_time` overwrites the stubs with native implementations.

for `(tein safe-regexp)`: we put the `.sld` and `.scm` as embedded VFS files. the `.scm` contains `regexp-fold` (plus stub defines for the native fns that the macro registration will overwrite). the `.sld` exports everything including `regexp-fold`.

**BUT** — checking the `(tein time)` pattern more carefully: the macro calls `register_vfs_module` for *its own* generated `.sld/.scm`, AND the embedded VFS has stubs. does the macro-generated content conflict with the embedded content?

for `(tein time)`, `VfsSource::Embedded` means the files are in the static VFS (compiled into the binary via `build.rs`). the macro's `register_module_time` then calls `define_fn_variadic` which registers native fns in the top-level env — it does NOT re-register VFS content. chibi resolves imports from the VFS first, finds the embedded `.sld`, compiles the `.scm` stubs, then the native fns shadow the stubs via top-level env.

so the flow for `(tein safe-regexp)` would be:
1. embedded `.sld` exports all names (including `regexp-fold`)
2. embedded `.scm` contains `regexp-fold` implementation + stub definitions for all native fns
3. macro's `register_module_safe_regexp` calls `define_fn_variadic` for each native fn, making them available in top-level env
4. when scheme code does `(import (tein safe-regexp))`, chibi loads the embedded `.sld` → compiles `.scm` → `regexp-fold` uses the native fns (resolved via top-level env fallback from eval.c patch H)

this should work! BUT — the `#[tein_module]` macro also calls `register_vfs_module` for its generated `.sld/.scm`. that would conflict with the embedded versions.

let me check what `register_vfs_module` does — if it writes to the dynamic VFS, it might take priority over the embedded VFS.

checking the (tein time) approach — it uses `VfsSource::Embedded` in the registry. if the macro ALSO registers VFS content dynamically, there'd be a conflict. let me verify:

the macro generates a `register_module_time` fn that:
1. registers types
2. calls `define_fn_variadic` for each fn
3. calls `register_vfs_module` for `.sld` and `.scm`

but `(tein time)` also has `VfsSource::Embedded` files. so... does the dynamic registration override the embedded ones? or are both present?

i think the dynamic VFS takes priority (it's checked first). so the macro-generated `.sld/.scm` would shadow the embedded ones. for `(tein time)`, this means the macro-generated content wins, and the embedded stubs are there as fallback for when the module is a transitive dep resolved before registration.

**OK — clear approach now:**

1. use `VfsSource::Dynamic` (macro handles VFS registration)
2. the macro generates `.sld` exporting all `#[tein_fn]` names
3. add `regexp-fold` as a manual `define_fn_variadic` trampoline in context.rs (not via `#[tein_fn]`)

wait, `regexp-fold` takes a scheme closure. we can't implement it as a rust trampoline without re-entry.

**FINAL CLEAN APPROACH:**

register an additional VFS file after the macro registration:

```rust
// in context.rs
#[cfg(feature = "regex")]
if self.standard_env {
    crate::safe_regexp::safe_regexp_impl::register_module_safe_regexp(&context)?;
    // override the macro-generated .sld to also export regexp-fold
    // and include our hand-written scheme code
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

where `SAFE_REGEXP_SLD` and `SAFE_REGEXP_SCM` are `const &str` in `safe_regexp.rs` containing the hand-written `.sld` (with all exports including `regexp-fold`) and `.scm` (with `regexp-fold` implementation). the overwrite replaces the macro-generated versions.

this is clean: macro handles native fn registration, we handle the VFS content. native fns are available via top-level env (eval.c patch H).

**files:**
- modify: `tein/src/safe_regexp.rs` — add SLD/SCM constants
- modify: `tein/src/context.rs` — override VFS after macro registration

**step 1: add VFS content constants to `safe_regexp.rs`**

```rust
/// hand-written `.sld` — overrides macro-generated version to include `regexp-fold`.
pub(crate) const SAFE_REGEXP_SLD: &str = r#"(define-library (tein safe-regexp)
  (import (scheme base))
  (export
    regexp regexp?
    regexp-search regexp-search-from
    regexp-matches regexp-matches?
    regexp-replace regexp-replace-all
    regexp-extract regexp-split
    regexp-match-count regexp-match-submatch regexp-match->list
    regexp-fold)
  (include "safe-regexp.scm"))
"#;

/// hand-written `.scm` — `regexp-fold` implementation.
///
/// native fns (`regexp-search-from`, etc.) are resolved via top-level env
/// (eval.c patch H) — no need to redefine them here.
pub(crate) const SAFE_REGEXP_SCM: &str = r#";; (tein safe-regexp) — scheme-side code
;; native fns registered by #[tein_module] via define_fn_variadic

;; regexp-fold: iterate over non-overlapping matches.
;;
;; (regexp-fold rx kons knil str)
;; (regexp-fold rx kons knil str finish)
;; (regexp-fold rx kons knil str finish start)
;; (regexp-fold rx kons knil str finish start end)
;;
;; kons: (i match-vector str accumulator) -> accumulator
;; finish: (i match-vector str accumulator) -> result (default: (lambda (i m s a) a))
;; i: match index (0-based)
(define (regexp-fold rx kons knil str . opts)
  (let* ((finish (if (pair? opts) (car opts) (lambda (i m s a) a)))
         (start (if (and (pair? opts) (pair? (cdr opts))) (cadr opts) 0))
         (end (if (and (pair? opts) (pair? (cdr opts)) (pair? (cddr opts)))
                  (caddr opts)
                  (string-length str)))
         (rx (if (string? rx) (regexp rx) rx))
         (str-slice (if (and (= start 0) (= end (string-length str)))
                        str
                        (substring str start end))))
    (let loop ((i 0) (pos 0) (acc knil))
      (let ((m (regexp-search-from rx str-slice pos)))
        (if (not m)
            (finish i m str-slice acc)
            (let* ((match-end (vector-ref (vector-ref m 0) 2))
                   (new-acc (kons i m str-slice acc))
                   ;; advance past empty matches to avoid infinite loop
                   (next-pos (if (= match-end pos) (+ pos 1) match-end)))
              (if (> next-pos (string-length str-slice))
                  (finish (+ i 1) #f str-slice new-acc)
                  (loop (+ i 1) next-pos new-acc))))))))
"#;
```

**step 2: update context.rs to override VFS**

after the `register_module_safe_regexp` call, add the VFS overrides:

```rust
#[cfg(feature = "regex")]
if self.standard_env {
    crate::safe_regexp::safe_regexp_impl::register_module_safe_regexp(&context)?;
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

note: check that `register_vfs_module` is a method on `Context`. if it's not public, check what method the macro uses and use the same.

**step 3: write tests for `regexp-fold`**

```rust
#[test]
fn fold_collect_matches() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-fold "\\d+"
              (lambda (i m s acc)
                (cons (vector-ref (vector-ref m 0) 0) acc))
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
feat(safe-regexp): regexp-fold in scheme + VFS overrides (#37)
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
    // "é" is 2 bytes in UTF-8, so "café" starts at byte 7, not char 7
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
fn split_consecutive_delimiters() {
    let ctx = Context::new_standard().unwrap();
    let result = ctx
        .evaluate(r#"
            (import (tein safe-regexp))
            (regexp-split "," "a,,b,,,c")
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
fn matches_with_captures() {
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
- modify: `tein/tests/scheme_tests.rs` (add test runner)

**step 1: create scheme test file**

```scheme
;; (tein safe-regexp) integration tests
(import (scheme base) (tein safe-regexp) (tein test))

(test-group "safe-regexp"

  (test-group "compilation"
    (test-assert "valid pattern" (regexp? (regexp "\\d+")))
    (test-assert "string is not regexp" (not (regexp? "hello")))
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

**step 2: add test runner to `scheme_tests.rs`**

check the existing test runner pattern and add:

```rust
#[test]
fn safe_regexp_scheme_tests() {
    run_scheme_test(include_str!("scheme/safe_regexp.scm"));
}
```

(adjust to match exact runner fn name — check `scheme_tests.rs` for the pattern)

**step 3: run tests**

run: `cargo test -p tein safe_regexp_scheme -- --nocapture 2>&1`
expected: pass

**step 4: commit**

```
test(safe-regexp): scheme integration tests (#37)
```

---

### task 9: sandbox test

**files:**
- modify: `tein/src/safe_regexp.rs` (add sandbox test)

**step 1: add sandbox test**

```rust
#[test]
fn sandbox_safe_regexp() {
    let ctx = Context::builder()
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
```

**step 2: run test**

run: `cargo test -p tein sandbox_safe_regexp -- --nocapture 2>&1`
expected: pass

**step 3: run full test suite**

run: `just test 2>&1`
expected: all previous 820 tests pass + new safe-regexp tests. no regressions.

**step 4: run lint**

run: `just lint 2>&1`
expected: clean

**step 5: commit**

```
test(safe-regexp): sandbox integration test (#37)
```

---

### task 10: docs + AGENTS.md + lib.rs

**files:**
- modify: `tein/src/lib.rs` (feature flag table)
- modify: `tein/AGENTS.md` (architecture, gotchas)
- modify: `docs/reference.md` (if exists, VFS module list)

**step 1: update lib.rs feature flag docs** (already done in task 1)

verify the feature flag table entry is present.

**step 2: update AGENTS.md**

add to architecture section:
```
  safe_regexp.rs — #[tein_module]: regexp, regexp?, regexp-search, regexp-matches, etc. feature=regex
```

add gotcha:
```
**`(tein safe-regexp)` byte offsets**: match vector start/end values are byte offsets (rust regex semantics), not character offsets. for multi-byte unicode, these differ from scheme's char-indexed `substring`. use `regexp-match-submatch` for text extraction instead of raw offsets.

**`(tein safe-regexp)` handle-based regexp**: compiled regexps are integer handles into a thread-local store, not foreign types. `regexp?` checks handle validity. handles persist for the thread's lifetime — no explicit cleanup needed.

**`(tein safe-regexp)` VFS override**: the macro-generated `.sld/.scm` are overridden in context.rs with hand-written versions that include `regexp-fold` (scheme-side). native fns resolve via top-level env (eval.c patch H).
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
expected: 820 + ~25 new tests, all passing

**step 2: run lint**

run: `just lint 2>&1`
expected: clean

**step 3: update implementation plan with final notes**

note any gotchas discovered during implementation for AGENTS.md collection.

---

## notes for implementer

- `register_vfs_module` method: verify exact signature and accessibility — check how the macro calls it (it may be a method on `Context` or called through the raw FFI). look at the generated code from `#[tein_module("uuid")]` to see the exact call.
- `Value::Vector` construction: verify that `Value::Vector(vec![...])` is the correct way to build scheme vectors. check `value.rs` for `Vector` variant.
- test harness: check `scheme_tests.rs` for the exact runner function name (`run_scheme_test` or similar) and import pattern.
- `regexp-fold` edge case: empty pattern match needs the `(+ pos 1)` advance guard to prevent infinite loops.
- the `with_regexp` helper borrows the `RefCell` briefly — ensure no nested borrows (don't call `regexp_compile` from within `with_regexp`).
