# `(tein crypto)` Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `(tein crypto)` module with SHA-256, SHA-512, BLAKE3 hashing and CSPRNG functions, closes #38.

**Architecture:** Feature-gated `#[tein_module("crypto")]` in `src/crypto.rs`. All-native (VfsSource::Dynamic), no foreign types. Hash functions accept string or bytevector input via shared `extract_input_bytes` helper, return hex string or bytevector. CSPRNG uses `rand 0.9` (already in dep tree via uuid).

**Tech Stack:** `sha2 0.10`, `blake3 1`, `rand 0.9` (existing), `tein-macros`

**Branch:** `just feature crypto-module-2603`

**Design doc:** `docs/plans/2026-03-04-tein-crypto-design.md`

---

### Task 1: Create branch and add dependencies

**Files:**
- Modify: `tein/Cargo.toml`

**Step 1: Create the feature branch**

```bash
just feature crypto-module-2603
```

**Step 2: Add dependencies and feature to `tein/Cargo.toml`**

Add to `[dependencies]` (after the `regex` line):

```toml
sha2 = { version = "0.10", optional = true }
blake3 = { version = "1", optional = true }
rand = { version = "0.9", optional = true }
```

Add to `[features]` (after `regex` feature, before `debug-chibi`):

```toml
## enables `(tein crypto)` module with SHA-256, SHA-512, BLAKE3 hashing and CSPRNG.
## pulls in sha2, blake3, and rand crates.
crypto = ["dep:sha2", "dep:blake3", "dep:rand"]
```

Update `default` feature list:

```toml
default = ["json", "toml", "uuid", "time", "regex", "crypto"]
```

**Step 3: Verify it compiles**

```bash
cargo build -p tein 2>&1 | tail -5
```

Expected: successful build (no new code uses the deps yet).

**Step 4: Commit**

```bash
git add tein/Cargo.toml Cargo.lock
git commit -m "feat(crypto): add sha2, blake3, rand deps + crypto feature gate (#38)"
```

---

### Task 2: Write the crypto module with hash functions

**Files:**
- Create: `tein/src/crypto.rs`

**Step 1: Create `tein/src/crypto.rs`**

