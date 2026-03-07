# design: `(tein crypto)` — hashing and CSPRNG

**issue**: #38
**date**: 2026-03-04
**status**: approved

## summary

new feature-gated `#[tein_module("crypto")]` providing cryptographic hashing
(sha256, sha512, blake3) and CSPRNG (random bytes, integers, floats). all
functions are pure input→output with no foreign types or side effects beyond
entropy reads.

## API

| binding | signature | returns |
|---|---|---|
| `sha256` | `(sha256 input)` | hex string |
| `sha256-bytes` | `(sha256-bytes input)` | bytevector |
| `sha512` | `(sha512 input)` | hex string |
| `sha512-bytes` | `(sha512-bytes input)` | bytevector |
| `blake3` | `(blake3 input)` | hex string |
| `blake3-bytes` | `(blake3-bytes input)` | bytevector |
| `random-bytes` | `(random-bytes n)` | bytevector of n CSPRNG bytes |
| `random-integer` | `(random-integer n)` | integer in [0, n), CSPRNG |
| `random-float` | `(random-float)` | float in [0.0, 1.0), CSPRNG |

`input` for hash functions: string (hashes UTF-8 bytes) or bytevector.
invalid type → error string.

`random-integer` follows SRFI-27 convention: exclusive upper bound, zero-based.

## architecture

- **single file**: `src/crypto.rs` with `#[tein_module("crypto")]`
- **feature**: `crypto` added to default feature set
- **VFS source**: `Dynamic` (all-native, no scheme code files)
- **sandbox**: `default_safe: true` — pure computation + entropy reads
- **no foreign types** — all functions are stateless

### internal structure

hash functions share an `extract_input_bytes(input: &Value) -> Result<Vec<u8>, String>`
helper to avoid duplicating string-or-bytevector dispatch. each hash algo gets
a hex + bytes variant pair that call through this helper.

### dependencies

```toml
sha2 = { version = "0.10", optional = true }
blake3 = { version = "1", optional = true }
rand = { version = "0.9", optional = true }  # already in tree via uuid
```

single `crypto` feature gate: `crypto = ["dep:sha2", "dep:blake3", "dep:rand"]`

### integration points (7 files)

1. `Cargo.toml` — deps + feature
2. `src/crypto.rs` — new module
3. `src/lib.rs` — `#[cfg(feature = "crypto")] mod crypto;`
4. `src/context.rs` — conditional `register_module_crypto()`
5. `src/vfs_registry.rs` — `VfsEntry` with `Dynamic`, `default_safe: true`
6. `build.rs` — `DYNAMIC_MODULE_EXPORTS` + `feature_enabled`
7. `src/sandbox.rs` — `feature_enabled` mirror

## testing

- hash known inputs against NIST/reference test vectors
- bytevector output has correct length (32 for sha256, 64 for sha512, 32 for blake3)
- string vs bytevector input equivalence (same bytes → same digest)
- `random-bytes` returns correct length bytevector
- `random-integer` bounds: 0 ≤ result < n over many iterations
- `random-float` bounds: 0.0 ≤ result < 1.0
- sandbox access works (import in sandboxed context)
- invalid input type → error

## not in scope

- HMAC / key derivation (future issue if needed)
- streaming/incremental hashing
- cdylib extension (overhead not justified for this module size)
