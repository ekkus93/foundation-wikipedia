//! Fail-closed Enterprise NDJSON import with nested evidence checks.
use std::io::BufRead;

use crate::enterprise::JoinedEnterpriseArticle;
use crate::enterprise_integrity::{validate_joined_evidence, IntegrityError};
use crate::enterprise_ndjson::{join_enterprise_ndjson, BatchImportError};
use crate::join::PageKey;

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
