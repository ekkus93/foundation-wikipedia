//! Shared engine for GUI and CLI pack construction.
//!
//! This crate currently only establishes a dependency boundary. It exposes no
//! production API until its canonical TODO tasks are implemented.

pub mod curated;
pub mod enterprise_graph;
pub mod graph_digest;
pub mod joined_preview;
pub mod media;
pub mod media_inventory;
pub mod preview;
pub mod related;
pub mod selection;
pub mod visual_discovery;

/// Shared provisional pack envelope contract for CLI and future desktop UI.
pub use wiki_pack_format;
