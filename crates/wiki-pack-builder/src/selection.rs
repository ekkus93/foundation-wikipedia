//! Cycle-safe topic selection against a version-pinned category graph.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, Default)]
pub struct Category {
    pub articles: Vec<u64>,
    pub children: Vec<String>,
    pub administrative: bool,
}

#[derive(Clone, Debug)]
pub struct Rules {
    pub roots: Vec<String>,
    pub include: Vec<u64>,
    pub exclude: Vec<u64>,
    pub depth_limit: usize,
    pub page_limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveError {
    BadLimit,
    Missing(String),
    InvalidPage,
    InvalidSnapshot,
    TooManyPages,
    TooManyCategories,
    TooManyCandidates,
    RedirectCycle(u64),
    RedirectTooDeep,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub page_ids: Vec<u64>,
    pub warnings: Vec<String>,
}

pub fn resolve(
    graph: &BTreeMap<String, Category>,
    rules: &Rules,
) -> Result<Resolution, ResolveError> {
    resolve_with_redirects(graph, rules, &BTreeMap::new(), &BTreeSet::new())
}

/// Apply exact-snapshot redirect identities before counting or excluding pages.
/// Disambiguation pages remain readable articles, but are flagged for review.
pub fn resolve_with_redirects(
    graph: &BTreeMap<String, Category>,
    rules: &Rules,
    redirects: &BTreeMap<u64, u64>,
    disambiguations: &BTreeSet<u64>,
) -> Result<Resolution, ResolveError> {
    if rules.page_limit == 0 || rules.depth_limit > 100 {
        return Err(ResolveError::BadLimit);
    }
    // Bound queued work before expansion: a single category can have far more
    // children than selected pages, including cycles and duplicate links.
    let category_budget = rules.page_limit.saturating_mul(32).clamp(1024, 100_000);
    if rules.roots.len() > category_budget {
        return Err(ResolveError::TooManyCategories);
    }
    // A category can list millions of duplicate, excluded or redirected IDs
    // without ever exceeding the unique-page limit.
    let candidate_budget = rules.page_limit.saturating_mul(64).clamp(4096, 1_000_000);
    if rules.include.len().saturating_add(rules.exclude.len()) > candidate_budget {
        return Err(ResolveError::TooManyCandidates);
    }
    if rules
        .include
        .iter()
        .chain(&rules.exclude)
        .any(|id| *id == 0)
    {
        return Err(ResolveError::InvalidPage);
    }
    let excluded: BTreeSet<u64> = rules
        .exclude
        .iter()
        .map(|id| canonical_page(*id, redirects))
        .collect::<Result<_, _>>()?;
    let mut pages = BTreeSet::new();
    let mut inspected_candidates = rules.include.len().saturating_add(rules.exclude.len());
    let mut visited = BTreeSet::new();
    let mut warnings = BTreeSet::new();
    let mut queue: VecDeque<_> = rules.roots.iter().map(|root| (root.clone(), 0)).collect();
    while let Some((name, depth)) = queue.pop_front() {
        if !visited.insert(name.clone()) {
            continue;
        }
        if visited.len() > category_budget {
            return Err(ResolveError::TooManyCategories);
        }
        let node = graph
            .get(&name)
            .ok_or(ResolveError::Missing(name.clone()))?;
        if node.administrative {
            warnings.insert(format!("Skipped administrative category: {name}"));
            continue;
        }
        inspected_candidates = inspected_candidates.saturating_add(node.articles.len());
        if inspected_candidates > candidate_budget {
            return Err(ResolveError::TooManyCandidates);
        }
        for id in &node.articles {
            let resolved = canonical_page(*id, redirects)?;
            if resolved != *id {
                warnings.insert(format!("Resolved redirect: {id} -> {resolved}"));
            }
            if !excluded.contains(&resolved) {
                pages.insert(resolved);
                if disambiguations.contains(&resolved) {
                    warnings.insert(format!("Disambiguation article: {resolved}"));
                }
            }
        }
        if pages.len() > rules.page_limit {
            return Err(ResolveError::TooManyPages);
        }
        if depth < rules.depth_limit {
            if node.children.len() > category_budget.saturating_sub(queue.len()) {
                return Err(ResolveError::TooManyCategories);
            }
            queue.extend(node.children.iter().map(|child| (child.clone(), depth + 1)));
        } else if !node.children.is_empty() {
            warnings.insert(format!("Depth limit reached: {name}"));
        }
    }
    for id in &rules.include {
        let resolved = canonical_page(*id, redirects)?;
        if resolved != *id {
            warnings.insert(format!("Resolved redirect: {id} -> {resolved}"));
        }
        if !excluded.contains(&resolved) {
            pages.insert(resolved);
            if disambiguations.contains(&resolved) {
                warnings.insert(format!("Disambiguation article: {resolved}"));
            }
        }
    }
    if pages.len() > rules.page_limit {
        return Err(ResolveError::TooManyPages);
    }
    Ok(Resolution {
        page_ids: pages.into_iter().collect(),
        warnings: warnings.into_iter().collect(),
    })
}

fn canonical_page(mut id: u64, redirects: &BTreeMap<u64, u64>) -> Result<u64, ResolveError> {
    let mut visited = BTreeSet::new();
    loop {
        if id == 0 {
            return Err(ResolveError::InvalidPage);
        }
        match redirects.get(&id) {
            Some(next) => {
                if !visited.insert(id) {
                    return Err(ResolveError::RedirectCycle(id));
                }
                if visited.len() > 4096 {
                    return Err(ResolveError::RedirectTooDeep);
                }
                id = *next;
            }
            None => return Ok(id),
        }
    }
}

/// Snapshot-scoped deterministic topic selection receipt. A caller-provided
/// snapshot ID is a provenance *claim*, not authenticated source evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedResolution {
    pub snapshot_id: String,
    pub definition_sha256: String,
    /// Full supplied category graph, redirects and disambiguation input digest.
    pub graph_sha256: String,
    pub snapshot_selection_sha256: String,
    pub page_ids: Vec<u64>,
    pub warnings: Vec<String>,
}

