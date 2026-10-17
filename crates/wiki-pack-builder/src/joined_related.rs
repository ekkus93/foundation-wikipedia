//! Revision-bound optional related-link expansion for a joined topic preview.
//!
//! All inputs are caller-supplied. Matching project/generation/revision identities
//! prevent local mixing, but do NOT authenticate Wikimedia or prove completeness.
use std::collections::{BTreeMap, BTreeSet};

use wiki_source::enterprise::JoinedEnterpriseArticle;

use crate::enterprise_graph::{PageRedirectIdentity, RevisionPinnedSelection};
use crate::joined_preview::{preview_joined_selection, JoinedPreviewError, JoinedSelectionPreview};
use crate::related::expand_related_links;
use crate::selection::{ResolveError, Rules};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelatedLimits {
    pub max_hops: usize,
    pub max_new_pages: usize,
    pub max_edge_visits: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedLinkIdentity {
    pub project: String,
    pub generation_id: String,
    pub from_page_id: u64,
    pub to_page_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinedRelatedError {
    Joined(JoinedPreviewError),
    MixedSource,
    InvalidPage,
    NotMainspace(u64),
    InconsistentBase(u64),
    Budget(ResolveError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedRevisionReceipt {
    /// Selected canonical mainspace IDs, including bounded related additions.
    pub page_revisions: BTreeMap<u64, u64>,
    pub related_added_page_count: usize,
    /// Domain-separated receipt of base revisions, limits, link claims and result.
    pub related_selection_sha256: String,
    /// Local revision matching is NOT official publisher authentication.
    pub related_source_authenticated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinedRelatedPreview {
    pub base: JoinedSelectionPreview,
    pub related: RelatedRevisionReceipt,
}

fn append_text(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

/// Fail closed on links to unknown, non-mainspace or differently sourced pages,
/// including unreachable edges. Do not silently treat redirect alias IDs as
/// revision-backed articles. The caller must resolve aliases before ingestion.
fn expand_verified_links(
    base: &RevisionPinnedSelection,
    mainspace: &BTreeMap<u64, u64>,
    links: &[RelatedLinkIdentity],
    excluded: &BTreeSet<u64>,
    limits: RelatedLimits,
    page_limit: usize,
) -> Result<RelatedRevisionReceipt, JoinedRelatedError> {
    for (&id, &revision) in &base.page_revisions {
        if mainspace.get(&id) != Some(&revision) {
            return Err(JoinedRelatedError::InconsistentBase(id));
        }
    }
    let mut adjacency: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut canonical_edges = Vec::with_capacity(links.len());
    for link in links {
        if link.project != base.project || link.generation_id != base.generation_id {
            return Err(JoinedRelatedError::MixedSource);
        }
        if link.from_page_id == 0 || link.to_page_id == 0 {
            return Err(JoinedRelatedError::InvalidPage);
        }
        for id in [link.from_page_id, link.to_page_id] {
            if !mainspace.contains_key(&id) {
                return Err(JoinedRelatedError::NotMainspace(id));
            }
        }
        adjacency
            .entry(link.from_page_id)
            .or_default()
            .push(link.to_page_id);
        canonical_edges.push((link.from_page_id, link.to_page_id));
    }
    let expanded = expand_related_links(
        &base.selection.page_ids,
        &adjacency,
        excluded,
        limits.max_hops,
        limits.max_new_pages,
        limits.max_edge_visits,
    )
    .map_err(JoinedRelatedError::Budget)?;
    if expanded.len() > page_limit {
        return Err(JoinedRelatedError::Budget(ResolveError::TooManyPages));
    }
    let mut page_revisions = BTreeMap::new();
    for id in expanded {
        let revision = mainspace
            .get(&id)
            .ok_or(JoinedRelatedError::NotMainspace(id))?;
        page_revisions.insert(id, *revision);
    }
    canonical_edges.sort_unstable();
    canonical_edges.dedup();
    let mut bytes = b"foundation-wikipedia/joined-related-revisions/v1\0".to_vec();
    append_text(&mut bytes, &base.project);
    append_text(&mut bytes, &base.generation_id);
    append_text(&mut bytes, &base.revision_selection_sha256);
    for value in [
        limits.max_hops,
        limits.max_new_pages,
        limits.max_edge_visits,
        page_limit,
    ] {
        bytes.extend_from_slice(&(value as u64).to_be_bytes());
    }
    bytes.extend_from_slice(&(canonical_edges.len() as u64).to_be_bytes());
    for (from, to) in canonical_edges {
        bytes.extend_from_slice(&from.to_be_bytes());
        bytes.extend_from_slice(&to.to_be_bytes());
    }
    bytes.extend_from_slice(&(page_revisions.len() as u64).to_be_bytes());
    for (&id, &revision) in &page_revisions {
        bytes.extend_from_slice(&id.to_be_bytes());
        bytes.extend_from_slice(&revision.to_be_bytes());
    }
    Ok(RelatedRevisionReceipt {
        related_added_page_count: page_revisions.len() - base.page_revisions.len(),
        page_revisions,
        related_selection_sha256: wiki_pack_format::sha256_hex(&bytes),
        related_source_authenticated: false,
    })
}

/// Preview a revision-joined topic with bounded related mainspace links.
/// Neither matching source labels nor a digest establish publisher authenticity.
/// The result is diagnostic only, not an installable or verified pack manifest.
pub fn preview_joined_selection_with_related(
    records: &[JoinedEnterpriseArticle],
    administrative: &BTreeSet<String>,
    rules: &Rules,
    redirect_identities: &[PageRedirectIdentity],
    links: &[RelatedLinkIdentity],
    limits: RelatedLimits,
) -> Result<JoinedRelatedPreview, JoinedRelatedError> {
    let base = preview_joined_selection(records, administrative, rules, redirect_identities)
        .map_err(JoinedRelatedError::Joined)?;
    let mainspace: BTreeMap<u64, u64> = records
        .iter()
        .filter(|record| record.structured.namespace == 0)
        .map(|record| {
            (
                record.structured.page.page_id,
                record.structured.revision_id,
            )
        })
        .collect();
    // The base joined resolver has already validated all redirect identities,
    // including cycles and exact-generation ownership.
    let redirects: BTreeMap<u64, u64> = redirect_identities
        .iter()
        .map(|identity| (identity.from_page_id, identity.to_page_id))
        .collect();
    let mut excluded = BTreeSet::new();
    for &id in &rules.exclude {
        let mut canonical = id;
        let mut hops = 0usize;
        while let Some(&next) = redirects.get(&canonical) {
            canonical = next;
            hops += 1;
            if hops > 4096 {
                return Err(JoinedRelatedError::Budget(ResolveError::RedirectTooDeep));
            }
        }
        excluded.insert(canonical);
    }
    let related = expand_verified_links(
        &base.revision_pinned,
        &mainspace,
        links,
        &excluded,
        limits,
        rules.page_limit,
    )?;
    Ok(JoinedRelatedPreview { base, related })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::PinnedResolution;

    fn fixture() -> (RevisionPinnedSelection, BTreeMap<u64, u64>, RelatedLimits) {
        let base = RevisionPinnedSelection {
            project: "enwiki".into(),
            generation_id: "20261016".into(),
            selection: PinnedResolution {
                snapshot_id: "enwiki-20261016".into(),
                definition_sha256: "definition".into(),
                graph_sha256: "graph".into(),
                snapshot_selection_sha256: "base-selection".into(),
                page_ids: vec![1],
                warnings: vec![],
            },
            page_revisions: BTreeMap::from([(1, 11)]),
            revision_selection_sha256: "base-revisions".into(),
            unresolved_redirect_titles: BTreeSet::new(),
        };
        let mainspace = BTreeMap::from([(1, 11), (2, 22), (3, 33), (4, 44)]);
        let limits = RelatedLimits {
            max_hops: 2,
            max_new_pages: 3,
            max_edge_visits: 20,
        };
        (base, mainspace, limits)
    }

    fn link(from: u64, to: u64) -> RelatedLinkIdentity {
        RelatedLinkIdentity {
            project: "enwiki".into(),
            generation_id: "20261016".into(),
            from_page_id: from,
            to_page_id: to,
        }
    }

    #[test]
    fn revision_receipt_is_deterministic_and_binds_new_revisions() {
        let (base, mut mainspace, limits) = fixture();
        let links = vec![link(1, 3), link(1, 2), link(2, 4), link(3, 1)];
        let first =
            expand_verified_links(&base, &mainspace, &links, &BTreeSet::new(), limits, 4).unwrap();
        assert_eq!(first.page_revisions, mainspace);
        assert_eq!(first.related_added_page_count, 3);
        assert!(!first.related_source_authenticated);
        let mut reordered = links.clone();
        reordered.reverse();
        assert_eq!(
            first,
            expand_verified_links(&base, &mainspace, &reordered, &BTreeSet::new(), limits, 4)
                .unwrap()
        );
        mainspace.insert(4, 45);
        let changed =
            expand_verified_links(&base, &mainspace, &links, &BTreeSet::new(), limits, 4).unwrap();
        assert_ne!(first.related_selection_sha256, changed.related_selection_sha256);
        assert_eq!(
            expand_verified_links(&base, &mainspace, &links, &BTreeSet::new(), limits, 3),
            Err(JoinedRelatedError::Budget(ResolveError::TooManyPages))
        );
    }

    #[test]
    fn excludes_and_budget_fail_closed() {
        let (base, mainspace, limits) = fixture();
        let links = vec![link(1, 2), link(2, 3), link(3, 4)];
        let result =
            expand_verified_links(&base, &mainspace, &links, &BTreeSet::from([2]), limits, 4)
                .unwrap();
        assert_eq!(result.page_revisions, BTreeMap::from([(1, 11)]));
        assert_eq!(
            expand_verified_links(
                &base,
                &mainspace,
                &links,
                &BTreeSet::new(),
                RelatedLimits {
                    max_edge_visits: 0,
                    ..limits
                },
                4
            ),
            Err(JoinedRelatedError::Budget(ResolveError::BadLimit))
        );
        assert_eq!(
            expand_verified_links(
                &base,
                &mainspace,
                &links,
                &BTreeSet::new(),
                RelatedLimits {
                    max_new_pages: 1,
                    ..limits
                },
                4
            ),
            Err(JoinedRelatedError::Budget(ResolveError::TooManyPages))
        );
    }

    #[test]
    fn reject_unreachable_foreign_or_unknown_links_and_stale_base() {
        let (mut base, mainspace, limits) = fixture();
        let check = |links: &[RelatedLinkIdentity], base: &RevisionPinnedSelection| {
            expand_verified_links(base, &mainspace, links, &BTreeSet::new(), limits, 4)
        };
        assert_eq!(
            check(&[link(4, 99)], &base),
            Err(JoinedRelatedError::NotMainspace(99))
        );
        assert_eq!(
            check(&[link(0, 2)], &base),
            Err(JoinedRelatedError::InvalidPage)
        );
        let mut foreign = link(4, 2);
        foreign.generation_id = "other".into();
        assert_eq!(
            check(&[foreign], &base),
            Err(JoinedRelatedError::MixedSource)
        );
        base.page_revisions.insert(1, 999);
        assert_eq!(
            check(&[], &base),
            Err(JoinedRelatedError::InconsistentBase(1))
        );
    }
}