```rust
//! `(tein crypto)` — cryptographic hashing and CSPRNG.
//!
//! provides:
//! - `sha256`, `sha256-bytes` — SHA-256 hash (hex string / bytevector)
//! - `sha512`, `sha512-bytes` — SHA-512 hash (hex string / bytevector)
//! - `blake3`, `blake3-bytes` — BLAKE3 hash (hex string / bytevector)
//! - `random-bytes` — CSPRNG bytevector of n bytes
//! - `random-integer` — CSPRNG integer in [0, n)
//! - `random-float` — CSPRNG float in [0.0, 1.0)
//!
//! hash functions accept string (hashes UTF-8 bytes) or bytevector input.

use tein_macros::tein_module;

/// extract raw bytes from a string or bytevector Value.
///
/// strings are encoded as UTF-8 bytes. bytevectors pass through directly.
/// any other type returns an error message.
fn extract_input_bytes(input: &Value) -> Result<Vec<u8>, String> {
    match input {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Bytevector(bv) => Ok(bv.clone()),
        _ => Err("hash input must be a string or bytevector".into()),
    }
}

/// compute a hash digest and return it as a lowercase hex string.
fn hash_hex<D: ::sha2::Digest>(input: &Value) -> Result<String, String> {
    let bytes = extract_input_bytes(input)?;
    let mut hasher = D::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// compute a hash digest and return it as a byte vector.
fn hash_bytes<D: ::sha2::Digest>(input: &Value) -> Result<Vec<u8>, String> {
    let bytes = extract_input_bytes(input)?;
    let mut hasher = D::new();
    hasher.update(&bytes);
    Ok(hasher.finalize().to_vec())
}

#[tein_module("crypto")]
pub(crate) mod crypto_impl {
    /// compute SHA-256 hash, returned as lowercase hex string.
    #[tein_fn(name = "sha256")]
    pub fn sha256_hex(input: Value) -> Result<String, String> {
        super::hash_hex::<::sha2::Sha256>(&input)
    }

    /// compute SHA-256 hash, returned as bytevector (32 bytes).
    #[tein_fn(name = "sha256-bytes")]
    pub fn sha256_bytes(input: Value) -> Result<Vec<u8>, String> {
        super::hash_bytes::<::sha2::Sha256>(&input)
    }

    /// compute SHA-512 hash, returned as lowercase hex string.
    #[tein_fn(name = "sha512")]
    pub fn sha512_hex(input: Value) -> Result<String, String> {
        super::hash_hex::<::sha2::Sha512>(&input)
    }

    /// compute SHA-512 hash, returned as bytevector (64 bytes).
    #[tein_fn(name = "sha512-bytes")]
    pub fn sha512_bytes(input: Value) -> Result<Vec<u8>, String> {
        super::hash_bytes::<::sha2::Sha512>(&input)
    }

    /// compute BLAKE3 hash, returned as lowercase hex string.
    #[tein_fn(name = "blake3")]
    pub fn blake3_hex(input: Value) -> Result<String, String> {
        let bytes = super::extract_input_bytes(&input)?;
        Ok(::blake3::hash(&bytes).to_hex().to_string())
    }

    /// compute BLAKE3 hash, returned as bytevector (32 bytes).
    #[tein_fn(name = "blake3-bytes")]
    pub fn blake3_bytes(input: Value) -> Result<Vec<u8>, String> {
        let bytes = super::extract_input_bytes(&input)?;
        Ok(::blake3::hash(&bytes).as_bytes().to_vec())
    }

    /// generate a bytevector of n cryptographically random bytes.
    #[tein_fn(name = "random-bytes")]
    pub fn random_bytes(n: i64) -> Result<Vec<u8>, String> {
        if n < 0 {
            return Err("random-bytes: n must be non-negative".into());
        }
        let mut buf = vec![0u8; n as usize];
        ::rand::Fill::fill(&mut buf[..], &mut ::rand::rng());
        Ok(buf)
    }

    /// generate a random integer in [0, n) using CSPRNG.
    ///
    /// follows SRFI-27 convention: exclusive upper bound, zero-based.
    #[tein_fn(name = "random-integer")]
    pub fn random_integer(n: i64) -> Result<i64, String> {
        if n <= 0 {
            return Err("random-integer: n must be positive".into());
        }
        use ::rand::Rng;
        Ok(::rand::rng().random_range(0..n))
    }

    /// generate a random float in [0.0, 1.0) using CSPRNG.
    #[tein_fn(name = "random-float")]
    pub fn random_float() -> f64 {
        use ::rand::Rng;
        ::rand::rng().random::<f64>()
    }
}
```

**Note on blake3:** blake3 uses its own `Hasher` type, not the `sha2::Digest` trait, so blake3 functions call `::blake3::hash()` directly instead of using the generic `hash_hex`/`hash_bytes` helpers. this is the correct approach — blake3 intentionally does not implement the `digest` crate traits.

**Step 2: Don't commit yet** — module won't compile until it's wired up in the next tasks.

---

### Task 3: Wire up the module (lib.rs, context.rs, vfs_registry.rs)

**Files:**
- Modify: `tein/src/lib.rs:81` — add `#[cfg(feature = "crypto")] mod crypto;`
- Modify: `tein/src/context.rs:1937` — add registration block
- Modify: `tein/src/vfs_registry.rs:154` — add VfsEntry

**Step 1: Add module declaration in `src/lib.rs`**

After the `#[cfg(feature = "uuid")] mod uuid;` line (line 82), add:

```rust
#[cfg(feature = "crypto")]
mod crypto;
```

**Step 2: Add registration in `src/context.rs`**

After the `#[cfg(feature = "time")]` registration block (after line 1937), add:

```rust
#[cfg(feature = "crypto")]
if self.standard_env {
    crate::crypto::crypto_impl::register_module_crypto(&context)?;
}
```

**Step 3: Add VfsEntry in `src/vfs_registry.rs`**

After the `tein/uuid` entry (after line 153), add:

```rust
VfsEntry {
    path: "tein/crypto",
    deps: &[],
    files: &[],
    clib: None,
    default_safe: true,
    source: VfsSource::Dynamic,
    feature: Some("crypto"),
    shadow_sld: None,
},
```

**Step 4: Verify compilation**

```bash
cargo build -p tein 2>&1 | tail -5
```

Expected: successful build.

**Step 5: Commit**

