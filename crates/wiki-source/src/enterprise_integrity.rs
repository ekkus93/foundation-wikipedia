//! Verify that beta Structured Contents references resolve within one joined
//! article revision before using them for evidence or canonical normalization.

use std::collections::BTreeSet;

use crate::enterprise::{JoinedEnterpriseArticle, StructuredPart, MAX_PARTS, MAX_PART_DEPTH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntegrityError {
    UnknownCitation(String),
    UnknownTableReference(String),
    DuplicateReference(String),
    DuplicateTable(String),
    InvalidIdentifier(&'static str),
    PartDepthExceeded,
    PartCountExceeded,
}

fn check_parts(
    parts: &[StructuredPart],
    references: &BTreeSet<&str>,
    tables: &BTreeSet<&str>,
    depth: usize,
    count: &mut usize,
) -> Result<(), IntegrityError> {
    // A leaf at the maximum supported depth has an empty child slice at
    // depth + 1. Match the parser's limit: only actual parts count toward
    // nesting depth, not the empty child list after a leaf.
    if parts.is_empty() {
        return Ok(());
    }
    if depth > MAX_PART_DEPTH {
        return Err(IntegrityError::PartDepthExceeded);
    }
    for part in parts {
        *count = count
            .checked_add(1)
            .ok_or(IntegrityError::PartCountExceeded)?;
        if *count > MAX_PARTS {
            return Err(IntegrityError::PartCountExceeded);
        }
        for citation in &part.citations {
            if !references.contains(citation.identifier.as_str()) {
                return Err(IntegrityError::UnknownCitation(citation.identifier.clone()));
            }
        }
        for table in &part.table_references {
            if !tables.contains(table.identifier.as_str()) {
                return Err(IntegrityError::UnknownTableReference(
                    table.identifier.clone(),
                ));
            }
        }
        check_parts(&part.has_parts, references, tables, depth + 1, count)?;
    }
    Ok(())
}

/// A successful exact-revision join does not guarantee all nested evidence
/// identifiers are resolvable. Reject broken links rather than silently
/// inventing references when converting to the canonical article model.
pub fn validate_joined_evidence(article: &JoinedEnterpriseArticle) -> Result<(), IntegrityError> {
    let mut references = BTreeSet::new();
    for reference in &article.structured.references {
        if !references.insert(reference.identifier.as_str()) {
            return Err(IntegrityError::DuplicateReference(
                reference.identifier.clone(),
            ));
        }
    }
    let mut tables = BTreeSet::new();
    for table in &article.structured.tables {
        if !tables.insert(table.identifier.as_str()) {
            return Err(IntegrityError::DuplicateTable(table.identifier.clone()));
        }
    }
    let mut count = 0;
    check_parts(
        &article.structured.infoboxes,
        &references,
        &tables,
        0,
        &mut count,
    )?;
    check_parts(
        &article.structured.sections,
        &references,
        &tables,
        0,
        &mut count,
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

        // Defensive revalidation must not allow callers that construct or
        // transform typed joined records to bypass parser-level ID uniqueness.
        let repeated_reference = article.structured.references[0].clone();
        article.structured.references.push(repeated_reference);
        assert_eq!(
            validate_joined_evidence(&article),
            Err(IntegrityError::DuplicateReference("cite1".into()))
        );
        article.structured.references.pop();
        let repeated_table = article.structured.tables[0].clone();
        article.structured.tables.push(repeated_table);
        assert_eq!(
            validate_joined_evidence(&article),
            Err(IntegrityError::DuplicateTable("table1".into()))
        );
        article.structured.tables.pop();
        assert_eq!(validate_joined_evidence(&article), Ok(()));

        let mut leaf = article.structured.sections[0].clone();
        leaf.has_parts.clear();
        let mut nested = leaf.clone();
        for _ in 0..MAX_PART_DEPTH {
            let mut parent = leaf.clone();
            parent.has_parts = vec![nested];
            nested = parent;
        }
        article.structured.sections = vec![nested.clone()];
        assert_eq!(validate_joined_evidence(&article), Ok(()));
        let mut beyond_limit = leaf;
        beyond_limit.has_parts = vec![nested];
        article.structured.sections = vec![beyond_limit];
        assert_eq!(
            validate_joined_evidence(&article),
            Err(IntegrityError::PartDepthExceeded)
        );
    }
}
