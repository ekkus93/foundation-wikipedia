//! Portable versioned .wpack format and verification.
//!
//! Provisional manifest, local object-integrity, and pack-trust primitives.
//! The complete signed publisher format and installation acceptance are not
//! yet implemented.

pub mod container;
pub mod manifest;
pub mod objects;
pub mod stage;
pub mod trust;
