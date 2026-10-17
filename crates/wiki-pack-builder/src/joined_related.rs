//! Revision-bound optional related-link expansion for a joined topic preview.
//!
//! All inputs are caller-supplied. Matching project/generation/revision identities
//! prevent local mixing, but do NOT authenticate Wikimedia or prove completeness.
use std::collections::{BTreeMap, BTreeSet};

use wiki_pack_format::Sha256Transcript;
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
    /// Revision of the source article from which this edge was extracted.
    pub from_revision_id: u64,
    pub to_page_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinedRelatedError {
    Joined(JoinedPreviewError),
    MixedSource,
    InvalidPage,
    NotMainspace(u64),
    StaleLinkRevision(u64),
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

fn append_text(digest: &mut Sha256Transcript, value: &str) {
    digest.update(&(value.len() as u64).to_be_bytes());
    digest.update(value.as_bytes());
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
    // Bound even unreachable caller-supplied link records before allocating the
    // canonical edge inventory; traversal limits alone do not bound this input.
    if links.len() > 1_000_000 {
        return Err(JoinedRelatedError::Budget(ResolveError::TooManyCandidates));
    }
    let mut adjacency: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut canonical_edges = Vec::with_capacity(links.len());
    for link in links {
        if link.project != base.project || link.generation_id != base.generation_id {
            return Err(JoinedRelatedError::MixedSource);
        }
        if link.from_page_id == 0 || link.from_revision_id == 0 || link.to_page_id == 0 {
            return Err(JoinedRelatedError::InvalidPage);
        }
        for id in [link.from_page_id, link.to_page_id] {
            if !mainspace.contains_key(&id) {
                return Err(JoinedRelatedError::NotMainspace(id));
            }
        }
        if mainspace.get(&link.from_page_id) != Some(&link.from_revision_id) {
            return Err(JoinedRelatedError::StaleLinkRevision(link.from_page_id));
        }
        adjacency
            .entry(link.from_page_id)
            .or_default()
            .push(link.to_page_id);
        canonical_edges.push((link.from_page_id, link.from_revision_id, link.to_page_id));
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
    let mut digest = Sha256Transcript::new();
    digest.update(b"foundation-wikipedia/joined-related-revisions/v2\0");
    append_text(&mut digest, &base.project);
    append_text(&mut digest, &base.generation_id);
    append_text(&mut digest, &base.revision_selection_sha256);
    for value in [
        limits.max_hops,
        limits.max_new_pages,
        limits.max_edge_visits,
        page_limit,
    ] {
        digest.update(&(value as u64).to_be_bytes());
    }
    digest.update(&(canonical_edges.len() as u64).to_be_bytes());
    for (from, revision, to) in canonical_edges {
        digest.update(&from.to_be_bytes());
        digest.update(&revision.to_be_bytes());
        digest.update(&to.to_be_bytes());
    }
    digest.update(&(page_revisions.len() as u64).to_be_bytes());
    for (&id, &revision) in &page_revisions {
        digest.update(&id.to_be_bytes());
        digest.update(&revision.to_be_bytes());
    }
    Ok(RelatedRevisionReceipt {
        related_added_page_count: page_revisions.len() - base.page_revisions.len(),
        page_revisions,
        related_selection_sha256: digest.finish_hex(),
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
    use crate::enterprise_graph::{
        graph_from_joined, resolve_joined_revision_pinned_with_redirects, GraphBuildError,
        JoinedSelectionError, RedirectBuildError,
    };
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
            from_revision_id: from * 11,
            to_page_id: to,
        }
    }

    fn joined_link(from: u64, to: u64) -> RelatedLinkIdentity {
        let mut result = link(from, to);
        result.from_revision_id = from + 100;
        result
    }

    #[test]
    fn revision_receipt_is_deterministic_and_binds_new_revisions() {
        let (base, mut mainspace, limits) = fixture();
        let links = vec![link(1, 3), link(1, 2), link(2, 4), link(3, 1)];
        let first =
            expand_verified_links(&base, &mainspace, &links, &BTreeSet::new(), limits, 4).unwrap();
        assert_eq!(first.page_revisions, mainspace);
        assert_eq!(first.related_added_page_count, 3);
        assert_eq!(
            first.related_selection_sha256,
            "c5394be12b03f06e18b06b40dd7d47d25ccc3f960e3a765a254efc2fbacb83bc"
        );
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
        assert_ne!(
            first.related_selection_sha256,
            changed.related_selection_sha256
        );
        assert_eq!(
            expand_verified_links(&base, &mainspace, &links, &BTreeSet::new(), limits, 3),
            Err(JoinedRelatedError::Budget(ResolveError::TooManyPages))
        );
    }

    #[test]
    fn receipt_binds_unselected_link_source_revisions() {
        let (base, mut mainspace, limits) = fixture();
        // The edge source is not reachable from the seed, so neither the
        // selected page IDs nor their revisions change.
        let mut links = vec![link(4, 2)];
        let first =
            expand_verified_links(&base, &mainspace, &links, &BTreeSet::new(), limits, 4).unwrap();
        assert_eq!(first.page_revisions, base.page_revisions);
        mainspace.insert(4, 45);
        links[0].from_revision_id = 45;
        let changed =
            expand_verified_links(&base, &mainspace, &links, &BTreeSet::new(), limits, 4).unwrap();
        assert_eq!(changed.page_revisions, first.page_revisions);
        assert_ne!(
            changed.related_selection_sha256,
            first.related_selection_sha256
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
        let mut stale = link(1, 2);
        stale.from_revision_id = 999;
        assert_eq!(
            check(&[stale], &base),
            Err(JoinedRelatedError::StaleLinkRevision(1))
        );
        base.page_revisions.insert(1, 999);
        assert_eq!(
            check(&[], &base),
            Err(JoinedRelatedError::InconsistentBase(1))
        );
    }

    fn joined_record(
        id: u64,
        namespace: i32,
        name: &str,
        categories: &[&str],
    ) -> JoinedEnterpriseArticle {
        use wiki_source::enterprise::StructuredArticle;
        use wiki_source::join::PageKey;
        JoinedEnterpriseArticle {
            structured: StructuredArticle {
                page: PageKey {
                    project: "enwiki".into(),
                    page_id: id,
                },
                revision_id: id + 100,
                date_modified: "2026-10-16T00:00:00Z".into(),
                generation_id: "20261016".into(),
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
            rendered_html: String::new(),
            categories: categories.iter().map(|s| (*s).to_string()).collect(),
            redirects: vec![],
        }
    }

    #[test]
    fn joined_graph_and_redirects_reject_out_of_range_signed_ids() {
        let records = vec![
            joined_record(90, 14, "Category:Physics", &[]),
            joined_record(1, 0, "Seed", &["Category:Physics"]),
        ];
        let mut oversized_page = records.clone();
        oversized_page[1].structured.page.page_id = i64::MAX as u64 + 1;
        assert_eq!(
            graph_from_joined(&oversized_page, &BTreeSet::new()).err(),
            Some(GraphBuildError::InvalidIdentity)
        );

        let mut oversized_revision = records.clone();
        oversized_revision[1].structured.revision_id = i64::MAX as u64 + 1;
        assert_eq!(
            graph_from_joined(&oversized_revision, &BTreeSet::new()).err(),
            Some(GraphBuildError::InvalidIdentity)
        );

        let rules = Rules {
            roots: vec!["Category:Physics".into()],
            include: vec![],
            exclude: vec![],
            depth_limit: 0,
            page_limit: 2,
        };
        for (from, to) in [(i64::MAX as u64 + 1, 1), (2, i64::MAX as u64 + 1)] {
            let redirect = PageRedirectIdentity {
                project: "enwiki".into(),
                generation_id: "20261016".into(),
                from_page_id: from,
                to_page_id: to,
            };
            assert!(matches!(
                resolve_joined_revision_pinned_with_redirects(
                    &records,
                    &BTreeSet::new(),
                    &rules,
                    &[redirect]
                ),
                Err(JoinedSelectionError::Redirect(
                    RedirectBuildError::InvalidId
                ))
            ));
        }
    }

    #[test]
    fn public_joined_api_enforces_revisions_exclusions_and_total_page_budget() {
        let records = vec![
            joined_record(90, 14, "Category:Physics", &[]),
            joined_record(1, 0, "Seed", &["Category:Physics"]),
            joined_record(2, 0, "Related", &[]),
            joined_record(3, 0, "Further", &[]),
        ];
        let mut rules = Rules {
            roots: vec!["Category:Physics".into()],
            include: vec![],
            exclude: vec![],
            depth_limit: 0,
            page_limit: 3,
        };
        let limits = RelatedLimits {
            max_hops: 2,
            max_new_pages: 2,
            max_edge_visits: 4,
        };
        let links = vec![joined_link(2, 3), joined_link(1, 2)];
        let run = |rules: &Rules, links: &[RelatedLinkIdentity]| {
            preview_joined_selection_with_related(
                &records,
                &BTreeSet::new(),
                rules,
                &[],
                links,
                limits,
            )
        };
        let result = run(&rules, &links).unwrap();
        assert_eq!(result.base.revision_pinned.selection.page_ids, vec![1]);
        assert_eq!(
            result.related.page_revisions,
            BTreeMap::from([(1, 101), (2, 102), (3, 103)])
        );
        assert_eq!(result.related.related_added_page_count, 2);

        assert!(!result.related.related_source_authenticated);
        let mut reversed = links.clone();
        reversed.reverse();
        assert_eq!(result.related, run(&rules, &reversed).unwrap().related);
        rules.page_limit = 2;
        assert_eq!(
            run(&rules, &links),
            Err(JoinedRelatedError::Budget(ResolveError::TooManyPages))
        );
        rules.page_limit = 3;
        rules.exclude.push(2);
        let excluded = run(&rules, &links).unwrap();
        assert_eq!(excluded.related.page_revisions, BTreeMap::from([(1, 101)]));
        let mut stale = joined_link(1, 2);
        stale.from_revision_id = 999;
        assert_eq!(
            run(&rules, &[stale]),
            Err(JoinedRelatedError::StaleLinkRevision(1))
        );
        assert_eq!(
            run(&rules, &[joined_link(1, 90)]),
            Err(JoinedRelatedError::NotMainspace(90))
        );
    }
}
