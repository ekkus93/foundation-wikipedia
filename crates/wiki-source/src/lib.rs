//! Authoritative Wikimedia snapshot metadata validation.
//! Download transport and checksum verification are separate SRC-001 steps.

pub mod enterprise;
pub mod enterprise_integrity;
pub mod enterprise_ndjson;
pub mod enterprise_verified;
pub mod join;
pub mod online_article;
pub mod online_fetch;
pub mod online_flow;
pub mod online_requests;
pub mod online_search;
pub mod online_selection;
pub mod online_title;
pub mod public_dump;
pub mod public_dump_pinned;
pub mod public_dump_stage;
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
    let upper = value.to_ascii_uppercase();
    let base = upper.split('.').next().unwrap_or("");
    let numbered = base.len() == 4
        && (base.starts_with("COM") || base.starts_with("LPT"))
        && (b'1'..=b'9').contains(&base.as_bytes()[3]);
    ["CON", "PRN", "AUX", "NUL"].contains(&base) || numbered
}

impl SourceGeneration {
    /// Validate publication metadata before trusting any mirror or cache.
    pub fn validate(&self) -> Result<(), SourceError> {
        if self.project.is_empty()
            || self.project.len() > 255
            || !self
                .project
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(SourceError::InvalidProject);
        }
        if self.upstream_id.is_empty()
            || self.upstream_id.len() > 255
            || self.upstream_id == "."
            || self.upstream_id == ".."
            || self.upstream_id.starts_with('.')
            || self.upstream_id.ends_with('.')
            || windows_device_name(&self.upstream_id)
            || !self
                .upstream_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(SourceError::MissingGeneration);
        }
        if !self.completed {
            return Err(SourceError::Incomplete);
        }
        if self.files.is_empty() {
            return Err(SourceError::EmptyFileSet);
        }
        if self.files.len() > MAX_SOURCE_FILES {
            return Err(SourceError::ResourceBudget);
        }
        let mut total_bytes = 0u64;
        let mut names = BTreeSet::new();
        for file in &self.files {
            total_bytes = total_bytes
                .checked_add(file.bytes)
                .filter(|sum| *sum <= MAX_SOURCE_GENERATION_BYTES)
                .ok_or(SourceError::ResourceBudget)?;
            if file.bytes == 0
                || file.name.is_empty()
                || file.name.len() > 255
                || file.name.contains('/')
                || file.name.contains('\\')
                || file.name == "."
                || file.name == ".."
                || file.name.starts_with('.')
                || file.name.ends_with('.')
                || windows_device_name(&file.name)
                || !file
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
                || file.sha256.len() != 64
                || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(SourceError::InvalidFile(file.name.clone()));
            }
            if !names.insert(file.name.to_ascii_lowercase()) {
                return Err(SourceError::DuplicateFile(file.name.clone()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SourceGeneration {
        SourceGeneration {
            project: "enwiki".into(),
            upstream_id: "20261001".into(),
            completed: true,
            files: vec![SourceFile {
                name: "articles.parquet".into(),
                sha256: "a".repeat(64),
                bytes: 1024,
            }],
        }
    }

    #[test]
    fn accepts_completed_generation_and_rejects_partial() {
