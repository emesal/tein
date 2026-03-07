# (tein http) — rust-backed HTTP client

closes #130

## summary

eager HTTP client module backed by `ureq` (blocking, minimal deps, rustls TLS).
feature-gated as `http`. `default_safe: false` — network access is inherently
unsandboxable.

## API

```scheme
(import (tein http))

;; generic
(http-request method url headers body)           ; 30s default timeout
(http-request method url headers body timeout)    ; custom timeout (seconds)

;; convenience
(http-get url headers)
(http-get url headers timeout)
(http-post url headers body)
(http-post url headers body timeout)
(http-put url headers body)
(http-put url headers body timeout)
(http-delete url headers)
(http-delete url headers timeout)
```

### parameters

- `method` — string: `"GET"`, `"POST"`, `"PUT"`, `"DELETE"`, etc.
- `url` — string: full URL including scheme
- `headers` — alist: `((name . value) ...)`
- `body` — string or `#f` for no body
- `timeout` — number (seconds, fractional ok) or omitted for 30s default

### response

alist with three keys:

```scheme
((status . 200)
 (headers (content-type . "application/json") (x-request-id . "abc") ...)
 (body . "{\"ok\": true}"))
```

- `status` — integer HTTP status code
- `headers` — alist, names lowercased, duplicate keys preserved as separate entries
- `body` — string (v1: text only)

### errors

network/TLS/DNS/timeout failures return an error string (consistent with tein's
`Result::Err` → string pattern). HTTP 4xx/5xx are normal responses, not errors.

## implementation

### rust side

- `src/http.rs` with `#[tein_module("http")]`
- one native fn: `http-request-internal` (variadic, handles optional timeout)
- takes method, url, headers alist, body (string or `#f`), optional timeout
- uses `ureq` to execute the request
- builds response alist from `ureq::Response`
- returns error string on transport failure

### scheme side

- Dynamic VFS module (like uuid/crypto)
- `.sld` + `.scm` registered as VFS entries
- convenience procs (`http-get`, `http-post`, `http-put`, `http-delete`) delegate
  to `http-request` which calls `http-request-internal`

### integration points

following the established tein module pattern:

1. `Cargo.toml` — `http = ["dep:ureq"]` feature
2. `src/http.rs` — `#[tein_module("http")]`
3. `src/lib.rs` — `#[cfg(feature = "http")] mod http;`
4. `src/context.rs` — feature-gated registration in `build()`
5. `src/vfs_registry.rs` — `VfsEntry` with `default_safe: false`, `VfsSource::Dynamic`
6. `build.rs` — `DYNAMIC_MODULE_EXPORTS` table + `feature_enabled()` match arm
7. `src/sandbox.rs` — `feature_enabled()` match arm (mirror)

### sandbox

`default_safe: false`. sandboxed contexts cannot import `(tein http)` unless
explicitly allowed via `.allow_module("tein/http")`. UX stubs generated for
sandbox error messages.

### timeout

default 30 seconds. `ureq` timeouts are set per-request via the agent builder
or request-level config.