```bash
git add tein/src/crypto.rs tein/src/lib.rs tein/src/context.rs tein/src/vfs_registry.rs
git commit -m "feat(crypto): add (tein crypto) module with hashing + CSPRNG (#38)"
```

---

### Task 4: Wire up build.rs + sandbox.rs feature gates

**Files:**
- Modify: `tein/build.rs:291` — add `feature_enabled` arm
- Modify: `tein/build.rs:314` — add `DYNAMIC_MODULE_EXPORTS` entry
- Modify: `tein/src/sandbox.rs:394` — add `feature_enabled` arm

**Step 1: Add `feature_enabled` arm in `build.rs`**

In `feature_enabled()` (line 284), add before the `Some(f)` catch-all:

```rust
Some("crypto") => cfg!(feature = "crypto"),
```

**Step 2: Add `DYNAMIC_MODULE_EXPORTS` entry in `build.rs`**

After the `tein/safe-regexp` entry in `DYNAMIC_MODULE_EXPORTS` (after line 343), add:

```rust
// src/crypto.rs — #[tein_module("crypto")] feature=crypto
(
    "tein/crypto",
    &[
        "sha256",
        "sha256-bytes",
        "sha512",
        "sha512-bytes",
        "blake3",
        "blake3-bytes",
        "random-bytes",
        "random-integer",
        "random-float",
    ],
),
```

**Step 3: Add `feature_enabled` arm in `src/sandbox.rs`**

In `feature_enabled()` (line 387), add before the `Some(f)` catch-all:

```rust
Some("crypto") => cfg!(feature = "crypto"),
```

**Step 4: Verify compilation**

```bash
cargo build -p tein 2>&1 | tail -5
```

Expected: successful build.

**Step 5: Commit**

```bash
git add tein/build.rs tein/src/sandbox.rs
git commit -m "feat(crypto): wire feature gates in build.rs + sandbox.rs (#38)"
```

---

### Task 5: Write tests — hash functions

**Files:**
- Modify: `tein/src/context.rs` (add test block at end of test module)

Tests go in `context.rs` alongside existing module tests (search for `mod tests` or `#[cfg(test)]`).

**Step 1: Write hash tests**

Add in the `#[cfg(test)]` module in `context.rs`:

```rust
#[cfg(feature = "crypto")]
mod crypto_tests {
    use super::*;

    // NIST test vectors: SHA-256("") and SHA-512("")
    const SHA256_EMPTY: &str =
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const SHA512_EMPTY: &str =
        "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
         47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e";
    // BLAKE3("") from reference implementation
    const BLAKE3_EMPTY: &str =
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";

    // SHA-256("hello")
    const SHA256_HELLO: &str =
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    #[test]
    fn test_sha256_string() {
        let ctx = Context::builder().standard_env().build().unwrap();
        let result = ctx.evaluate("(import (tein crypto)) (sha256 \"\")").unwrap();
        assert_eq!(result.to_string(), SHA256_EMPTY);
    }

    #[test]
    fn test_sha256_hello() {
        let ctx = Context::builder().standard_env().build().unwrap();
        let result = ctx.evaluate("(import (tein crypto)) (sha256 \"hello\")").unwrap();
        assert_eq!(result.to_string(), SHA256_HELLO);
    }

    #[test]
    fn test_sha256_bytes_length() {
        let ctx = Context::builder().standard_env().build().unwrap();
        let result = ctx.evaluate("(import (tein crypto)) (bytevector-length (sha256-bytes \"\"))").unwrap();
        assert_eq!(result, Value::Integer(32));
    }

    #[test]
    fn test_sha512_string() {
        let ctx = Context::builder().standard_env().build().unwrap();
        let result = ctx.evaluate("(import (tein crypto)) (sha512 \"\")").unwrap();
        assert_eq!(result.to_string(), SHA512_EMPTY);
    }

    #[test]
    fn test_sha512_bytes_length() {
        let ctx = Context::builder().standard_env().build().unwrap();
        let result = ctx.evaluate("(import (tein crypto)) (bytevector-length (sha512-bytes \"\"))").unwrap();
        assert_eq!(result, Value::Integer(64));
    }

    #[test]
    fn test_blake3_string() {
        let ctx = Context::builder().standard_env().build().unwrap();
        let result = ctx.evaluate("(import (tein crypto)) (blake3 \"\")").unwrap();
        assert_eq!(result.to_string(), BLAKE3_EMPTY);
    }

    #[test]
    fn test_blake3_bytes_length() {
        let ctx = Context::builder().standard_env().build().unwrap();
        let result = ctx.evaluate("(import (tein crypto)) (bytevector-length (blake3-bytes \"\"))").unwrap();
        assert_eq!(result, Value::Integer(32));
    }

    #[test]
    fn test_hash_string_bytevector_equivalence() {
        let ctx = Context::builder().standard_env().build().unwrap();
        // "hello" as string vs as bytevector #u8(104 101 108 108 111)
        let hex = ctx.evaluate("(import (tein crypto)) (sha256 \"hello\")").unwrap();
        let bv_hex = ctx.evaluate("(sha256 #u8(104 101 108 108 111))").unwrap();
        assert_eq!(hex, bv_hex);
    }

    #[test]
    fn test_hash_invalid_input() {
        let ctx = Context::builder().standard_env().build().unwrap();
        ctx.evaluate("(import (tein crypto))").unwrap();
        let result = ctx.evaluate("(sha256 42)").unwrap();
        // Result::Err returns a scheme string (see AGENTS.md critical gotchas)
        assert!(matches!(result, Value::String(s) if s.contains("string or bytevector")));
    }
}
```

