# (tein http) implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** add a feature-gated `(tein http)` scheme module backed by `ureq` for TLS-capable HTTP from scheme. closes #130.

**Architecture:** one native rust fn (`http-request-internal`) registered via `define_fn_variadic` (json/toml pattern — no `#[tein_module]`). scheme wrappers in VFS `.sld`/`.scm` provide `http-request` (default 30s timeout) and convenience procs (`http-get`, `http-post`, `http-put`, `http-delete`). Dynamic VFS module, `default_safe: false`.

**Tech stack:** `ureq` 3.x (blocking, rustls TLS), `http_status_as_error(false)` so 4xx/5xx are normal responses.

**Design doc:** `docs/plans/2026-03-06-http-module-design.md`

**Branch:** create with `just feature http-module-2603`

---

### task 1: branch + cargo.toml + feature gate

**files:**
- modify: `tein/Cargo.toml` (deps around line 25, features around line 48)

**step 1: create branch**

```bash
just feature http-module-2603
```

**step 2: add ureq dependency**

in `[dependencies]` (after line 25 — after the `rand` line), add:

```toml
ureq = { version = "3", default-features = false, features = ["rustls"], optional = true }
```

`default-features = false` avoids gzip. `rustls` for TLS without native openssl.

**step 3: add feature gate**

in `[features]` (after line 48 — after the `crypto` line), add:

```toml
## enables `(tein http)` module with HTTP client (GET/POST/PUT/DELETE) and TLS support.
## pulls in ureq with rustls. not included in default — network access is opt-in.
http = ["dep:ureq"]
```

do NOT add `"http"` to the `default` feature list.

**step 4: verify**

run: `cargo build -p tein`
expected: compiles (no code references ureq yet).

**step 5: commit**

```
feat(http): add ureq dependency and feature gate (#130)
```

---

### task 2: rust module — `src/http.rs`

**files:**
- create: `tein/src/http.rs`
- modify: `tein/src/lib.rs` (around line 68, add the cfg mod)

**step 1: create `src/http.rs`**

```rust
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

use std::time::Duration;

use crate::{Value, ffi};

/// scheme library definition for `(tein http)`.
pub(crate) const HTTP_SLD: &str = "\
(define-library (tein http)
  (import (scheme base))
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

/// execute an HTTP request. returns `Ok(response_alist)` or `Err(message)`.
fn do_http_request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&str>,
    timeout_secs: f64,
) -> std::result::Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs_f64(timeout_secs)))
        .build()
        .into();

    let mut req = agent.request(method, url).map_err(|e| format!("http: {e}"))?;

    for (name, value) in headers {
        req = req.header(name.as_str(), value.as_str());
    }

    let mut response = match body {
        Some(b) => req.send(b),
        None => req.call(),
    }
    .map_err(|e| format!("http: {e}"))?;

    let status = response.status().as_u16();

    let resp_headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_lowercase(),
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

/// FFI trampoline for `http-request-internal`.
///
/// scheme signature: `(http-request-internal method url headers body timeout)`
/// - method: string
/// - url: string
/// - headers: alist ((name . value) ...)
/// - body: string or #f
/// - timeout: number (seconds, fractional ok)
///
/// returns response alist on success, error string on transport failure.
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
    // extract method (string)
    let method_sexp = ffi::sexp_car(args);
    args = ffi::sexp_cdr(args);
    if ffi::sexp_stringp(method_sexp) == 0 {
        return ffi::sexp_c_str(ctx, "http-request-internal: method must be a string");
    }
    let method = ffi::sexp_string_to_rust(method_sexp);

    // extract url (string)
    let url_sexp = ffi::sexp_car(args);
    args = ffi::sexp_cdr(args);
    if ffi::sexp_stringp(url_sexp) == 0 {
        return ffi::sexp_c_str(ctx, "http-request-internal: url must be a string");
    }
    let url = ffi::sexp_string_to_rust(url_sexp);

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
                headers.push((
                    ffi::sexp_string_to_rust(k),
                    ffi::sexp_string_to_rust(v),
                ));
            }
        }
        h = ffi::sexp_cdr(h);
    }

    // extract body (string or #f)
    let body_sexp = ffi::sexp_car(args);
    args = ffi::sexp_cdr(args);
    let body = if ffi::sexp_stringp(body_sexp) != 0 {
        Some(ffi::sexp_string_to_rust(body_sexp))
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
    match do_http_request(
        &method,
        &url,
        &headers,
        body.as_deref(),
        timeout_secs,
    ) {
        Ok(value) => value.to_raw(ctx),
        Err(msg) => ffi::sexp_c_str(ctx, &msg),
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
            Value::List(items) => {
                match &items[1] {
                    Value::Pair(_, v) => assert_eq!(**v, Value::Nil),
                    other => panic!("expected pair, got {other:?}"),
                }
            }
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
}
```

