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
        graph_from_joined(&[root.clone(), article.clone(), other], &BTreeSet::new())
            .unwrap_err(),
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
    for code in ['\u{2028}', '\u{2029}', '\u{206a}', '\u{206b}', '\u{206c}', '\u{206d}', '\u{206e}', '\u{206f}'] {
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
