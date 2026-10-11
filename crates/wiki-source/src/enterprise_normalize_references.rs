//! Preserve revision-local Wikimedia references.
use super::enterprise_normalize::NormalizeError;
use crate::enterprise::{JoinedEnterpriseArticle, StructuredReference};
use serde_json::Value;
use wiki_model::Reference;

/// Never allow an upstream reference to become an executable or ambiguous link.
fn safe_reference_url(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 4096
        || value
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control() || byte == 92)
    {
        return false;
    }
    let Ok(parsed) = url::Url::parse(value) else {
        return false;
    };
    matches!(parsed.scheme(), "https" | "http")
        && parsed.host_str().is_some()
        && parsed.username().is_empty()
        && parsed.password().is_none()
}

fn label(reference: &StructuredReference) -> Result<String, NormalizeError> {
    match reference.payload.get("title") {
        None | Some(Value::Null) => Ok(reference.identifier.clone()),
        Some(Value::String(title)) if !title.trim().is_empty() => Ok(title.clone()),
        Some(Value::String(_)) => Ok(reference.identifier.clone()),
        Some(_) => Err(NormalizeError::InvalidReference(
            reference.identifier.clone(),
        )),
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
                Some(Value::String(s)) if safe_reference_url(s) => Some(s.clone()),
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
        for title in [
            serde_json::json!(42),
            serde_json::json!({"text":"wrong-shape"}),
        ] {
            assert_eq!(
                label(&reference(Some(title))),
                Err(NormalizeError::InvalidReference("cite-42".into()))
            );
        }
    }

    #[test]
    fn rejects_active_ambiguous_and_credentialed_reference_urls() {
        for unsafe_url in [
            "",
            "javascript:alert(1)",
            "data:text/html,evil",
            "file:///etc/passwd",
            "/wiki/Relative",
            "https://user:secret@example.org/path",
            "https://example.org\\\\@evil.example/",
            "https://example.org/with space",
            "https://",
        ] {
            assert!(!safe_reference_url(unsafe_url), "{unsafe_url}");
        }
        assert!(safe_reference_url(
            "https://en.wikipedia.org/wiki/Physics#Notes"
        ));
        assert!(safe_reference_url("http://example.org/reference?id=7"));
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
