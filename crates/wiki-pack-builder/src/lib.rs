//! Shared engine for GUI and CLI pack construction.
//!
//! This crate currently only establishes a dependency boundary. It exposes no
//! production API until its canonical TODO tasks are implemented.

pub mod curated;
pub mod enterprise_graph;
pub mod gif_preflight;
pub mod graph_digest;
pub mod joined_preview;
pub mod joined_related;
pub mod media;
pub mod media_inventory;
pub mod png_crc;
pub mod png_preflight;
pub mod preview;
pub mod related;
pub mod rendered_dependencies;
pub mod rendered_local_urls;
pub mod rendered_objects;
pub mod selection;
pub mod visual_discovery;
pub mod webp_preflight;

/// Shared provisional pack envelope contract for CLI and future desktop UI.
pub use wiki_pack_format;
