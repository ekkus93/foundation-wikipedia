//! Exact-revision Wikimedia Enterprise canonical normalization.
use sha2::{Digest, Sha256};
use wiki_model::{Article, ModelError};
use crate::enterprise::JoinedEnterpriseArticle;
use crate::enterprise_integrity::{validate_joined_evidence, IntegrityError};

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
    Serialization,
}

pub fn digest_article(article: &Article) -> Result<String, NormalizeError> {
    let mut record = article.clone();
    record.revision.content_sha256 = "0".repeat(64);
    record.validate().map_err(NormalizeError::Model)?;
    let bytes = serde_json::to_vec(&record).map_err(|_| NormalizeError::Serialization)?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

pub fn normalize_enterprise_article(
    joined: &JoinedEnterpriseArticle,
) -> Result<CanonicalEnterpriseArticle, NormalizeError> {
    validate_joined_evidence(joined).map_err(NormalizeError::Integrity)?;
    todo!("canonical mapping must be implemented before use")
}
