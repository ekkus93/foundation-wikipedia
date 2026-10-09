use std::collections::BTreeMap;
use wiki_pack_builder::selection::{resolve, Category, ResolveError, Rules};

fn fixture() -> BTreeMap<String, Category> {
    BTreeMap::from([
        ("Root".into(), Category {
            articles: vec![1, 2, 3],
            children: vec!["Child".into()],
            administrative: false,
        }),
        ("Child".into(), Category {
            articles: vec![4],
            children: vec![],
            administrative: false,
        }),
    ])
}

fn rules() -> Rules {
    Rules {
        roots: vec!["Root".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 1,
        page_limit: 10,
    }
}

#[test]
fn depth_limit_reports_truncation() {
    let mut config = rules();
    config.depth_limit = 0;
    let result = resolve(&fixture(), &config).unwrap();
    assert_eq!(result.page_ids, vec![1, 2, 3]);
    assert_eq!(result.warnings, vec!["Depth limit reached: Root"]);
}

#[test]
fn invalid_limits_and_missing_categories_fail_closed() {
    let mut config = rules();
    config.page_limit = 3;
    assert_eq!(
        resolve(&fixture(), &config),
        Err(ResolveError::TooManyPages)
    );
    config.page_limit = 10;
    config.include = vec![0];
    assert_eq!(resolve(&fixture(), &config), Err(ResolveError::InvalidPage));
    config.include.clear();
    config.roots = vec!["Missing".into()];
    assert_eq!(
        resolve(&fixture(), &config),
        Err(ResolveError::Missing("Missing".into()))
    );
}
