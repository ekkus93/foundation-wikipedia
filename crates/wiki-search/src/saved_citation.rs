//! RAG-002: citations stored with saved conversations, and how they resolve
//! after the article changes.
//!
//! A `SavedCitation` records exactly what the model was given: the article,
//! revision, optional installed snapshot, block ID, heading path and the
//! exact excerpt with its SHA-256. Resolving it against the article as it is
//! now never points at different text:
//!
//! - same revision and identical block → `Current` (a jump target);
//! - newer or older revision → `Stale`, optionally with the single block in
//!   the current revision whose excerpt and heading path are byte-identical
//!   (ambiguous or changed text yields no relocation);
//! - same revision but the block is missing or its text differs →
//!   `Unavailable` (corruption or tampering; never jump);
//! - a different article → `WrongArticle`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use wiki_model::{Article, ArticleKey, ModelError};

use crate::{ArticleLexicalIndex, EvidenceHit};

pub const SAVED_CITATION_SCHEMA: u32 = 1;
pub const MAX_SAVED_EXCERPT_BYTES: usize = 64 * 1024;
const MAX_HEADING_DEPTH: usize = 32;
const MAX_FIELD_BYTES: usize = 4096;

/// Serializable, self-checking record of one cited evidence block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedCitation {
    pub schema_version: u32,
    pub project: String,
    pub page_id: u64,
    pub revision_id: u64,
    /// Installed offline snapshot the evidence came from; `None` when the
    /// article was read online.
    pub snapshot_id: Option<String>,
    pub block_id: String,
    pub heading_path: Vec<String>,
    pub excerpt: String,
    pub excerpt_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SavedCitationError {
    UnsupportedSchema(u32),
    /// The block ID does not belong to the recorded article and revision.
    InconsistentIdentity,
    /// The stored excerpt does not match its recorded digest.
    ExcerptDigestMismatch,
    TooLarge,
    InvalidSnapshotId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitationStatus {
    /// Same revision, identical block: safe to jump to and highlight.
    Current(EvidenceHit),
    /// The article is now at another revision. Show the cited text as of
    /// `cited_revision`; `same_text_in_current` is offered only when exactly
    /// one current block has byte-identical text and heading path.
    Stale {
        cited_revision: u64,
        current_revision: u64,
        same_text_in_current: Option<EvidenceHit>,
    },
    /// Same revision, but the cited block is missing or its text differs.
    Unavailable,
    WrongArticle,
}

fn sha256_hex(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn block_prefix(project: &str, page_id: u64, revision_id: u64) -> String {
    format!("wkb:{project}:{page_id}:{revision_id}:")
}

impl SavedCitation {
    /// Record evidence that was actually supplied for `key`'s revision.
    pub fn from_evidence(
        key: &ArticleKey,
        hit: &EvidenceHit,
        snapshot_id: Option<&str>,
    ) -> Result<Self, SavedCitationError> {
        let saved = Self {
            schema_version: SAVED_CITATION_SCHEMA,
            project: key.project.clone(),
            page_id: key.page_id,
            revision_id: hit.revision_id,
            snapshot_id: snapshot_id.map(str::to_owned),
            block_id: hit.block_id.clone(),
            heading_path: hit.heading_path.clone(),
            excerpt: hit.excerpt.clone(),
            excerpt_sha256: sha256_hex(&hit.excerpt),
        };
        saved.validate()?;
        Ok(saved)
    }

    /// Check a record loaded from storage before trusting any field.
    pub fn validate(&self) -> Result<(), SavedCitationError> {
        if self.schema_version != SAVED_CITATION_SCHEMA {
            return Err(SavedCitationError::UnsupportedSchema(self.schema_version));
        }
        if self.excerpt.len() > MAX_SAVED_EXCERPT_BYTES
            || self.heading_path.len() > MAX_HEADING_DEPTH
            || self.heading_path.iter().any(|h| h.len() > MAX_FIELD_BYTES)
            || self.block_id.len() > MAX_FIELD_BYTES
            || self.project.len() > MAX_FIELD_BYTES
        {
            return Err(SavedCitationError::TooLarge);
        }
        if let Some(id) = &self.snapshot_id {
            if id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
            {
                return Err(SavedCitationError::InvalidSnapshotId);
            }
        }
        let prefix = block_prefix(&self.project, self.page_id, self.revision_id);
        if self.project.is_empty()
            || self.page_id == 0
            || self.revision_id == 0
            || !self.block_id.starts_with(&prefix)
        {
            return Err(SavedCitationError::InconsistentIdentity);
        }
        if sha256_hex(&self.excerpt) != self.excerpt_sha256 {
            return Err(SavedCitationError::ExcerptDigestMismatch);
        }
        Ok(())
    }

    /// The evidence exactly as it was supplied, for display "as cited".
    pub fn as_cited(&self) -> EvidenceHit {
        EvidenceHit {
            block_id: self.block_id.clone(),
            revision_id: self.revision_id,
            heading_path: self.heading_path.clone(),
            excerpt: self.excerpt.clone(),
            score: 0,
        }
    }
}

/// Resolve a validated saved citation against the article as it is now.
pub fn resolve_saved_citation(
    current: &Article,
    saved: &SavedCitation,
) -> Result<CitationStatus, ResolveError> {
    saved.validate().map_err(ResolveError::Saved)?;
    if current.key.project != saved.project || current.key.page_id != saved.page_id {
        return Ok(CitationStatus::WrongArticle);
    }
    let index = ArticleLexicalIndex::build(current).map_err(ResolveError::Article)?;
    if current.revision.revision_id == saved.revision_id {
        let cited = saved.as_cited();
        return Ok(if index.contains_evidence(&cited) {
            CitationStatus::Current(cited)
        } else {
            CitationStatus::Unavailable
        });
    }
    let mut matches = index
        .entries
        .iter()
        .filter(|entry| entry.excerpt == saved.excerpt && entry.heading_path == saved.heading_path);
    let relocated = match (matches.next(), matches.next()) {
        (Some(only), None) => Some(only.clone()),
        _ => None,
    };
    Ok(CitationStatus::Stale {
        cited_revision: saved.revision_id,
        current_revision: current.revision.revision_id,
        same_text_in_current: relocated,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveError {
    Saved(SavedCitationError),
    Article(ModelError),
}
