//! Fail-closed Enterprise NDJSON import with nested evidence checks.
use std::io::BufRead;

use crate::enterprise::JoinedEnterpriseArticle;
use crate::enterprise_integrity::{validate_joined_evidence, IntegrityError};
use crate::enterprise_ndjson::{
    join_enterprise_ndjson, join_enterprise_ndjson_scoped, BatchImportError,
};
use crate::join::{DeletionMarker, PageKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifiedImportError {
    Batch(BatchImportError),
    Integrity(IntegrityError),
}

pub fn import_verified_enterprise_ndjson<S: BufRead, R: BufRead>(
    structured: S,
    regular: R,
    generation_id: &str,
    deleted: Vec<PageKey>,
) -> Result<Vec<JoinedEnterpriseArticle>, VerifiedImportError> {
    let joined = join_enterprise_ndjson(structured, regular, generation_id, deleted)
        .map_err(VerifiedImportError::Batch)?;
    for article in &joined {
        validate_joined_evidence(article).map_err(VerifiedImportError::Integrity)?;
    }
    Ok(joined)
}

/// Apply project- and generation-scoped deletion evidence before accepting
/// a batch of revision-matched Structured Contents and rendered components.
pub fn import_verified_enterprise_ndjson_scoped<S: BufRead, R: BufRead>(
    structured: S,
    regular: R,
    project: &str,
    generation_id: &str,
    deleted: Vec<DeletionMarker>,
) -> Result<Vec<JoinedEnterpriseArticle>, VerifiedImportError> {
    let joined =
        join_enterprise_ndjson_scoped(structured, regular, project, generation_id, deleted)
            .map_err(VerifiedImportError::Batch)?;
    for article in &joined {
        validate_joined_evidence(article).map_err(VerifiedImportError::Integrity)?;
    }
    Ok(joined)
}

#[path = "enterprise_normalize.rs"]
pub mod enterprise_normalize;
#[path = "enterprise_normalize_article.rs"]
pub mod enterprise_normalize_article;
#[path = "enterprise_normalize_blocks.rs"]
mod enterprise_normalize_blocks;
#[path = "enterprise_normalize_citations.rs"]
mod enterprise_normalize_citations;
#[path = "enterprise_normalize_complex.rs"]
mod enterprise_normalize_complex;
#[path = "enterprise_normalize_html.rs"]
mod enterprise_normalize_html;
#[path = "enterprise_normalize_links.rs"]
mod enterprise_normalize_links;
#[path = "enterprise_normalize_references.rs"]
mod enterprise_normalize_references;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalImportError {
    Import(VerifiedImportError),
    Normalize(enterprise_normalize::NormalizeError),
}

/// All-or-nothing verified NDJSON import into canonical article records.
pub fn import_canonical_enterprise_ndjson<S: BufRead, R: BufRead>(
    structured: S,
    regular: R,
    generation_id: &str,
    deleted: Vec<PageKey>,
) -> Result<Vec<enterprise_normalize::CanonicalEnterpriseArticle>, CanonicalImportError> {
    let joined = import_verified_enterprise_ndjson(structured, regular, generation_id, deleted)
        .map_err(CanonicalImportError::Import)?;
    enterprise_normalize_article::normalize_enterprise_batch(&joined)
        .map_err(CanonicalImportError::Normalize)
}

/// All-or-nothing canonical import that refuses deletion evidence lacking
/// explicit matching project and source generation identity. Upstream feed
/// authenticity and published completeness remain separate obligations.
pub fn import_canonical_enterprise_ndjson_scoped<S: BufRead, R: BufRead>(
    structured: S,
    regular: R,
    project: &str,
    generation_id: &str,
    deleted: Vec<DeletionMarker>,
) -> Result<Vec<enterprise_normalize::CanonicalEnterpriseArticle>, CanonicalImportError> {
    let joined =
        import_verified_enterprise_ndjson_scoped(structured, regular, project, generation_id, deleted)
            .map_err(CanonicalImportError::Import)?;
    enterprise_normalize_article::normalize_enterprise_batch(&joined)
        .map_err(CanonicalImportError::Normalize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn scoped_verified_import_requires_matching_tombstone_generation() {
        let deletion = DeletionMarker {
            page: PageKey { project: "enwiki".into(), page_id: 2 },
            generation_id: "20261009".into(),
        };
        assert_eq!(
            import_verified_enterprise_ndjson_scoped(
                Cursor::new(Vec::<u8>::new()),
                Cursor::new(Vec::<u8>::new()),
                "enwiki",
                "20261010",
                vec![deletion],
            ),
            Err(VerifiedImportError::Batch(BatchImportError::Join(
                crate::enterprise::EnterpriseError::GenerationMismatch
            )))
        );
    }
}
