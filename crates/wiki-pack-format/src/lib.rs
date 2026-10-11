//! Portable versioned .wpack format and verification.
//!
//! Provisional manifest, local object-integrity, and pack-trust primitives.
//! The complete portable container, signature verification, and installation
//! acceptance are not yet implemented.

pub mod manifest;
pub mod objects;
pub mod trust;
