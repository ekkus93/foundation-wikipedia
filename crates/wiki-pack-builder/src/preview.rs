//! Deterministic pre-download preview of a snapshot-pinned category selection.
//!
//! Counts describe the supplied graph, **not** an authenticated or complete
//! Wikimedia snapshot. No media download or pack install is performed here.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::selection::{resolve_pinned, Category, PinnedResolution, ResolveError, Rules};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryFanout {
    pub name: String,
    /// Raw category article mentions, before redirect and exclusion handling.
    pub article_mentions: usize,
    pub child_categories: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionPreview {
    pub selection: PinnedResolution,
    pub selected_page_count: usize,
    pub visited_category_count: usize,
    pub skipped_administrative_count: usize,
    pub inspected_article_mentions: usize,
    pub explicit_include_ids: usize,
    pub explicit_exclude_ids: usize,
    pub largest_category: Option<CategoryFanout>,
    /// Original resolver warnings plus deterministic high-fanout signals.
    pub warnings: Vec<String>,
}

/// Preview exactly the selection `resolve_pinned` would return. Additional
/// diagnostics do not alter or independently authenticate its selection hash.
pub fn preview_pinned_selection(
    graph: &BTreeMap<String, Category>,
    rules: &Rules,
    redirects: &BTreeMap<u64, u64>,
    disambiguations: &BTreeSet<u64>,
    snapshot_id: &str,
) -> Result<SelectionPreview, ResolveError> {
    let selection = resolve_pinned(graph, rules, redirects, disambiguations, snapshot_id)?;
    let mut visited = BTreeSet::new();
    let mut queue: VecDeque<_> = rules.roots.iter().map(|name| (name.clone(), 0)).collect();
    let mut skipped_administrative_count = 0usize;
    let mut inspected_article_mentions = 0usize;
    let mut largest_category: Option<CategoryFanout> = None;
    let mut warnings: BTreeSet<String> = selection.warnings.iter().cloned().collect();

    while let Some((name, depth)) = queue.pop_front() {
        if !visited.insert(name.clone()) {
            continue;
        }
        // Successful resolution has already validated every reached category
        // and enforced the category/candidate budgets.
        let node = graph
            .get(&name)
            .ok_or_else(|| ResolveError::Missing(name.clone()))?;
        if node.administrative {
            skipped_administrative_count += 1;
            continue;
        }
        inspected_article_mentions = inspected_article_mentions.saturating_add(node.articles.len());
        let fanout = CategoryFanout {
            name: name.clone(),
            article_mentions: node.articles.len(),
            child_categories: node.children.len(),
        };
        if largest_category.as_ref().is_none_or(|current| {
            fanout.article_mentions > current.article_mentions
                || (fanout.article_mentions == current.article_mentions
                    && fanout.name < current.name)
        }) {
            largest_category = Some(fanout);
        }
        if depth < rules.depth_limit {
            queue.extend(node.children.iter().map(|child| (child.clone(), depth + 1)));
        }
    }
    let selected_page_count = selection.page_ids.len();
    if let Some(ref fanout) = largest_category {
        if fanout.article_mentions >= 512 && fanout.article_mentions >= rules.page_limit.div_ceil(2)
        {
            warnings.insert(format!(
                "Review broad category: {} has {} raw article mentions (page limit {})",
                fanout.name, fanout.article_mentions, rules.page_limit
            ));
        }
    }
    if selected_page_count >= 100
        && selected_page_count >= rules.page_limit.saturating_sub(rules.page_limit / 10)
    {
        warnings.insert(format!(
            "Selection near page limit: {selected_page_count}/{}",
            rules.page_limit
        ));
    }
    Ok(SelectionPreview {
        selection,
        selected_page_count,
        visited_category_count: visited.len(),
        skipped_administrative_count,
        inspected_article_mentions,
        explicit_include_ids: rules.include.iter().copied().collect::<BTreeSet<_>>().len(),
        explicit_exclude_ids: rules.exclude.iter().copied().collect::<BTreeSet<_>>().len(),
        largest_category,
        warnings: warnings.into_iter().collect(),
    })
}