fn append_text(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn append_strings(out: &mut Vec<u8>, values: &[String]) {
    let mut canonical = values.to_vec();
    canonical.sort();
    canonical.dedup();
    out.extend_from_slice(&(canonical.len() as u64).to_be_bytes());
    for value in canonical {
        append_text(out, &value);
    }
}

fn append_ids(out: &mut Vec<u8>, values: &[u64]) {
    let mut canonical = values.to_vec();
    canonical.sort_unstable();
    canonical.dedup();
    out.extend_from_slice(&(canonical.len() as u64).to_be_bytes());
    for value in canonical {
        out.extend_from_slice(&value.to_be_bytes());
    }
}

/// Produce stable hashes for a definition and its resolved page-ID selection.
/// Root and include/exclude order is intentionally immaterial. The snapshot
/// identity and full input graph digest are included in the selection digest
/// to prevent reuse across inconsistent inputs with the same selected IDs.
/// This detects local drift but does not authenticate Wikimedia provenance.
pub fn resolve_pinned(
    graph: &BTreeMap<String, Category>,
    rules: &Rules,
    redirects: &BTreeMap<u64, u64>,
    disambiguations: &BTreeSet<u64>,
    snapshot_id: &str,
) -> Result<PinnedResolution, ResolveError> {
    if snapshot_id.is_empty()
        || snapshot_id.len() > 256
        || !snapshot_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    {
        return Err(ResolveError::InvalidSnapshot);
    }
    let resolved = resolve_with_redirects(graph, rules, redirects, disambiguations)?;
    let mut definition = b"foundation-wikipedia/topic-definition/v1\0".to_vec();
    append_strings(&mut definition, &rules.roots);
    append_ids(&mut definition, &rules.include);
    append_ids(&mut definition, &rules.exclude);
    definition.extend_from_slice(&(rules.depth_limit as u64).to_be_bytes());
    definition.extend_from_slice(&(rules.page_limit as u64).to_be_bytes());
    let definition_sha256 = wiki_pack_format::sha256_hex(&definition);

    let graph_sha256 = crate::graph_digest::graph_digest(graph, redirects, disambiguations);
    let mut selection = b"foundation-wikipedia/topic-selection/v2\0".to_vec();
    append_text(&mut selection, snapshot_id);
    append_text(&mut selection, &definition_sha256);
    append_text(&mut selection, &graph_sha256);
    append_ids(&mut selection, &resolved.page_ids);
    Ok(PinnedResolution {
        snapshot_id: snapshot_id.to_owned(),
        definition_sha256,
        graph_sha256,
        snapshot_selection_sha256: wiki_pack_format::sha256_hex(&selection),
        page_ids: resolved.page_ids,
        warnings: resolved.warnings,
    })
}

#[cfg(test)]
mod pinned_tests {
    use super::*;

    #[test]
    fn selection_receipts_are_stable_and_bound_to_snapshot_and_rules() {
        let graph = BTreeMap::from([
            (
                "Category:Science".to_owned(),
                Category {
                    articles: vec![9, 2],
                    children: vec!["Category:Math".into()],
                    administrative: false,
                },
            ),
            (
                "Category:Math".to_owned(),
                Category {
                    articles: vec![3, 2],
                    children: Vec::new(),
                    administrative: false,
                },
            ),
        ]);
        let mut rules = Rules {
            roots: vec!["Category:Science".into(), "Category:Math".into()],
            include: vec![4, 5],
            exclude: vec![9],
            depth_limit: 2,
            page_limit: 100,
        };
        let redirects = BTreeMap::new();
        let disambiguations = BTreeSet::new();
        let first = resolve_pinned(
            &graph,
            &rules,
            &redirects,
            &disambiguations,
            "enwiki-20261001",
        )
        .unwrap();
        assert_eq!(first.page_ids, vec![2, 3, 4, 5]);
        assert_eq!(first.definition_sha256.len(), 64);
        assert_eq!(first.graph_sha256.len(), 64);
        // A category outside the traversed depth can change without changing
        // selected IDs. The receipt must still detect the changed input graph.
        let mut changed_graph = graph.clone();
        changed_graph.get_mut("Category:Math").unwrap().articles.push(777);
        let changed_graph_receipt = resolve_pinned(
            &changed_graph,
            &rules,
            &redirects,
            &disambiguations,
            "enwiki-20261001",
        )
        .unwrap();
        // This test uses a root that traverses Math, so assert the graph digest
        // independently before constraining depth below.
        assert_ne!(first.graph_sha256, changed_graph_receipt.graph_sha256);
        let mut root_only = rules.clone();
        root_only.depth_limit = 0;
        root_only.roots = vec!["Category:Science".into()];
        let base_root = resolve_pinned(
            &graph, &root_only, &redirects, &disambiguations, "enwiki-20261001",
        )
        .unwrap();
        let modified_root = resolve_pinned(
            &changed_graph, &root_only, &redirects, &disambiguations, "enwiki-20261001",
        )
        .unwrap();
        assert_eq!(base_root.page_ids, modified_root.page_ids);
        assert_eq!(base_root.definition_sha256, modified_root.definition_sha256);
        assert_ne!(base_root.graph_sha256, modified_root.graph_sha256);
        assert_ne!(base_root.snapshot_selection_sha256, modified_root.snapshot_selection_sha256);
        let altered_redirects = BTreeMap::from([(900, 901)]);
        let redirect_receipt = resolve_pinned(
            &graph, &root_only, &altered_redirects, &disambiguations, "enwiki-20261001",
        )
        .unwrap();
        assert_eq!(base_root.page_ids, redirect_receipt.page_ids);
        assert_ne!(base_root.snapshot_selection_sha256, redirect_receipt.snapshot_selection_sha256);

        assert_eq!(first.snapshot_selection_sha256.len(), 64);
        rules.roots.reverse();
        rules.include.reverse();
        assert_eq!(
            first,
            resolve_pinned(
                &graph,
                &rules,
                &redirects,
                &disambiguations,
                "enwiki-20261001",
            )
            .unwrap()
        );
        let other_snapshot = resolve_pinned(
            &graph,
            &rules,
            &redirects,
            &disambiguations,
            "enwiki-20261002",
        )
        .unwrap();
        assert_eq!(first.definition_sha256, other_snapshot.definition_sha256);
        assert_ne!(
            first.snapshot_selection_sha256,
            other_snapshot.snapshot_selection_sha256
        );
        rules.depth_limit = 0;
        let changed_rules = resolve_pinned(
            &graph,
            &rules,
            &redirects,
            &disambiguations,
            "enwiki-20261001",
        )
        .unwrap();
        assert_ne!(first.definition_sha256, changed_rules.definition_sha256);
        assert_ne!(
            first.snapshot_selection_sha256,
            changed_rules.snapshot_selection_sha256
        );
        for invalid in ["", "../escape", "snapshot\nspoof", "x:y"] {
            assert_eq!(
                resolve_pinned(&graph, &rules, &redirects, &disambiguations, invalid),
                Err(ResolveError::InvalidSnapshot)
            );
        }
    }
}
