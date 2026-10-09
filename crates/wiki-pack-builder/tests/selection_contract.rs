use std::collections::BTreeMap;
use wiki_pack_builder::selection::{resolve, Category, ResolveError, Rules};

fn rules() -> Rules {
    Rules {
        roots: vec!["Root".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 5,
        page_limit: 10,
    }
}

fn graph() -> BTreeMap<String, Category> {
    BTreeMap::from([
        (
            "Root".into(),
            Category {
                articles: vec![3, 1, 2],
                children: vec!["Child".into(), "Admin".into()],
                administrative: false,
            },
        ),
        (
            "Child".into(),
            Category {
                articles: vec![4, 3],
                children: vec!["Root".into()],
                administrative: false,
            },
        ),
        (
            "Admin".into(),
            Category {
                articles: vec![999],
                children: vec![],
                administrative: true,
            },
        ),
    ])
}

#[test]
fn cycle_and_admin_category_are_bounded_and_reported() {
    let mut config = rules();
    config.include = vec![5, 5];
    config.exclude = vec![2];
    let result = resolve(&graph(), &config).unwrap();
    assert_eq!(result.page_ids, vec![1, 3, 4, 5]);
    assert_eq!(
        result.warnings,
        vec!["Skipped administrative category: Admin"]
    );
}

#[test]
fn selection_is_deterministic_across_graph_orderings() {
    let original = graph();
    let mut reordered = original.clone();
    reordered.get_mut("Root").unwrap().articles.reverse();
    reordered.get_mut("Root").unwrap().children.reverse();
    let expected = resolve(&original, &rules()).unwrap();
    let actual = resolve(&reordered, &rules()).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn depth_limit_reports_truncation_without_following_children() {
    let mut config = rules();
    config.depth_limit = 0;
    let result = resolve(&graph(), &config).unwrap();
    assert_eq!(result.page_ids, vec![1, 2, 3]);
    assert_eq!(result.warnings, vec!["Depth limit reached: Root"]);
}

#[test]
fn invalid_page_limits_and_missing_categories_fail_closed() {
    let mut config = rules();
    config.page_limit = 3;
    assert_eq!(
        resolve(&graph(), &config),
        Err(ResolveError::TooManyPages)
    );
    config.page_limit = 10;
    config.include = vec![0];
    assert_eq!(resolve(&graph(), &config), Err(ResolveError::InvalidPage));
    config.include.clear();
    config.roots = vec!["Missing".into()];
    assert_eq!(
        resolve(&graph(), &config),
        Err(ResolveError::Missing("Missing".into()))
    );
}
