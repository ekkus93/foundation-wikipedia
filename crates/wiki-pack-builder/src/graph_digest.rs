//! Canonical digest of a resolver's full category graph and related metadata.
//! A digest detects accidental input drift; it does not authenticate Wikimedia
//! source provenance or prove that a graph is complete.
use std::collections::{BTreeMap, BTreeSet};

use crate::selection::Category;

/// Stable across record insertion order and duplicate category members.
/// Includes unvisited categories so a snapshot's full supplied graph identity
/// changes even if the currently selected page IDs remain identical.
pub fn graph_digest(
    graph: &BTreeMap<String, Category>,
    redirects: &BTreeMap<u64, u64>,
    disambiguations: &BTreeSet<u64>,
) -> String {
    let mut bytes = b"foundation-wikipedia/category-graph/v1".to_vec();
    bytes.push(0);
    bytes.extend_from_slice(&(graph.len() as u64).to_be_bytes());
    for (name, category) in graph {
        add_text(&mut bytes, name);
        bytes.push(u8::from(category.administrative));
        add_ids(&mut bytes, &category.articles);
        add_texts(&mut bytes, &category.children);
    }
    bytes.extend_from_slice(&(redirects.len() as u64).to_be_bytes());
    for (from, to) in redirects {
        bytes.extend_from_slice(&from.to_be_bytes());
        bytes.extend_from_slice(&to.to_be_bytes());
    }
    bytes.extend_from_slice(&(disambiguations.len() as u64).to_be_bytes());
    for id in disambiguations {
        bytes.extend_from_slice(&id.to_be_bytes());
    }
    wiki_pack_format::sha256_hex(&bytes)
}

fn add_text(bytes: &mut Vec<u8>, text: &str) {
    bytes.extend_from_slice(&(text.len() as u64).to_be_bytes());
    bytes.extend_from_slice(text.as_bytes());
}

fn add_texts(bytes: &mut Vec<u8>, values: &[String]) {
    let mut canonical = values.to_vec();
    canonical.sort();
    canonical.dedup();
    bytes.extend_from_slice(&(canonical.len() as u64).to_be_bytes());
    for value in canonical {
        add_text(bytes, &value);
    }
}

fn add_ids(bytes: &mut Vec<u8>, values: &[u64]) {
    let mut canonical = values.to_vec();
    canonical.sort_unstable();
    canonical.dedup();
    bytes.extend_from_slice(&(canonical.len() as u64).to_be_bytes());
    for value in canonical {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_under_duplicate_member_reordering() {
        let mut graph = BTreeMap::from([(
            "Root".into(),
            Category {
                articles: vec![3, 2],
                children: vec!["Child".into()],
                administrative: false,
            },
        )]);
        let first = graph_digest(&graph, &BTreeMap::new(), &BTreeSet::new());
        graph.get_mut("Root").unwrap().articles = vec![2, 3, 2];
        graph.get_mut("Root").unwrap().children = vec!["Child".into(), "Child".into()];
        assert_eq!(
            first,
            graph_digest(&graph, &BTreeMap::new(), &BTreeSet::new())
        );
    }

    #[test]
    fn hash_detects_unselected_categories_and_redirects() {
        let mut graph = BTreeMap::from([(
            "Root".into(),
            Category {
                articles: vec![2],
                children: vec![],
                administrative: false,
            },
        )]);
        let first = graph_digest(&graph, &BTreeMap::new(), &BTreeSet::new());
        graph.insert(
            "Other".into(),
            Category {
                articles: vec![3],
                children: vec![],
                administrative: false,
            },
        );
        assert_ne!(
            first,
            graph_digest(&graph, &BTreeMap::new(), &BTreeSet::new())
        );
        assert_ne!(
            graph_digest(&graph, &BTreeMap::new(), &BTreeSet::new()),
            graph_digest(&graph, &BTreeMap::from([(99, 2)]), &BTreeSet::new()),
        );
        assert_ne!(
            graph_digest(&graph, &BTreeMap::new(), &BTreeSet::new()),
            graph_digest(&graph, &BTreeMap::new(), &BTreeSet::from([2])),
        );
    }
}
