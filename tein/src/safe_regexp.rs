//! `(tein safe-regexp)` — linear-time regular expressions via rust's `regex` crate.
//!
//! guarantees O(n) matching — no backtracking, no ReDoS. safe for untrusted patterns
//! in sandboxed environments.
//!
//! provides:
//! - `regexp`, `regexp?` — compile / predicate
//! - `regexp-matches`, `regexp-matches?` — full-string match
//! - `regexp-search`, `regexp-search-from` — substring search (string-or-regexp)
//! - `regexp-replace`, `regexp-replace-all` — substitution (string-or-regexp)
//! - `regexp-extract`, `regexp-split` — collection operations (string-or-regexp)
//! - `regexp-match-count`, `regexp-match-submatch`, `regexp-match->list` — match accessors
//! - `regexp-fold` — iteration (string-or-regexp, calls scheme closures)

use crate::Value;
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

/// coerce a scheme Value (either a compiled Regexp or a pattern string) into a `regex::Regex`.
///
/// returns the Regex on success, or an error string if the value is neither a
/// Regexp foreign object nor a String, or if string compilation fails.
/// cloning a `regex::Regex` is cheap — it uses `Arc` internally.
fn ensure_regexp(val: Value) -> Result<::regex::Regex, String> {
    use self::safe_regexp_impl::Regexp;
    match &val {
        Value::String(s) => ::regex::Regex::new(s).map_err(|e| e.to_string()),
        Value::Foreign {
            type_name,
            handle_id,
        } if type_name == "safe-regexp" => {
            let store_ptr = crate::context::FOREIGN_STORE_PTR.with(|c| c.get());
            if store_ptr.is_null() {
                return Err("regexp: no active context store (internal error)".into());
            }
            let store = unsafe { &*store_ptr };
            let borrow = store.borrow();
            let (obj, _) = borrow
                .get(*handle_id)
                .ok_or_else(|| "regexp: stale foreign handle".to_string())?;
            let rx = obj
                .downcast_ref::<Regexp>()
                .ok_or_else(|| "regexp: foreign object is not a Regexp".to_string())?;
            Ok(rx.inner.clone())
        }
        _ => Err(format!("regexp: expected string or regexp, got {}", val)),
    }
}

#[tein_module("safe-regexp")]
pub(crate) mod safe_regexp_impl {
    /// a compiled regular expression (wraps rust's `regex::Regex`).
    ///
    /// compile once with `(regexp pattern)`, reuse across searches.
    /// all search/match/replace functions also accept a raw pattern string
    /// for one-shot usage via automatic string-or-regexp dispatch.
    ///
    /// the scheme type name is `safe-regexp` (not `regexp`) so that auto-generated
    /// method names are `safe-regexp-search` etc., avoiding collision with the
    /// user-facing `regexp-search` free fns.
    #[tein_type(name = "safe-regexp")]
    pub struct Regexp {
        /// inner regex — `Clone` is cheap (regex::Regex uses Arc internally).
        pub(super) inner: ::regex::Regex,
    }

    /// compile a regular expression pattern string into a reusable `regexp` object.
    ///
    /// returns a compiled regexp on success, or an error string describing
    /// the parse failure. uses rust `regex` syntax (PCRE-ish, no backrefs/lookaround).
    ///
    /// note: returns `Result<Value, String>` rather than `Result<Regexp, String>` because
    /// `#[tein_fn]` free fns can't construct foreign values directly via the macro system.
    /// we insert into the active context's foreign store via `make_foreign_via_ptr`.
    #[tein_fn(name = "regexp")]
    pub fn regexp_compile(pattern: String) -> Result<Value, String> {
        let rx = ::regex::Regex::new(&pattern).map_err(|e| e.to_string())?;
        crate::context::Context::make_foreign_via_ptr(Regexp { inner: rx })
    }

    /// test whether a value is a compiled regexp object.
    ///
    /// returns `#t` for values created by `(regexp pattern)`, `#f` for everything else.
    /// note: `#[tein_type(name = "safe-regexp")]` auto-generates `safe-regexp?` with the
    /// type-name prefix; this free fn provides the user-facing `regexp?` name.
    #[tein_fn(name = "regexp?")]
    pub fn regexp_pred(val: Value) -> bool {
        matches!(val, Value::Foreign { ref type_name, .. } if type_name == "safe-regexp")
    }

