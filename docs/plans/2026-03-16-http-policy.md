# HttpPolicy Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add URL-prefix allowlist for `(tein http)` in sandboxed contexts, enabling embedders to grant controlled HTTP access.

**Architecture:** Follows the `FsPolicy` pattern — `HttpPolicy` struct in `sandbox.rs`, `HTTP_POLICY` thread-local, `http_allow` builder method that auto-enables the module and sandbox, policy check in the http trampoline, RAII restore on drop. No C-level gate needed since the HTTP trampoline is pure Rust.

**Tech Stack:** Rust, tein's existing sandbox infrastructure.

**Spec:** `docs/specs/2026-03-16-http-policy-design.md`

---

### Task 1: HttpPolicy struct and thread-local

**Files:**
- Modify: `tein/src/sandbox.rs:66-116` (add `HttpPolicy` after `FsPolicy`, add `HTTP_POLICY` thread-local after `FS_POLICY`)

- [ ] **Step 1: Write failing test**

Add test in `tein/src/sandbox.rs` (or a new `#[cfg(test)] mod tests` at the bottom if one doesn't exist — check first):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_policy_check_url_allowed() {
        let policy = HttpPolicy::new(vec![
            "https://api.example.com/v1/".to_string(),
            "https://cdn.example.com/".to_string(),
        ]);
        assert!(policy.check_url("https://api.example.com/v1/users"));
        assert!(policy.check_url("https://cdn.example.com/image.png"));
    }

    #[test]
    fn http_policy_check_url_blocked() {
        let policy = HttpPolicy::new(vec!["https://api.example.com/v1/".to_string()]);
        assert!(!policy.check_url("https://evil.com/exfil"));
        assert!(!policy.check_url("https://api.example.com/v2/users"));
    }

    #[test]
    fn http_policy_empty_prefixes_blocks_all() {
        let policy = HttpPolicy::new(vec![]);
        assert!(!policy.check_url("https://anything.com/"));
    }

    #[test]
    fn http_policy_no_trailing_slash_prefix_extension() {
        // documents that without trailing slash, prefix extends to unintended URLs
        let policy = HttpPolicy::new(vec!["https://api.example.com/v1".to_string()]);
        assert!(policy.check_url("https://api.example.com/v1-evil/exfil"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p tein --lib sandbox::tests -- --nocapture`
Expected: FAIL — `HttpPolicy` not defined yet.

- [ ] **Step 3: Implement HttpPolicy struct and thread-local**

Add after `FsPolicy` (after line 111 of `sandbox.rs`), before the existing `FS_POLICY` thread-local:

```rust
/// HTTP URL access policy for sandboxed contexts.
///
/// Controls which URLs scheme code can access via `(tein http)`.
/// Uses prefix matching against the raw URL string.
///
/// **Important**: use trailing slashes on path prefixes to avoid
/// prefix-extension attacks — `"https://api.example.com/v1/"` is safe,
/// but `"https://api.example.com/v1"` also matches
/// `"https://api.example.com/v1-evil/exfil"`.
#[derive(Clone)]
pub(crate) struct HttpPolicy {
    /// allowed URL prefixes
    pub url_prefixes: Vec<String>,
}

impl HttpPolicy {
    /// Create a new HTTP policy with the given URL prefixes.
    pub fn new(prefixes: Vec<String>) -> Self {
        Self { url_prefixes: prefixes }
    }

    /// Check if a URL is allowed by this policy.
    ///
    /// Returns true if `url` starts with any allowed prefix.
    pub fn check_url(&self, url: &str) -> bool {
        self.url_prefixes.iter().any(|p| url.starts_with(p))
    }
}
```

Add after the `FS_POLICY` thread-local (after line 116):

```rust
thread_local! {
    /// Active HTTP URL policy for the current context (set during build, cleared on drop).
    pub(crate) static HTTP_POLICY: RefCell<Option<HttpPolicy>> = const { RefCell::new(None) };
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p tein --lib sandbox::tests -- --nocapture`
Expected: all 4 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add tein/src/sandbox.rs
git commit -m "feat: add HttpPolicy struct and HTTP_POLICY thread-local

URL-prefix allowlist for (tein http) in sandboxed contexts.
Mirrors FsPolicy pattern."
```

---

### Task 2: Builder method, build() wiring, and check_http_access

**Files:**
- Modify: `tein/src/context.rs:1904-1925` (add `http_prefixes` field to `ContextBuilder`)
- Modify: `tein/src/context.rs:2020-2048` (add `http_allow` builder method after `file_write`)
- Modify: `tein/src/context.rs:1097-1123` (add `check_http_access` after `check_fs_access`)
- Modify: `tein/src/context.rs:2348-2356` (save `prev_http_policy` alongside other prev_ saves)
- Modify: `tein/src/context.rs:2517-2528` (set `HTTP_POLICY` thread-local alongside `FS_POLICY`)
- Modify: `tein/src/context.rs:2530-2546` (add `prev_http_policy` field to `Context` struct initializer)
- Modify: `tein/src/context.rs:2669-2700` (add `prev_http_policy` field to `Context` struct definition)
- Modify: `tein/src/context.rs:2751-2764` (add `http_prefixes: None` to `Context::builder()`)
- Modify: `tein/src/context.rs:4218-4221` (restore `HTTP_POLICY` in `drop()` alongside `FS_POLICY`)

**cfg gating note**: `http_prefixes` (on `ContextBuilder`) and `prev_http_policy` (on `Context`)
are **unconditional** fields — no `#[cfg(feature = "http")]`. This avoids cfg complexity in struct
literals and `drop()`. The cost is one `Option<Vec<String>>` and one `Option<HttpPolicy>` (both `None`)
when http is disabled. Only the `http_allow` builder method itself is `#[cfg(feature = "http")]`.

- [ ] **Step 1: Write failing test**

Add test in `tein/src/context.rs` in the existing http test section (search for `#[cfg(feature = "http")]` test blocks):

```rust
#[cfg(feature = "http")]
#[test]
fn test_http_allow_makes_module_importable() {
    // without http_allow, sandboxed context cannot import (tein http)
    let ctx_blocked = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .build()
        .expect("build");
    let err = ctx_blocked.evaluate("(import (tein http))").unwrap_err();
    assert!(matches!(err, Error::SandboxViolation(_)));

    // with http_allow, sandboxed context CAN import (tein http)
    let ctx_allowed = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .http_allow(&["https://example.com/"])
        .build()
        .expect("build");
    // should not error — module is importable
    ctx_allowed
        .evaluate("(import (tein http)) #t")
        .expect("import should succeed");
}

#[cfg(feature = "http")]
#[test]
fn test_http_allow_auto_activates_sandbox() {
    // http_allow without explicit sandboxed() should auto-activate sandbox
    let ctx = Context::builder()
        .standard_env()
        .http_allow(&["https://example.com/"])
        .build()
        .expect("build");
    // verify sandboxed by checking that unrestricted modules are blocked
    let err = ctx.evaluate("(import (scheme regex))").unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_)),
        "expected sandbox to be active, got: {:?}",
        err
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p tein --features http -- test_http_allow --nocapture`
Expected: FAIL — `http_allow` method does not exist.

- [ ] **Step 3: Add `http_prefixes` field to `ContextBuilder`**

In the `ContextBuilder` struct (line 1904), add after `file_write_prefixes` (line 1910):

```rust
    /// URL prefixes allowed for `(tein http)` in sandboxed contexts.
    /// when set, auto-enables sandboxing and adds `tein/http` to the allowlist.
    http_prefixes: Option<Vec<String>>,
```

In `Context::builder()` (line 2751), add to the `ContextBuilder { ... }` struct literal (after `file_write_prefixes: None,` at line 2758):

```rust
            http_prefixes: None,
```

- [ ] **Step 4: Add `http_allow` builder method**

After the `file_write` method (after line 2048), add:

```rust
    /// Allow HTTP requests to URLs matching the given prefixes.
    ///
    /// Enables `(tein http)` in sandboxed contexts with a URL-prefix allowlist.
    /// Requests to URLs not matching any prefix are blocked with a scheme exception.
    ///
    /// Auto-activates `sandboxed(Modules::Safe)` when called without an explicit
    /// `sandboxed()` call. Automatically adds `tein/http` to the module allowlist.
    ///
    /// **Important**: use trailing slashes on path prefixes to avoid prefix-extension
    /// attacks — `"https://api.example.com/v1/"` is safe, but without a trailing
    /// slash, `"https://api.example.com/v1"` also matches
    /// `"https://api.example.com/v1-evil/"`.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, sandbox::Modules};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::builder()
    ///     .standard_env()
    ///     .sandboxed(Modules::Safe)
    ///     .http_allow(&["https://api.example.com/v1/"])
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "http")]
    pub fn http_allow(mut self, prefixes: &[&str]) -> Self {
        let list = self.http_prefixes.get_or_insert_with(Vec::new);
        for p in prefixes {
            list.push(p.to_string());
        }
        if self.sandbox_modules.is_none() {
            self.sandbox_modules = Some(crate::sandbox::Modules::Safe);
        }
        self.allow_module("tein/http")
    }
```

- [ ] **Step 5: Add `prev_http_policy` to `Context` struct**

In the `Context` struct definition (line 2669), add after `prev_fs_policy` (line 2677):

```rust
    /// previous HTTP_POLICY value, restored on drop
    prev_http_policy: Option<crate::sandbox::HttpPolicy>,
```

- [ ] **Step 6: Save prev HTTP_POLICY in `build()`**

At line 2352 (where `prev_fs_policy` is saved), add:

```rust
            let prev_http_policy = crate::sandbox::HTTP_POLICY.with(|cell| cell.borrow().clone());
```

- [ ] **Step 7: Set HTTP_POLICY thread-local in `build()`**

After the `FS_POLICY` wiring block (after line 2528), add:

```rust
            // set HttpPolicy if http_allow() was configured.
            // same pattern as FS_POLICY: http_allow() auto-activates sandboxing,
            // so HTTP_POLICY is always paired with IS_SANDBOXED=true in practice.
            #[cfg(feature = "http")]
            {
                if let Some(prefixes) = self.http_prefixes.take() {
                    crate::sandbox::HTTP_POLICY.with(|cell| {
                        *cell.borrow_mut() = Some(crate::sandbox::HttpPolicy::new(prefixes));
                    });
                }
            }
```

- [ ] **Step 8: Wire `prev_http_policy` into `Context` struct initializer**

In the `Context { ... }` struct literal (around line 2530), add the field:

```rust
                prev_http_policy,
```

- [ ] **Step 9: Add `check_http_access` function**

After `check_fs_access` (line 1123 in `context.rs`), add:

```rust
/// Check if an HTTP request to `url` is allowed by the active HTTP policy.
///
/// - unsandboxed (`IS_SANDBOXED=false`): allows unconditionally
/// - sandboxed + `Some(policy)`: prefix match
/// - sandboxed + `None`: deny (defense-in-depth, unreachable via public API)
pub(crate) fn check_http_access(url: &str) -> bool {
    let sandboxed = IS_SANDBOXED.with(|c| c.get());
    if !sandboxed {
        return true;
    }
    crate::sandbox::HTTP_POLICY.with(|cell| {
        let policy = cell.borrow();
        match &*policy {
            Some(p) => p.check_url(url),
            None => false, // sandboxed + no policy = deny
        }
    })
}
```

This follows the `check_fs_access` pattern: lives in `context.rs` where `IS_SANDBOXED` is a private
thread-local, and is `pub(crate)` so `http.rs` can call it.

- [ ] **Step 10: Restore HTTP_POLICY in `drop()`**

In `impl Drop for Context` (after the `FS_POLICY` restore at line 4221), add:

```rust
        // restore previous HTTP_POLICY
        crate::sandbox::HTTP_POLICY.with(|cell| {
            *cell.borrow_mut() = std::mem::take(&mut self.prev_http_policy);
        });
```

- [ ] **Step 11: Run tests to verify they pass**

Run: `cargo test -p tein --features http -- test_http_allow --nocapture`
Expected: both tests PASS.

- [ ] **Step 12: Run full test suite**

Run: `just test`
Expected: all existing tests still pass.

- [ ] **Step 13: Commit**

```bash
git add tein/src/context.rs tein/src/sandbox.rs
git commit -m "feat: http_allow builder method with sandbox auto-activation

Adds http_prefixes to ContextBuilder, auto-enables sandbox and
tein/http module. RAII save/restore of HTTP_POLICY thread-local."
```

---

### Task 3: Trampoline policy enforcement

**Files:**
- Modify: `tein/src/http.rs:214-293` (add policy check using `check_http_access` from `context.rs`)
- Modify: `tein/src/value.rs:470-473` (add `[sandbox:http]` sentinel detection)

- [ ] **Step 1: Write failing test**

Add in `tein/src/context.rs`:

```rust
#[cfg(feature = "http")]
#[test]
fn test_http_policy_blocks_disallowed_url() {
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .http_allow(&["https://allowed.example.com/"])
        .build()
        .expect("build");
    let err = ctx
        .evaluate(
            r#"(import (tein http)) (http-get "https://blocked.example.com/secret" '())"#,
        )
        .unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_)),
        "expected SandboxViolation, got: {:?}",
        err
    );
    let msg = format!("{}", err);
    assert!(
        msg.contains("http request blocked"),
        "expected 'http request blocked', got: {}",
        msg
    );
}

#[cfg(feature = "http")]
#[test]
fn test_http_policy_allows_matching_url() {
    // this test actually makes a network request — use a URL that will fail
    // at the transport level (connection refused) AFTER passing the policy check.
    // the error should be EvalError (transport), NOT SandboxViolation (policy).
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .http_allow(&["http://127.0.0.1:1/"])
        .build()
        .expect("build");
    let err = ctx
        .evaluate(r#"(import (tein http)) (http-get "http://127.0.0.1:1/test" '())"#)
        .unwrap_err();
    // should be a transport error, NOT a sandbox violation
    assert!(
        matches!(err, Error::EvalError(_)),
        "expected EvalError (transport), got: {:?}",
        err
    );
}

#[cfg(feature = "http")]
#[test]
fn test_http_unsandboxed_ignores_policy() {
    // unsandboxed context — http requests should work without any policy.
    // use connection-refused to verify the request actually reaches ureq.
    let ctx = Context::builder()
        .standard_env()
        .build()
        .expect("build");
    let err = ctx
        .evaluate(r#"(import (tein http)) (http-get "http://127.0.0.1:1/test" '())"#)
        .unwrap_err();
    // should be transport error, not sandbox violation
    assert!(
        matches!(err, Error::EvalError(_)),
        "expected EvalError (transport), got: {:?}",
        err
    );
}

#[cfg(feature = "http")]
#[test]
fn test_http_policy_empty_blocks_all() {
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .http_allow(&[])
        .build()
        .expect("build");
    let err = ctx
        .evaluate(
            r#"(import (tein http)) (http-get "http://127.0.0.1:1/test" '())"#,
        )
        .unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_)),
        "expected SandboxViolation, got: {:?}",
        err
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p tein --features http -- test_http_policy --nocapture`
Expected: FAIL — policy check not implemented yet; blocked URL will get a transport error instead of SandboxViolation.

- [ ] **Step 3: Add policy check to http_request_trampoline**

In `http_request_trampoline` (`tein/src/http.rs`), after the URL is extracted (after line 239
where `let url = ffi::sexp_to_rust_string(url_sexp);`), add the policy check.
Uses `check_http_access` from `context.rs` (added in Task 2 Step 9) — this function has access
to the private `IS_SANDBOXED` thread-local, following the same pattern as `check_fs_access`.

```rust
        // check HTTP URL policy before making the request
        if !crate::context::check_http_access(&url) {
            let msg = format!(
                "[sandbox:http] http request blocked: URL not in allowlist: {}",
                url
            );
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }
```

- [ ] **Step 4: Add `[sandbox:http]` sentinel detection in value.rs**

In `tein/src/value.rs`, after the `[sandbox:file]` sentinel block (after line 473), add:

```rust
            // sentinel: HTTP policy denial
            if let Some(rest) = message.strip_prefix("[sandbox:http] ") {
                return Error::SandboxViolation(rest.to_string());
            }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p tein --features http -- test_http_policy --nocapture`
Expected: all 4 tests PASS.

Run: `cargo test -p tein --features http -- test_http_unsandboxed --nocapture`
Expected: PASS.

- [ ] **Step 6: Run full test suite**

Run: `just test`
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add tein/src/http.rs tein/src/value.rs tein/src/context.rs
git commit -m "feat: enforce HttpPolicy in http_request_trampoline

Checks URL against policy before making request. Uses [sandbox:http]
sentinel for SandboxViolation detection. Unsandboxed contexts unaffected."
```

---

### Task 4: RAII restoration test

**Files:**
- Modify: `tein/src/context.rs` (add test)

- [ ] **Step 1: Write RAII test**

```rust
#[cfg(feature = "http")]
#[test]
fn test_http_policy_raii_restoration() {
    // verify that HTTP_POLICY is restored after context is dropped,
    // so sequential contexts on the same thread don't leak state.

    // first context: set http policy
    {
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .http_allow(&["https://example.com/"])
            .build()
            .expect("build first");
        // verify policy is active
        ctx.evaluate("(import (tein http)) #t")
            .expect("import should work");
    } // ctx dropped here — HTTP_POLICY should be restored to None

    // second context: no http policy — module should not be importable
    {
        let ctx2 = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .build()
            .expect("build second");
        let err = ctx2.evaluate("(import (tein http))").unwrap_err();
        assert!(
            matches!(err, Error::SandboxViolation(_)),
            "expected SandboxViolation after RAII restore, got: {:?}",
            err
        );
    }
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p tein --features http -- test_http_policy_raii --nocapture`
Expected: PASS (should already work given Task 2 wiring).

- [ ] **Step 3: Commit**

```bash
git add tein/src/context.rs
git commit -m "test: verify HTTP_POLICY RAII restoration across contexts"
```

---

### Task 5: Lint and documentation

**Files:**
- Modify: `tein/AGENTS.md` (add HTTP policy flow section)
- Modify: `docs/sandboxing.md` (add HTTP policy section)
- Modify: `docs/reference.md` (update `(tein http)` entry)

- [ ] **Step 1: Run lint**

Run: `just lint`
Expected: clean. Fix any issues before proceeding.

- [ ] **Step 2: Update AGENTS.md**

After the **IO policy flow** section (around line 93 in AGENTS.md), add:

```markdown
**HTTP policy flow**: ContextBuilder with `http_allow(prefixes)` → auto-activates sandboxed if needed → `allow_module("tein/http")` → during build, sets `HTTP_POLICY` thread-local → `http_request_trampoline` checks `IS_SANDBOXED` + `HTTP_POLICY` before ureq call → allowed or exception `"http request blocked: URL not in allowlist: {url}"`. unsandboxed contexts skip the check. `HTTP_POLICY` restored to previous value on `Context::drop()`.
```

- [ ] **Step 3: Update docs/sandboxing.md**

After the file IO policy section, add a new `## HTTP policy` section with:
- Heading: `## HTTP policy`
- Intro: "Control which URLs sandboxed scheme code can access via `(tein http)`:"
- Rust code example showing `Context::builder()` with `.http_allow(&["https://api.example.com/v1/"])`
- Three-item list of what `.http_allow()` does: adds module to allowlist, sets URL-prefix policy, auto-activates sandbox
- Error message example: `"http request blocked: URL not in allowlist: https://evil.com/exfil"`
- Trailing slash warning: without trailing slash, prefix matches unintended URLs
- Empty allowlist note: `http_allow(&[])` enables import but blocks all requests

- [ ] **Step 4: Update docs/reference.md**

Update the `tein/http` row to note the policy:

```
| `tein/http` | ✗ | ... — sandboxed via `http_allow()` URL-prefix policy |
```

- [ ] **Step 5: Commit**

```bash
git add tein/AGENTS.md docs/sandboxing.md docs/reference.md
git commit -m "docs: document HttpPolicy flow in AGENTS.md, sandboxing guide, and reference"
```

- [ ] **Step 6: Final lint check**

Run: `just lint`
Expected: clean.

- [ ] **Step 7: Collect AGENTS.md notes**

Review all tasks for any gotchas that should be added to AGENTS.md (per implementation workflow). Candidates:
- `[sandbox:http]` sentinel prefix in value.rs
- `HttpPolicy` is `pub(crate)`, not public API (same as `FsPolicy`)
- `http_allow` is `#[cfg(feature = "http")]` — only available when the feature is enabled
