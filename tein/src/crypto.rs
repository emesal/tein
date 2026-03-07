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

use crate::Value;
use tein_macros::tein_module;

/// encode a byte slice as a lowercase hex string.
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

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

/// compute a sha2 hash digest and return it as a lowercase hex string.
fn hash_hex<D: ::sha2::Digest>(input: &Value) -> Result<String, String> {
    let bytes = extract_input_bytes(input)?;
    let mut hasher = D::new();
    hasher.update(&bytes);
    Ok(bytes_to_hex(hasher.finalize().as_slice()))
}

/// compute a sha2 hash digest and return it as a bytevector Value.
fn hash_bytevector<D: ::sha2::Digest>(input: &Value) -> Result<Value, String> {
    let bytes = extract_input_bytes(input)?;
    let mut hasher = D::new();
    hasher.update(&bytes);
    Ok(Value::Bytevector(hasher.finalize().to_vec()))
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
    pub fn sha256_bytes(input: Value) -> Result<Value, String> {
        super::hash_bytevector::<::sha2::Sha256>(&input)
    }

    /// compute SHA-512 hash, returned as lowercase hex string.
    #[tein_fn(name = "sha512")]
    pub fn sha512_hex(input: Value) -> Result<String, String> {
        super::hash_hex::<::sha2::Sha512>(&input)
    }

    /// compute SHA-512 hash, returned as bytevector (64 bytes).
    #[tein_fn(name = "sha512-bytes")]
    pub fn sha512_bytes(input: Value) -> Result<Value, String> {
        super::hash_bytevector::<::sha2::Sha512>(&input)
    }

    /// compute BLAKE3 hash, returned as lowercase hex string.
    ///
    /// note: blake3 does not implement the `sha2::Digest` trait; its `Hasher`
    /// type has a distinct API. we call `::blake3::hash()` directly.
    #[tein_fn(name = "blake3")]
    pub fn blake3_hex(input: Value) -> Result<String, String> {
        let bytes = super::extract_input_bytes(&input)?;
        Ok(::blake3::hash(&bytes).to_hex().to_string())
    }

    /// compute BLAKE3 hash, returned as bytevector (32 bytes).
    #[tein_fn(name = "blake3-bytes")]
    pub fn blake3_bytes(input: Value) -> Result<Value, String> {
        let bytes = super::extract_input_bytes(&input)?;
        Ok(Value::Bytevector(
            ::blake3::hash(&bytes).as_bytes().to_vec(),
        ))
    }

    /// generate a bytevector of n cryptographically random bytes.
    #[tein_fn(name = "random-bytes")]
    pub fn random_bytes(n: i64) -> Result<Value, String> {
        if n < 0 {
            return Err("random-bytes: n must be non-negative".into());
        }
        use ::rand::RngCore;
        let mut buf = vec![0u8; n as usize];
        ::rand::rng().fill_bytes(&mut buf);
        Ok(Value::Bytevector(buf))
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
