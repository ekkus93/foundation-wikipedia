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
    RevisionMismatch(PageKey),
    GenerationMismatch(PageKey),
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
            || item.revision_id == 0
            || item.page.project.is_empty()
            || !item
                .page
                .project
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || item.generation_id.trim().is_empty()
            || item.generation_id != item.generation_id.trim()
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
    let mut batch_generation: Option<&str> = None;
    for (key, source) in s {
        // A batch must not silently combine independently valid generations.
        if batch_generation.is_some_and(|expected| expected != source.generation_id) {
            return Err(JoinError::GenerationMismatch(key));
        }
        batch_generation = Some(&source.generation_id);
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
    for key in deleted {
        if key.page_id == 0
            || key.project.is_empty()
            || !key
                .project
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(JoinError::InvalidIdentity);
        }
        if !tombstones.insert(key.clone()) {
            return Err(JoinError::DuplicateTombstone(key));
        }
    }
    for item in structured.iter().chain(&rendered).chain(&categories) {
        if tombstones.contains(&item.page) {
            return Err(JoinError::DeletedPageConflict(item.page.clone()));
        }
    }
    join_pages(structured, rendered, categories)
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
            revision_id: 17,
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
}
