//! Pre-download preview of curated Mathematics/Physics/Computer Science
//! topic definitions against one revision-joined Enterprise generation.
//!
//! This connects editable curated seeds to the shared pinned resolver. A
//! matching project/generation label and a local digest do not authenticate
//! Wikimedia publication, prove full category coverage, or produce a pack.

use std::collections::BTreeSet;

use wiki_pack_format::Sha256Transcript;
use wiki_source::enterprise::JoinedEnterpriseArticle;

use crate::curated::curated_topic;
use crate::enterprise_graph::PageRedirectIdentity;
use crate::joined_preview::{preview_joined_selection, JoinedPreviewError, JoinedSelectionPreview};
use crate::selection::Rules;

const MAX_REQUESTED_TOPICS: usize = 32;

/// Encode length-delimited strings to prevent ambiguous concatenation.
fn hash_text(digest: &mut Sha256Transcript, text: &str) {
    digest.update(&(text.len() as u64).to_be_bytes());
    digest.update(text.as_bytes());
}

/// Fingerprint the editable curated seed identity and an effective selection
/// rule digest. This receipt is independent of source revisions and cannot
/// authenticate a publisher. Topic IDs are canonicalized before hashing.
pub fn curated_definition_fingerprint(
    topic_ids: &[&str],
    rules_definition_sha256: &str,
) -> Result<String, CuratedPreviewError> {
    if topic_ids.is_empty() {
        return Err(CuratedPreviewError::EmptyTopics);
    }
    if topic_ids.len() > MAX_REQUESTED_TOPICS {
        return Err(CuratedPreviewError::TooManyTopics);
    }
    if rules_definition_sha256.len() != 64
        || !rules_definition_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(CuratedPreviewError::InvalidDefinitionDigest);
    }
    let ids: BTreeSet<&str> = topic_ids.iter().copied().collect();
    let mut digest = Sha256Transcript::new();
    digest.update(b"foundation-wikipedia/curated-topic-definition/v1\0");
    digest.update(&(ids.len() as u64).to_be_bytes());
    for id in ids {
        let topic =
            curated_topic(id).ok_or_else(|| CuratedPreviewError::UnknownTopic(id.to_owned()))?;
        for value in [topic.id, topic.label, topic.project] {
            hash_text(&mut digest, value);
        }
        digest.update(&(topic.roots.len() as u64).to_be_bytes());
        for root in topic.roots {
            hash_text(&mut digest, root);
        }
        digest.update(&(topic.depth_limit as u64).to_be_bytes());
        digest.update(&(topic.page_limit as u64).to_be_bytes());
    }
    // SHA-256 is a byte identity: upper- and lower-case hexadecimal spellings
    // denote the same digest. Canonicalize before incorporating the upstream
    // rules receipt, or an unchanged selection would acquire two different
    // curated definition fingerprints solely because of letter case.
    hash_text(&mut digest, &rules_definition_sha256.to_ascii_lowercase());
    Ok(digest.finish_hex())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CuratedPreviewError {
    EmptyTopics,
    TooManyTopics,
    UnknownTopic(String),
    IncompatiblePolicy,
    InvalidLimit,
    ExceedsPolicy,
    InvalidDefinitionDigest,
    WrongProject { expected: String, actual: String },
    Preview(JoinedPreviewError),
}

/// Optional caller-selected bounds for curated previews. Omitting a field
/// uses the curated seed's conservative default; widening is rejected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CuratedLimits {
    pub depth_limit: Option<usize>,
    pub page_limit: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct CuratedTopicSelection {
    /// Canonical sorted, deduplicated curated IDs.
    pub topic_ids: Vec<String>,
    /// Domain-separated digest of curated IDs, editable seed definitions,
    /// and effective selection rules. This is not a publisher signature.
    pub curated_definition_sha256: String,
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
    preview_curated_topics_with_limits(
        records,
        administrative,
        topic_ids,
        redirect_identities,
        include,
        exclude,
        CuratedLimits::default(),
    )
}

