//! Verify that beta Structured Contents references resolve within one joined
//! article revision before using them for evidence or canonical normalization.

use std::collections::{BTreeMap, BTreeSet};

use crate::enterprise::{JoinedEnterpriseArticle, StructuredPart};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntegrityError {
    UnknownCitation(String),
    UnknownTableReference(String),
    ConflictingCitationText(String),
}

fn check_parts(
    parts: &[StructuredPart],
    references: &BTreeSet<&str>,
    tables: &BTreeSet<&str>,
    citation_texts: &mut BTreeMap<String, String>,
) -> Result<(), IntegrityError> {
    for part in parts {
        for citation in &part.citations {
            if !references.contains(citation.identifier.as_str()) {
                return Err(IntegrityError::UnknownCitation(citation.identifier.clone()));
            }
            if let Some(previous) =
                citation_texts.insert(citation.identifier.clone(), citation.text.clone())
            {
                if previous != citation.text {
                    return Err(IntegrityError::ConflictingCitationText(
                        citation.identifier.clone(),
                    ));
                }
            }
        }
        for table in &part.table_references {
            if !tables.contains(table.identifier.as_str()) {
                return Err(IntegrityError::UnknownTableReference(
                    table.identifier.clone(),
                ));
            }
        }
        check_parts(&part.has_parts, references, tables, citation_texts)?;
    }
    Ok(())
}

/// A successful exact-revision join does not guarantee all nested evidence
/// identifiers are resolvable. Reject broken links rather than silently
/// inventing references when converting to the canonical article model.
pub fn validate_joined_evidence(article: &JoinedEnterpriseArticle) -> Result<(), IntegrityError> {
    let references: BTreeSet<&str> = article
        .structured
        .references
        .iter()
        .map(|reference| reference.identifier.as_str())
        .collect();
    let tables: BTreeSet<&str> = article
        .structured
        .tables
        .iter()
        .map(|table| table.identifier.as_str())
        .collect();
    let mut citation_texts = BTreeMap::new();
    check_parts(
        &article.structured.infoboxes,
        &references,
        &tables,
        &mut citation_texts,
    )?;
    check_parts(
        &article.structured.sections,
        &references,
        &tables,
        &mut citation_texts,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enterprise::{PartKind, StructuredCitation, StructuredTableReference};

    #[test]
    fn nested_evidence_requires_exact_local_identifiers() {
        let mut article = JoinedEnterpriseArticle {
            structured: crate::enterprise::StructuredArticle {
                page: crate::join::PageKey {
                    project: "enwiki".into(),
                    page_id: 42,
                },
                revision_id: 99,
                date_modified: "2026-10-10T00:00:00Z".into(),
                generation_id: "20261010".into(),
                namespace: 0,
                language: "en".into(),
                name: "Gravity".into(),
                wikidata_id: None,
                description: None,
                infoboxes: vec![],
                sections: vec![StructuredPart {
                    kind: PartKind::Section,
                    name: Some("Theory".into()),
                    value: None,
                    values: vec![],
                    links: vec![],
                    citations: vec![],
                    table_references: vec![],
                    images: vec![],
                    has_parts: vec![StructuredPart {
                        kind: PartKind::Paragraph,
                        name: None,
                        value: Some("Text".into()),
                        values: vec![],
                        links: vec![],
                        citations: vec![StructuredCitation {
                            identifier: "cite1".into(),
                            text: "[1]".into(),
                        }],
                        table_references: vec![StructuredTableReference {
                            identifier: "table1".into(),
                            confidence_score: None,
                        }],
                        images: vec![],
                        has_parts: vec![],
                    }],
                }],
                references: vec![],
                tables: vec![],
            },
            rendered_html: "<article>Text</article>".into(),
            categories: vec!["Physics".into()],
            redirects: vec![],
        };
        assert_eq!(
            validate_joined_evidence(&article),
            Err(IntegrityError::UnknownCitation("cite1".into()))
        );
        article
            .structured
            .references
            .push(crate::enterprise::StructuredReference {
                identifier: "cite1".into(),
                payload: serde_json::Map::new(),
            });
        assert_eq!(
            validate_joined_evidence(&article),
            Err(IntegrityError::UnknownTableReference("table1".into()))
        );
        article
            .structured
            .tables
            .push(crate::enterprise::StructuredTable {
                identifier: "table1".into(),
                headers: vec![],
                rows: vec![],
                confidence_score: None,
            });
        assert_eq!(validate_joined_evidence(&article), Ok(()));

        // One revision-local reference identifier must not acquire two
        // incompatible visible markers in separate article blocks.
        let repeated = article.structured.sections[0].has_parts[0].clone();
        article.structured.infoboxes.push(repeated);
        assert_eq!(validate_joined_evidence(&article), Ok(()));
        article.structured.infoboxes[0].citations[0].text = "[99]".into();
        assert_eq!(
            validate_joined_evidence(&article),
            Err(IntegrityError::ConflictingCitationText("cite1".into()))
        );
    }
}
