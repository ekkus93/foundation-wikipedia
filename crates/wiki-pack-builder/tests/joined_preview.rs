use std::collections::BTreeSet;

use wiki_pack_builder::enterprise_graph::{
    JoinedSelectionError, PageRedirectIdentity, RedirectBuildError,
};
use wiki_pack_builder::joined_preview::{preview_joined_selection, JoinedPreviewError};
use wiki_pack_builder::selection::Rules;
use wiki_source::enterprise::{JoinedEnterpriseArticle, StructuredArticle};
use wiki_source::join::PageKey;

fn record(id: u64, namespace: i32, title: &str, categories: &[&str]) -> JoinedEnterpriseArticle {
    JoinedEnterpriseArticle {
        structured: StructuredArticle {
            page: PageKey {
                project: "enwiki".into(),
                page_id: id,
            },
            revision_id: 100 + id,
            date_modified: "2026-10-01T00:00:00Z".into(),
            generation_id: "20261001".into(),
            namespace,
            language: "en".into(),
            name: title.into(),
            wikidata_id: None,
            description: None,
            infoboxes: vec![],
            sections: vec![],
            references: vec![],
            tables: vec![],
        },
        rendered_html: "<p>fixture</p>".into(),
        categories: categories.iter().map(|name| (*name).into()).collect(),
        redirects: vec![],
    }
}

#[test]
fn joined_preview_preserves_revision_receipt_and_exact_selection_counts() {
    let records = vec![
        record(20, 14, "Category:Math", &[]),
        record(21, 14, "Category:Algebra", &["Category:Math"]),
        record(2, 0, "Group", &["Category:Algebra"]),
        record(3, 0, "Number", &["Category:Math"]),
        record(4, 0, "Unselected", &[]),
    ];
    let rules = Rules {
        roots: vec!["Category:Math".into()],
        include: vec![101],
        exclude: vec![3],
        depth_limit: 2,
        page_limit: 20,
    };
    let redirects = vec![
        PageRedirectIdentity {
            project: "enwiki".into(),
            generation_id: "20261001".into(),
            from_page_id: 101,
            to_page_id: 100,
        },
        PageRedirectIdentity {
            project: "enwiki".into(),
            generation_id: "20261001".into(),
            from_page_id: 100,
            to_page_id: 2,
        },
    ];
    let first = preview_joined_selection(&records, &BTreeSet::new(), &rules, &redirects).unwrap();
    assert_eq!(first.revision_pinned.selection.page_ids, vec![2]);
    assert_eq!(first.revision_pinned.page_revisions[&2], 102);
    assert_eq!(first.preview.selection, first.revision_pinned.selection);
    assert_eq!(first.preview.selected_page_count, 1);
    assert_eq!(first.preview.visited_category_count, 2);
    assert_eq!(first.preview.inspected_article_mentions, 2);
    assert_eq!(first.preview.explicit_include_ids, 1);
    assert_eq!(first.preview.explicit_exclude_ids, 1);
    assert!(first
        .preview
        .warnings
        .iter()
        .any(|warning| warning.contains("Resolved redirect: 101 -> 2")));
    let mut reordered_records = records;
    reordered_records.reverse();
    let mut reordered_redirects = redirects;
    reordered_redirects.reverse();
    assert_eq!(
        first,
        preview_joined_selection(
            &reordered_records,
            &BTreeSet::new(),
            &rules,
            &reordered_redirects,
        )
        .unwrap()
    );
}

#[test]
fn joined_preview_rejects_unverified_generation_and_missing_categories() {
    let records = vec![
        record(20, 14, "Category:Math", &[]),
        record(2, 0, "Group", &["Category:Math"]),
    ];
    let rules = Rules {
        roots: vec!["Category:Math".into()],
        include: vec![101],
        exclude: vec![],
        depth_limit: 0,
        page_limit: 20,
    };
    let invalid = PageRedirectIdentity {
        project: "enwiki".into(),
        generation_id: "different".into(),
        from_page_id: 101,
        to_page_id: 2,
    };
    assert_eq!(
        preview_joined_selection(&records, &BTreeSet::new(), &rules, &[invalid]),
        Err(JoinedPreviewError::Joined(JoinedSelectionError::Redirect(
            RedirectBuildError::MixedSource
        )))
    );
    let missing = vec![record(2, 0, "Group", &["Category:Math"])];
    assert!(matches!(
        preview_joined_selection(&missing, &BTreeSet::new(), &rules, &[]),
        Err(JoinedPreviewError::Joined(JoinedSelectionError::Graph(_)))
    ));
}