**Step 2: Run tests**

```bash
cargo test -p tein crypto_tests -- --nocapture 2>&1 | tail -20
```

Expected: all tests pass.

**Step 3: Commit**

```bash
git add tein/src/context.rs
git commit -m "test(crypto): hash function tests with NIST/reference vectors (#38)"
```

---

### Task 6: Write tests — CSPRNG functions

**Files:**
- Modify: `tein/src/context.rs` (add to `crypto_tests` module)

**Step 1: Add CSPRNG tests to the `crypto_tests` module**

```rust
    #[test]
    fn test_random_bytes_length() {
        let ctx = Context::builder().standard_env().build().unwrap();
        let result = ctx.evaluate("(import (tein crypto)) (bytevector-length (random-bytes 16))").unwrap();
        assert_eq!(result, Value::Integer(16));
    }

    #[test]
    fn test_random_bytes_zero() {
        let ctx = Context::builder().standard_env().build().unwrap();
        let result = ctx.evaluate("(import (tein crypto)) (bytevector-length (random-bytes 0))").unwrap();
        assert_eq!(result, Value::Integer(0));
    }

    #[test]
    fn test_random_bytes_negative() {
        let ctx = Context::builder().standard_env().build().unwrap();
        ctx.evaluate("(import (tein crypto))").unwrap();
        let result = ctx.evaluate("(random-bytes -1)").unwrap();
        assert!(matches!(result, Value::String(s) if s.contains("non-negative")));
    }

    #[test]
    fn test_random_integer_bounds() {
        let ctx = Context::builder().standard_env().build().unwrap();
        ctx.evaluate("(import (tein crypto))").unwrap();
        // run 100 iterations, all results must be in [0, 10)
        let result = ctx.evaluate(
            "(let loop ((i 0) (ok #t))
               (if (= i 100) ok
                 (let ((r (random-integer 10)))
                   (loop (+ i 1) (and ok (>= r 0) (< r 10))))))"
        ).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_random_integer_invalid() {
        let ctx = Context::builder().standard_env().build().unwrap();
        ctx.evaluate("(import (tein crypto))").unwrap();
        let result = ctx.evaluate("(random-integer 0)").unwrap();
        assert!(matches!(result, Value::String(s) if s.contains("positive")));
    }

    #[test]
    fn test_random_float_bounds() {
        let ctx = Context::builder().standard_env().build().unwrap();
        ctx.evaluate("(import (tein crypto))").unwrap();
        let result = ctx.evaluate(
            "(let loop ((i 0) (ok #t))
               (if (= i 100) ok
                 (let ((r (random-float)))
                   (loop (+ i 1) (and ok (>= r 0.0) (< r 1.0))))))"
        ).unwrap();
        assert_eq!(result, Value::Bool(true));
    }
```

**Step 2: Run all crypto tests**

```bash
cargo test -p tein crypto_tests -- --nocapture 2>&1 | tail -20
```

Expected: all tests pass.

**Step 3: Commit**

