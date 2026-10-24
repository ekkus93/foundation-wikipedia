//! Cycle-safe topic selection against a version-pinned category graph.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use wiki_pack_format::Sha256Transcript;

// Preview unusually broad category membership before media acquisition.
// These thresholds affect diagnostics only, never the selected page-ID set.
const LARGE_CATEGORY_CHILDREN: usize = 32;
const LARGE_CATEGORY_ARTICLES: usize = 512;

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
    InvalidCategory(String),
    TooManyPages,
    TooManyCategories,
    TooManyCandidates,
    RedirectCycle(u64),
    RedirectTooDeep,
    TooManyRedirects,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub page_ids: Vec<u64>,
    pub warnings: Vec<String>,
}

fn invalid_page_id(id: u64) -> bool {
    id == 0 || id > i64::MAX as u64
}

/// Raw topic DTOs can bypass Enterprise parsing. Reject spoofed category
/// labels, including those in graph nodes outside the selected traversal.
/// ZWNJ/ZWJ remain legal for Persian and Indic orthography.
fn valid_category_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && value.trim() == value
        && !value.chars().any(|ch| {
            ch.is_control()
                || matches!(
                    ch,
                    '\u{061c}'
                        | '\u{200b}'
                        | '\u{200e}'..='\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2060}'
                        | '\u{2066}'..='\u{2069}'
                        | '\u{feff}'
                )
        })
}

