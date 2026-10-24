//! Pre-download preview of curated Mathematics/Physics/Computer Science
//! topic definitions against one revision-joined Enterprise generation.
//!
//! This connects editable curated seeds to the shared pinned resolver. A
//! matching project/generation label and a local digest do not authenticate
//! Wikimedia publication, prove full category coverage, or produce a pack.

use std::collections::BTreeSet;

use wiki_source::enterprise::JoinedEnterpriseArticle;

use crate::curated::curated_topic;
use crate::enterprise_graph::PageRedirectIdentity;
use crate::joined_preview::{preview_joined_selection, JoinedPreviewError, JoinedSelectionPreview};
use crate::selection::Rules;

const MAX_REQUESTED_TOPICS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CuratedPreviewError {
    EmptyTopics,
    TooManyTopics,
    UnknownTopic(String),
    IncompatiblePolicy,
    WrongProject { expected: String, actual: String },
    Preview(JoinedPreviewError),
}

#[derive(Clone, Debug)]
pub struct CuratedTopicSelection {
    /// Canonical sorted, deduplicated curated IDs.
    pub topic_ids: Vec<String>,
    /// Editable definition, with the curated topic limits and caller overrides.
    pub rules: Rules,
    /// Source-local, revision-bound selection and pre-download diagnostics.
    pub preview: JoinedSelectionPreview,
}