/// Preview curated topics with optional caller-selected tighter traversal
/// limits, matching the CLI's curated safety policy. None uses the seed
/// default; a depth of zero selects only root memberships. Page limits must
/// be positive. This is still an unauthenticated source-local preview.
pub fn preview_curated_topics_with_limits(
    records: &[JoinedEnterpriseArticle],
    administrative: &BTreeSet<String>,
    topic_ids: &[&str],
    redirect_identities: &[PageRedirectIdentity],
    include: &[u64],
    exclude: &[u64],
    limits: CuratedLimits,
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
    if limits.page_limit == Some(0) {
        return Err(CuratedPreviewError::InvalidLimit);
    }
    if limits.depth_limit.is_some_and(|limit| limit > first.depth_limit)
        || limits.page_limit.is_some_and(|limit| limit > first.page_limit)
    {
        return Err(CuratedPreviewError::ExceedsPolicy);
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
        depth_limit: limits.depth_limit.unwrap_or(first.depth_limit),
        page_limit: limits.page_limit.unwrap_or(first.page_limit),
    };
    let preview = preview_joined_selection(records, administrative, &rules, redirect_identities)
        .map_err(CuratedPreviewError::Preview)?;
    // The shared receipt is also emitted by the local preview CLI.
    let curated_definition_sha256 =
        curated_definition_fingerprint(topic_ids, &preview.preview.selection.definition_sha256)?;
    Ok(CuratedTopicSelection {
        topic_ids: ids.into_iter().map(str::to_owned).collect(),
        curated_definition_sha256,
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
        let first =
            preview_curated_topics(&records, &admin, &["physics", "mathematics"], &[], &[], &[])
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
            first.curated_definition_sha256,
            reordered.curated_definition_sha256
        );
        assert_eq!(first.curated_definition_sha256.len(), 64);
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
            reordered
                .preview
                .preview
                .selection
                .snapshot_selection_sha256
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
        assert_ne!(
            single.curated_definition_sha256,
            selected.curated_definition_sha256
        );
        let unchanged = preview_curated_topics(
            &records,
            &BTreeSet::new(),
            &["mathematics", "physics"],
            &[],
            &[],
            &[],
        )
        .unwrap();
        assert_ne!(
            unchanged.curated_definition_sha256,
            selected.curated_definition_sha256
        );
    }

    #[test]
    fn curated_library_tighter_limits_match_cli_policy_and_receipts() {
        let records = fixture();
        let admin = BTreeSet::new();
        let defaults =
            preview_curated_topics(&records, &admin, &["physics", "mathematics"], &[], &[], &[])
                .unwrap();
        let explicit_defaults = preview_curated_topics_with_limits(
            &records,
            &admin,
            &["mathematics", "physics"],
            &[],
            &[],
            &[],
            CuratedLimits::default(),
        )
        .unwrap();
        assert_eq!(
            defaults.curated_definition_sha256,
            explicit_defaults.curated_definition_sha256
        );
        let limited = preview_curated_topics_with_limits(
            &records,
            &admin,
            &["mathematics", "physics"],
            &[],
            &[],
            &[],
            CuratedLimits {
                depth_limit: Some(0),
                page_limit: Some(2),
            },
        )
        .unwrap();
        assert_eq!(limited.rules.depth_limit, 0);
        assert_eq!(limited.rules.page_limit, 2);
        assert_eq!(limited.preview.preview.selection.page_ids, vec![1, 2]);
        assert_ne!(
            defaults.curated_definition_sha256,
            limited.curated_definition_sha256
        );
        let reordered = preview_curated_topics_with_limits(
            &records,
            &admin,
            &["physics", "mathematics", "physics"],
            &[],
            &[],
            &[],
            CuratedLimits {
                depth_limit: Some(0),
                page_limit: Some(2),
            },
        )
        .unwrap();
        assert_eq!(
            limited.curated_definition_sha256,
            reordered.curated_definition_sha256
        );
    }

    #[test]
    fn curated_library_rejects_zero_or_widened_limits() {
        let records = fixture();
        let admin = BTreeSet::new();
        for (depth, pages, expected) in [
            (None, Some(0), CuratedPreviewError::InvalidLimit),
            (Some(3), None, CuratedPreviewError::ExceedsPolicy),
            (None, Some(50_001), CuratedPreviewError::ExceedsPolicy),
        ] {
            assert_eq!(
                preview_curated_topics_with_limits(
                    &records,
                    &admin,
                    &["mathematics"],
                    &[],
                    &[],
                    &[],
                    CuratedLimits {
                        depth_limit: depth,
                        page_limit: pages,
                    },
                )
                .unwrap_err(),
                expected
            );
        }
        let too_small = preview_curated_topics_with_limits(
            &records,
            &admin,
            &["mathematics", "physics"],
            &[],
            &[],
            &[],
            CuratedLimits {
                depth_limit: None,
                page_limit: Some(1),
            },
        );
        assert!(matches!(too_small, Err(CuratedPreviewError::Preview(_))));
    }

    #[test]
    fn public_fingerprint_rejects_invalid_rule_digests() {
        assert_eq!(
            curated_definition_fingerprint(&["mathematics"], "not-a-digest"),
            Err(CuratedPreviewError::InvalidDefinitionDigest)
        );
        assert_eq!(
            curated_definition_fingerprint(&["mathematics"], &"g".repeat(64)),
            Err(CuratedPreviewError::InvalidDefinitionDigest)
        );
        let digest = "a".repeat(64);
        assert_eq!(
            curated_definition_fingerprint(&["physics", "mathematics"], &digest),
            curated_definition_fingerprint(&["mathematics", "physics", "physics"], &digest)
        );
        assert_ne!(
            curated_definition_fingerprint(&["mathematics"], &digest),
            curated_definition_fingerprint(&["physics"], &digest)
        );
    }

    #[test]
    fn fingerprint_canonicalizes_rule_digest_hex_case() {
        let lower = "a1b2c3d4e5f60789".repeat(4);
        let upper = lower.to_ascii_uppercase();
        let mixed = "A1b2C3d4E5f60789".repeat(4);
        let ids = ["physics", "mathematics"];
        let expected = curated_definition_fingerprint(&ids, &lower).unwrap();
        assert_eq!(
            curated_definition_fingerprint(&ids, &upper),
            Ok(expected.clone())
        );
        assert_eq!(curated_definition_fingerprint(&ids, &mixed), Ok(expected));
        assert_ne!(
            curated_definition_fingerprint(&ids, &lower),
            curated_definition_fingerprint(&ids, &"0".repeat(64))
        );
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
