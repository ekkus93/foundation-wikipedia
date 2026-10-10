//! Authoritative Wikimedia snapshot metadata validation.
//! Download transport and checksum verification are separate SRC-001 steps.

pub mod enterprise;
pub mod join;

use std::collections::BTreeSet;

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
}

/// Reject names that alias Windows device files even when given an extension.
fn windows_device_name(value: &str) -> bool {
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
            || !self
                .project
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(SourceError::InvalidProject);
        }
        if self.upstream_id.is_empty()
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
        let mut names = BTreeSet::new();
        for file in &self.files {
            if file.bytes == 0
                || file.name.is_empty()
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
        assert!(sample().validate().is_ok());
