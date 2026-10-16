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
    if seeds.contains(&0) || excluded.contains(&0) {
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
                if neighbor == 0 {
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
    }
}
