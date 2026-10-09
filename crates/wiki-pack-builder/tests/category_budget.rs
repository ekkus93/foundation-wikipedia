use std::collections::BTreeMap;
use wiki_pack_builder::selection::{resolve, Category, ResolveError, Rules};

#[test]
fn empty_category_fanout_has_a_finite_budget() {
    let children: Vec<String> = (0..1025).map(|index| format!("C{index}")).collect();
    let mut graph = BTreeMap::new();
    for child in &children {
        graph.insert(child.clone(), Category::default());
    }
    graph.insert(
        "Root".into(),
        Category {
            articles: vec![],
            children,
            administrative: false,
        },
    );
    let rules = Rules {
        roots: vec!["Root".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 1,
        page_limit: 1,
    };
    assert_eq!(resolve(&graph, &rules), Err(ResolveError::TooManyCategories));
}
