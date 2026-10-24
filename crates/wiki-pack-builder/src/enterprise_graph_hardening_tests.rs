//! Adversarial regressions for public typed Enterprise graph inputs.
//! These inputs deliberately bypass the source JSON decoder.

use super::*;
use wiki_source::enterprise::StructuredArticle;
use wiki_source::join::PageKey;

fn joined_page(
    id: u64,
    namespace: i32,
    name: &str,
    categories: Vec<String>,
    redirects: Vec<String>,
) -> JoinedEnterpriseArticle {
    JoinedEnterpriseArticle {
        structured: StructuredArticle {
            page: PageKey {
                project: "enwiki".into(),
                page_id: id,
            },
            revision_id: id + 100,
            date_modified: "2026-10-23T00:00:00Z".into(),
            generation_id: "20261023".into(),
            namespace,
            language: "en".into(),
            name: name.into(),
            wikidata_id: None,
            description: None,
            infoboxes: Vec::new(),
            sections: Vec::new(),
            references: Vec::new(),
            tables: Vec::new(),
        },
        rendered_html: "<p>Verified elsewhere</p>".into(),
        categories,
        redirects,
    }
}

#[test]
fn typed_joined_graph_revalidates_memberships_and_source_identity() {
    let root = joined_page(14, 14, "Category:Science", vec![], vec![]);
    let article = joined_page(
        42,
        0,
        "Physics",
        vec!["Science".into()],
        vec!["علم الفيزياء".into()],
    );
    let graph = graph_from_joined(&[root.clone(), article.clone()], &BTreeSet::new()).unwrap();
    assert_eq!(graph.graph["Category:Science"].articles, vec![42]);
    assert!(graph.unresolved_redirect_titles.contains("علم الفيزياء"));

    for invalid in [
        " Science".to_owned(),
        "Science ".to_owned(),
        format!("Sci{}ence", '\u{202e}'),
        format!("Sci{}ence", '\u{200f}'),
        "x".repeat(4097),
        "Science#fragment".to_owned(),
        format!("Sci{}ence", '\u{200b}'),
        format!("Sci{}ence", '\u{2060}'),
        format!("Sci{}ence", '\u{feff}'),
    ] {
        let mut bad = article.clone();
        bad.categories = vec![invalid.clone()];
        assert!(
            matches!(
                graph_from_joined(&[root.clone(), bad], &BTreeSet::new()),
                Err(GraphBuildError::InvalidCategory(_))
            ),
            "invalid category accepted: {invalid}"
        );
    }
    for invalid in [
        " Physics".to_owned(),
        "Physics ".to_owned(),
        format!("Phy{}sics", '\u{2066}'),
        "x".repeat(4097),
    ] {
        let mut bad = article.clone();
        bad.redirects = vec![invalid.clone()];
        assert!(
            matches!(
                graph_from_joined(&[root.clone(), bad], &BTreeSet::new()),
                Err(GraphBuildError::InvalidRedirect(_))
            ),
            "invalid redirect accepted: {invalid}"
        );
    }
    let mut bad_root = root.clone();
    bad_root.structured.name = "Category:Sci\u{202e}ence".into();
    assert!(matches!(
        graph_from_joined(&[bad_root, article.clone()], &BTreeSet::new()),
        Err(GraphBuildError::InvalidCategory(_))
    ));

    for project in ["Enwiki", "bad/wiki", "enwiki_"] {
        let mut bad = article.clone();
        bad.structured.page.project = project.into();
        assert!(matches!(
            graph_from_joined(&[bad], &BTreeSet::new()),
            Err(GraphBuildError::InvalidIdentity)
        ));
    }
    for generation in [
        ".hidden".to_owned(),
        "2026/10/23".to_owned(),
        "bad ".to_owned(),
        "x".repeat(256),
    ] {
        let mut bad = article.clone();
        bad.structured.generation_id = generation;
        assert!(matches!(
            graph_from_joined(&[bad], &BTreeSet::new()),
            Err(GraphBuildError::InvalidIdentity)
        ));
    }
}

#[test]
fn administrative_policy_requires_canonical_safe_category_keys() {
    let root = joined_page(14, 14, "Category:Science", vec![], vec![]);
    let article = joined_page(42, 0, "Physics", vec!["Science".into()], vec![]);
    let records = [root, article];
    let allowed = BTreeSet::from(["Category:Science".to_owned()]);
    let graph = graph_from_joined(&records, &allowed).unwrap();
    assert!(graph.graph["Category:Science"].administrative);

    // A syntactically valid but absent administrative category must not be
    // silently ignored, since that can expand an unsafe topic subtree.
    let unknown = BTreeSet::from(["Category:Missing".to_owned()]);
    assert_eq!(
        graph_from_joined(&records, &unknown).unwrap_err(),
        GraphBuildError::MissingCategory("Category:Missing".into())
    );

    for invalid in ["Science", "Category: Science", "Category:Sci\u{202e}ence"] {
        let policy = BTreeSet::from([invalid.to_owned()]);
        assert!(matches!(
            graph_from_joined(&records, &policy),
            Err(GraphBuildError::InvalidCategory(_))
        ));
    }
}

