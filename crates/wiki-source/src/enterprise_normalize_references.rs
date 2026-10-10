//! Preserve revision-local Wikimedia references.
use super::enterprise_normalize::NormalizeError;
use crate::enterprise::{JoinedEnterpriseArticle, StructuredReference};
use serde_json::Value;
use wiki_model::Reference;

fn label(reference: &StructuredReference) -> Result<String, NormalizeError> {
    match reference.payload.get("title") {
        None | Some(Value::Null) => Ok(reference.identifier.clone()),
        Some(Value::String(title)) if !title.trim().is_empty() => Ok(title.clone()),
        Some(Value::String(_)) => Ok(reference.identifier.clone()),
        Some(_) => Err(NormalizeError::InvalidReference(reference.identifier.clone())),
    }
}

pub(crate) fn references(
    joined: &JoinedEnterpriseArticle,
) -> Result<Vec<Reference>, NormalizeError> {
    joined
        .structured
        .references
        .iter()
        .map(|r| {
            let label = label(r)?;
            let source_url = match r.payload.get("url") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.clone()),
                Some(_) => return Err(NormalizeError::InvalidReference(r.identifier.clone())),
            };
            Ok(Reference {
                id: r.identifier.clone(),
                label,
                source_url,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(title: Option<Value>) -> StructuredReference {
        let mut payload = serde_json::Map::new();
        if let Some(value) = title {
            payload.insert("title".into(), value);
        }
        StructuredReference {
            identifier: "cite-42".into(),
            payload,
        }
    }

    #[test]
    fn rejects_explicit_non_string_reference_title() {
        for title in [serde_json::json!(42), serde_json::json!({"text":"unsafe"})] {
            assert_eq!(
                label(&reference(Some(title))),
                Err(NormalizeError::InvalidReference("cite-42".into()))
            );
        }
    }

    #[test]
    fn absent_or_null_reference_title_uses_stable_identifier() {
        assert_eq!(label(&reference(None)), Ok("cite-42".into()));
        assert_eq!(label(&reference(Some(Value::Null))), Ok("cite-42".into()));
        assert_eq!(
            label(&reference(Some(Value::String("Gravity".into())))),
            Ok("Gravity".into())
        );
    }
}
