//! Strict category membership graph from already revision-joined Enterprise
//! article records. A joined batch is NOT proof of a complete or authenticated
//! Wikimedia snapshot; missing category pages are rejected, not invented.
use std::collections::{BTreeMap, BTreeSet};

use wiki_source::enterprise::JoinedEnterpriseArticle;

use crate::selection::{resolve_pinned, Category, PinnedResolution, ResolveError, Rules};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphBuildError {
    Empty,
    InvalidIdentity,
    MixedProject,
    MixedGeneration,
    DuplicatePage(u64),
    DuplicateCategory(String),
    InvalidCategory(String),
    MissingCategory(String),
}

#[derive(Clone, Debug)]
pub struct JoinedCategoryGraph {
    pub project: String,
    pub generation_id: String,
    pub graph: BTreeMap<String, Category>,
    pub page_revisions: BTreeMap<u64, u64>,
    /// Redirect title aliases cannot be converted to page-ID redirects
    /// without a separate exact-generation page identity mapping.
    pub unresolved_redirect_titles: BTreeSet<String>,
}

fn category_title(raw: &str) -> Result<String, GraphBuildError> {
    if raw.is_empty() || raw.trim() != raw || raw.chars().any(char::is_control) || raw.contains('#')
    {
        return Err(GraphBuildError::InvalidCategory(raw.to_owned()));
    }
    let name = raw.strip_prefix("Category:").unwrap_or(raw);
    if name.is_empty() {
        return Err(GraphBuildError::InvalidCategory(raw.to_owned()));
    }
    Ok(format!("Category:{name}"))
}

/// Require all referenced category pages in the supplied joined records.
/// This is an intentionally strict whole-graph constructor, not a streaming
/// chunk importer. Only mainspace articles and category pages contribute
/// membership. Administrative classifications must come from caller policy.
pub fn graph_from_joined(
    records: &[JoinedEnterpriseArticle],
    administrative: &BTreeSet<String>,
) -> Result<JoinedCategoryGraph, GraphBuildError> {
    let first = records.first().ok_or(GraphBuildError::Empty)?;
    let project = first.structured.page.project.clone();
    let generation_id = first.structured.generation_id.clone();
    if project.is_empty() || generation_id.is_empty() {
        return Err(GraphBuildError::InvalidIdentity);
    }
    let mut graph = BTreeMap::new();
    let mut page_revisions = BTreeMap::new();
    let mut unresolved_redirect_titles = BTreeSet::new();
    for record in records {
        let page = &record.structured;
        if page.page.project != project {
            return Err(GraphBuildError::MixedProject);
        }
        if page.generation_id != generation_id {
            return Err(GraphBuildError::MixedGeneration);
        }
        if page.page.page_id == 0
            || page.revision_id == 0
            || page.page.page_id > i64::MAX as u64
            || page.revision_id > i64::MAX as u64
        {
            return Err(GraphBuildError::InvalidIdentity);
        }
        if page_revisions
            .insert(page.page.page_id, page.revision_id)
            .is_some()
        {
            return Err(GraphBuildError::DuplicatePage(page.page.page_id));
        }
        unresolved_redirect_titles.extend(record.redirects.iter().cloned());
        if page.namespace == 14 {
            let title = category_title(&page.name)?;
            let node = Category {
                articles: Vec::new(),
                children: Vec::new(),
                administrative: administrative.contains(&title),
            };
            if graph.insert(title.clone(), node).is_some() {
                return Err(GraphBuildError::DuplicateCategory(title));
            }
        }
    }
    for record in records {
        let page = &record.structured;
        if page.namespace != 0 && page.namespace != 14 {
            continue;
        }
        for category in &record.categories {
            let parent = category_title(category)?;
            let node = graph
                .get_mut(&parent)
                .ok_or_else(|| GraphBuildError::MissingCategory(parent.clone()))?;
            if page.namespace == 0 {
                node.articles.push(page.page.page_id);
            } else {
                node.children.push(category_title(&page.name)?);
            }
        }
    }
    for node in graph.values_mut() {
        node.articles.sort_unstable();
        node.articles.dedup();
        node.children.sort();
        node.children.dedup();
    }
    Ok(JoinedCategoryGraph {
        project,
        generation_id,
        graph,
        page_revisions,
        unresolved_redirect_titles,
    })
}

