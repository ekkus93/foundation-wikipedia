//! Shared engine for GUI and CLI pack construction.
//!
//! This crate currently only establishes a dependency boundary. It exposes no
//! production API until its canonical TODO tasks are implemented.

pub mod curated;
pub mod graph_digest;
pub mod media;
pub mod media_inventory;
pub mod selection;

/// Shared provisional pack envelope contract for CLI and future desktop UI.
pub use wiki_pack_format;
