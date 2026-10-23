//! Authoritative Wikimedia snapshot metadata validation.
//! Download transport and checksum verification are separate SRC-001 steps.

pub mod enterprise;
pub mod enterprise_integrity;
pub mod enterprise_ndjson;
pub mod enterprise_pinned;
pub mod enterprise_pinned_deletions;
pub mod enterprise_verified;
pub mod join;
pub mod local_title_index;
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
pub mod public_dump_checked_read;
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
pub mod public_dump_xml_text;

mod strict_json;

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

/// Conservative Wikimedia project identifier shared by verified source
/// generation manifests and official dump URL identity checks.
pub(crate) fn valid_wiki_project(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 255
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
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
        if !valid_wiki_project(&self.project) {
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
        assert!(sample().validate().is_ok());
        let mut bad = sample();
        bad.completed = false;
        assert_eq!(bad.validate(), Err(SourceError::Incomplete));
    }

    #[test]
    fn generation_project_identity_matches_official_dump_syntax() {
        for project in ["", "_enwiki", "enwiki_", "123wiki", "___", "Enwiki"] {
            let mut bad = sample();
            bad.project = project.into();
            assert_eq!(bad.validate(), Err(SourceError::InvalidProject));
            assert!(!valid_wiki_project(project));
        }
        let mut good = sample();
        good.project = "zh_min_nanwiki".into();
        assert_eq!(good.validate(), Ok(()));
        assert!(valid_wiki_project(&good.project));
    }

    #[test]
    fn rejects_unsafe_generation_identifiers() {
        for id in ["", ".", "..", "../outside", "a/b", "a\\\\b", "a b", "a:b"] {
            let mut bad = sample();
            bad.upstream_id = id.into();
            assert_eq!(bad.validate(), Err(SourceError::MissingGeneration));
        }
        let mut good = sample();
        good.upstream_id = "2026-10-09T00_00_00Z".into();
        assert_eq!(good.validate(), Ok(()));
    }

    #[test]
    fn rejects_unsafe_member_names_before_transport() {
        for name in [
            ".",
            "..",
            ".hidden",
            "article.",
            "../outside",
            "folder/article.xml",
            "folder\\\\article.xml",
            "C:article.xml",
            "article\\u{0000}.xml",
            "article name.xml",
        ] {
            let mut bad = sample();
            bad.files[0].name = name.into();
            assert_eq!(
                bad.validate(),
                Err(SourceError::InvalidFile(name.into())),
                "unsafe source member {name:?} was accepted"
            );
        }
        let mut good = sample();
        good.files[0].name = "enwiki-20261001-pages-articles.xml.bz2".into();
        assert_eq!(good.validate(), Ok(()));
    }

    #[test]
    fn reserved_windows_names_and_case_aliases_are_rejected() {
        for name in ["CON", "nul.xml", "COM1", "LPT9.txt", "prn.dat"] {
            let mut bad = sample();
            bad.files[0].name = name.into();
            assert_eq!(bad.validate(), Err(SourceError::InvalidFile(name.into())));
        }
        let mut bad = sample();
        bad.upstream_id = "CON".into();
        assert_eq!(bad.validate(), Err(SourceError::MissingGeneration));

        let mut colliding = sample();
        colliding.files.push(SourceFile {
            name: "ARTICLES.PARQUET".into(),
            sha256: "b".repeat(64),
            bytes: 12,
        });
        assert_eq!(
            colliding.validate(),
            Err(SourceError::DuplicateFile("ARTICLES.PARQUET".into()))
        );
    }

    #[test]
    fn rejects_oversized_source_identity_fields() {
        let mut invalid = sample();
        invalid.project = "a".repeat(256);
        assert_eq!(invalid.validate(), Err(SourceError::InvalidProject));
        invalid = sample();
        invalid.upstream_id = "a".repeat(256);
        assert_eq!(invalid.validate(), Err(SourceError::MissingGeneration));
        invalid = sample();
        invalid.files[0].name = format!("{}.xml", "a".repeat(252));
        assert_eq!(
            invalid.validate(),
            Err(SourceError::InvalidFile(invalid.files[0].name.clone()))
        );
    }

    #[test]
    fn rejects_oversized_source_generation_before_staging() {
        let mut oversized = sample();
        oversized.files[0].bytes = MAX_SOURCE_GENERATION_BYTES + 1;
        assert_eq!(oversized.validate(), Err(SourceError::ResourceBudget));
        oversized.files[0].bytes = u64::MAX;
        assert_eq!(oversized.validate(), Err(SourceError::ResourceBudget));
        let mut too_many = sample();
        too_many.files = vec![too_many.files[0].clone(); MAX_SOURCE_FILES + 1];
        assert_eq!(too_many.validate(), Err(SourceError::ResourceBudget));
        let mut at_limit = sample();
        at_limit.files[0].bytes = MAX_SOURCE_GENERATION_BYTES;
        assert_eq!(at_limit.validate(), Ok(()));
    }

    #[test]
    fn rejects_invalid_checksums_and_duplicate_members() {
        let mut bad = sample();
        bad.files[0].sha256 = "bad".into();
        assert_eq!(
            bad.validate(),
            Err(SourceError::InvalidFile("articles.parquet".into()))
        );
        let mut duplicate = sample();
        duplicate.files.push(duplicate.files[0].clone());
        assert_eq!(
            duplicate.validate(),
            Err(SourceError::DuplicateFile("articles.parquet".into()))
        );
    }
}