    // --- user-facing API: string-or-regexp dispatch ---
    //
    // each wrapper fn accepts `rx: Value` (string or compiled Regexp),
    // coerces via `ensure_regexp`, then delegates to the regex operations.
    // this is the user-facing API; the `safe-regexp-*` method names remain
    // accessible as the native method layer.

    /// search for the first match anywhere in the string.
    ///
    /// `rx` may be a compiled `regexp` or a pattern string.
    /// returns a match vector or `#f` if no match.
    #[tein_fn(name = "regexp-search")]
    pub fn regexp_search(rx: Value, text: String) -> Result<Value, String> {
        let inner = super::ensure_regexp(rx)?;
        Ok(match inner.captures(&text) {
            Some(caps) => super::captures_to_match_vec(&caps),
            None => Value::Boolean(false),
        })
    }

    /// search starting from a byte offset.
    ///
    /// `rx` may be a compiled `regexp` or a pattern string.
    /// returns a match vector or `#f`. if `start` exceeds string length, returns `#f`.
    /// if `start` falls mid-character, the regex engine scans to the next char boundary.
    #[tein_fn(name = "regexp-search-from")]
    pub fn regexp_search_from(rx: Value, text: String, start: i64) -> Result<Value, String> {
        let inner = super::ensure_regexp(rx)?;
        let start = start.max(0) as usize;
        if start > text.len() {
            return Ok(Value::Boolean(false));
        }
        Ok(match inner.captures_at(&text, start) {
            Some(caps) => super::captures_to_match_vec(&caps),
            None => Value::Boolean(false),
        })
    }