#[test]
fn administrative_policy_is_exact_and_does_not_reject_legitimate_joiners() {
    let root = joined_page(14, 14, "Category:می‌روم क्‍ष", vec![], vec![]);
    let article = joined_page(42, 0, "Physics", vec!["می‌روم क्‍ष".into()], vec![]);
    let policy = BTreeSet::from(["Category:می‌روم क्‍ष".to_owned()]);
    let graph = graph_from_joined(&[root, article], &policy).unwrap();
    assert!(graph.graph["Category:می‌روم क्‍ष"].administrative);
    assert_eq!(graph.graph["Category:می‌روم क्‍ष"].articles, vec![42]);
}

#[test]
fn typed_graph_rejects_semantically_duplicate_memberships_and_aliases() {
    let root = joined_page(14, 14, "Category:Science", vec![], vec![]);
    let article = joined_page(
        42,
        0,
        "Physics",
        vec!["Science".into()],
        vec!["Physics alias".into()],
    );
    let mut duplicate_category = article.clone();
    duplicate_category.categories = vec!["Science".into(), "Category:Science".into()];
    assert_eq!(
        graph_from_joined(&[root.clone(), duplicate_category], &BTreeSet::new()).unwrap_err(),
        GraphBuildError::DuplicateCategory("Category:Science".into())
    );
    let mut duplicate_redirect = article.clone();
    duplicate_redirect.redirects = vec!["Physics alias".into(), "Physics alias".into()];
    assert_eq!(
        graph_from_joined(&[root.clone(), duplicate_redirect], &BTreeSet::new()).unwrap_err(),
        GraphBuildError::DuplicateRedirect("Physics alias".into())
    );
    let other = joined_page(
        43,
        0,
        "Chemistry",
        vec!["Science".into()],
        vec!["Physics alias".into()],
    );
    assert_eq!(
        graph_from_joined(&[root.clone(), article.clone(), other], &BTreeSet::new()).unwrap_err(),
        GraphBuildError::DuplicateRedirect("Physics alias".into())
    );
    let mut unique = article.clone();
    unique.redirects = vec!["Physics alias".into(), "فيزياء".into()];
    let graph = graph_from_joined(&[root, unique], &BTreeSet::new()).unwrap();
    assert_eq!(graph.unresolved_redirect_titles.len(), 2);
}

#[test]
fn typed_graph_shares_full_enterprise_unicode_membership_policy() {
    let root = joined_page(14, 14, "Category:Science", vec![], vec![]);
    let article = joined_page(42, 0, "Physics", vec!["Science".into()], vec![]);
    for code in [
        '\u{2028}', '\u{2029}', '\u{206a}', '\u{206b}', '\u{206c}', '\u{206d}', '\u{206e}',
        '\u{206f}',
    ] {
        let label = format!("Sci{code}ence");
        let mut bad = article.clone();
        bad.categories = vec![label.clone()];
        assert!(
            matches!(
                graph_from_joined(&[root.clone(), bad], &BTreeSet::new()),
                Err(GraphBuildError::InvalidCategory(_))
            ),
            "unsafe category accepted: U+{:04X}",
            code as u32
        );
        let mut bad = article.clone();
        bad.redirects = vec![label];
        assert!(
            matches!(
                graph_from_joined(&[root.clone(), bad], &BTreeSet::new()),
                Err(GraphBuildError::InvalidRedirect(_))
            ),
            "unsafe redirect accepted: U+{:04X}",
            code as u32
        );
    }
    let mut bad_title = article.clone();
    bad_title.structured.name = "Phy\u{206b}sics".into();
    assert_eq!(
        graph_from_joined(&[root.clone(), bad_title], &BTreeSet::new()).unwrap_err(),
        GraphBuildError::InvalidIdentity
    );
    let mut multilingual = article;
    multilingual.redirects = vec!["می‌روم क्‍ष".into()];
    let graph = graph_from_joined(&[root, multilingual], &BTreeSet::new()).unwrap();
    assert!(graph.unresolved_redirect_titles.contains("می‌روم क्‍ष"));
}

