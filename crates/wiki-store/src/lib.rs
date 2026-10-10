//! Immutable snapshots, catalogs and user-state boundaries.
//!
//! This crate currently only establishes a dependency boundary. It exposes no
//! production API until its canonical TODO tasks are implemented.

pub mod activation;
pub mod record_codec;
pub mod shard_io;

pub mod catalog;
pub mod media_ownership;