    /// test whether the entire string matches the pattern.
    ///
    /// `rx` may be a compiled `regexp` or a pattern string.
    /// returns a match vector if the full string matches, `#f` otherwise.
    #[tein_fn(name = "regexp-matches")]
    pub fn regexp_matches(rx: Value, text: String) -> Result<Value, String> {
        let inner = super::ensure_regexp(rx)?;
        Ok(match inner.captures(&text) {
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

    /// test whether the entire string matches (boolean only, faster than `regexp-matches`).
    ///
    /// `rx` may be a compiled `regexp` or a pattern string.
    #[tein_fn(name = "regexp-matches?")]
    pub fn regexp_matches_q(rx: Value, text: String) -> Result<bool, String> {
        let inner = super::ensure_regexp(rx)?;
        Ok(match inner.find(&text) {
            Some(m) => m.start() == 0 && m.end() == text.len(),
            None => false,
        })
    }

    /// replace the first match with the replacement string.
    ///
    /// `rx` may be a compiled `regexp` or a pattern string.
    /// replacement supports `$1`, `$2` etc. for capture group backrefs (rust regex syntax).
    #[tein_fn(name = "regexp-replace")]
    pub fn regexp_replace(rx: Value, text: String, replacement: String) -> Result<String, String> {
        let inner = super::ensure_regexp(rx)?;
        Ok(inner.replace(&text, replacement.as_str()).into_owned())
    }

    /// replace all non-overlapping matches.
    ///
    /// `rx` may be a compiled `regexp` or a pattern string.
    #[tein_fn(name = "regexp-replace-all")]
    pub fn regexp_replace_all(
        rx: Value,
        text: String,
        replacement: String,
    ) -> Result<String, String> {
        let inner = super::ensure_regexp(rx)?;
        Ok(inner.replace_all(&text, replacement.as_str()).into_owned())
    }

    /// split the string by the pattern. returns a scheme list of strings.
    ///
    /// `rx` may be a compiled `regexp` or a pattern string.
    #[tein_fn(name = "regexp-split")]
    pub fn regexp_split(rx: Value, text: String) -> Result<Value, String> {
        let inner = super::ensure_regexp(rx)?;
        Ok(Value::List(
            inner
                .split(&text)
                .map(|s| Value::String(s.to_string()))
                .collect(),
        ))
    }

    /// extract all non-overlapping matches as a list of match vectors.
    ///
    /// `rx` may be a compiled `regexp` or a pattern string.
    #[tein_fn(name = "regexp-extract")]
    pub fn regexp_extract(rx: Value, text: String) -> Result<Value, String> {
        let inner = super::ensure_regexp(rx)?;
        Ok(Value::List(
            inner
                .captures_iter(&text)
                .map(|caps| super::captures_to_match_vec(&caps))
                .collect(),
        ))
    }

    // --- match accessors ---

    /// return the number of groups in a match vector (including group 0 — the whole match).
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
    /// was unmatched (optional group). group 0 is the whole match.
    #[tein_fn(name = "regexp-match-submatch")]
    pub fn regexp_match_submatch(m: Value, n: i64) -> Result<Value, String> {
        let vec = match m {
            Value::Vector(v) => v,
            _ => return Err("regexp-match-submatch: expected match vector".into()),
        };
        let idx = n as usize;
        match vec.get(idx) {
            Some(Value::Vector(group)) => {
                Ok(group.first().cloned().unwrap_or(Value::Boolean(false)))
            }
            Some(Value::Boolean(false)) => Ok(Value::Boolean(false)),
            Some(_) => Err("regexp-match-submatch: malformed match vector".into()),
            None => Err(format!(
                "regexp-match-submatch: group index {} out of range ({})",
                n,
                vec.len()
            )),
        }
    }

    /// return a list of matched texts from all groups (group 0 first).
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

    // --- internal methods (used via safe-regexp-* naming from tests) ---

    #[tein_methods]
    impl Regexp {
        /// search for the first match anywhere in the string (internal method interface).
        ///
        /// available as `safe-regexp-search` in scheme. user-facing API: `regexp-search`.
        pub fn search(&self, text: String) -> Value {
            match self.inner.captures(&text) {
                Some(caps) => super::captures_to_match_vec(&caps),
                None => Value::Boolean(false),
            }
        }

        /// test whether the entire string matches (internal method interface).
        ///
        /// available as `safe-regexp-matches` in scheme. user-facing API: `regexp-matches`.
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

        /// test whether the entire string matches, boolean (internal method interface).
        ///
        /// available as `safe-regexp-matches?` in scheme. user-facing API: `regexp-matches?`.
        pub fn matches_q(&self, text: String) -> bool {
            match self.inner.find(&text) {
                Some(m) => m.start() == 0 && m.end() == text.len(),
                None => false,
            }
        }

        /// search starting from a byte offset (internal method interface).
        ///
        /// available as `safe-regexp-search-from`. user-facing API: `regexp-search-from`.
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

        /// replace first match (internal method interface).
        pub fn replace(&self, text: String, replacement: String) -> String {
            self.inner.replace(&text, replacement.as_str()).into_owned()
        }

        /// replace all matches (internal method interface).
        pub fn replace_all(&self, text: String, replacement: String) -> String {
            self.inner
                .replace_all(&text, replacement.as_str())
                .into_owned()
        }

        /// split by pattern (internal method interface).
        pub fn split(&self, text: String) -> Value {
            Value::List(
                self.inner
                    .split(&text)
                    .map(|s| Value::String(s.to_string()))
                    .collect(),
            )
        }

        /// extract all matches (internal method interface).
        pub fn extract(&self, text: String) -> Value {
            Value::List(
                self.inner
                    .captures_iter(&text)
                    .map(|caps| super::captures_to_match_vec(&caps))
                    .collect(),
            )
        }
    }
}

/// hand-written `.sld` — overrides macro-generated version to add `regexp-fold`.
///
/// all user-facing fns (`regexp-search`, `regexp-matches`, etc.) are native rust fns
/// registered via `define_fn_variadic` (patch H makes them importable from the library).
/// `regexp-fold` is registered separately as a hand-written native fn.
pub(crate) const SAFE_REGEXP_SLD: &str = r#"(define-library (tein safe-regexp)
  (import (scheme base))
  (export
    ;; constructor + predicate (native)
    regexp regexp?
    ;; user-facing API with string-or-regexp dispatch (native free fns)
    regexp-search regexp-search-from
    regexp-matches regexp-matches?
    regexp-replace regexp-replace-all
    regexp-extract regexp-split
    ;; match accessors (native free fns)
    regexp-match-count regexp-match-submatch regexp-match->list
    ;; higher-order iteration (native — calls scheme closures via sexp_apply)
    regexp-fold)
  (include "safe-regexp.scm"))
"#;

/// minimal `.scm` — all fns are native; this file only documents the module.
///
/// `regexp-fold` is registered by `register_regexp_fold` in context.rs as a
/// hand-written native fn that calls scheme closures via `ffi::sexp_apply`.
pub(crate) const SAFE_REGEXP_SCM: &str = r#";; (tein safe-regexp) — linear-time regex via rust's regex crate.
;;
;; all fns are native rust implementations registered at context build time.
;; regexp-fold iterates over non-overlapping matches via regexp-search-from,
;; calling the scheme kons closure for each match.
;;
;; byte-offset note: match vector start/end values are byte offsets (rust
;; regex semantics), not character offsets. use regexp-match-submatch for
;; text extraction — raw offsets are only needed for advanced use cases.
"#;

/// the `regexp-fold` native fn wrapper.
///
/// signature: `(regexp-fold rx kons knil str [finish [start [end]]])
///
/// iterates over non-overlapping matches. kons is called as `(kons i match str acc)`.
/// finish defaults to `(lambda (i m s a) a)`.
/// implemented as a hand-written `unsafe extern "C" fn` so we can call scheme
/// closures via `ffi::sexp_apply` using the ctx passed by chibi at call time.
///
/// # Safety
///
/// called by chibi-scheme — all pointers are valid in the chibi C API contract.
#[allow(unused_assignments)] // next_arg! macro: last `args = sexp_cdr(args)` is intentionally unused
pub(crate) unsafe extern "C" fn regexp_fold_wrapper(
    ctx: crate::ffi::sexp,
    _self: crate::ffi::sexp,
    _n: crate::ffi::sexp_sint_t,
    mut args: crate::ffi::sexp,
) -> crate::ffi::sexp {
    unsafe {
        use crate::Value;
        use crate::ffi;

        // helper: return a scheme error string
        macro_rules! err {
            ($msg:expr) => {{
                let msg: &str = $msg;
                let c = match ::std::ffi::CString::new(msg) {
                    Ok(c) => c,
                    Err(_) => return ffi::get_void(),
                };
                return ffi::sexp_c_str(ctx, c.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }};
        }

        // helper: extract one positional arg (sexp_car/sexp_cdr)
        macro_rules! next_arg {
            () => {{
                if ffi::sexp_nullp(args) != 0 {
                    err!("regexp-fold: too few arguments");
                }
                let v = ffi::sexp_car(args);
                args = ffi::sexp_cdr(args);
                v
            }};
        }

        // parse required args: rx, kons, knil, str
        let rx_sexp = next_arg!();
        let kons = next_arg!();
        let knil = next_arg!();
        let str_sexp = next_arg!();

        // parse optional: finish, start, end
        let has_finish = ffi::sexp_nullp(args) == 0;
        let finish_sexp = if has_finish {
            next_arg!()
        } else {
            ffi::get_void()
        };
        let has_start = ffi::sexp_nullp(args) == 0;
        let start_sexp = if has_start {
            next_arg!()
        } else {
            ffi::get_void()
        };
        let has_end = ffi::sexp_nullp(args) == 0;
        let end_sexp = if has_end {
            next_arg!()
        } else {
            ffi::get_void()
        };

        // extract the string
        let str_val = match Value::from_raw(ctx, str_sexp) {
            Ok(Value::String(s)) => s,
            _ => err!("regexp-fold: str argument must be a string"),
        };

        // extract the regex
        let rx_val = match Value::from_raw(ctx, rx_sexp) {
            Ok(v) => v,
            Err(_) => err!("regexp-fold: failed to extract rx argument"),
        };
        let inner: ::regex::Regex = match ensure_regexp(rx_val) {
            Ok(r) => r,
            Err(e) => {
                let msg: String = e;
                let c = match ::std::ffi::CString::new(msg.as_str()) {
                    Ok(c) => c,
                    Err(_) => return ffi::get_void(),
                };
                return ffi::sexp_c_str(ctx, c.as_ptr(), msg.len() as ffi::sexp_sint_t);
            }
        };

        // extract start / end (byte offsets into str_slice)
        let str_len = str_val.len() as i64;
        let start = if has_start {
            match Value::from_raw(ctx, start_sexp) {
                Ok(Value::Integer(n)) => n.max(0) as usize,
                _ => err!("regexp-fold: start must be an integer"),
            }
        } else {
            0
        };
        let end = if has_end {
            match Value::from_raw(ctx, end_sexp) {
                Ok(Value::Integer(n)) => n.max(0).min(str_len) as usize,
                _ => err!("regexp-fold: end must be an integer"),
            }
        } else {
            str_val.len()
        };

        let str_slice = if start == 0 && end == str_val.len() {
            str_val.clone()
        } else {
            str_val[start..end].to_string()
        };
        let slice_len = str_slice.len();

        // helper: call a scheme 4-arg proc (i, match, str, acc) via sexp_apply
        // i_raw: scheme fixnum for the match index
        // m_raw: scheme match vector (or #f for no match)
        // s_raw: scheme string for the slice
        // acc_raw: accumulator sexp
        let call4 = |proc: ffi::sexp,
                     i_raw: ffi::sexp,
                     m_raw: ffi::sexp,
                     s_raw: ffi::sexp,
                     acc_raw: ffi::sexp|
         -> ffi::sexp {
            // build arg list: (list i m s acc)
            let null = ffi::get_null();
            let _a4 = ffi::GcRoot::new(ctx, acc_raw);
            let mut lst = ffi::sexp_cons(ctx, acc_raw, null);
            let _r3 = ffi::GcRoot::new(ctx, lst);
            lst = ffi::sexp_cons(ctx, s_raw, lst);
            let _r2 = ffi::GcRoot::new(ctx, lst);
            lst = ffi::sexp_cons(ctx, m_raw, lst);
            let _r1 = ffi::GcRoot::new(ctx, lst);
            lst = ffi::sexp_cons(ctx, i_raw, lst);
            let _r0 = ffi::GcRoot::new(ctx, lst);
            ffi::sexp_apply_proc(ctx, proc, lst)
        };

        // build a scheme string for the slice
        let slice_cstr = match ::std::ffi::CString::new(str_slice.as_str()) {
            Ok(c) => c,
            Err(_) => err!("regexp-fold: str contains null bytes"),
        };
        let s_raw = ffi::sexp_c_str(ctx, slice_cstr.as_ptr(), slice_len as ffi::sexp_sint_t);
        let _s_root = ffi::GcRoot::new(ctx, s_raw);

        // iteration loop
        let mut acc = knil;
        let mut pos: usize = 0;
        let mut i: i64 = 0;

        loop {
            if pos > slice_len {
                break;
            }
            let m_val = inner.captures_at(&str_slice, pos);

            match m_val {
                None => {
                    // no match — call finish if provided, else return acc
                    if has_finish {
                        let i_raw = ffi::sexp_make_fixnum(i as ffi::sexp_sint_t);
                        acc = call4(finish_sexp, i_raw, ffi::get_false(), s_raw, acc);
                    }
                    break;
                }
                Some(caps) => {
                    let whole = caps.get(0).unwrap();
                    let match_end = whole.end();

                    // convert match to scheme vector
                    let m_val = captures_to_match_vec(&caps);
                    let m_raw = match m_val.to_raw(ctx) {
                        Ok(r) => r,
                        Err(_) => err!("regexp-fold: failed to convert match vector"),
                    };
                    let _m_root = ffi::GcRoot::new(ctx, m_raw);

                    let i_raw = ffi::sexp_make_fixnum(i as ffi::sexp_sint_t);
                    acc = call4(kons, i_raw, m_raw, s_raw, acc);
                    let _acc_root = ffi::GcRoot::new(ctx, acc);

                    // advance: +1 for zero-width matches (safe — captures_at handles boundary)
                    let next_pos = if match_end == pos { pos + 1 } else { match_end };
                    pos = next_pos;
                    i += 1;

                    if pos > slice_len {
                        // past end after advance — call finish
                        if has_finish {
                            let i_raw = ffi::sexp_make_fixnum(i as ffi::sexp_sint_t);
                            acc = call4(finish_sexp, i_raw, ffi::get_false(), s_raw, acc);
                        }
                        break;
                    }
                }
            }
        }

        acc
    }
}

#[cfg(test)]
mod tests {
    use crate::{Context, Value};

    fn ctx() -> Context {
        Context::new_standard().unwrap()
    }

    // --- compilation ---

    #[test]
    fn compile_valid_pattern() {
        let result = ctx()
            .evaluate(r#"(import (tein safe-regexp)) (regexp? (regexp "\\d+"))"#)
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn compile_invalid_pattern() {
        // invalid pattern raises a scheme exception (see AGENTS.md)
        let result = ctx().evaluate(r#"(import (tein safe-regexp)) (regexp "[")"#);
        assert!(result.is_err(), "expected error, got {result:?}");
    }

    #[test]
    fn regexp_q_non_regexp() {
        let result = ctx()
            .evaluate(r#"(import (tein safe-regexp)) (regexp? "hello")"#)
            .unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn regexp_q_integer_not_regexp() {
        let result = ctx()
            .evaluate(r#"(import (tein safe-regexp)) (regexp? 42)"#)
            .unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    // --- search + matches (user-facing, string-or-regexp dispatch) ---

    #[test]
    fn search_basic() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (let ((m (regexp-search "(\\d+)-(\\d+)" "foo-42-7-bar")))
                  (vector-ref (vector-ref m 0) 0))
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("42-7".into()));
    }

    #[test]
    fn search_with_compiled_regexp() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (let ((rx (regexp "(\\d+)-(\\d+)")))
                  (let ((m (regexp-search rx "foo-42-7-bar")))
                    (vector-ref (vector-ref m 0) 0)))
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("42-7".into()));
    }

    #[test]
    fn search_no_match() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-search "xyz" "abc")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn matches_full_string() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-matches? "\\d+" "42")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn matches_partial_rejects() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-matches? "\\d+" "foo42bar")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn search_from_offset() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (let ((m (regexp-search-from "\\d+" "abc 42 def 99" 7)))
                  (vector-ref (vector-ref m 0) 0))
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("99".into()));
    }

    #[test]
    fn match_vector_shape() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (let ((m (regexp-search "(\\d+)-(\\d+)" "x-42-7-y")))
                  (list
                    (vector-length m)
                    (vector-ref (vector-ref m 0) 0)
                    (vector-ref (vector-ref m 1) 0)
                    (vector-ref (vector-ref m 2) 0)))
            "#,
            )
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
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (let ((m (regexp-search "(a)(b)?(c)" "ac")))
                  (vector-ref m 2))
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    // --- replace, split, extract ---

