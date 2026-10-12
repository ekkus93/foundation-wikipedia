use std::collections::{BTreeMap, BTreeSet};
use wiki_pack_builder::preview::preview_pinned_selection;
use wiki_pack_builder::selection::{resolve_pinned, Category, ResolveError, Rules};

fn graph() -> BTreeMap<String, Category> {
    BTreeMap::from([
        (
            "Root".into(),
            Category {
                articles: vec![3, 2, 3],
                children: vec!["Child".into(), "Admin".into()],
                administrative: false,
            },
        ),
        (
            "Child".into(),
            Category {
                articles: vec![4, 2],
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
        include: vec![8, 8],
        exclude: vec![2, 2],
        depth_limit: 2,
        page_limit: 100,
    }
}

#[test]
fn previews_resolved_ids_counts_warnings_and_snapshot_receipts() {
    let graph = graph();
    let redirects = BTreeMap::from([(4, 3)]);
    let disambiguations = BTreeSet::from([3]);
    let preview =
        preview_pinned_selection(&graph, &rules(), &redirects, &disambiguations, "enwiki-1")
            .unwrap();
    let original = resolve_pinned(&graph, &rules(), &redirects, &disambiguations, "enwiki-1")
        .unwrap();
    assert_eq!(preview.selection, original);
    assert_eq!(preview.selection.page_ids, vec![3, 8]);
    assert_eq!(preview.selected_page_count, 2);
    assert_eq!(preview.visited_category_count, 3);
    assert_eq!(preview.skipped_administrative_count, 1);
    assert_eq!(preview.inspected_article_mentions, 5);
    assert_eq!(preview.explicit_include_ids, 1);
    assert_eq!(preview.explicit_exclude_ids, 1);
    assert_eq!(preview.largest_category.as_ref().unwrap().name, "Root");
    assert_eq!(preview.largest_category.as_ref().unwrap().article_mentions, 3);
    assert!(preview.warnings.iter().any(|warning| warning.contains("administrative")));
    assert!(preview.warnings.iter().any(|warning| warning.contains("Disambiguation")));
    assert_eq!(
        preview,
        preview_pinned_selection(&graph, &rules(), &redirects, &disambiguations, "enwiki-1")
            .unwrap()
    );
    assert_ne!(
        preview.selection.snapshot_selection_sha256,
        preview_pinned_selection(&graph, &rules(), &redirects, &disambiguations, "enwiki-2")
            .unwrap()
            .selection
            .snapshot_selection_sha256
    );
}

#[test]
fn unusually_large_selection_is_visible_before_media_download() {
    let graph = BTreeMap::from([(
        "Huge".into(),
        Category {
            articles: (1..=950).collect(),
            children: vec![],
            administrative: false,
        },
    )]);
    let rules = Rules {
        roots: vec!["Huge".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 0,
        page_limit: 1000,
    };
    let preview =
        preview_pinned_selection(&graph, &rules, &BTreeMap::new(), &BTreeSet::new(), "enwiki-1")
            .unwrap();
    assert_eq!(preview.selected_page_count, 950);
    assert!(preview.warnings.iter().any(|warning| warning.contains("broad category")));
    assert!(preview.warnings.iter().any(|warning| warning.contains("near page limit")));
}

#[test]
fn preview_fails_closed_when_selection_is_invalid() {
    let mut invalid = rules();
    invalid.roots = vec!["Missing".into()];
    assert_eq!(
        preview_pinned_selection(
            &graph(),
            &invalid,
            &BTreeMap::new(),
            &BTreeSet::new(),
            "enwiki-1"
        ),
        Err(ResolveError::Missing("Missing".into()))
    );
    invalid.roots = vec!["Root".into()];
    invalid.page_limit = 1;
    assert_eq!(
        preview_pinned_selection(
            &graph(),
            &invalid,
            &BTreeMap::new(),
            &BTreeSet::new(),
            "enwiki-1"
        ),
        Err(ResolveError::TooManyPages)
    );
}
