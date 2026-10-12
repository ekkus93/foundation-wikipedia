use std::collections::BTreeSet;
use wiki_pack_builder::enterprise_graph::{
    graph_from_joined, resolve_joined_revision_pinned, GraphBuildError, JoinedSelectionError,
};
use wiki_pack_builder::selection::{resolve, Rules};
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
        categories: categories.iter().map(|s| (*s).into()).collect(),
        redirects: vec![],
    }
}

#[test]
fn joined_categories_feed_deterministic_resolver() {
    let records = vec![
        record(20, 14, "Category:Math", &[]),
        record(21, 14, "Category:Algebra", &["Category:Math"]),
        record(2, 0, "Group", &["Category:Algebra"]),
        record(3, 0, "Number", &["Category:Math", "Category:Algebra"]),
    ];
    let graph = graph_from_joined(&records, &BTreeSet::new()).unwrap();
    assert_eq!(graph.project, "enwiki");
    assert_eq!(graph.generation_id, "20261001");
    assert_eq!(graph.page_revisions[&2], 102);
    assert_eq!(graph.graph["Category:Math"].articles, vec![3]);
    assert_eq!(
        graph.graph["Category:Math"].children,
        vec!["Category:Algebra"]
    );
    let rules = Rules {
        roots: vec!["Category:Math".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 1,
        page_limit: 10,
    };
    assert_eq!(resolve(&graph.graph, &rules).unwrap().page_ids, vec![2, 3]);
    let mut reordered = records.clone();
    reordered.reverse();
    let again = graph_from_joined(&reordered, &BTreeSet::new()).unwrap();
    assert_eq!(again.graph["Category:Math"].articles, vec![3]);
    assert_eq!(
        again.graph["Category:Math"].children,
        vec!["Category:Algebra"]
    );
}

#[test]
fn incomplete_or_mixed_source_fails_closed() {
    let page = record(2, 0, "Group", &["Category:Math"]);
    assert!(matches!(
        graph_from_joined(&[page.clone()], &BTreeSet::new()),
        Err(GraphBuildError::MissingCategory(_))
    ));
    let category = record(20, 14, "Category:Math", &[]);
    let mut mixed = page.clone();
    mixed.structured.generation_id = "20261002".into();
    assert!(matches!(
        graph_from_joined(&[category.clone(), mixed], &BTreeSet::new()),
        Err(GraphBuildError::MixedGeneration)
    ));
    let mut wrong_project = page.clone();
    wrong_project.structured.page.project = "frwiki".into();
    assert!(matches!(
        graph_from_joined(&[category.clone(), wrong_project], &BTreeSet::new()),
        Err(GraphBuildError::MixedProject)
    ));
    assert!(matches!(
        graph_from_joined(&[category.clone(), category], &BTreeSet::new()),
        Err(GraphBuildError::DuplicatePage(20))
    ));
}

#[test]
fn administrative_policy_is_explicit_and_redirect_aliases_are_not_ids() {
    let mut article = record(2, 0, "Group", &["Category:Maintenance"]);
    article.redirects.push("Group theory alias".into());
    let records = vec![record(20, 14, "Category:Maintenance", &[]), article];
    let graph =
        graph_from_joined(&records, &BTreeSet::from(["Category:Maintenance".into()])).unwrap();
    assert!(graph.graph["Category:Maintenance"].administrative);
    assert!(graph
        .unresolved_redirect_titles
        .contains("Group theory alias"));
    let rules = Rules {
        roots: vec!["Category:Maintenance".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 1,
        page_limit: 10,
    };
    let result = resolve(&graph.graph, &rules).unwrap();
    assert!(result.page_ids.is_empty());
    assert_eq!(result.warnings.len(), 1);
}

#[test]
fn pinned_selection_detects_revision_changes_and_reordering() {
    let records = vec![
        record(20, 14, "Category:Math", &[]),
        record(21, 14, "Category:Algebra", &["Category:Math"]),
        record(2, 0, "Group", &["Category:Algebra"]),
        record(3, 0, "Number", &["Category:Math"]),
    ];
    let rules = Rules {
        roots: vec!["Category:Math".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 1,
        page_limit: 10,
    };
    let first = resolve_joined_revision_pinned(&records, &BTreeSet::new(), &rules).unwrap();
    assert_eq!(first.selection.page_ids, vec![2, 3]);
    assert_eq!(first.page_revisions[&2], 102);
    assert_eq!(first.page_revisions[&3], 103);
    assert_eq!(first.revision_selection_sha256.len(), 64);
    let mut reordered = records.clone();
    reordered.reverse();
    assert_eq!(
        first,
        resolve_joined_revision_pinned(&reordered, &BTreeSet::new(), &rules).unwrap()
    );
    let mut updated = records.clone();
    updated[2].structured.revision_id += 1;
    let second = resolve_joined_revision_pinned(&updated, &BTreeSet::new(), &rules).unwrap();
    assert_eq!(first.selection.page_ids, second.selection.page_ids);
    assert_eq!(
        first.selection.snapshot_selection_sha256,
        second.selection.snapshot_selection_sha256
    );
    assert_ne!(
        first.revision_selection_sha256,
        second.revision_selection_sha256
    );
    let mut new_generation = records.clone();
    for record in &mut new_generation {
        record.structured.generation_id = "20261002".into();
    }
    assert_ne!(
        first.revision_selection_sha256,
        resolve_joined_revision_pinned(&new_generation, &BTreeSet::new(), &rules)
            .unwrap()
            .revision_selection_sha256
    );
}

#[test]
fn pinned_selection_rejects_unjoined_explicit_ids_and_category_page_ids() {
    let records = vec![
        record(20, 14, "Category:Math", &[]),
        record(2, 0, "Group", &["Category:Math"]),
    ];
    let mut rules = Rules {
        roots: vec!["Category:Math".into()],
        include: vec![999],
        exclude: vec![],
        depth_limit: 1,
        page_limit: 10,
    };
    assert_eq!(
        resolve_joined_revision_pinned(&records, &BTreeSet::new(), &rules).unwrap_err(),
        JoinedSelectionError::NotMainspace(999)
    );
    rules.include = vec![20];
    assert_eq!(
        resolve_joined_revision_pinned(&records, &BTreeSet::new(), &rules).unwrap_err(),
        JoinedSelectionError::NotMainspace(20)
    );
    rules.include.clear();
    rules.exclude = vec![999];
    assert_eq!(
        resolve_joined_revision_pinned(&records, &BTreeSet::new(), &rules).unwrap_err(),
        JoinedSelectionError::NotMainspace(999)
    );
    rules.exclude = vec![2];
    assert!(
        resolve_joined_revision_pinned(&records, &BTreeSet::new(), &rules)
            .unwrap()
            .selection
            .page_ids
            .is_empty()
    );
}

#[test]
fn pinned_selection_preserves_unresolved_aliases_and_rejects_mixed_sources() {
    let mut records = vec![
        record(20, 14, "Category:Math", &[]),
        record(2, 0, "Group", &["Category:Math"]),
    ];
    records[1].redirects.push("Old group name".into());
    let rules = Rules {
        roots: vec!["Category:Math".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 1,
        page_limit: 10,
    };
    let result = resolve_joined_revision_pinned(&records, &BTreeSet::new(), &rules).unwrap();
    assert!(result.unresolved_redirect_titles.contains("Old group name"));
    assert_eq!(result.selection.page_ids, vec![2]);
    records[1].structured.generation_id = "20261002".into();
    assert!(matches!(
        resolve_joined_revision_pinned(&records, &BTreeSet::new(), &rules),
        Err(JoinedSelectionError::Graph(
            GraphBuildError::MixedGeneration
        ))
    ));
}

#[test]
fn pinned_selection_changes_when_unselected_source_revision_or_alias_changes() {
    let original = vec![
        record(20, 14, "Category:Math", &[]),
        record(2, 0, "Group", &["Category:Math"]),
        record(55, 0, "Unselected", &[]),
    ];
    let rules = Rules {
        roots: vec!["Category:Math".into()],
        include: vec![],
        exclude: vec![],
        depth_limit: 1,
        page_limit: 10,
    };
    let first = resolve_joined_revision_pinned(&original, &BTreeSet::new(), &rules).unwrap();
    assert_eq!(first.selection.page_ids, vec![2]);

    let mut category_changed = original.clone();
    category_changed[0].structured.revision_id += 1;
    let changed =
        resolve_joined_revision_pinned(&category_changed, &BTreeSet::new(), &rules).unwrap();
    assert_eq!(first.selection.page_ids, changed.selection.page_ids);
    assert_ne!(
        first.revision_selection_sha256,
        changed.revision_selection_sha256
    );

    let mut other_changed = original.clone();
    other_changed[2].structured.revision_id += 1;
    let changed = resolve_joined_revision_pinned(&other_changed, &BTreeSet::new(), &rules).unwrap();
    assert_eq!(first.selection.page_ids, changed.selection.page_ids);
    assert_ne!(
        first.revision_selection_sha256,
        changed.revision_selection_sha256
    );

    let mut alias_changed = original.clone();
    alias_changed[1].redirects.push("Previous name".into());
    let changed = resolve_joined_revision_pinned(&alias_changed, &BTreeSet::new(), &rules).unwrap();
    assert_eq!(first.selection.page_ids, changed.selection.page_ids);
    assert_ne!(
        first.revision_selection_sha256,
        changed.revision_selection_sha256
    );
}