    #[test]
    fn replace_first() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-replace "\\d+" "a1b2c3" "X")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("aXb2c3".into()));
    }

    #[test]
    fn replace_all() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-replace-all "\\d+" "a1b2c3" "X")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("aXbXcX".into()));
    }

    #[test]
    fn replace_with_backref() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-replace "(\\w+)@(\\w+)" "user@host" "$2/$1")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("host/user".into()));
    }

    #[test]
    fn split_basic() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-split ",\\s*" "a, b,c , d")
            "#,
            )
            .unwrap();
        // ",\\s*" on "c , d": the comma and trailing space are consumed, leaving "c "
        // (leading space from "c " is not part of the delimiter match).
        assert_eq!(
            result,
            Value::List(vec![
                Value::String("a".into()),
                Value::String("b".into()),
                Value::String("c ".into()),
                Value::String("d".into()),
            ])
        );
    }

    #[test]
    fn extract_all_matches() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (length (regexp-extract "\\d+" "a1b22c333"))
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn replace_no_match() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-replace "xyz" "hello" "X")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("hello".into()));
    }

    #[test]
    fn split_no_match() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-split "," "hello")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::List(vec![Value::String("hello".into())]));
    }

    #[test]
    fn split_consecutive_delimiters() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-split "," "a,,b,,,c")
            "#,
            )
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

    // --- match accessors ---

    #[test]
    fn match_count() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-match-count (regexp-search "(a)(b)(c)" "abc"))
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Integer(4)); // whole + 3 groups
    }

    #[test]
    fn match_submatch() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-match-submatch (regexp-search "(\\d+)-(\\d+)" "x-42-7-y") 2)
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("7".into()));
    }

    #[test]
    fn match_to_list() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-match->list (regexp-search "(a)(b)?(c)" "ac"))
            "#,
            )
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

    // --- regexp-fold ---

    #[test]
    fn fold_collect_matches() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-fold "\\d+"
                  (lambda (i m s acc)
                    (cons (regexp-match-submatch m 0) acc))
                  '()
                  "a1b22c333")
            "#,
            )
            .unwrap();
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
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-fold "\\d+"
                  (lambda (i m s acc) (+ acc 1))
                  0
                  "a1b22c333"
                  (lambda (i m s acc) (* acc 10)))
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Integer(30));
    }

    #[test]
    fn fold_empty_string() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-fold "\\d+"
                  (lambda (i m s acc) (+ acc 1))
                  0
                  "")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Integer(0));
    }

    #[test]
    fn fold_no_matches() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-fold "\\d+"
                  (lambda (i m s acc) (+ acc 1))
                  0
                  "no numbers here")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Integer(0));
    }

    // --- edge cases + unicode ---

    #[test]
    fn empty_pattern_matches_everywhere() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (length (regexp-extract "" "abc"))
            "#,
            )
            .unwrap();
        // empty pattern matches at: before a, before b, before c, after c = 4
        assert_eq!(result, Value::Integer(4));
    }

    #[test]
    fn unicode_match() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (let ((m (regexp-search "café" "I love café!")))
                  (vector-ref (vector-ref m 0) 0))
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("café".into()));
    }

    #[test]
    fn unicode_byte_offsets() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (let ((m (regexp-search "café" "I love café!")))
                  (list (vector-ref (vector-ref m 0) 1)
                        (vector-ref (vector-ref m 0) 2)))
            "#,
            )
            .unwrap();
        // "I love " = 7 bytes, "café" = 5 bytes (c=1, a=1, f=1, é=2)
        assert_eq!(
            result,
            Value::List(vec![Value::Integer(7), Value::Integer(12)])
        );
    }

    #[test]
    fn search_from_out_of_bounds() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-search-from "\\d+" "abc" 100)
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn matches_with_captures_full() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (let ((m (regexp-matches "(\\d{4})-(\\d{2})-(\\d{2})" "2026-03-04")))
                  (regexp-match->list m))
            "#,
            )
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
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-fold ""
                  (lambda (i m s acc) (+ acc 1))
                  0
                  "ab")
            "#,
            )
            .unwrap();
        // matches at positions 0, 1, 2 (before a, before b, after b)
        assert_eq!(result, Value::Integer(3));
    }

    // --- internal method names (for backwards compat + low-level use) ---

    #[test]
    fn internal_safe_regexp_search() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (let ((m (safe-regexp-search (regexp "(\\d+)-(\\d+)") "foo-42-7-bar")))
                  (vector-ref (vector-ref m 0) 0))
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::String("42-7".into()));
    }

    #[test]
    fn internal_safe_regexp_matches_q() {
        let result = ctx()
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (safe-regexp-matches? (regexp "\\d+") "42")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    // --- sandbox ---

    #[test]
    fn sandbox_safe_regexp() {
        let ctx = crate::Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .build()
            .unwrap();
        let result = ctx
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp-matches? "\\d+" "42")
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn sandbox_modules_safe_includes_safe_regexp() {
        let ctx = crate::Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .build()
            .unwrap();
        let result = ctx
            .evaluate(
                r#"
                (import (tein safe-regexp))
                (regexp? (regexp "test"))
            "#,
            )
            .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }
}