fn validate_category_labels(
    graph: &BTreeMap<String, Category>,
    rules: &Rules,
) -> Result<(), ResolveError> {
    for root in &rules.roots {
        if !valid_category_label(root) {
            return Err(ResolveError::InvalidCategory(root.clone()));
        }
    }
    for (name, node) in graph {
        if !valid_category_label(name) {
            return Err(ResolveError::InvalidCategory(name.clone()));
        }
        for child in &node.children {
            if !valid_category_label(child) {
                return Err(ResolveError::InvalidCategory(child.clone()));
            }
        }
    }
    Ok(())
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
    validate_category_labels(graph, rules)?;
    validate_source_identities(graph, redirects, disambiguations)?;
    // Bound queued work before expansion: a single category can have far more
    // children than selected pages, including cycles and duplicate links.
    let category_budget = rules.page_limit.saturating_mul(32).clamp(1024, 100_000);
    // Repeated edges do not consume unique-category capacity, but still
    // consume work. Bound the raw traversal separately to prevent a source
    // graph with millions of duplicate edges from monopolizing the builder.
    let edge_budget = category_budget.saturating_mul(64).min(1_000_000);
    if rules.roots.len() > edge_budget {
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
        .any(|id| invalid_page_id(*id))
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
    // BFS discovers each category at its minimum root-relative depth. Queue
    // identities, not raw links: duplicate roots, diamond edges and cycles
    // must not cause false TooManyCategories errors or excess allocations.
    let mut enqueued = BTreeSet::new();
    let mut warnings = BTreeSet::new();
    let mut queue = VecDeque::new();
    for root in &rules.roots {
        if enqueued.insert(root.clone()) {
            if enqueued.len() > category_budget {
                return Err(ResolveError::TooManyCategories);
            }
            queue.push_back((root.clone(), 0));
        }
    }
    let mut inspected_edges = rules.roots.len();
    while let Some((name, depth)) = queue.pop_front() {
        let node = graph
            .get(&name)
            .ok_or(ResolveError::Missing(name.clone()))?;
        if node.administrative {
            warnings.insert(format!("Skipped administrative category: {name}"));
            continue;
        }
        if node.children.len() > LARGE_CATEGORY_CHILDREN {
            warnings.insert(format!(
                "Large category child fan-out: {name} ({} children)",
                node.children.len()
            ));
        }
        if node.articles.len() > LARGE_CATEGORY_ARTICLES {
            warnings.insert(format!(
                "Large category article fan-out: {name} ({} memberships)",
                node.articles.len()
            ));
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
            inspected_edges = inspected_edges.saturating_add(node.children.len());
            if inspected_edges > edge_budget {
                return Err(ResolveError::TooManyCategories);
            }
            for child in &node.children {
                if enqueued.insert(child.clone()) {
                    if enqueued.len() > category_budget {
                        return Err(ResolveError::TooManyCategories);
                    }
                    queue.push_back((child.clone(), depth + 1));
                }
            }
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

/// All public resolver entry points reject invalid supplied identities,
/// including graph nodes and redirect chains not reached by a chosen topic.
/// Otherwise callers of the unpinned preview could see a valid selection
/// over malformed source data that the pinned receipt would later reject.
fn validate_source_identities(
    graph: &BTreeMap<String, Category>,
    redirects: &BTreeMap<u64, u64>,
    disambiguations: &BTreeSet<u64>,
) -> Result<(), ResolveError> {
    if graph
        .values()
        .any(|category| category.articles.iter().any(|&id| invalid_page_id(id)))
        || redirects
            .iter()
            .any(|(&from, &to)| invalid_page_id(from) || invalid_page_id(to))
        || disambiguations.iter().any(|&id| invalid_page_id(id))
    {
        return Err(ResolveError::InvalidPage);
    }
    validate_pinned_redirect_map(redirects)
}

/// Validate the entire supplied redirect identity map before fingerprinting it.
/// Memoized suffix depths keep shared chains linear in their total edges.
/// This checks internal consistency, not Wikimedia publisher authenticity.
fn validate_pinned_redirect_map(redirects: &BTreeMap<u64, u64>) -> Result<(), ResolveError> {
    if redirects.len() > 1_000_000 {
        return Err(ResolveError::TooManyRedirects);
    }
    let mut suffix_depths: BTreeMap<u64, usize> = BTreeMap::new();
    for &start in redirects.keys() {
        if suffix_depths.contains_key(&start) {
            continue;
        }
        let mut seen = BTreeSet::new();
        let mut path = Vec::new();
        let mut current = start;
        let mut depth = loop {
            if invalid_page_id(current) {
                return Err(ResolveError::InvalidPage);
            }
            if let Some(&cached) = suffix_depths.get(&current) {
                break cached;
            }
            let Some(&next) = redirects.get(&current) else {
                break 0usize;
            };
            if !seen.insert(current) {
                return Err(ResolveError::RedirectCycle(current));
            }
            path.push(current);
            if path.len() > 4096 {
                return Err(ResolveError::RedirectTooDeep);
            }
            current = next;
        };
        if path.len().saturating_add(depth) > 4096 {
            return Err(ResolveError::RedirectTooDeep);
        }
        for source in path.into_iter().rev() {
            depth += 1;
            suffix_depths.insert(source, depth);
        }
    }
    Ok(())
}

fn canonical_page(mut id: u64, redirects: &BTreeMap<u64, u64>) -> Result<u64, ResolveError> {
    let mut visited = BTreeSet::new();
    loop {
        if invalid_page_id(id) {
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

fn append_text(digest: &mut Sha256Transcript, value: &str) {
    digest.update(&(value.len() as u64).to_be_bytes());
    digest.update(value.as_bytes());
}

fn append_strings(digest: &mut Sha256Transcript, values: &[String]) {
    let mut canonical = values.to_vec();
    canonical.sort();
    canonical.dedup();
    digest.update(&(canonical.len() as u64).to_be_bytes());
    for value in canonical {
        append_text(digest, &value);
    }
}

fn append_ids(digest: &mut Sha256Transcript, values: &[u64]) {
    let mut canonical = values.to_vec();
    canonical.sort_unstable();
    canonical.dedup();
    digest.update(&(canonical.len() as u64).to_be_bytes());
    for value in canonical {
        digest.update(&value.to_be_bytes());
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
    // All resolver entry points share the same full-input validation,
    // including identities in unselected category and redirect records.
    let resolved = resolve_with_redirects(graph, rules, redirects, disambiguations)?;
    let mut definition = Sha256Transcript::new();
    definition.update(b"foundation-wikipedia/topic-definition/v1\0");
    append_strings(&mut definition, &rules.roots);
    append_ids(&mut definition, &rules.include);
    append_ids(&mut definition, &rules.exclude);
    definition.update(&(rules.depth_limit as u64).to_be_bytes());
    definition.update(&(rules.page_limit as u64).to_be_bytes());
    let definition_sha256 = definition.finish_hex();

    let graph_sha256 = crate::graph_digest::graph_digest(graph, redirects, disambiguations);
    let mut selection = Sha256Transcript::new();
    selection.update(b"foundation-wikipedia/topic-selection/v2\0");
    append_text(&mut selection, snapshot_id);
    append_text(&mut selection, &definition_sha256);
    append_text(&mut selection, &graph_sha256);
    append_ids(&mut selection, &resolved.page_ids);
    Ok(PinnedResolution {
        snapshot_id: snapshot_id.to_owned(),
        definition_sha256,
        graph_sha256,
        snapshot_selection_sha256: selection.finish_hex(),
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

    #[test]
    fn streamed_pinned_receipts_match_original_byte_encoding() {
        let graph = BTreeMap::from([
            (
                "Café".to_owned(),
                Category {
                    articles: vec![2],
                    children: vec![],
                    administrative: false,
                },
            ),
            (
                "Science".to_owned(),
                Category {
                    articles: vec![3],
                    children: vec![],
                    administrative: false,
                },
            ),
        ]);
        let rules = Rules {
            roots: vec!["Science".into(), "Café".into(), "Science".into()],
            include: vec![9, 2, 2],
            exclude: vec![8],
            depth_limit: 0,
            page_limit: 20,
        };
        let pinned = resolve_pinned(
            &graph,
            &rules,
            &BTreeMap::new(),
            &BTreeSet::new(),
            "enwiki-20261016",
        )
        .unwrap();
        let mut legacy_definition = b"foundation-wikipedia/topic-definition/v1\0".to_vec();
        let mut roots = rules.roots.clone();
        roots.sort();
        roots.dedup();
        legacy_definition.extend_from_slice(&(roots.len() as u64).to_be_bytes());
        for root in &roots {
            legacy_definition.extend_from_slice(&(root.len() as u64).to_be_bytes());
            legacy_definition.extend_from_slice(root.as_bytes());
        }
        for values in [&rules.include, &rules.exclude] {
            let mut ids = values.clone();
            ids.sort_unstable();
            ids.dedup();
            legacy_definition.extend_from_slice(&(ids.len() as u64).to_be_bytes());
            for id in ids {
                legacy_definition.extend_from_slice(&id.to_be_bytes());
            }
        }
        legacy_definition.extend_from_slice(&(rules.depth_limit as u64).to_be_bytes());
        legacy_definition.extend_from_slice(&(rules.page_limit as u64).to_be_bytes());
        assert_eq!(
            pinned.definition_sha256,
            wiki_pack_format::sha256_hex(&legacy_definition)
        );

        let mut legacy_selection = b"foundation-wikipedia/topic-selection/v2\0".to_vec();
        for value in [
            "enwiki-20261016",
            &pinned.definition_sha256,
            &pinned.graph_sha256,
        ] {
            legacy_selection.extend_from_slice(&(value.len() as u64).to_be_bytes());
            legacy_selection.extend_from_slice(value.as_bytes());
        }
        legacy_selection.extend_from_slice(&(pinned.page_ids.len() as u64).to_be_bytes());
        for id in &pinned.page_ids {
            legacy_selection.extend_from_slice(&id.to_be_bytes());
        }
        assert_eq!(
            pinned.snapshot_selection_sha256,
            wiki_pack_format::sha256_hex(&legacy_selection)
        );
    }

    #[test]
    fn pinned_receipts_reject_unselected_redirect_cycles_and_overlong_chains() {
        let graph = BTreeMap::from([(
            "Category:Selected".to_owned(),
            Category {
                articles: vec![2],
                children: vec![],
                administrative: false,
            },
        )]);
        let rules = Rules {
            roots: vec!["Category:Selected".into()],
            include: vec![],
            exclude: vec![],
            depth_limit: 0,
            page_limit: 10,
        };
        let check = |redirects: &BTreeMap<u64, u64>| {
            resolve_pinned(
                &graph,
                &rules,
                redirects,
                &BTreeSet::new(),
                "enwiki-20261001",
            )
        };
        let cycle = BTreeMap::from([(100, 101), (101, 100)]);
        assert_eq!(check(&cycle), Err(ResolveError::RedirectCycle(100)));
        let valid: BTreeMap<_, _> = (10_000..14_096)
            .map(|id| (id, if id == 14_095 { 2 } else { id + 1 }))
            .collect();
        assert_eq!(check(&valid).unwrap().page_ids, vec![2]);
        let mut too_long = valid;
        too_long.insert(9_999, 10_000);
        assert_eq!(check(&too_long), Err(ResolveError::RedirectTooDeep));
    }

    #[test]
    fn raw_topic_resolver_rejects_spoofed_labels_even_in_unselected_nodes() {
        let rules = Rules {
            roots: vec!["Category:Science".into()],
            include: vec![],
            exclude: vec![],
            depth_limit: 0,
            page_limit: 10,
        };
        let base = BTreeMap::from([(
            "Category:Science".into(),
            Category {
                articles: vec![2],
                children: vec![],
                administrative: false,
            },
        )]);
        let pinned = |graph: &BTreeMap<String, Category>, rules: &Rules| {
            resolve_pinned(
                graph,
                rules,
                &BTreeMap::new(),
                &BTreeSet::new(),
                "enwiki-20261023",
            )
        };
        assert_eq!(pinned(&base, &rules).unwrap().page_ids, vec![2]);
        for bad in [
            "",
            " Category:Science",
            "Category:Science ",
            "Category:Science\nSpoof",
            "Category:Science\u{202e}Spoof",
            "Category:Science\u{2066}Spoof",
            "Category:Science\u{200b}Spoof",
            "Category:Science\u{feff}Spoof",
        ] {
            let mut bad_root = rules.clone();
            bad_root.roots = vec![bad.into()];
            assert_eq!(
                pinned(&base, &bad_root),
                Err(ResolveError::InvalidCategory(bad.into()))
            );
            let mut unselected = base.clone();
            unselected.insert(bad.into(), Category::default());
            assert_eq!(
                pinned(&unselected, &rules),
                Err(ResolveError::InvalidCategory(bad.into()))
            );
            let mut child = base.clone();
            child
                .get_mut("Category:Science")
                .unwrap()
                .children
                .push(bad.into());
            assert_eq!(
                pinned(&child, &rules),
                Err(ResolveError::InvalidCategory(bad.into()))
            );
        }
        let too_long = "x".repeat(4097);
        let mut bad_root = rules.clone();
        bad_root.roots = vec![too_long.clone()];
        assert_eq!(
            pinned(&base, &bad_root),
            Err(ResolveError::InvalidCategory(too_long))
        );
        let mut graph = base;
        graph.insert("Category:می‌روم क्‍ष".into(), Category::default());
        assert!(pinned(&graph, &rules).is_ok());
    }

    #[test]
    fn unpinned_preview_rejects_unselected_invalid_source_identities() {
        let rules = Rules {
            roots: vec!["Category:Root".into()],
            include: vec![],
            exclude: vec![],
            depth_limit: 0,
            page_limit: 10,
        };
        let mut graph = BTreeMap::from([
            (
                "Category:Root".into(),
                Category {
                    articles: vec![2],
                    children: vec![],
                    administrative: false,
                },
            ),
            (
                "Category:Unselected".into(),
                Category {
                    articles: vec![0],
                    children: vec![],
                    administrative: false,
                },
            ),
        ]);
        assert_eq!(resolve(&graph, &rules), Err(ResolveError::InvalidPage));
        graph.get_mut("Category:Unselected").unwrap().articles = vec![3];
        assert_eq!(resolve(&graph, &rules).unwrap().page_ids, vec![2]);

        let redirects = BTreeMap::from([(100, 101), (101, 100)]);
        assert_eq!(
            resolve_with_redirects(&graph, &rules, &redirects, &BTreeSet::new()),
            Err(ResolveError::RedirectCycle(100))
        );
        assert_eq!(
            resolve_with_redirects(
                &graph,
                &rules,
                &BTreeMap::from([(100, 0)]),
                &BTreeSet::new(),
            ),
            Err(ResolveError::InvalidPage)
        );
        assert_eq!(
            resolve_with_redirects(
                &graph,
                &rules,
                &BTreeMap::new(),
                &BTreeSet::from([i64::MAX as u64 + 1]),
            ),
            Err(ResolveError::InvalidPage)
        );
    }

    #[test]
    fn repeated_category_edges_use_unique_queue_budget() {
        let mut rules = Rules {
            roots: vec!["Category:Root".into(); 2048],
            include: vec![],
            exclude: vec![],
            depth_limit: 2,
            page_limit: 10,
        };
        let mut graph = BTreeMap::from([
            (
                "Category:Root".into(),
                Category {
                    articles: vec![2],
                    children: vec!["Category:Leaf".into(); 4096],
                    administrative: false,
                },
            ),
            (
                "Category:Leaf".into(),
                Category {
                    articles: vec![3],
                    children: vec!["Category:Root".into(); 2048],
                    administrative: false,
                },
            ),
        ]);
        let repeated = resolve(&graph, &rules).unwrap();
        assert_eq!(repeated.page_ids, vec![2, 3]);
        rules.roots = vec!["Category:Root".into()];
        graph.get_mut("Category:Root").unwrap().children = vec!["Category:Leaf".into()];
        graph.get_mut("Category:Leaf").unwrap().children = vec!["Category:Root".into()];
        let minimal = resolve(&graph, &rules).unwrap();
        assert_eq!(repeated.page_ids, minimal.page_ids);

        // A large number of *distinct* categories must still fail closed.
        graph.get_mut("Category:Root").unwrap().children =
            (0..1024).map(|id| format!("Category:Child{id}")).collect();
        assert_eq!(
            resolve(&graph, &rules),
            Err(ResolveError::TooManyCategories)
        );

        // Repeated raw edges are also bounded independently of unique nodes.
        rules.roots = vec!["Category:Root".into(); 65_537];
        assert_eq!(
            resolve(&graph, &rules),
            Err(ResolveError::TooManyCategories)
        );
    }

    #[test]
    fn warns_on_unusually_broad_category_fanout_without_changing_selection() {
        let name = "Category:Broad";
        let rules = Rules {
            roots: vec![name.into()],
            include: vec![],
            exclude: vec![],
            depth_limit: 0,
            page_limit: 1024,
        };
        let graph = |children: usize, articles: usize| {
            BTreeMap::from([(
                name.to_owned(),
                Category {
                    articles: (1..=articles as u64).collect(),
                    children: (0..children)
                        .map(|id| format!("Category:Child{id}"))
                        .collect(),
                    administrative: false,
                },
            )])
        };
        let below = resolve(&graph(32, 512), &rules).unwrap();
        assert!(below
            .warnings
            .iter()
            .all(|warning| !warning.contains("fan-out")));
        assert_eq!(below.page_ids.len(), 512);

        let above = resolve(&graph(33, 513), &rules).unwrap();
        assert_eq!(above.page_ids.len(), 513);
        assert_eq!(
            above.warnings,
            vec![
                "Depth limit reached: Category:Broad".to_owned(),
                "Large category article fan-out: Category:Broad (513 memberships)".to_owned(),
                "Large category child fan-out: Category:Broad (33 children)".to_owned(),
            ]
        );
        let pinned = resolve_pinned(
            &graph(33, 513),
            &rules,
            &BTreeMap::new(),
            &BTreeSet::new(),
            "enwiki-20261023",
        )
        .unwrap();
        assert_eq!(pinned.page_ids, above.page_ids);
        assert_eq!(pinned.warnings, above.warnings);
    }

    #[test]
    fn pinned_receipts_reject_invalid_unselected_source_identities() {
        let rules = Rules {
            roots: vec!["Category:Selected".into()],
            include: vec![],
            exclude: vec![],
            depth_limit: 0,
            page_limit: 10,
        };
        let mut graph = BTreeMap::from([
            (
                "Category:Selected".into(),
                Category {
                    articles: vec![1],
                    children: vec![],
                    administrative: false,
                },
            ),
            (
                "Category:Unselected".into(),
                Category {
                    articles: vec![0],
                    children: vec![],
                    administrative: false,
                },
            ),
        ]);
        let mut redirects = BTreeMap::new();
        let mut disambiguations = BTreeSet::new();
        let pinned = |graph: &BTreeMap<String, Category>,
                      redirects: &BTreeMap<u64, u64>,
                      disambiguations: &BTreeSet<u64>| {
            resolve_pinned(graph, &rules, redirects, disambiguations, "enwiki-20261001")
        };
        for invalid in [0, i64::MAX as u64 + 1] {
            graph.get_mut("Category:Unselected").unwrap().articles = vec![invalid];
            assert_eq!(
                pinned(&graph, &redirects, &disambiguations),
                Err(ResolveError::InvalidPage)
            );
        }
        graph.get_mut("Category:Unselected").unwrap().articles = vec![2];
        assert!(pinned(&graph, &redirects, &disambiguations).is_ok());
        for invalid in [0, i64::MAX as u64 + 1] {
            redirects.insert(99, invalid);
            assert_eq!(
                pinned(&graph, &redirects, &disambiguations),
                Err(ResolveError::InvalidPage)
            );
        }
        redirects.clear();
        redirects.insert(0, 99);
        assert_eq!(
            pinned(&graph, &redirects, &disambiguations),
            Err(ResolveError::InvalidPage)
        );
        redirects.clear();
        disambiguations.insert(0);
        assert_eq!(
            pinned(&graph, &redirects, &disambiguations),
            Err(ResolveError::InvalidPage)
        );
    }
}
