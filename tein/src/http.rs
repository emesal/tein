//! `(tein http)` — HTTP client via the `ureq` crate.
//!
//! provides a single native fn `http-request-internal` registered via
//! `define_fn_variadic` in `context.rs`. scheme wrappers in the VFS
//! `.sld`/`.scm` provide the user-facing API:
//! - `http-request` — generic request with optional timeout (default 30s)
//! - `http-get`, `http-post`, `http-put`, `http-delete` — convenience procs
//!
//! follows the json/toml pattern: plain rust module + hand-written trampoline,
//! no `#[tein_module]` macro.

use std::ffi::CString;
use std::time::Duration;

use crate::{Value, ffi};

/// scheme library definition for `(tein http)`.
pub(crate) const HTTP_SLD: &str = "\
(define-library (tein http)
  (import (scheme base) (chibi))
  (export http-request http-get http-post http-put http-delete)
  (include \"http.scm\"))";

/// scheme implementation for `(tein http)`.
///
/// `http-request-internal` is a native fn registered by the rust runtime
/// via `define_fn_variadic` when a standard-env context is built.
pub(crate) const HTTP_SCM: &str = "\
;;; (tein http) — HTTP client
;;;
;;; http-request-internal is registered by the rust runtime.
;;; this file provides the user-facing API with default timeout.
;;;
;;; on success, returns an alist: ((status . N) (headers ...) (body . \"...\"))
;;; on transport error, raises a scheme exception (error object).
;;; use (guard ...) to catch errors. closes gh #135.

(define %default-timeout 30)

(define (http-request method url headers body . args)
  (let ((timeout (if (null? args) %default-timeout (car args))))
    (http-request-internal method url headers body timeout)))

(define (http-get url headers . args)
  (apply http-request \"GET\" url headers #f args))

(define (http-post url headers body . args)
  (apply http-request \"POST\" url headers body args))

(define (http-put url headers body . args)
  (apply http-request \"PUT\" url headers body args))

(define (http-delete url headers . args)
  (apply http-request \"DELETE\" url headers #f args))
";

/// build a response alist: `((status . N) (headers (k . v) ...) (body . "..."))`
fn build_response_alist(status: u16, headers: &[(String, String)], body: &str) -> Value {
    let status_pair = Value::Pair(
        Box::new(Value::Symbol("status".into())),
        Box::new(Value::Integer(i64::from(status))),
    );

    let header_pairs: Vec<Value> = headers
        .iter()
        .map(|(k, v)| {
            Value::Pair(
                Box::new(Value::String(k.clone())),
                Box::new(Value::String(v.clone())),
            )
        })
        .collect();

    let headers_entry = Value::Pair(
        Box::new(Value::Symbol("headers".into())),
        Box::new(if header_pairs.is_empty() {
            Value::Nil
        } else {
            Value::List(header_pairs)
        }),
    );

    let body_pair = Value::Pair(
        Box::new(Value::Symbol("body".into())),
        Box::new(Value::String(body.to_string())),
    );

    Value::List(vec![status_pair, headers_entry, body_pair])
}

/// set headers on a request builder. works for any `RequestBuilder<S>` via macro
/// since ureq doesn't expose the `HasHeaders` trait publicly.
macro_rules! with_headers {
    ($req:expr, $headers:expr) => {{
        let mut r = $req;
        for (name, value) in $headers {
            r = r.header(name.as_str(), value.as_str());
        }
        r
    }};
}

/// extract status, headers, and body from an HTTP response.
fn extract_response(
    response: &mut ureq::http::Response<ureq::Body>,
) -> std::result::Result<Value, String> {
    let status = response.status().as_u16();

    let resp_headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_lowercase(),
                // non-ASCII header values (rare) become "" — acceptable for
                // an embedding context; callers needing raw bytes should use
                // a lower-level HTTP library.
                v.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();

    let body_str = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("http: failed to read body: {e}"))?;

    Ok(build_response_alist(status, &resp_headers, &body_str))
}

/// execute an HTTP request. returns `Ok(response_alist)` or `Err(message)`.
///
/// ureq 3.x uses typed builders per method: GET/HEAD/DELETE/OPTIONS/TRACE
/// yield `WithoutBody` (`.call()`), POST/PUT/PATCH yield `WithBody` (`.send()`).
fn do_http_request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&str>,
    timeout_secs: f64,
) -> std::result::Result<Value, String> {
    // clamp to 1ms minimum — Duration::from_secs_f64 panics on negative values
    // and a zero timeout is meaningless.
    let timeout_secs = timeout_secs.max(0.001);

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs_f64(timeout_secs)))
        .build()
        .into();

    let mut response = match method {
        "GET" => with_headers!(agent.get(url), headers)
            .call()
            .map_err(|e| format!("http: {e}"))?,
        "HEAD" => with_headers!(agent.head(url), headers)
            .call()
            .map_err(|e| format!("http: {e}"))?,
        "DELETE" => with_headers!(agent.delete(url), headers)
            .call()
            .map_err(|e| format!("http: {e}"))?,
        "OPTIONS" => with_headers!(agent.options(url), headers)
            .call()
            .map_err(|e| format!("http: {e}"))?,
        "TRACE" => with_headers!(agent.trace(url), headers)
            .call()
            .map_err(|e| format!("http: {e}"))?,
        "POST" => {
            let req = with_headers!(agent.post(url), headers);
            match body {
                Some(b) => req.send(b),
                None => req.send_empty(),
            }
            .map_err(|e| format!("http: {e}"))?
        }
        "PUT" => {
            let req = with_headers!(agent.put(url), headers);
            match body {
                Some(b) => req.send(b),
                None => req.send_empty(),
            }
            .map_err(|e| format!("http: {e}"))?
        }
        "PATCH" => {
            let req = with_headers!(agent.patch(url), headers);
            match body {
                Some(b) => req.send(b),
                None => req.send_empty(),
            }
            .map_err(|e| format!("http: {e}"))?
        }
        _ => return Err(format!("http: unsupported method: {method}")),
    };

    extract_response(&mut response)
}

/// FFI trampoline for `http-request-internal`.
///
/// scheme signature: `(http-request-internal method url headers body timeout)`
/// - method: string
/// - url: string
/// - headers: alist ((name . value) ...)
/// - body: string or #f
/// - timeout: number (seconds, fractional ok)
///
/// returns response alist on success, raises exception on transport failure.
///
/// # Safety
///
/// called from chibi scheme VM via `define_fn_variadic`. all sexp pointers
/// are valid for the duration of the call. no GC-rooting needed — no
/// allocating FFI calls between arg extraction and the ureq call.
pub(crate) unsafe extern "C" fn http_request_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    mut args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        // extract method (string)
        let method_sexp = ffi::sexp_car(args);
        args = ffi::sexp_cdr(args);
        if ffi::sexp_stringp(method_sexp) == 0 {
            let msg = "http-request-internal: method must be a string";
            let c_msg = CString::new(msg).unwrap();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let method = ffi::sexp_to_rust_string(method_sexp);

        // extract url (string)
        let url_sexp = ffi::sexp_car(args);
        args = ffi::sexp_cdr(args);
        if ffi::sexp_stringp(url_sexp) == 0 {
            let msg = "http-request-internal: url must be a string";
            let c_msg = CString::new(msg).unwrap();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
        let url = ffi::sexp_to_rust_string(url_sexp);

        // check HTTP URL policy before making the request
        if !crate::context::check_http_access(&url) {
            let msg = format!(
                "[sandbox:http] http request blocked: URL not in allowlist: {}",
                url
            );
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        // extract headers (list of pairs)
        let headers_sexp = ffi::sexp_car(args);
        args = ffi::sexp_cdr(args);
        let mut headers = Vec::new();
        let mut h = headers_sexp;
        while ffi::sexp_pairp(h) != 0 {
            let entry = ffi::sexp_car(h);
            if ffi::sexp_pairp(entry) != 0 {
                let k = ffi::sexp_car(entry);
                let v = ffi::sexp_cdr(entry);
                if ffi::sexp_stringp(k) != 0 && ffi::sexp_stringp(v) != 0 {
                    headers.push((ffi::sexp_to_rust_string(k), ffi::sexp_to_rust_string(v)));
                }
            }
            h = ffi::sexp_cdr(h);
        }

        // extract body (string or #f)
        let body_sexp = ffi::sexp_car(args);
        args = ffi::sexp_cdr(args);
        let body = if ffi::sexp_stringp(body_sexp) != 0 {
            Some(ffi::sexp_to_rust_string(body_sexp))
        } else {
            None // #f or anything else → no body
        };

        // extract timeout (number)
        let timeout_sexp = ffi::sexp_car(args);
        let timeout_secs = if ffi::sexp_flonump(timeout_sexp) != 0 {
            ffi::sexp_flonum_value(timeout_sexp)
        } else if ffi::sexp_integerp(timeout_sexp) != 0 {
            ffi::sexp_unbox_fixnum(timeout_sexp) as f64
        } else {
            30.0
        };

        // execute the request
        match do_http_request(&method, &url, &headers, body.as_deref(), timeout_secs) {
            Ok(value) => match value.to_raw(ctx) {
                Ok(raw) => raw,
                Err(e) => {
                    let msg = format!("http: response conversion failed: {e}");
                    let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                    ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
                }
            },
            Err(msg) => {
                let c_msg = CString::new(msg.as_str()).unwrap_or_default();
                ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_response_alist_structure() {
        let headers = vec![
            ("content-type".to_string(), "application/json".to_string()),
            ("x-req-id".to_string(), "abc".to_string()),
        ];
        let alist = build_response_alist(200, &headers, "hello");
        match &alist {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                // status
                match &items[0] {
                    Value::Pair(k, v) => {
                        assert_eq!(**k, Value::Symbol("status".into()));
                        assert_eq!(**v, Value::Integer(200));
                    }
                    other => panic!("expected pair for status, got {other:?}"),
                }
                // headers
                match &items[1] {
                    Value::Pair(k, v) => {
                        assert_eq!(**k, Value::Symbol("headers".into()));
                        match &**v {
                            Value::List(hs) => assert_eq!(hs.len(), 2),
                            other => panic!("expected list for headers, got {other:?}"),
                        }
                    }
                    other => panic!("expected pair for headers, got {other:?}"),
                }
                // body
                match &items[2] {
                    Value::Pair(k, v) => {
                        assert_eq!(**k, Value::Symbol("body".into()));
                        assert_eq!(**v, Value::String("hello".into()));
                    }
                    other => panic!("expected pair for body, got {other:?}"),
                }
            }
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn build_response_alist_empty_headers() {
        let alist = build_response_alist(404, &[], "not found");
        match &alist {
            Value::List(items) => match &items[1] {
                Value::Pair(_, v) => assert_eq!(**v, Value::Nil),
                other => panic!("expected pair, got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn do_http_request_bad_url() {
        let result = do_http_request("GET", "not-a-url", &[], None, 5.0);
        assert!(result.is_err());
    }

    #[test]
    fn do_http_request_connection_refused() {
        let result = do_http_request("GET", "http://127.0.0.1:1", &[], None, 1.0);
        assert!(result.is_err());
    }

    #[test]
    fn do_http_request_unsupported_method() {
        let result = do_http_request("BOGUS", "http://example.com", &[], None, 1.0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unsupported method"));
    }
}