/// Combine one or more initial curated topics, retaining explicit include and
/// exclude overrides. A composite must share one edition and identical depth/
/// page policies; do not silently widen one seed's safety limits.
///
/// All supplied records are checked for edition identity before resolution.
/// Full category membership, official source authority, visual media and
/// installability must be independently qualified by later pipeline stages.
pub fn preview_curated_topics(
    records: &[JoinedEnterpriseArticle],
    administrative: &BTreeSet<String>,
    topic_ids: &[&str],
    redirect_identities: &[PageRedirectIdentity],
    include: &[u64],
    exclude: &[u64],
) -> Result<CuratedTopicSelection, CuratedPreviewError> {
    if topic_ids.is_empty() {
        return Err(CuratedPreviewError::EmptyTopics);
    }
    if topic_ids.len() > MAX_REQUESTED_TOPICS {
        return Err(CuratedPreviewError::TooManyTopics);
    }
    let ids: BTreeSet<&str> = topic_ids.iter().copied().collect();
    let mut topics = Vec::with_capacity(ids.len());
    for id in &ids {
        topics.push(
            curated_topic(id).ok_or_else(|| CuratedPreviewError::UnknownTopic((*id).to_owned()))?,
        );
    }
    let first = topics[0];
    if topics.iter().any(|topic| {
        topic.project != first.project
            || topic.depth_limit != first.depth_limit
            || topic.page_limit != first.page_limit
    }) {
        return Err(CuratedPreviewError::IncompatiblePolicy);
    }
    for record in records {
        let actual = &record.structured.page.project;
        if actual != first.project {
            return Err(CuratedPreviewError::WrongProject {
                expected: first.project.to_owned(),
                actual: actual.clone(),
            });
        }
    }
    let roots: BTreeSet<String> = topics
        .iter()
        .flat_map(|topic| topic.roots.iter().map(|root| (*root).to_owned()))
        .collect();
    let rules = Rules {
        roots: roots.into_iter().collect(),
        include: include.to_vec(),
        exclude: exclude.to_vec(),
        depth_limit: first.depth_limit,
        page_limit: first.page_limit,
    };
    let preview = preview_joined_selection(records, administrative, &rules, redirect_identities)
        .map_err(CuratedPreviewError::Preview)?;
    Ok(CuratedTopicSelection {
        topic_ids: ids.into_iter().map(str::to_owned).collect(),
        rules,
        preview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiki_source::enterprise::StructuredArticle;
    use wiki_source::join::PageKey;

    fn page(id: u64, namespace: i32, name: &str, categories: &[&str]) -> JoinedEnterpriseArticle {
        JoinedEnterpriseArticle {
            structured: StructuredArticle {
                page: PageKey {
                    project: "enwiki".into(),
                    page_id: id,
                },
                revision_id: id + 1000,
                date_modified: "2026-10-23T00:00:00Z".into(),
                generation_id: "20261023".into(),
                namespace,
                language: "en".into(),
                name: name.into(),
                wikidata_id: None,
                description: None,
                infoboxes: vec![],
                sections: vec![],
                references: vec![],
                tables: vec![],
            },
            rendered_html: "<p>Preview fixture only</p>".into(),
            categories: categories.iter().map(|value| (*value).into()).collect(),
            redirects: vec![],
        }
    }

    fn fixture() -> Vec<JoinedEnterpriseArticle> {
        vec![
            page(100, 14, "Category:Mathematics", &[]),
            page(101, 14, "Category:Physics", &[]),
            page(102, 14, "Category:Computer science", &[]),
            page(1, 0, "Algebra", &["Mathematics"]),
            page(2, 0, "Mechanics", &["Physics"]),
            page(3, 0, "Algorithms", &["Computer science"]),
        ]
    }

    #[test]
    fn curated_composites_are_deterministic_and_revision_bound() {
        let records = fixture();
        let admin = BTreeSet::new();
        let first = preview_curated_topics(
            &records,
            &admin,
            &["physics", "mathematics"],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let reordered = preview_curated_topics(
            &records,
            &admin,
            &["mathematics", "physics", "mathematics"],
            &[],
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(first.topic_ids, vec!["mathematics", "physics"]);
        assert_eq!(
            first.rules.roots,
            vec!["Category:Mathematics", "Category:Physics"]
        );
        assert_eq!(first.rules.depth_limit, 2);
        assert_eq!(first.rules.page_limit, 50_000);
        assert_eq!(first.preview.preview.selection.page_ids, vec![1, 2]);
        assert_eq!(first.preview.preview.selected_page_count, 2);
        assert_eq!(first.preview.revision_pinned.page_revisions.len(), 2);
        assert_eq!(
            first.preview.revision_pinned.revision_selection_sha256,
            reordered.preview.revision_pinned.revision_selection_sha256
        );
        assert_eq!(
            first.preview.preview.selection.snapshot_selection_sha256,
            reordered.preview.preview.selection.snapshot_selection_sha256
        );
        assert_eq!(
            first.preview.preview.selection.definition_sha256,
            reordered.preview.preview.selection.definition_sha256
        );
    }

    #[test]
    fn explicit_overrides_and_single_topic_selection() {
        let records = fixture();
        let selected = preview_curated_topics(
            &records,
            &BTreeSet::new(),
            &["mathematics", "physics"],
            &[],
            &[3],
            &[1],
        )
        .unwrap();
        assert_eq!(selected.preview.preview.selection.page_ids, vec![2, 3]);
        assert_eq!(selected.preview.preview.explicit_include_ids, 1);
        assert_eq!(selected.preview.preview.explicit_exclude_ids, 1);
        let single = preview_curated_topics(
            &records,
            &BTreeSet::new(),
            &["computer-science"],
            &[],
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(single.preview.preview.selection.page_ids, vec![3]);
    }

    #[test]
    fn rejects_unknown_topics_and_wrong_edition_without_fallback() {
        let records = fixture();
        let admin = BTreeSet::new();
        assert!(matches!(
            preview_curated_topics(&records, &admin, &[], &[], &[], &[]),
            Err(CuratedPreviewError::EmptyTopics)
        ));
        assert_eq!(
            preview_curated_topics(&records, &admin, &["unknown"], &[], &[], &[]).unwrap_err(),
            CuratedPreviewError::UnknownTopic("unknown".into())
        );
        assert!(matches!(
            preview_curated_topics(&records, &admin, &["physics"; 33], &[], &[], &[]),
            Err(CuratedPreviewError::TooManyTopics)
        ));
        let mut wrong = records;
        wrong[0].structured.page.project = "frwiki".into();
        assert_eq!(
            preview_curated_topics(&wrong, &admin, &["physics"], &[], &[], &[]).unwrap_err(),
            CuratedPreviewError::WrongProject {
                expected: "enwiki".into(),
                actual: "frwiki".into(),
            }
        );
    }
}
