use std::collections::BTreeMap;
use wiki_pack_builder::selection::{resolve, Category, Rules};

#[test]
fn explicit_exclusion_overrides_category_and_manual_inclusion() {
    let mut graph = BTreeMap::new();
    graph.insert(
        "Root".into(),
        Category {
            articles: vec![2, 1],
            children: vec![],
            administrative: false,
        },
    );
    let rules = Rules {
        roots: vec!["Root".into()],
        include: vec![3, 2, 3],
        exclude: vec![2],
        depth_limit: 0,
        page_limit: 2,
    };
    let result = resolve(&graph, &rules).unwrap();
    assert_eq!(result.page_ids, vec![1, 3]);
}
