//! Editable-in-code, versioned seed topic definitions for the initial catalog.
//!
//! These are deterministic selection *rules*, not prebuilt or publisher-verified
//! packs. A completed, authenticated Wikimedia snapshot and a review of actual
//! category coverage/fan-out are still required before distribution.

use crate::selection::Rules;

/// Conservative bounded traversal prevents accidentally selecting an entire
/// administrative category tree. The bounds are provisional pending a measured
/// official-snapshot coverage review.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CuratedTopic {
    pub id: &'static str,
    pub label: &'static str,
    pub project: &'static str,
    pub roots: &'static [&'static str],
    pub depth_limit: usize,
    pub page_limit: usize,
}

impl CuratedTopic {
    /// Convert a curated seed to the same editable rules used by custom packs.
    /// The resolver, not this seed, determines exact page IDs for a snapshot.
    pub fn rules(self) -> Rules {
        Rules {
            roots: self.roots.iter().map(|root| (*root).to_owned()).collect(),
            include: Vec::new(),
            exclude: Vec::new(),
            depth_limit: self.depth_limit,
            page_limit: self.page_limit,
        }
    }
}

/// Stable identifiers are part of the pack-definition identity and must not
/// be reused for a different subject. Root names use enwiki category titles.
pub const INITIAL_TOPICS: [CuratedTopic; 3] = [
    CuratedTopic {
        id: "mathematics",
        label: "Mathematics",
        project: "enwiki",
        roots: &["Category:Mathematics"],
        depth_limit: 2,
        page_limit: 50_000,
    },
    CuratedTopic {
        id: "physics",
        label: "Physics",
        project: "enwiki",
        roots: &["Category:Physics"],
        depth_limit: 2,
        page_limit: 50_000,
    },
    CuratedTopic {
        id: "computer-science",
        label: "Computer Science",
        project: "enwiki",
        roots: &["Category:Computer science"],
        depth_limit: 2,
        page_limit: 50_000,
    },
];

pub fn curated_topic(id: &str) -> Option<CuratedTopic> {
    INITIAL_TOPICS.iter().copied().find(|topic| topic.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::{resolve_with_redirects, Category, ResolveError};
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn initial_topic_identities_and_limits_are_stable() {
        let mut ids = BTreeSet::new();
        for topic in INITIAL_TOPICS {
            assert!(ids.insert(topic.id), "duplicate curated topic id");
            assert_eq!(curated_topic(topic.id), Some(topic));
            assert_eq!(topic.project, "enwiki");
            assert!(!topic.roots.is_empty());
            assert!(topic.roots.iter().all(|root| root.starts_with("Category:")));
            assert!(topic.depth_limit <= 3);
            assert!((1..=50_000).contains(&topic.page_limit));
            let rules = topic.rules();
            assert_eq!(
                rules.roots,
                topic
                    .roots
                    .iter()
                    .map(|r| r.to_string())
                    .collect::<Vec<_>>()
            );
            assert!(rules.include.is_empty() && rules.exclude.is_empty());
        }
        assert_eq!(ids.len(), 3);
        assert!(curated_topic("Mathematics").is_none());
        assert!(curated_topic("unknown").is_none());
    }

    #[test]
    fn seed_definitions_resolve_deterministically_against_a_pinned_fixture() {
        for (offset, topic) in INITIAL_TOPICS.iter().enumerate() {
            let root = topic.roots[0].to_owned();
            let child = format!("{root}/intro");
            let admin = format!("{root}/maintenance");
            let base = 10 * (offset as u64 + 1);
            let graph = BTreeMap::from([
                (
                    root.clone(),
                    Category {
                        articles: vec![base + 1, base + 2],
                        children: vec![child.clone(), admin.clone()],
                        administrative: false,
                    },
                ),
                (
                    child,
                    Category {
                        articles: vec![base + 3, base + 1],
                        children: vec![root],
                        administrative: false,
                    },
                ),
                (
                    admin,
                    Category {
                        articles: vec![base + 4],
                        children: Vec::new(),
                        administrative: true,
                    },
                ),
            ]);
            let redirects = BTreeMap::from([(base + 2, base + 1)]);
            let disambiguations = BTreeSet::from([base + 3]);
            let rules = topic.rules();
            let first =
                resolve_with_redirects(&graph, &rules, &redirects, &disambiguations).unwrap();
            let again =
                resolve_with_redirects(&graph, &rules, &redirects, &disambiguations).unwrap();
            assert_eq!(first, again);
            assert_eq!(first.page_ids, vec![base + 1, base + 3]);
            assert!(first.warnings.iter().any(|w| w.contains("administrative")));
            assert!(first.warnings.iter().any(|w| w.contains("Disambiguation")));
            let mut limited = topic.rules();
            limited.page_limit = 1;
            assert_eq!(
                resolve_with_redirects(&graph, &limited, &redirects, &disambiguations),
                Err(ResolveError::TooManyPages)
            );
        }
    }
}
