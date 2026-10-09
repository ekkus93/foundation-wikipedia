use std::collections::BTreeMap;
use wiki_pack_builder::selection::{resolve, Category, ResolveError, Rules};

#[test]
fn rejects_zero_page_limit() {
    let graph = BTreeMap::from([(
        "Physics".to_owned(),
        Category {
            articles: vec![1],
            children: vec![],
            administrative: false,
        },
    )]);
    let rules = Rules {
        roots: vec!["Physics".to_owned()],
        include: vec![],
        exclude: vec![],
        depth_limit: 1,
        page_limit: 0,
    };
    assert_eq!(resolve(&graph, &rules), Err(ResolveError::BadLimit));
}