**step 2: add mod declaration to `lib.rs`**

in `tein/src/lib.rs`, around line 68 (after `mod ffi;`, before `pub mod foreign;`), add:

```rust
#[cfg(feature = "http")]
mod http;
```

keep alphabetical order among the feature-gated modules.

**step 3: run tests**

run: `cargo test -p tein --features http --lib http::tests -- --nocapture`
expected: all 4 tests pass.

**step 4: commit**

```
feat(http): add http.rs with native request fn and tests (#130)
```

---

### task 3: VFS registry entry

**files:**
- modify: `tein/src/vfs_registry.rs` (around line 164, after the crypto entry)

**step 1: add VfsEntry**

after the `tein/crypto` entry (line ~164), add:

```rust
VfsEntry {
    path: "tein/http",
    deps: &["scheme/base"],
    files: &[],
    clib: None,
    default_safe: false,
    source: VfsSource::Dynamic,
    feature: Some("http"),
    shadow_sld: None,
},
```

note: `default_safe: false` — network access is not sandbox-safe.

**step 2: verify**

run: `cargo build -p tein --features http`
expected: compiles.

**step 3: commit**

```
feat(http): add VFS registry entry (#130)
```

---

### task 4: feature_enabled + DYNAMIC_MODULE_EXPORTS

**files:**
- modify: `tein/build.rs` (lines ~284-299 and ~315-362)
- modify: `tein/src/sandbox.rs` (lines ~398-413)

**step 1: add to build.rs `feature_enabled()`**

in `build.rs` around line 292 (after the `"crypto"` arm), add:

```rust
Some("http") => cfg!(feature = "http"),
```

**step 2: add to build.rs `DYNAMIC_MODULE_EXPORTS`**

in `build.rs` after the `tein/modules` entry (around line 361), add:

```rust
// src/http.rs — hand-written trampoline, feature=http
("tein/http", &[
    "http-request",
    "http-get",
    "http-post",
    "http-put",
    "http-delete",
]),
```

**step 3: add to sandbox.rs `feature_enabled()`**

in `sandbox.rs` around line 406 (after the `"crypto"` arm), add:

```rust
Some("http") => cfg!(feature = "http"),
```

**step 4: verify**

run: `cargo build -p tein --features http`
expected: compiles.

**step 5: commit**

```
feat(http): add feature checks and dynamic module exports (#130)
```

---

### task 5: context.rs registration

**files:**
- modify: `tein/src/context.rs` (around line 2374, after the crypto registration block)

**step 1: add registration block**

after the `#[cfg(feature = "crypto")]` block (line ~2374), add:

```rust
#[cfg(feature = "http")]
if self.standard_env {
    context.define_fn_variadic(
        "http-request-internal",
        crate::http::http_request_trampoline,
    )?;
    context.register_vfs_module("lib/tein/http.sld", crate::http::HTTP_SLD)?;
    context.register_vfs_module("lib/tein/http.scm", crate::http::HTTP_SCM)?;
}
```

**step 2: verify**

run: `cargo build -p tein --features http`
expected: compiles.

**step 3: commit**

```
feat(http): register trampoline and VFS modules in context (#130)
```

---

### task 6: integration tests

**files:**
- create: `tein/tests/http_tests.rs`

these test the scheme-level API end-to-end via `Context::evaluate()`. we can't make real HTTP requests in tests, so test: module imports, error paths, and response structure validation.

**step 1: write integration tests**