/// The caller-supplied joined batch is not evidence of an authenticated or
/// complete Wikimedia source snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinedSelectionError {
    Graph(GraphBuildError),
    Resolution(ResolveError),
    NotMainspace(u64),
    Redirect(RedirectBuildError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRedirectIdentity {
    pub project: String,
    pub generation_id: String,
    pub from_page_id: u64,
    pub to_page_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RedirectBuildError {
    InvalidId,
    MixedSource,
    DuplicateSource(u64),
    ConflictsWithJoinedPage(u64),
    MissingMainspaceTarget(u64),
    Cycle(u64),
    TooDeep,
    TooMany,
}

/// Deterministic selection bound to exact selected article revisions, not just
/// the page-ID list. Unresolved redirect names are intentionally not IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionPinnedSelection {
    pub project: String,
    pub generation_id: String,
    pub selection: PinnedResolution,
    pub page_revisions: BTreeMap<u64, u64>,
    pub revision_selection_sha256: String,
    pub unresolved_redirect_titles: BTreeSet<String>,
}

fn append_text(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
}

/// Resolve against an already revision-joined batch, refusing explicit and
/// category-derived IDs which cannot be found as real mainspace article
/// records in the supplied batch. Title aliases await an authenticated
/// page-ID redirect join instead of being guessed into redirect identities.
///
/// The returned digest detects supplied-input drift but does NOT authenticate
/// a Wikimedia publication or prove full snapshot coverage.
pub fn resolve_joined_revision_pinned(
    records: &[JoinedEnterpriseArticle],
    administrative: &BTreeSet<String>,
    rules: &Rules,
) -> Result<RevisionPinnedSelection, JoinedSelectionError> {
    resolve_joined_revision_pinned_with_redirects(records, administrative, rules, &[])
}

/// Resolve redirects only from caller-supplied page-ID identities matching the
/// joined source generation. The caller must separately authenticate provenance.
pub fn resolve_joined_revision_pinned_with_redirects(
    records: &[JoinedEnterpriseArticle],
    administrative: &BTreeSet<String>,
    rules: &Rules,
    redirect_identities: &[PageRedirectIdentity],
) -> Result<RevisionPinnedSelection, JoinedSelectionError> {
    let joined = graph_from_joined(records, administrative).map_err(JoinedSelectionError::Graph)?;
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
    let redirects = validate_redirect_identities(&joined, &mainspace, redirect_identities)
        .map_err(JoinedSelectionError::Redirect)?;
    for id in rules.include.iter().chain(&rules.exclude) {
        if !mainspace.contains_key(id) && !redirects.contains_key(id) {
            return Err(JoinedSelectionError::NotMainspace(*id));
        }
    }
    let snapshot_id = format!("{}-{}", joined.project, joined.generation_id);
    let selection = resolve_pinned(
        &joined.graph,
        rules,
        &redirects,
        &BTreeSet::new(),
        &snapshot_id,
    )
    .map_err(JoinedSelectionError::Resolution)?;

    let mut page_revisions = BTreeMap::new();
    for id in &selection.page_ids {
        let revision = mainspace
            .get(id)
            .ok_or(JoinedSelectionError::NotMainspace(*id))?;
        page_revisions.insert(*id, *revision);
    }
    let mut bytes = b"foundation-wikipedia/revision-pinned-selection/v2\0".to_vec();
    append_text(&mut bytes, &joined.project);
    append_text(&mut bytes, &joined.generation_id);
    append_text(&mut bytes, &selection.snapshot_selection_sha256);
    // Bind the complete supplied joined page identity map, including category
    // source revisions, even when the selected page IDs do not change.
    bytes.extend_from_slice(&(joined.page_revisions.len() as u64).to_be_bytes());
    for (id, revision) in &joined.page_revisions {
        bytes.extend_from_slice(&id.to_be_bytes());
        bytes.extend_from_slice(&revision.to_be_bytes());
    }
    // Title aliases must not be mistaken for page-ID redirects, but drift in
    // the supplied alias inventory must still invalidate the local receipt.
    bytes.extend_from_slice(&(joined.unresolved_redirect_titles.len() as u64).to_be_bytes());
    for title in &joined.unresolved_redirect_titles {
        append_text(&mut bytes, title);
    }
    bytes.extend_from_slice(&(page_revisions.len() as u64).to_be_bytes());
    for (id, revision) in &page_revisions {
        bytes.extend_from_slice(&id.to_be_bytes());
        bytes.extend_from_slice(&revision.to_be_bytes());
    }
    Ok(RevisionPinnedSelection {
        project: joined.project,
        generation_id: joined.generation_id,
        selection,
        page_revisions,
        revision_selection_sha256: wiki_pack_format::sha256_hex(&bytes),
        unresolved_redirect_titles: joined.unresolved_redirect_titles,
    })
}

const MAX_REDIRECT_IDENTITIES: usize = 1_000_000;

/// Validate all redirects, not only the ones reached by this topic.
/// Unresolved redirect sources may be absent from the joined article records,
/// but every final target must be a real joined namespace-0 article.
fn validate_redirect_identities(
    joined: &JoinedCategoryGraph,
    mainspace: &BTreeMap<u64, u64>,
    identities: &[PageRedirectIdentity],
) -> Result<BTreeMap<u64, u64>, RedirectBuildError> {
    if identities.len() > MAX_REDIRECT_IDENTITIES {
        return Err(RedirectBuildError::TooMany);
    }
    let mut redirects = BTreeMap::new();
    for identity in identities {
        if identity.project != joined.project || identity.generation_id != joined.generation_id {
            return Err(RedirectBuildError::MixedSource);
        }
        if identity.from_page_id == 0
            || identity.to_page_id == 0
            || identity.from_page_id > i64::MAX as u64
            || identity.to_page_id > i64::MAX as u64
        {
            return Err(RedirectBuildError::InvalidId);
        }
        if joined.page_revisions.contains_key(&identity.from_page_id) {
            return Err(RedirectBuildError::ConflictsWithJoinedPage(
                identity.from_page_id,
            ));
        }
        if redirects
            .insert(identity.from_page_id, identity.to_page_id)
            .is_some()
        {
            return Err(RedirectBuildError::DuplicateSource(identity.from_page_id));
        }
    }
    // Memoize (terminal mainspace page, hop count). Each edge is traversed
    // once, even when many redirect sources share a long chain suffix.
    let mut resolved: BTreeMap<u64, (u64, usize)> = BTreeMap::new();
    for &start in redirects.keys() {
        if resolved.contains_key(&start) {
            continue;
        }
        let mut path = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = start;
        let (terminal, mut depth) = loop {
            if let Some(&(target, hops)) = resolved.get(&current) {
                break (target, hops);
            }
            let Some(&next) = redirects.get(&current) else {
                if !mainspace.contains_key(&current) {
                    return Err(RedirectBuildError::MissingMainspaceTarget(current));
                }
                break (current, 0);
            };
            if !seen.insert(current) {
                return Err(RedirectBuildError::Cycle(current));
            }
            path.push(current);
            if path.len() > 4096 {
                return Err(RedirectBuildError::TooDeep);
            }
            current = next;
        };
        if path.len().saturating_add(depth) > 4096 {
            return Err(RedirectBuildError::TooDeep);
        }
        for from in path.into_iter().rev() {
            depth += 1;
            resolved.insert(from, (terminal, depth));
        }
    }
    Ok(redirects)
}
