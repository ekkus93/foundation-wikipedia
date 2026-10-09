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
    TooManyPages,
    TooManyCategories,
    RedirectCycle(u64),
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
    let mut visited = BTreeSet::new();
    let mut warnings = BTreeSet::new();
    let mut queue: VecDeque<_> = rules.roots.iter().map(|root| (root.clone(), 0)).collect();
    while let Some((name, depth)) = queue.pop_front() {
        if !visited.insert(name.clone()) {
            continue;
        }
        if visited.len() > rules.page_limit.saturating_mul(32).max(1024) {
            return Err(ResolveError::TooManyCategories);
        }
        let node = graph
            .get(&name)
            .ok_or(ResolveError::Missing(name.clone()))?;
        if node.administrative {
            warnings.insert(format!("Skipped administrative category: {name}"));
            continue;
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
                id = *next;
            }
            None => return Ok(id),
        }
    }
}
