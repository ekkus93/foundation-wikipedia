//! Portable versioned .wpack format and verification.
//!
//! Provisional manifest, local object-integrity, and pack-trust primitives.
//! The complete signed publisher format and installation acceptance are not
//! yet implemented.

pub mod container;
pub mod export;
pub mod manifest;
pub mod objects;
pub mod stage;
pub mod trust;

/// SHA-256 of a domain-separated canonical byte transcript. This is a content
/// digest only, not a publisher signature or proof of source authenticity.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
