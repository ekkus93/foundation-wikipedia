use std::collections::BTreeMap;
use wiki_pack_builder::selection::{resolve, Category, ResolveError, Rules};

fn graph() -> BTreeMap<String, Category> {
    BTreeMap::from([
        (
            "Science".into(),
            Category {
                articles: vec![7, 3],
                children: vec!["Math".into(), "Admin".into()],
                administrative: false,
            },
        ),
        (
            "Math".into(),
            Category {
                articles: vec![5, 3],
                children: vec!["Science".into()],
                administrative: false,
            },
        ),
        (
            "Admin".into(),
            Category {
                articles: vec![900],
                children: vec![],
                administrative: true,
            },
        ),
    ])
}

fn rules() -> Rules {
    Rules {
        roots: vec!["Science".into()],
        include: vec![11],
        exclude: vec![3],
        depth_limit: 4,
        page_limit: 30,
    }
}

#[test]
fn stable_cycle_safe_selection_and_admin_filter() {
    let outcome = resolve(&graph(), &rules()).unwrap();
    assert_eq!(outcome.page_ids, vec![5, 7, 11]);
    assert_eq!(
        outcome.warnings,
        ["Skipped administrative category: Admin"]
    );
    assert_eq!(outcome, resolve(&graph(), &rules()).unwrap());
}

#[test]
fn strict_limits_and_missing_categories() {
    let mut limits = rules();
    limits.depth_limit = 0;
    assert_eq!(resolve(&graph(), &limits).unwrap().page_ids, vec![7, 11]);
    limits.page_limit = 1;
    assert_eq!(
        resolve(&graph(), &limits),
        Err(ResolveError::TooManyPages)
    );
    limits.page_limit = 20;
    limits.roots = vec!["Missing".into()];
    assert_eq!(
        resolve(&graph(), &limits),
        Err(ResolveError::Missing("Missing".into()))
    );
}

#[test]
fn explicit_exclusion_takes_precedence() {
    let mut selection = rules();
    selection.include = vec![3, 11];
    assert_eq!(resolve(&graph(), &selection).unwrap().page_ids, vec![5, 7, 11]);
}
