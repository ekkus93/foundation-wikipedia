//! Bounded optional linked-prerequisite expansion for already-canonical page IDs.
//!
//! The supplied adjacency map is a caller claim, not authenticated Wikimedia
//! evidence. Callers must pin and fingerprint it alongside their selection
//! definition before using the result in a reproducible pack manifest.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::selection::ResolveError;

/// Expand canonical page IDs by a bounded number of link hops.
///
/// Exclusions apply to seeds and discovered pages, including traversal.
/// Results are sorted and deduplicated regardless of input order. The edge
/// budget counts raw mentions, including duplicate and excluded links.
/// No source authentication or redirect resolution is performed here.
pub fn expand_related_links(
    seeds: &[u64],
    adjacency: &BTreeMap<u64, Vec<u64>>,
    excluded: &BTreeSet<u64>,
    max_hops: usize,
    max_new_pages: usize,
    max_edge_visits: usize,
) -> Result<Vec<u64>, ResolveError> {
    if max_hops == 0
        || max_hops > 8
        || max_new_pages == 0
        || max_new_pages > 100_000
        || max_edge_visits == 0
        || max_edge_visits > 1_000_000
    {
        return Err(ResolveError::BadLimit);
    }
    // All page identities must fit the signed SQLite key range, even when a
    // seed is excluded before traversal. Do not silently discard bad IDs.
    if seeds
        .iter()
        .chain(excluded.iter())
        .any(|&id| id == 0 || id > i64::MAX as u64)
    {
        return Err(ResolveError::InvalidPage);
    }
    // Validate the entire supplied link graph, not just edges reachable
    // within this preview's hop budget. Unselected or depth-limited records
    // must not conceal invalid page identities in a caller-provided DTO.
    // The revision-joined wrapper validates its link claims separately;
    // this low-level entry point must be safe when called directly.
    if adjacency.iter().any(|(&from, neighbors)| {
        from == 0
            || from > i64::MAX as u64
            || neighbors
                .iter()
                .any(|&to| to == 0 || to > i64::MAX as u64)
    }) {
        return Err(ResolveError::InvalidPage);
    }
    let mut selected: BTreeSet<u64> = seeds
        .iter()
        .copied()
        .filter(|id| !excluded.contains(id))
        .collect();
    let initial_count = selected.len();
    let mut queue: VecDeque<(u64, usize)> = selected.iter().map(|&id| (id, 0)).collect();
    let mut inspected_edges = 0usize;
    while let Some((page, depth)) = queue.pop_front() {
        if depth == max_hops {
            continue;
        }
        if let Some(neighbors) = adjacency.get(&page) {
            inspected_edges = inspected_edges.saturating_add(neighbors.len());
            if inspected_edges > max_edge_visits {
                return Err(ResolveError::TooManyCandidates);
            }
            for &neighbor in neighbors {
                if neighbor == 0 || neighbor > i64::MAX as u64 {
                    return Err(ResolveError::InvalidPage);
                }
                if !excluded.contains(&neighbor) && selected.insert(neighbor) {
                    if selected.len() - initial_count > max_new_pages {
                        return Err(ResolveError::TooManyPages);
                    }
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }
    }
    Ok(selected.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expansion_is_sorted_cycle_safe_and_exclusion_aware() {
        let mut adjacency = BTreeMap::from([
            (1, vec![3, 2, 2]),
            (2, vec![4, 1]),
            (3, vec![5, 4]),
            (4, vec![6]),
            (5, vec![7]),
        ]);
        let excluded = BTreeSet::from([5, 8]);
        let result = expand_related_links(&[8, 1, 1], &adjacency, &excluded, 2, 10, 20);
        assert_eq!(result, Ok(vec![1, 2, 3, 4]));
        adjacency.get_mut(&1).unwrap().reverse();
        assert_eq!(
            expand_related_links(&[1, 8], &adjacency, &excluded, 2, 10, 20),
            result
        );
        assert_eq!(
            expand_related_links(&[1], &adjacency, &excluded, 1, 10, 20),
            Ok(vec![1, 2, 3])
        );
    }

    #[test]
    fn unreachable_and_depth_limited_link_identities_fail_closed() {
        let oversized = i64::MAX as u64 + 1;
        let excluded = BTreeSet::new();
        let mut adjacency = BTreeMap::from([(1, vec![2]), (2, vec![3])]);
        // Both a malformed source and a malformed destination in an
        // unrelated component are rejected, even if the result is [1, 2].
        adjacency.insert(0, vec![9]);
        assert_eq!(
            expand_related_links(&[1], &adjacency, &excluded, 1, 10, 20),
            Err(ResolveError::InvalidPage)
        );
        adjacency.remove(&0);
        adjacency.insert(99, vec![oversized]);
        assert_eq!(
            expand_related_links(&[1], &adjacency, &excluded, 1, 10, 20),
            Err(ResolveError::InvalidPage)
        );
        adjacency.remove(&99);
        adjacency.insert(oversized, vec![9]);
        assert_eq!(
            expand_related_links(&[1], &adjacency, &excluded, 1, 10, 20),
            Err(ResolveError::InvalidPage)
        );
        adjacency.remove(&oversized);
        // A neighbor beyond the requested hop limit is also part of the
        // supplied input and must not evade validation.
        adjacency.insert(2, vec![0]);
        assert_eq!(
            expand_related_links(&[1], &adjacency, &excluded, 1, 10, 20),
            Err(ResolveError::InvalidPage)
        );
        adjacency.insert(2, vec![3]);
        assert_eq!(
            expand_related_links(&[1], &adjacency, &excluded, 1, 10, 20),
            Ok(vec![1, 2])
        );
    }

    #[test]
    fn budgets_fail_closed_before_returning_partial_results() {
        let adjacency = BTreeMap::from([(1, vec![2, 3]), (2, vec![4])]);
        let excluded = BTreeSet::new();
        assert_eq!(
            expand_related_links(&[1], &adjacency, &excluded, 2, 1, 20),
            Err(ResolveError::TooManyPages)
        );
        assert_eq!(
            expand_related_links(&[1], &adjacency, &excluded, 2, 10, 1),
            Err(ResolveError::TooManyCandidates)
        );
        assert_eq!(
            expand_related_links(&[1], &adjacency, &excluded, 0, 10, 20),
            Err(ResolveError::BadLimit)
        );
        assert_eq!(
            expand_related_links(&[0], &adjacency, &excluded, 1, 10, 20),
            Err(ResolveError::InvalidPage)
        );
        let malformed = BTreeMap::from([(1, vec![0])]);
        assert_eq!(
            expand_related_links(&[1], &malformed, &excluded, 1, 10, 20),
            Err(ResolveError::InvalidPage)
        );
        // Direct callers must not produce IDs that cannot be stored in SQLite.
        let oversized = i64::MAX as u64 + 1;
        assert_eq!(
            expand_related_links(&[oversized], &adjacency, &excluded, 1, 10, 20),
            Err(ResolveError::InvalidPage)
        );
        assert_eq!(
            expand_related_links(&[1], &adjacency, &BTreeSet::from([oversized]), 1, 10, 20),
            Err(ResolveError::InvalidPage)
        );
        assert_eq!(
            expand_related_links(
                &[1],
                &BTreeMap::from([(1, vec![oversized])]),
                &excluded,
                1,
                10,
                20
            ),
            Err(ResolveError::InvalidPage)
        );
    }
}
