//! Exact-revision Enterprise canonical normalization, with fail-closed errors.
use crate::enterprise::PartKind;
use crate::enterprise_integrity::IntegrityError;
use sha2::{Digest, Sha256};
use wiki_model::{Article, ArticleKey, ModelError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalEnterpriseArticle {
    pub article: Article,
    pub categories: Vec<String>,
    pub generation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NormalizeError {
    Integrity(IntegrityError),
    Model(ModelError),
    Missing(&'static str),
    Unsupported(PartKind),
    UnresolvedVisual,
    InvalidTable(String),
    InvalidReference(String),
    UnresolvedLink,
    GenerationMismatch,
    ProjectMismatch,
    DuplicatePage(ArticleKey),
    TooManyBlocks,
    Serialization,
}

/// Hash deterministic serialized canonical content with the digest field zeroed.
pub fn digest_article(article: &Article) -> Result<String, NormalizeError> {
    let mut record = article.clone();
    record.revision.content_sha256 = "0".repeat(64);
    record.validate().map_err(NormalizeError::Model)?;
    let bytes = serde_json::to_vec(&record).map_err(|_| NormalizeError::Serialization)?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}