#[test]
fn typed_graph_rejects_mainspace_title_and_redirect_alias_collisions() {
    let root = joined_page(14, 14, "Category:Science", vec![], vec![]);
    let article = joined_page(42, 0, "Quantum mechanics", vec!["Science".into()], vec![]);
    let same_title = joined_page(43, 0, "Quantum mechanics", vec![], vec![]);
    let underscore_title = joined_page(43, 0, "Quantum_mechanics", vec![], vec![]);
    for duplicate in [same_title, underscore_title] {
        assert!(matches!(
            graph_from_joined(
                &[root.clone(), article.clone(), duplicate],
                &BTreeSet::new()
            ),
            Err(GraphBuildError::DuplicateTitle(_))
        ));
    }

    // The alias may precede or follow the conflicting canonical record.
    let redirecting = joined_page(
        43,
        0,
        "Other topic",
        vec![],
        vec!["Quantum_mechanics".into()],
    );
    for records in [
        vec![root.clone(), redirecting.clone(), article.clone()],
        vec![root.clone(), article.clone(), redirecting.clone()],
    ] {
        assert_eq!(
            graph_from_joined(&records, &BTreeSet::new()).unwrap_err(),
            GraphBuildError::RedirectTitleCollision("Quantum_mechanics".into())
        );
    }

    // Two differently spelled aliases may still resolve to the same title.
    let alias_a = joined_page(43, 0, "Other topic", vec![], vec!["Quantum_alias".into()]);
    let alias_b = joined_page(44, 0, "Another topic", vec![], vec!["Quantum alias".into()]);
    assert_eq!(
        graph_from_joined(&[root.clone(), alias_a, alias_b], &BTreeSet::new()).unwrap_err(),
        GraphBuildError::DuplicateRedirect("Quantum alias".into())
    );

    // A page cannot claim its own canonical title as a redirect alias.
    let mut self_alias = article.clone();
    self_alias.redirects = vec!["Quantum_mechanics".into()];
    assert_eq!(
        graph_from_joined(&[root.clone(), self_alias], &BTreeSet::new()).unwrap_err(),
        GraphBuildError::RedirectTitleCollision("Quantum_mechanics".into())
    );

    // Names in different namespaces are not conflicting mainspace identities.
    let category_with_same_name = joined_page(50, 14, "Category:Quantum mechanics", vec![], vec![]);
    let graph =
        graph_from_joined(&[root, article, category_with_same_name], &BTreeSet::new()).unwrap();
    assert!(graph.graph.contains_key("Category:Quantum mechanics"));
}

#[test]
fn category_underscore_space_aliases_share_one_canonical_graph_identity() {
    let category = joined_page(14, 14, "Category:Quantum_mechanics", vec![], vec![]);
    let article = joined_page(
        42,
        0,
        "Wavefunction",
        vec!["Quantum mechanics".into()],
        vec![],
    );
    let policy = BTreeSet::from(["Category:Quantum mechanics".to_owned()]);
    let graph = graph_from_joined(&[category.clone(), article.clone()], &policy).unwrap();
    assert_eq!(graph.graph.len(), 1);
    assert_eq!(graph.graph["Category:Quantum mechanics"].articles, vec![42]);
    assert!(graph.graph["Category:Quantum mechanics"].administrative);

    // Duplicate page titles and memberships that differ only by an
    // underscore are not distinct category identities.
    let duplicate = joined_page(15, 14, "Category:Quantum mechanics", vec![], vec![]);
    assert_eq!(
        graph_from_joined(&[category.clone(), duplicate], &BTreeSet::new()).unwrap_err(),
        GraphBuildError::DuplicateCategory("Category:Quantum mechanics".into())
    );
    let mut duplicate_membership = article.clone();
    duplicate_membership.categories = vec![
        "Quantum_mechanics".into(),
        "Category:Quantum mechanics".into(),
    ];
    assert_eq!(
        graph_from_joined(&[category.clone(), duplicate_membership], &BTreeSet::new()).unwrap_err(),
        GraphBuildError::DuplicateCategory("Category:Quantum mechanics".into())
    );

    // Administrative policy must use canonical category keys; accepting an
    // alias here would silently make an intended exclusion ineffective.
    let noncanonical_policy = BTreeSet::from(["Category:Quantum_mechanics".to_owned()]);
    assert_eq!(
        graph_from_joined(&[category.clone(), article.clone()], &noncanonical_policy).unwrap_err(),
        GraphBuildError::InvalidCategory("Category:Quantum_mechanics".into())
    );

    // Leading/trailing underscores normalize to illegal whitespace, not
    // silently to a different category root.
    for bad in ["Category:_Quantum", "Category:Quantum_", "___"] {
        let mut invalid = category.clone();
        invalid.structured.name = bad.into();
        assert_eq!(
            graph_from_joined(&[invalid, article.clone()], &BTreeSet::new()).unwrap_err(),
            GraphBuildError::InvalidCategory(bad.into())
        );
    }
}
