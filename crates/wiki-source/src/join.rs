//! Revision-scoped join guard for independently acquired Wikimedia components.
//! This does not parse upstream dumps or sanitize rendered HTML.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PageKey {
    pub project: String,
    pub page_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub page: PageKey,
    pub revision_id: u64,
    pub generation_id: String,
    pub payload: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinedPage {
    pub page: PageKey,
    pub revision_id: u64,
    pub generation_id: String,
    pub structured: String,
    pub rendered_html: String,
    pub categories: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinError {
    InvalidIdentity,
    EmptyPayload,
    Duplicate(PageKey),
    DuplicateRevision(u64),
    RevisionMismatch(PageKey),
    GenerationMismatch(PageKey),
    MixedProject(PageKey),
    MissingComponent(PageKey),
    DeletedPageConflict(PageKey),
    DuplicateTombstone(PageKey),
}

fn collect(
    items: Vec<Component>,
    allow_empty_payload: bool,
) -> Result<BTreeMap<PageKey, Component>, JoinError> {
    let mut result = BTreeMap::new();
    for item in items {
        if item.page.page_id == 0
            || item.page.page_id > i64::MAX as u64
            || item.revision_id == 0
            || item.revision_id > i64::MAX as u64
            || !crate::valid_wiki_project(&item.page.project)
            || !crate::enterprise_ndjson::valid_generation_id(&item.generation_id)
        {
            return Err(JoinError::InvalidIdentity);
        }
        if !allow_empty_payload && item.payload.trim().is_empty() {
            return Err(JoinError::EmptyPayload);
        }
        if result.insert(item.page.clone(), item.clone()).is_some() {
            return Err(JoinError::Duplicate(item.page));
        }
    }
    Ok(result)
}

/// Require every page to have matching exact-revision structured, HTML and
/// category records. Never silently join by title or use partial data.
/// An explicit empty category payload is valid; a missing record is not.
pub fn join_pages(
    structured: Vec<Component>,
    rendered: Vec<Component>,
    categories: Vec<Component>,
) -> Result<Vec<JoinedPage>, JoinError> {
    let s = collect(structured, false)?;
    let mut r = collect(rendered, false)?;
    let mut c = collect(categories, true)?;
    let mut joined = Vec::new();
    let mut batch_generation: Option<String> = None;
    let mut batch_project: Option<String> = None;
    let mut seen_revisions = BTreeSet::new();
    for (key, source) in s {
        if batch_project
            .as_deref()
            .is_some_and(|expected| expected != key.project)
        {
            return Err(JoinError::MixedProject(key));
        }
        batch_project = Some(key.project.clone());
        // A batch must not silently combine independently valid generations.
        if batch_generation
            .as_deref()
            .is_some_and(|expected| expected != source.generation_id)
        {
            return Err(JoinError::GenerationMismatch(key));
        }
        batch_generation = Some(source.generation_id.clone());
        if !seen_revisions.insert(source.revision_id) {
            return Err(JoinError::DuplicateRevision(source.revision_id));
        }
        let html = r
            .remove(&key)
            .ok_or_else(|| JoinError::MissingComponent(key.clone()))?;
        let cats = c
            .remove(&key)
            .ok_or_else(|| JoinError::MissingComponent(key.clone()))?;
        for part in [&html, &cats] {
            if part.revision_id != source.revision_id {
                return Err(JoinError::RevisionMismatch(key));
            }
            if part.generation_id != source.generation_id {
                return Err(JoinError::GenerationMismatch(key));
            }
        }
        joined.push(JoinedPage {
            page: key,
            revision_id: source.revision_id,
            generation_id: source.generation_id,
            structured: source.payload,
            rendered_html: html.payload,
            categories: cats.payload,
        });
    }
    if let Some((key, _)) = r.into_iter().next().or_else(|| c.into_iter().next()) {
        return Err(JoinError::MissingComponent(key));
    }
    Ok(joined)
}

/// Filter exact-snapshot deletion markers before a live component join.
/// A deleted page cannot retain a live body, HTML or category record.
pub fn join_pages_with_tombstones(
    structured: Vec<Component>,
    rendered: Vec<Component>,
    categories: Vec<Component>,
    deleted: Vec<PageKey>,
) -> Result<Vec<JoinedPage>, JoinError> {
    let mut tombstones = BTreeSet::new();
    let mut project: Option<String> = None;
    for key in deleted {
        if key.page_id == 0
            || key.page_id > i64::MAX as u64
            || !crate::valid_wiki_project(&key.project)
        {
            return Err(JoinError::InvalidIdentity);
        }
        if project.as_deref().is_some_and(|name| name != key.project) {
            return Err(JoinError::MixedProject(key));
        }
        project = Some(key.project.clone());
        if !tombstones.insert(key.clone()) {
            return Err(JoinError::DuplicateTombstone(key));
        }
    }
    for item in structured.iter().chain(&rendered).chain(&categories) {
        if project
            .as_deref()
            .is_some_and(|name| name != item.page.project)
        {
            return Err(JoinError::MixedProject(item.page.clone()));
        }
        if tombstones.contains(&item.page) {
            return Err(JoinError::DeletedPageConflict(item.page.clone()));
        }
    }
    join_pages(structured, rendered, categories)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeletionMarker {
    pub page: PageKey,
    pub generation_id: String,
}

/// Join revision-matched components with deletion markers from the SAME source
/// generation. Prefer this over the legacy PageKey-only tombstone API when
/// deleted-page evidence is used to qualify a canonical snapshot.
///
/// A tombstone without a source generation cannot prove same-snapshot deletion.
pub fn join_pages_with_generation_tombstones(
    structured: Vec<Component>,
    rendered: Vec<Component>,
    categories: Vec<Component>,
    deleted: Vec<DeletionMarker>,
) -> Result<Vec<JoinedPage>, JoinError> {
    let joined = join_pages(structured, rendered, categories)?;
    let live_pages: BTreeSet<_> = joined.iter().map(|page| page.page.clone()).collect();
    let mut project = joined.first().map(|page| page.page.project.clone());
    let mut generation = joined.first().map(|page| page.generation_id.clone());
    let mut deleted_pages = BTreeSet::new();
    for marker in deleted {
        let key = marker.page;
        let id = marker.generation_id;
        if key.page_id == 0
            || key.page_id > i64::MAX as u64
            || !crate::valid_wiki_project(&key.project)
            || !crate::enterprise_ndjson::valid_generation_id(&id)
        {
            return Err(JoinError::InvalidIdentity);
        }
        if project
            .as_deref()
            .is_some_and(|expected| expected != key.project)
        {
            return Err(JoinError::MixedProject(key));
        }
        if generation.as_deref().is_some_and(|expected| expected != id) {
            return Err(JoinError::GenerationMismatch(key));
        }
        project = Some(key.project.clone());
        generation = Some(id);
        if !deleted_pages.insert(key.clone()) {
            return Err(JoinError::DuplicateTombstone(key));
        }
        if live_pages.contains(&key) {
            return Err(JoinError::DeletedPageConflict(key));
        }
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(page_id: u64, payload: &str) -> Component {
        Component {
            page: PageKey {
                project: "enwiki".into(),
                page_id,
            },
            revision_id: 17 + page_id,
            generation_id: "20261009".into(),
            payload: payload.into(),
        }
    }

    #[test]
    fn joins_sorted_and_preserves_exact_provenance() {
        let result = join_pages(
            vec![part(2, "s2"), part(1, "s1")],
            vec![part(1, "h1"), part(2, "h2")],
            vec![part(2, ""), part(1, "c1")],
        )
        .unwrap();
        assert_eq!(
            result.iter().map(|p| p.page.page_id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(result[0].rendered_html, "h1");
        assert!(result[1].categories.is_empty());
    }

    #[test]
    fn rejects_one_revision_identity_on_two_distinct_pages() {
        let first = part(1, "s1");
        let mut second = part(2, "s2");
        second.revision_id = first.revision_id;
        let mut html_second = second.clone();
        html_second.payload = "h2".into();
        let mut cats_second = second.clone();
        cats_second.payload = "c2".into();
        assert_eq!(
            join_pages(
                vec![first.clone(), second],
                vec![part(1, "h1"), html_second],
                vec![part(1, "c1"), cats_second]
            ),
            Err(JoinError::DuplicateRevision(first.revision_id))
        );
    }

    #[test]
    fn rejects_unsafe_join_generation_labels_before_provenance_linkage() {
        for invalid in [
            "",
            " ",
            ".snapshot",
            "snapshot.",
            "../outside",
            "2026/10/01",
            "2026\\stage",
            "2026\n10",
            "2026\0export",
        ] {
            let mut structured = part(1, "structured");
            structured.generation_id = invalid.into();
            let mut html = structured.clone();
            html.payload = "rendered".into();
            let mut categories = structured.clone();
            categories.payload = "".into();
            assert_eq!(
                join_pages(vec![structured], vec![html], vec![categories]),
                Err(JoinError::InvalidIdentity),
                "accepted generation label {invalid:?}"
            );
        }
        let mut valid = part(1, "s");
        valid.generation_id = "2026-10-01T00_00_00Z".into();
        let mut html = valid.clone();
        html.payload = "h".into();
        let mut categories = valid.clone();
        categories.payload = "".into();
        assert_eq!(
            join_pages(vec![valid], vec![html], vec![categories])
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn shared_publisher_identity_rules_cover_live_components_and_tombstones() {
        for project in ["_enwiki", "enwiki_", "123wiki", "Enwiki", "___"] {
            let mut live = part(1, "structured");
            live.page.project = project.into();
            let mut html = live.clone();
            html.payload = "html".into();
            let mut categories = live.clone();
            categories.payload.clear();
            assert_eq!(
                join_pages(vec![live.clone()], vec![html], vec![categories]),
                Err(JoinError::InvalidIdentity),
            );
            assert_eq!(
                join_pages_with_tombstones(vec![], vec![], vec![], vec![live.page.clone()]),
                Err(JoinError::InvalidIdentity),
            );
            assert_eq!(
                join_pages_with_generation_tombstones(
                    vec![], vec![], vec![],
                    vec![DeletionMarker { page: live.page, generation_id: "20261009".into() }],
                ),
                Err(JoinError::InvalidIdentity),
            );
        }
        for generation in ["", ".hidden", "name.", "bad/name", "bad name"] {
            let mut live = part(1, "structured");
            live.generation_id = generation.into();
            let mut html = live.clone();
            html.payload = "html".into();
            let mut categories = live.clone();
            categories.payload.clear();
            assert_eq!(
                join_pages(vec![live], vec![html], vec![categories]),
                Err(JoinError::InvalidIdentity),
            );
            assert_eq!(
                join_pages_with_generation_tombstones(
                    vec![], vec![], vec![],
                    vec![DeletionMarker { page: part(1, "deleted").page, generation_id: generation.into() }],
                ),
                Err(JoinError::InvalidIdentity),
            );
        }
        let mut source = part(1, "structured");
        source.page.project = "zh_min_nanwiki".into();
        source.generation_id = "2026-10-01T00_00_00Z".into();
        let mut html = source.clone();
        html.payload = "html".into();
        let mut categories = source.clone();
        categories.payload.clear();
        assert_eq!(join_pages(vec![source], vec![html], vec![categories]).unwrap().len(), 1);
    }

    #[test]
    fn rejects_locally_matching_pages_from_different_generations() {
        let first = part(1, "s1");
        let mut second = part(2, "s2");
        second.generation_id = "20261010".into();
        let mut html = second.clone();
        html.payload = "h2".into();
        let mut categories = second.clone();
        categories.payload = "c2".into();
        assert_eq!(
            join_pages(
                vec![first, second.clone()],
                vec![part(1, "h1"), html],
                vec![part(1, "c1"), categories]
            ),
            Err(JoinError::GenerationMismatch(second.page))
        );
    }

    #[test]
    fn rejects_locally_matching_pages_from_different_projects() {
        let first = part(1, "s1");
        let mut second = part(2, "s2");
        second.page.project = "frwiki".into();
        let mut html = second.clone();
        html.payload = "h2".into();
        let mut categories = second.clone();
        categories.payload = "c2".into();
        assert_eq!(
            join_pages(
                vec![first, second.clone()],
                vec![part(1, "h1"), html],
                vec![part(1, "c1"), categories]
            ),
            Err(JoinError::MixedProject(second.page))
        );
    }

    #[test]
    fn rejects_mismatched_revision_and_generation() {
        let mut html = part(1, "html");
        html.revision_id += 1;
        assert_eq!(
            join_pages(vec![part(1, "s")], vec![html.clone()], vec![part(1, "c")]),
            Err(JoinError::RevisionMismatch(html.page.clone()))
        );
        html.revision_id -= 1;
        html.generation_id = "different".into();
        assert_eq!(
            join_pages(vec![part(1, "s")], vec![html.clone()], vec![part(1, "c")]),
            Err(JoinError::GenerationMismatch(html.page))
        );
    }

    #[test]
    fn rejects_missing_or_duplicate_components() {
        assert_eq!(
            join_pages(vec![part(1, "s")], vec![part(1, "h")], vec![]),
            Err(JoinError::MissingComponent(part(1, "s").page))
        );
        assert_eq!(
            join_pages(vec![part(1, "s"), part(1, "s")], vec![], vec![]),
            Err(JoinError::Duplicate(part(1, "s").page))
        );
    }

    #[test]
    fn deleted_page_markers_cannot_coexist_with_live_components() {
        let page = part(1, "s").page;
        assert_eq!(
            join_pages_with_tombstones(
                vec![part(1, "s")],
                vec![part(1, "h")],
                vec![part(1, "c")],
                vec![page.clone()]
            ),
            Err(JoinError::DeletedPageConflict(page.clone()))
        );
        assert!(
            join_pages_with_tombstones(vec![], vec![], vec![], vec![page.clone()])
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            join_pages_with_tombstones(vec![], vec![], vec![], vec![page.clone(), page.clone()]),
            Err(JoinError::DuplicateTombstone(page))
        );
    }

    #[test]
    fn rejects_overlong_project_identity_in_live_and_deleted_pages() {
        let mut part = part(1, "structured");
        part.page.project = "a".repeat(256);
        assert_eq!(
            join_pages(vec![part.clone()], vec![], vec![]),
            Err(JoinError::InvalidIdentity)
        );
        assert_eq!(
            join_pages_with_tombstones(vec![], vec![], vec![], vec![part.page]),
            Err(JoinError::InvalidIdentity)
        );
    }

    #[test]
    fn rejects_foreign_project_tombstones() {
        let foreign = PageKey {
            project: "frwiki".into(),
            page_id: 9,
        };
        let local = part(1, "s").page;
        assert_eq!(
            join_pages_with_tombstones(
                vec![part(1, "s")],
                vec![part(1, "h")],
                vec![part(1, "c")],
                vec![foreign.clone()]
            ),
            Err(JoinError::MixedProject(local.clone()))
        );
        assert_eq!(
            join_pages_with_tombstones(vec![], vec![], vec![], vec![local, foreign.clone()]),
            Err(JoinError::MixedProject(foreign))
        );
    }

    #[test]
    fn rejects_unmatched_html_or_empty_evidence() {
        assert_eq!(
            join_pages(vec![], vec![part(1, "html")], vec![]),
            Err(JoinError::MissingComponent(part(1, "html").page))
        );
        assert_eq!(
            join_pages(vec![part(1, "")], vec![], vec![]),
            Err(JoinError::EmptyPayload)
        );
    }

    #[test]
    fn generation_scoped_tombstones_reject_stale_and_foreign_deletions() {
        let live = part(1, "structured");
        let html = part(1, "rendered");
        let categories = part(1, "");
        let local = DeletionMarker {
            page: part(2, "deleted").page,
            generation_id: "20261009".into(),
        };
        let mut stale = local.clone();
        stale.generation_id = "20261008".into();
        assert_eq!(
            join_pages_with_generation_tombstones(
                vec![live.clone()],
                vec![html.clone()],
                vec![categories.clone()],
                vec![stale.clone()],
            ),
            Err(JoinError::GenerationMismatch(stale.page))
        );
        let mut foreign = local.clone();
        foreign.page.project = "frwiki".into();
        assert_eq!(
            join_pages_with_generation_tombstones(
                vec![live.clone()],
                vec![html.clone()],
                vec![categories.clone()],
                vec![foreign.clone()],
            ),
            Err(JoinError::MixedProject(foreign.page))
        );
        assert_eq!(
            join_pages_with_generation_tombstones(
                vec![live.clone()],
                vec![html.clone()],
                vec![categories.clone()],
                vec![local.clone(), local.clone()],
            ),
            Err(JoinError::DuplicateTombstone(local.page.clone()))
        );
        let conflict = DeletionMarker {
            page: live.page.clone(),
            generation_id: live.generation_id.clone(),
        };
        assert_eq!(
            join_pages_with_generation_tombstones(
                vec![live.clone()],
                vec![html.clone()],
                vec![categories.clone()],
                vec![conflict.clone()],
            ),
            Err(JoinError::DeletedPageConflict(conflict.page))
        );
        assert_eq!(
            join_pages_with_generation_tombstones(
                vec![live],
                vec![html],
                vec![categories],
                vec![local],
            )
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn tombstone_only_batches_are_generation_and_project_scoped() {
        let first = DeletionMarker {
            page: part(10, "deleted").page,
            generation_id: "20261009".into(),
        };
        let mut stale = first.clone();
        stale.page.page_id = 11;
        stale.generation_id = "20261010".into();
        assert_eq!(
            join_pages_with_generation_tombstones(
                vec![],
                vec![],
                vec![],
                vec![first.clone(), stale.clone()],
            ),
            Err(JoinError::GenerationMismatch(stale.page))
        );
        assert!(
            join_pages_with_generation_tombstones(vec![], vec![], vec![], vec![first],)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_out_of_range_page_and_revision_ids_in_all_join_paths() {
        // The canonical article/catalog model stores identities in SQLite
        // signed 64-bit columns. An unchecked u64 must never be joined as
        // apparently valid provenance and later overflow during persistence.
        let overflow = i64::MAX as u64 + 1;
        for mutate_page in [true, false] {
            let mut source = part(1, "structured");
            if mutate_page {
                source.page.page_id = overflow;
            } else {
                source.revision_id = overflow;
            }
            let mut html = source.clone();
            html.payload = "rendered".into();
            let mut categories = source.clone();
            categories.payload.clear();
            assert_eq!(
                join_pages(vec![source], vec![html], vec![categories]),
                Err(JoinError::InvalidIdentity)
            );
        }

        let invalid = PageKey {
            project: "enwiki".into(),
            page_id: overflow,
        };
        assert_eq!(
            join_pages_with_tombstones(vec![], vec![], vec![], vec![invalid.clone()]),
            Err(JoinError::InvalidIdentity)
        );
        assert_eq!(
            join_pages_with_generation_tombstones(
                vec![],
                vec![],
                vec![],
                vec![DeletionMarker {
                    page: invalid,
                    generation_id: "20261009".into(),
                }]
            ),
            Err(JoinError::InvalidIdentity)
        );
    }
}
