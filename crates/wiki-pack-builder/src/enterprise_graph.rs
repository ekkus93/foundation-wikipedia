//! Strict category membership graph from already revision-joined Enterprise
//! article records. A joined batch is NOT proof of a complete or authenticated
//! Wikimedia snapshot; missing category pages are rejected, not invented.
use std::collections::{BTreeMap, BTreeSet};

use wiki_source::enterprise::JoinedEnterpriseArticle;

use crate::selection::Category;

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
    if raw.is_empty()
        || raw.trim() != raw
        || raw.chars().any(char::is_control)
        || raw.contains('#')
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
        if page.page.page_id == 0 || page.revision_id == 0 {
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
