use std::collections::{BTreeMap, BTreeSet};
use wiki_pack_builder::selection::{
    resolve, resolve_with_redirects, Category, ResolveError, Rules,
};

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

fn rules() -> Rules {
    Rules {
        roots: vec!["Root".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 5,
        page_limit: 10,
    }
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
    assert_eq!(resolve(&original, &rules()), resolve(&reordered, &rules()));
}

#[test]
fn redirect_aliases_count_only_once_and_flag_disambiguation() {
    let mut categories = BTreeMap::new();
    categories.insert(
        "Root".into(),
        Category {
            articles: vec![2, 3, 4],
            children: vec![],
            administrative: false,
        },
    );
    let redirects = BTreeMap::from([(2, 3), (3, 1), (4, 1)]);
    let disambiguations = BTreeSet::from([1]);
    let config = Rules {
        roots: vec!["Root".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 0,
        page_limit: 1,
    };
    let result =
        resolve_with_redirects(&categories, &config, &redirects, &disambiguations).unwrap();
    assert_eq!(result.page_ids, vec![1]);
    assert!(result
        .warnings
        .contains(&"Disambiguation article: 1".into()));
    assert!(result
        .warnings
        .contains(&"Resolved redirect: 2 -> 1".into()));
}

#[test]
fn exclusion_applies_to_canonical_target_not_just_redirect_alias() {
    let redirects = BTreeMap::from([(3, 2), (2, 1)]);
    let mut config = rules();
    config.roots.clear();
    config.include = vec![3];
    config.exclude = vec![1];
    let result = resolve_with_redirects(&graph(), &config, &redirects, &BTreeSet::new()).unwrap();
    assert!(result.page_ids.is_empty());
    config.exclude = vec![3];
    let result = resolve_with_redirects(&graph(), &config, &redirects, &BTreeSet::new()).unwrap();
    assert!(result.page_ids.is_empty());
}

#[test]
fn redirect_cycles_and_zero_targets_fail_closed() {
    let mut config = rules();
    config.roots.clear();
    config.include = vec![3];
    let cycle = BTreeMap::from([(3, 4), (4, 3)]);
    assert!(matches!(
        resolve_with_redirects(&graph(), &config, &cycle, &BTreeSet::new()),
        Err(ResolveError::RedirectCycle(_))
    ));
    let zero = BTreeMap::from([(3, 0)]);
    assert_eq!(
        resolve_with_redirects(&graph(), &config, &zero, &BTreeSet::new()),
        Err(ResolveError::InvalidPage)
    );
}

#[test]
fn excessive_category_fanout_is_rejected_before_enqueuing_children() {
    let mut categories = BTreeMap::new();
    categories.insert(
        "Root".into(),
        Category {
            articles: vec![1],
            children: (0..1025).map(|n| format!("Child{n}")).collect(),
            administrative: false,
        },
    );
    let mut config = rules();
    config.page_limit = 1;
    assert_eq!(
        resolve(&categories, &config),
        Err(ResolveError::TooManyCategories)
    );
    config.depth_limit = 0;
    let result = resolve(&categories, &config).unwrap();
    assert_eq!(result.page_ids, vec![1]);
    assert!(result.warnings.contains(&"Depth limit reached: Root".into()));
}

#[test]
fn excessive_root_fanout_is_rejected_before_queue_allocation() {
    let mut config = rules();
    config.page_limit = 1;
    config.roots = (0..1025).map(|n| format!("Root{n}")).collect();
    assert_eq!(resolve(&BTreeMap::new(), &config), Err(ResolveError::TooManyCategories));
}
