//! Authoritative Wikimedia snapshot metadata validation.
//! Download transport and checksum verification are separate SRC-001 steps.

pub mod enterprise;
pub mod enterprise_integrity;
pub mod enterprise_ndjson;
pub mod enterprise_pinned;
pub mod enterprise_pinned_deletions;
pub mod enterprise_verified;
pub mod join;
pub mod online_article;
#[cfg(not(target_os = "android"))]
pub mod online_curl;
pub mod online_fetch;
pub mod online_flow;
pub mod online_requests;
pub mod online_search;
pub mod online_selection;
pub mod online_title;
pub mod public_dump;
pub mod public_dump_census_export;
pub mod public_dump_census_gate;
pub mod public_dump_census_ingest;
pub mod public_dump_census_stream;
pub mod public_dump_census_text;
pub mod public_dump_coverage;
pub mod public_dump_ingest;
pub mod public_dump_origin;
pub mod public_dump_page_meta;
pub mod public_dump_pinned;
pub mod public_dump_revision_jsonl;
pub mod public_dump_shards;
pub mod public_dump_stage;
pub mod public_dump_stream;
pub mod public_dump_strict;
pub mod public_dump_verify;

use std::collections::BTreeSet;

/// Provisional resource bounds for one verified source generation.
pub const MAX_SOURCE_FILES: usize = 10_000;
pub const MAX_SOURCE_GENERATION_BYTES: u64 = 1_u64 << 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    pub name: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceGeneration {
    pub project: String,
    pub upstream_id: String,
    pub completed: bool,
    pub files: Vec<SourceFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceError {
    InvalidProject,
    MissingGeneration,
    Incomplete,
    EmptyFileSet,
    InvalidFile(String),
    DuplicateFile(String),
    ResourceBudget,
}

/// Reject names that alias Windows device files even when given an extension.
pub(crate) fn windows_device_name(value: &str) -> bool {
