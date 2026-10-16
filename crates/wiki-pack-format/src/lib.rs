//! Portable versioned .wpack format and verification.
//!
//! Provisional manifest, local object-integrity, and pack-trust primitives.
//! The complete signed publisher format and installation acceptance are not
//! yet implemented.

pub mod container;
pub mod export;
pub mod manifest;
pub mod media_coverage;
pub mod objects;
pub mod stage;
pub mod trust;

/// SHA-256 of a domain-separated canonical byte transcript. This is a content
/// digest only, not a publisher signature or proof of source authenticity.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// Incremental canonical transcript digest. Callers can hash large graphs
/// without retaining a second complete serialized copy in memory.
pub struct Sha256Transcript(sha2::Sha256);

impl Default for Sha256Transcript {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256Transcript {
    pub fn new() -> Self {
        use sha2::Digest;
        Self(sha2::Sha256::new())
    }

    pub fn update(&mut self, bytes: &[u8]) {
        use sha2::Digest;
        self.0.update(bytes);
    }

    pub fn finish_hex(self) -> String {
        use sha2::Digest;
        format!("{:x}", self.0.finalize())
    }
}