```rust
//! integration tests for `(tein http)` module.
//!
//! NOTE: these tests do NOT make real HTTP requests (no network in CI).
//! they verify: module import, error returns on bad URLs, scheme wrapper
//! availability, and sandbox blocking.

#[cfg(feature = "http")]
mod http_integration {
    use tein::{Context, ContextBuilder, Value};

    fn ctx() -> Context {
        ContextBuilder::new().standard_env().build().unwrap()
    }

    #[test]
    fn import_tein_http() {
        let ctx = ctx();
        let result = ctx.evaluate("(import (tein http))");
        assert!(result.is_ok(), "failed to import (tein http): {result:?}");
    }

    #[test]
    fn http_get_bad_url_returns_error_string() {
        let ctx = ctx();
        ctx.evaluate("(import (tein http))").unwrap();
        let result = ctx.evaluate("(http-get \"not-a-url\" '())").unwrap();
        assert!(
            matches!(result, Value::String(_)),
            "expected error string, got {result:?}"
        );
    }

    #[test]
    fn http_request_bad_url_returns_error_string() {
        let ctx = ctx();
        ctx.evaluate("(import (tein http))").unwrap();
        let result = ctx
            .evaluate("(http-request \"GET\" \"not-a-url\" '() #f)")
            .unwrap();
        assert!(
            matches!(result, Value::String(_)),
            "expected error string, got {result:?}"
        );
    }

    #[test]
    fn http_request_with_timeout() {
        let ctx = ctx();
        ctx.evaluate("(import (tein http))").unwrap();
        let result = ctx
            .evaluate("(http-request \"GET\" \"http://127.0.0.1:1\" '() #f 0.5)")
            .unwrap();
        assert!(
            matches!(result, Value::String(_)),
            "expected error string on refused connection, got {result:?}"
        );
    }

    #[test]
    fn convenience_procs_exist() {
        let ctx = ctx();
        ctx.evaluate("(import (tein http))").unwrap();
        // verify all convenience procs are bound (calling them would need network)
        for proc_name in &[
            "http-request",
            "http-get",
            "http-post",
            "http-put",
            "http-delete",
        ] {
            let check = format!("(procedure? {proc_name})");
            let result = ctx.evaluate(&check).unwrap();
            assert_eq!(
                result,
                Value::Boolean(true),
                "{proc_name} should be a procedure"
            );
        }
    }

    #[test]
    fn sandbox_blocks_tein_http() {
        let ctx = ContextBuilder::new()
            .standard_env()
            .sandboxed()
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (tein http))");
        assert!(
            result.is_err(),
            "sandboxed context should block (tein http)"
        );
    }
}
```

**step 2: run integration tests**

run: `cargo test -p tein --features http --test http_tests -- --nocapture`
expected: all 6 tests pass.

**step 3: commit**

```
test(http): integration tests for (tein http) module (#130)
```

---

### task 7: lint + final verification

**step 1: run full test suite**

run: `just test` (ensure no regressions in the 439+ existing tests)
expected: all tests pass. the http tests only run when `--features http`.

also: `cargo test -p tein --features http`
expected: all http tests pass.

**step 2: lint**

run: `just lint`
expected: clean.

**step 3: fix any issues, commit**

```
chore: cargo fmt (#130)
```

---

### task 8: update AGENTS.md

**files:**
- modify: `tein/AGENTS.md`

**step 1: update architecture section**

add to the `src/` listing:

```
  http.rs        — HTTP_SLD/HTTP_SCM constants, do_http_request (ureq), http_request_trampoline. feature=http
```

add to commands section:

```bash
cargo test -p tein --features http   # http module tests
```

**step 2: add critical gotcha if any emerged during implementation**

review any quirks discovered during implementation and document them.

**step 3: commit**

```
docs: update AGENTS.md for (tein http) module (#130)
```

---

### task 9: update docs

**files:**
- modify: `docs/reference.md` (if it exists — add `(tein http)` to VFS module list)

add `(tein http)` to the module reference with its exports and usage examples.

**step 1: commit**

```
docs: add (tein http) to reference docs (#130)

closes #130
```

---

## notes for agent

- `ureq` 3.x: `Agent::config_builder().http_status_as_error(false)` makes 4xx/5xx normal responses
- `ureq` re-exports `http` crate as `ureq::http` — response uses `http::Response<Body>`
- response headers via `.headers().iter()` gives `(HeaderName, &HeaderValue)` pairs
- body via `.body_mut().read_to_string()` — requires `use std::io::Read` (ureq re-exports)
- the trampoline uses `Value::to_raw(ctx)` to convert the rust `Value` alist back to a scheme sexp
- `sexp_c_str` allocates — but we only call it on error paths after all arg extraction is done, so no GC rooting needed for extracted args
- if `ureq` API doesn't match exactly (version drift), check `https://docs.rs/ureq/3/ureq/`
- `default-features = false` on ureq: verify that `rustls` alone is sufficient for HTTPS
