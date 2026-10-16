//! Canonical digest of a resolver's full category graph and related metadata.
//! A digest detects accidental input drift; it does not authenticate Wikimedia
//! source provenance or prove that a graph is complete.
use std::collections::{BTreeMap, BTreeSet};

use crate::selection::Category;
use wiki_pack_format::Sha256Transcript;

/// Stable across record insertion order and duplicate category members.
/// Includes unvisited categories so a snapshot's full supplied graph identity
/// changes even if the currently selected page IDs remain identical.
pub fn graph_digest(
    graph: &BTreeMap<String, Category>,
    redirects: &BTreeMap<u64, u64>,
    disambiguations: &BTreeSet<u64>,
) -> String {
    let mut digest = Sha256Transcript::new();
    digest.update(b"foundation-wikipedia/category-graph/v1\0");
    digest.update(&(graph.len() as u64).to_be_bytes());
    for (name, category) in graph {
        add_text(&mut digest, name);
        digest.update(&[u8::from(category.administrative)]);
        add_ids(&mut digest, &category.articles);
        add_texts(&mut digest, &category.children);
    }
    digest.update(&(redirects.len() as u64).to_be_bytes());
    for (from, to) in redirects {
        digest.update(&from.to_be_bytes());
        digest.update(&to.to_be_bytes());
    }
    digest.update(&(disambiguations.len() as u64).to_be_bytes());
    for id in disambiguations {
        digest.update(&id.to_be_bytes());
    }
    digest.finish_hex()
}

fn add_text(digest: &mut Sha256Transcript, text: &str) {
    digest.update(&(text.len() as u64).to_be_bytes());
    digest.update(text.as_bytes());
}

fn add_texts(digest: &mut Sha256Transcript, values: &[String]) {
    let mut canonical = values.to_vec();
    canonical.sort();
    canonical.dedup();
    digest.update(&(canonical.len() as u64).to_be_bytes());
    for value in canonical {
        add_text(digest, &value);
    }
}

fn add_ids(digest: &mut Sha256Transcript, values: &[u64]) {
    let mut canonical = values.to_vec();
    canonical.sort_unstable();
    canonical.dedup();
    digest.update(&(canonical.len() as u64).to_be_bytes());
    for value in canonical {
        digest.update(&value.to_be_bytes());
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
        assert_eq!(
            first,
            "fd4bb682bafeeeb1dac5f58072b51967fe7f4995e5cc3db9f92f305ba7458c21"
        );
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
