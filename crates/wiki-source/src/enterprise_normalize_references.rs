//! Preserve revision-local Wikimedia references.
use serde_json::Value;
use wiki_model::Reference;
use crate::enterprise::JoinedEnterpriseArticle;
use crate::enterprise_normalize::NormalizeError;

pub(crate) fn references(joined: &JoinedEnterpriseArticle) -> Result<Vec<Reference>, NormalizeError> {
    joined.structured.references.iter().map(|r| {
        let label = r.payload.get("title").and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty()).unwrap_or(&r.identifier);
        let source_url = match r.payload.get("url") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => return Err(NormalizeError::InvalidReference(r.identifier.clone())),
        };
        Ok(Reference { id: r.identifier.clone(), label: label.into(), source_url })
    }).collect()
}
