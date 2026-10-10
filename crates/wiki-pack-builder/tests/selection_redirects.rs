use std::collections::{BTreeMap, BTreeSet};
use wiki_pack_builder::selection::{resolve_with_redirects, Category, ResolveError, Rules};

fn selection() -> Rules {
    Rules {
        roots: vec!["Physics".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 2,
        page_limit: 10,
    }
}

#[test]
fn redirect_cycles_fail_closed() {
    let graph = BTreeMap::from([(
        "Physics".into(),
        Category {
            articles: vec![10],
            children: vec![],
            administrative: false,
        },
    )]);
    let redirects = BTreeMap::from([(10, 20), (20, 10)]);
    assert!(matches!(
        resolve_with_redirects(&graph, &selection(), &redirects, &BTreeSet::new()),
        Err(ResolveError::RedirectCycle(_))
    ));
}

#[test]
fn duplicate_candidate_bombs_are_bounded() {
    let graph = BTreeMap::from([(
        "Physics".into(),
        Category {
            articles: vec![1; 4097],
            children: vec![],
            administrative: false,
        },
    )]);
    assert_eq!(
        resolve_with_redirects(&graph, &selection(), &BTreeMap::new(), &BTreeSet::new()),
        Err(ResolveError::TooManyCandidates)
    );
}