```bash
git add tein/src/context.rs
git commit -m "test(crypto): CSPRNG tests for random-bytes, random-integer, random-float (#38)"
```

---

### Task 7: Write tests — sandbox access

**Files:**
- Modify: `tein/src/context.rs` (add to `crypto_tests` module)

**Step 1: Add sandbox test**

```rust
    #[test]
    fn test_crypto_sandbox_access() {
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(crate::sandbox::Modules::Safe)
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (tein crypto)) (sha256 \"test\")").unwrap();
        assert!(matches!(result, Value::String(_)));
    }
```

**Step 2: Run it**

```bash
cargo test -p tein test_crypto_sandbox -- --nocapture 2>&1 | tail -10
```

Expected: PASS.

**Step 3: Commit**

```bash
git add tein/src/context.rs
git commit -m "test(crypto): verify sandbox access for (tein crypto) (#38)"
```

---

### Task 8: Add scheme integration test

**Files:**
- Create: `tein/tests/scheme/crypto.scm`

**Step 1: Create scheme test file**

```scheme
;; integration tests for (tein crypto)

(import (scheme base) (tein crypto))

;; sha256 of empty string — NIST test vector
(assert-equal "sha256 empty"
  (sha256 "")
  "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")

;; sha512 of empty string
(assert-equal "sha512 empty"
  (sha512 "")
  "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e")

;; blake3 of empty string
(assert-equal "blake3 empty"
  (blake3 "")
  "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262")

;; bytevector output has correct length
(assert-equal "sha256-bytes length" (bytevector-length (sha256-bytes "x")) 32)
(assert-equal "sha512-bytes length" (bytevector-length (sha512-bytes "x")) 64)
(assert-equal "blake3-bytes length" (bytevector-length (blake3-bytes "x")) 32)

;; random-bytes returns correct length
(assert-equal "random-bytes 0" (bytevector-length (random-bytes 0)) 0)
(assert-equal "random-bytes 32" (bytevector-length (random-bytes 32)) 32)

;; random-integer in bounds
(let loop ((i 0))
  (when (< i 100)
    (let ((r (random-integer 10)))
      (assert-true "random-integer >= 0" (>= r 0))
      (assert-true "random-integer < n" (< r 10))
      (loop (+ i 1)))))

;; random-float in bounds
(let loop ((i 0))
  (when (< i 100)
    (let ((r (random-float)))
      (assert-true "random-float >= 0" (>= r 0.0))
      (assert-true "random-float < 1" (< r 1.0))
      (loop (+ i 1)))))
```

**Step 2: Check how existing scheme tests work** to ensure the test runner picks this up.

The scheme test runner is in `tein/tests/scheme_tests.rs`. Check whether it auto-discovers `.scm` files or requires manual registration. If manual, add an entry for `crypto.scm`.

**Step 3: Run the scheme test**

```bash
cargo test -p tein --test scheme_tests crypto 2>&1 | tail -10
```

Expected: PASS.

**Step 4: Commit**

```bash
git add tein/tests/scheme/crypto.scm
git commit -m "test(crypto): scheme integration tests for (tein crypto) (#38)"
```

---

### Task 9: Lint, docs, and final commit

**Files:**
- Modify: `tein/AGENTS.md` — add crypto gotchas if any arise
- Modify: `docs/plans/2026-03-04-tein-crypto.md` — mark complete

**Step 1: Lint**

```bash
just lint
```

Fix any issues. If clippy flags `3.14` in tests, that's a known false positive (ignore).

**Step 2: Run full test suite**

```bash
just test
```

Expected: all existing tests still pass + new crypto tests pass.

**Step 3: Update AGENTS.md if needed**

Add any gotchas discovered during implementation (e.g. blake3 not implementing `Digest` trait, `rand 0.9` API differences).

**Step 4: Update docs/reference.md**

Add `(tein crypto)` entry to the VFS module list in `docs/reference.md` if it exists.

**Step 5: Commit any remaining changes**

```bash
git add -A
git commit -m "chore(crypto): lint + docs for (tein crypto) (#38), closes #38"
```

**Step 6: Update implementation plan status**

Mark `docs/plans/2026-03-04-tein-crypto.md` task list as complete.

---

### Task 10: Finish branch

Use superpowers:finishing-a-development-branch to decide merge/PR/cleanup.
