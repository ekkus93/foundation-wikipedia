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
    /// Raw child-category edges eligible for traversal, including duplicates.
    pub inspected_child_category_mentions: usize,
    pub explicit_include_ids: usize,
    pub explicit_exclude_ids: usize,
    pub largest_category: Option<CategoryFanout>,
    /// Original resolver warnings plus deterministic high-fanout signals.
    pub warnings: Vec<String>,
}

/// Report up to eight deterministic back edges in categories actually expanded.
/// Iterative DFS avoids recursion over untrusted graphs; shared DAG descendants
/// are not incorrectly reported as cycles. The boolean flags further cycles.
fn traversed_cycles(edges: &BTreeMap<String, BTreeSet<String>>) -> (Vec<(String, String)>, bool) {
    const MAX_CYCLE_WARNINGS: usize = 8;
    let mut active = BTreeSet::new();
    let mut finished = BTreeSet::new();
    let mut cycles = Vec::new();
    for root in edges.keys() {
        if finished.contains(root) {
            continue;
        }
        active.insert(root.clone());
        let mut stack = vec![(root, edges.get(root).expect("root comes from keys").iter())];
        while let Some((parent, children)) = stack.last_mut() {
            if let Some(child) = children.next() {
                if !edges.contains_key(child) {
                    continue;
                }
                if active.contains(child) {
                    if cycles.len() == MAX_CYCLE_WARNINGS {
                        return (cycles, true);
                    }
                    cycles.push(((*parent).clone(), child.clone()));
                } else if !finished.contains(child) {
                    active.insert(child.clone());
                    stack.push((child, edges.get(child).expect("child was checked").iter()));
                }
            } else {
                active.remove(*parent);
                finished.insert((*parent).clone());
                stack.pop();
            }
        }
    }
    (cycles, false)
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
    let mut traversed_edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut queue: VecDeque<_> = rules.roots.iter().map(|name| (name.clone(), 0)).collect();
    let mut skipped_administrative_count = 0usize;
    let mut inspected_article_mentions = 0usize;
    let mut inspected_child_category_mentions = 0usize;
    let mut largest_category: Option<CategoryFanout> = None;
    let mut warnings: BTreeSet<String> = selection.warnings.iter().cloned().collect();
    for id in &rules.include {
        let mut canonical = *id;
        while let Some(&target) = redirects.get(&canonical) {
            canonical = target;
        }
        if selection.page_ids.binary_search(&canonical).is_err() {
            warnings.insert(format!(
                "Explicit include suppressed by exclusion: {id} -> {canonical}"
            ));
        }
    }

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
            traversed_edges.insert(name.clone(), node.children.iter().cloned().collect());
            inspected_child_category_mentions =
                inspected_child_category_mentions.saturating_add(node.children.len());
            if node.children.len() >= 128 {
                warnings.insert(format!(
                    "Review broad subcategory expansion: {name} has {} child-category mentions",
                    node.children.len()
                ));
            }
            queue.extend(node.children.iter().map(|child| (child.clone(), depth + 1)));
        }
    }
    let (cycles, more_cycles) = traversed_cycles(&traversed_edges);
    for (from, to) in cycles {
        warnings.insert(format!("Category cycle detected: {from} -> {to}"));
    }
    if more_cycles {
        warnings.insert("Additional category cycles omitted after 8 warnings".into());
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
    if selected_page_count == rules.page_limit
        || (selected_page_count >= 10
            && selected_page_count >= rules.page_limit.saturating_sub(rules.page_limit / 10))
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
        inspected_child_category_mentions,
        explicit_include_ids: rules.include.iter().copied().collect::<BTreeSet<_>>().len(),
        explicit_exclude_ids: rules.exclude.iter().copied().collect::<BTreeSet<_>>().len(),
        largest_category,
        warnings: warnings.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_counts_only_traversed_content_and_deduplicates_rules() {
        let graph = BTreeMap::from([
            (
                "Root".into(),
                Category {
                    articles: vec![1, 2],
                    children: vec!["Admin".into(), "Child".into()],
                    administrative: false,
                },
            ),
            (
                "Admin".into(),
                Category {
                    articles: vec![999],
                    children: vec![],
                    administrative: true,
                },
            ),
            (
                "Child".into(),
                Category {
                    articles: vec![3],
                    children: vec!["Root".into()],
                    administrative: false,
                },
            ),
        ]);
        let rules = Rules {
            roots: vec!["Root".into(), "Root".into()],
            include: vec![4, 4],
            exclude: vec![2, 2],
            depth_limit: 3,
            page_limit: 10,
        };
        let preview = preview_pinned_selection(
            &graph,
            &rules,
            &BTreeMap::new(),
            &BTreeSet::new(),
            "snapshot-1",
        )
        .unwrap();
        assert_eq!(vec![1, 3, 4], preview.selection.page_ids);
        assert_eq!(3, preview.selected_page_count);
        assert_eq!(3, preview.visited_category_count);
        assert_eq!(1, preview.skipped_administrative_count);
        assert_eq!(3, preview.inspected_article_mentions);
        assert_eq!(3, preview.inspected_child_category_mentions);
        assert_eq!(1, preview.explicit_include_ids);
        assert_eq!(1, preview.explicit_exclude_ids);
        assert_eq!(
            Some(CategoryFanout {
                name: "Root".into(),
                article_mentions: 2,
                child_categories: 2,
            }),
            preview.largest_category
        );
        assert!(preview
            .warnings
            .iter()
            .any(|w| w.contains("Skipped administrative")));
    }

    #[test]
    fn reports_multiple_disjoint_cycles_and_bounds_diagnostics() {
        let edges = BTreeMap::from([
            ("A".into(), BTreeSet::from(["B".into()])),
            ("B".into(), BTreeSet::from(["A".into()])),
            ("C".into(), BTreeSet::from(["D".into()])),
            ("D".into(), BTreeSet::from(["C".into()])),
        ]);
        let (cycles, truncated) = traversed_cycles(&edges);
        assert_eq!(cycles, vec![("B".into(), "A".into()), ("D".into(), "C".into())]);
        assert!(!truncated);

        let mut many = BTreeMap::new();
        for i in 0..10 {
            let name = format!("Loop{i:02}");
            many.insert(name.clone(), BTreeSet::from([name]));
        }
        let (cycles, truncated) = traversed_cycles(&many);
        assert_eq!(cycles.len(), 8);
        assert!(truncated);
    }

    #[test]
    fn preview_reports_real_cycles_without_mislabeling_shared_dag_descendants() {
        let mut graph = BTreeMap::from([
            (
                "Root".into(),
                Category {
                    articles: vec![1],
                    children: vec!["A".into(), "B".into()],
                    administrative: false,
                },
            ),
            (
                "A".into(),
                Category {
                    articles: vec![],
                    children: vec!["Shared".into()],
                    administrative: false,
                },
            ),
            (
                "B".into(),
                Category {
                    articles: vec![],
                    children: vec!["Shared".into()],
                    administrative: false,
                },
            ),
            (
                "Shared".into(),
                Category {
                    articles: vec![2],
                    children: vec![],
                    administrative: false,
                },
            ),
        ]);
        let mut rules = Rules {
            roots: vec!["Root".into()],
            include: vec![],
            exclude: vec![],
            depth_limit: 4,
            page_limit: 10,
        };
        let inspect = |graph: &BTreeMap<String, Category>, rules: &Rules| {
            preview_pinned_selection(
                graph,
                rules,
                &BTreeMap::new(),
                &BTreeSet::new(),
                "snapshot-1",
            )
            .unwrap()
        };
        assert!(!inspect(&graph, &rules)
            .warnings
            .iter()
            .any(|warning| warning.starts_with("Category cycle detected:")));
        graph
            .get_mut("Shared")
            .unwrap()
            .children
            .push("Root".into());
        let cyclic = inspect(&graph, &rules);
        assert_eq!(cyclic.selection.page_ids, vec![1, 2]);
        assert!(cyclic
            .warnings
            .iter()
            .any(|warning| warning.starts_with("Category cycle detected:")));
        rules.depth_limit = 2;
        assert!(!inspect(&graph, &rules)
            .warnings
            .iter()
            .any(|warning| warning.starts_with("Category cycle detected:")));
    }

    #[test]
    fn preview_warns_near_limit_without_changing_pinned_selection() {
        let graph = BTreeMap::from([(
            "Root".into(),
            Category {
                articles: (1..=100).collect(),
                children: vec![],
                administrative: false,
            },
        )]);
        let rules = Rules {
            roots: vec!["Root".into()],
            include: vec![],
            exclude: vec![],
            depth_limit: 1,
            page_limit: 100,
        };
        let preview = preview_pinned_selection(
            &graph,
            &rules,
            &BTreeMap::new(),
            &BTreeSet::new(),
            "snapshot-2",
        )
        .unwrap();
        assert_eq!(100, preview.selected_page_count);
        assert!(preview
            .warnings
            .iter()
            .any(|w| w.contains("Selection near page limit")));
        assert_eq!(
            preview.selection,
            resolve_pinned(
                &graph,
                &rules,
                &BTreeMap::new(),
                &BTreeSet::new(),
                "snapshot-2",
            )
            .unwrap()
        );
    }
    #[test]
    fn small_topic_at_page_limit_warns_without_changing_selection() {
        let graph = BTreeMap::from([(
            "Root".into(),
            Category {
                articles: (1..=10).collect(),
                children: vec![],
                administrative: false,
            },
        )]);
        let rules = Rules {
            roots: vec!["Root".into()],
            include: vec![],
            exclude: vec![],
            depth_limit: 0,
            page_limit: 10,
        };
        let preview = preview_pinned_selection(
            &graph,
            &rules,
            &BTreeMap::new(),
            &BTreeSet::new(),
            "snapshot-small",
        )
        .unwrap();
        assert_eq!(preview.selection.page_ids, (1..=10).collect::<Vec<_>>());
        assert!(preview
            .warnings
            .iter()
            .any(|warning| warning.contains("Selection near page limit: 10/10")));
    }
}
