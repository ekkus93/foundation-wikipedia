//! Authoritative Wikimedia snapshot metadata validation.
//! Download transport and checksum verification are separate SRC-001 steps.

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
                || !file
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
                || file.sha256.len() != 64
                || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(SourceError::InvalidFile(file.name.clone()));
            }
            if !names.insert(&file.name) {
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
