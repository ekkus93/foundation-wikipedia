//! Strict category membership graph from already revision-joined Enterprise
//! article records. A joined batch is NOT proof of a complete or authenticated
//! Wikimedia snapshot; missing category pages are rejected, not invented.
use std::collections::{BTreeMap, BTreeSet};

use wiki_pack_format::Sha256Transcript;
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
    InvalidRedirect(String),
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

// Typed JoinedEnterpriseArticle records are public DTOs: callers can bypass
// the upstream JSON parser. Reapply its bounded, anti-spoof membership rules
// at the topic-selection trust boundary before hashing or traversing labels.
fn valid_membership_label(value: &str) -> bool {
    !value.trim().is_empty()
        && value.trim() == value
        && value.len() <= 4096
        && !value.chars().any(|ch| {
            ch.is_control()
                || matches!(
                    ch,
                    '\u{061c}'
                        | '\u{200e}'..='\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2066}'..='\u{2069}'
                )
        })
}

fn valid_generation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('.')
        && !value.ends_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn category_title(raw: &str) -> Result<String, GraphBuildError> {
    if !valid_membership_label(raw) || raw.contains('#') {
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
    if (wiki_model::ArticleKey {
        project: project.clone(),
        page_id: 1,
    })
    .validate()
    .is_err()
        || !valid_generation_id(&generation_id)
    {
        return Err(GraphBuildError::InvalidIdentity);
    }
    // Administrative skip policy must use the same canonical category
    // keys as the graph. A bare name would silently fail to mark a category.
    for category in administrative {
        if !category.starts_with("Category:") || category_title(category)? != *category {
            return Err(GraphBuildError::InvalidCategory(category.clone()));
        }
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
        for redirect in &record.redirects {
            if !valid_membership_label(redirect) {
                return Err(GraphBuildError::InvalidRedirect(redirect.clone()));
            }
            unresolved_redirect_titles.insert(redirect.clone());
        }
        // Validate even membership labels on pages that will not contribute
        // edges, so forged typed inputs cannot evade validation by namespace.
        for category in &record.categories {
            category_title(category)?;
        }
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

fn append_text(digest: &mut Sha256Transcript, value: &str) {
    digest.update(&(value.len() as u64).to_be_bytes());
    digest.update(value.as_bytes());
}

/// Stream the original v2 revision receipt encoding without retaining a
/// potentially full-snapshot-sized byte transcript in memory.
fn revision_receipt_sha256(
    project: &str,
    generation_id: &str,
    selection_sha256: &str,
    joined_page_revisions: &BTreeMap<u64, u64>,
    unresolved_redirect_titles: &BTreeSet<String>,
    page_revisions: &BTreeMap<u64, u64>,
) -> String {
    let mut digest = Sha256Transcript::new();
    digest.update(b"foundation-wikipedia/revision-pinned-selection/v2\0");
    append_text(&mut digest, project);
    append_text(&mut digest, generation_id);
    append_text(&mut digest, selection_sha256);
    // Include every joined page identity, including category revisions.
    digest.update(&(joined_page_revisions.len() as u64).to_be_bytes());
    for (id, revision) in joined_page_revisions {
        digest.update(&id.to_be_bytes());
        digest.update(&revision.to_be_bytes());
    }
    digest.update(&(unresolved_redirect_titles.len() as u64).to_be_bytes());
    for title in unresolved_redirect_titles {
        append_text(&mut digest, title);
    }
    digest.update(&(page_revisions.len() as u64).to_be_bytes());
    for (id, revision) in page_revisions {
        digest.update(&id.to_be_bytes());
        digest.update(&revision.to_be_bytes());
    }
    digest.finish_hex()
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
    let revision_selection_sha256 = revision_receipt_sha256(
        &joined.project,
        &joined.generation_id,
        &selection.snapshot_selection_sha256,
        &joined.page_revisions,
        &joined.unresolved_redirect_titles,
        &page_revisions,
    );
    Ok(RevisionPinnedSelection {
        project: joined.project,
        generation_id: joined.generation_id,
        selection,
        page_revisions,
        revision_selection_sha256,
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

#[cfg(test)]
#[path = "enterprise_graph_hardening_tests.rs"]
mod hardening_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_revision_receipt_matches_legacy_v2_bytes() {
        let all = BTreeMap::from([(1, 11), (90, 99), (123, 456)]);
        let aliases = BTreeSet::from(["Alias".to_owned(), "Café".to_owned()]);
        let selected = BTreeMap::from([(1, 11), (123, 456)]);
        let actual = revision_receipt_sha256(
            "enwiki",
            "20261016",
            "base-selection",
            &all,
            &aliases,
            &selected,
        );

        let mut legacy = b"foundation-wikipedia/revision-pinned-selection/v2\0".to_vec();
        for value in ["enwiki", "20261016", "base-selection"] {
            legacy.extend_from_slice(&(value.len() as u64).to_be_bytes());
            legacy.extend_from_slice(value.as_bytes());
        }
        legacy.extend_from_slice(&(all.len() as u64).to_be_bytes());
        for (id, revision) in &all {
            legacy.extend_from_slice(&id.to_be_bytes());
            legacy.extend_from_slice(&revision.to_be_bytes());
        }
        legacy.extend_from_slice(&(aliases.len() as u64).to_be_bytes());
        for title in &aliases {
            legacy.extend_from_slice(&(title.len() as u64).to_be_bytes());
            legacy.extend_from_slice(title.as_bytes());
        }
        legacy.extend_from_slice(&(selected.len() as u64).to_be_bytes());
        for (id, revision) in &selected {
            legacy.extend_from_slice(&id.to_be_bytes());
            legacy.extend_from_slice(&revision.to_be_bytes());
        }
        assert_eq!(actual, wiki_pack_format::sha256_hex(&legacy));
    }
}
