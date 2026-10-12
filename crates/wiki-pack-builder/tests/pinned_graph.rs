use std::collections::{BTreeMap, BTreeSet};
use wiki_pack_builder::selection::{resolve_pinned, Category, Rules};

#[test]
fn pinned_receipt_detects_graph_drift_without_changing_selected_page_ids() {
    let mut graph = BTreeMap::from([
        (
            "Root".to_owned(),
            Category {
                articles: vec![2, 3],
                children: vec!["Child".into()],
                administrative: false,
            },
        ),
        (
            "Child".to_owned(),
            Category {
                articles: vec![4],
                children: vec![],
                administrative: false,
            },
        ),
    ]);
    let rules = Rules {
        roots: vec!["Root".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 0,
        page_limit: 10,
    };
    let redirects = BTreeMap::new();
    let disambiguations = BTreeSet::new();
    let baseline = resolve_pinned(
        &graph,
        &rules,
        &redirects,
        &disambiguations,
        "enwiki-20261001",
    )
    .unwrap();

    graph.get_mut("Child").unwrap().articles.push(5);
    let changed = resolve_pinned(
        &graph,
        &rules,
        &redirects,
        &disambiguations,
        "enwiki-20261001",
    )
    .unwrap();
    assert_eq!(baseline.page_ids, changed.page_ids);
    assert_eq!(baseline.definition_sha256, changed.definition_sha256);
    assert_ne!(baseline.graph_sha256, changed.graph_sha256);
    assert_ne!(
        baseline.snapshot_selection_sha256,
        changed.snapshot_selection_sha256
    );

    let redirect_change = resolve_pinned(
        &graph,
        &rules,
        &BTreeMap::from([(99, 100)]),
        &disambiguations,
        "enwiki-20261001",
    )
    .unwrap();
    assert_eq!(changed.page_ids, redirect_change.page_ids);
    assert_ne!(changed.graph_sha256, redirect_change.graph_sha256);
    assert_ne!(
        changed.snapshot_selection_sha256,
        redirect_change.snapshot_selection_sha256
    );
}

#[test]
fn graph_digest_normalizes_duplicate_member_order() {
    let mut graph = BTreeMap::from([(
        "Root".to_owned(),
        Category {
            articles: vec![3, 2],
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
    let first = resolve_pinned(
        &graph,
        &rules,
        &BTreeMap::new(),
        &BTreeSet::new(),
        "snapshot",
    )
    .unwrap();
    graph.get_mut("Root").unwrap().articles = vec![2, 3, 2];
    let again = resolve_pinned(
        &graph,
        &rules,
        &BTreeMap::new(),
        &BTreeSet::new(),
        "snapshot",
    )
    .unwrap();
    assert_eq!(first, again);
}
