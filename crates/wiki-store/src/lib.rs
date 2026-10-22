//! Immutable snapshots, catalogs and user-state boundaries.
//!
//! This crate currently only establishes a dependency boundary. It exposes no
//! production API until its canonical TODO tasks are implemented.

pub mod activation;
pub mod record_codec;
pub mod shard_io;

pub mod catalog;
pub mod media_objects;
pub mod media_ownership;

pub mod media_install;
pub mod media_inventory;
pub mod media_manifest_install;
pub mod offline_reader;
pub mod online_cache;
pub mod search_index;
pub mod user_state;

pub mod revision_lookup;
