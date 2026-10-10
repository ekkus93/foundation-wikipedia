use std::collections::{BTreeMap, BTreeSet};
use wiki_pack_builder::selection::{
    resolve_with_redirects, Category, ResolveError, Rules,
};

fn rules() -> Rules {
    Rules {
        roots: vec!["Physics".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 2,
        page_limit: 10,
    }
}

#[test]
fn redirects_and_disambiguations_have_stable_identity() {
    let graph = BTreeMap::from([(
        "Physics".into(),
        Category {
            articles: vec![10, 30, 20],
            children: vec![],
            administrative: false,
        },
    )]);
    let redirects = BTreeMap::from([(10, 20)]);
    let disambiguations = BTreeSet::from([30]);
    let mut selection = rules();
    selection.exclude = vec![10];
    let result =
        resolve_with_redirects(&graph, &selection, &redirects, &disambiguations).unwrap();
    assert_eq!(result.page_ids, vec![30]);
    assert!(result.warnings.iter().any(|item| item.contains("Disambiguation")));
}

#[test]
fn redirect_cycles_and_duplicate_candidate_bombs_fail_closed() {
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
        resolve_with_redirects(&graph, &rules(), &redirects, &BTreeSet::new()),
        Err(ResolveError::RedirectCycle(_))
    ));
    let bomb = BTreeMap::from([(
        "Physics".into(),
        Category {
            articles: vec![1; 4097],
            children: vec![],
            administrative: false,
        },
    )]);
    assert_eq!(
        resolve_with_redirects(&bomb, &rules(), &BTreeMap::new(), &BTreeSet::new()),
        Err(ResolveError::TooManyCandidates)
    );
}
