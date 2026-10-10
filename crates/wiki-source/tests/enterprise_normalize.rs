use wiki_source::enterprise::{
    join_enterprise_article, parse_regular_companion, parse_structured_article,
};
use wiki_source::enterprise_verified::enterprise_normalize_article::normalize_enterprise_article;
use wiki_source::enterprise_verified::enterprise_normalize::NormalizeError;

fn fixture() -> wiki_source::enterprise::JoinedEnterpriseArticle {
    let base = serde_json::json!({
        "identifier": 42, "name": "Gravity", "version": {"identifier": 99},
        "date_modified": "2026-10-10T00:00:00Z",
        "is_part_of": {"identifier": "enwiki"},
        "in_language": {"identifier": "en"}, "namespace": {"identifier": 0}
    });
    let mut structured = base.clone();
    structured["sections"] = serde_json::json!([{
        "type": "section", "name": "Theory",
        "has_parts": [{"type": "paragraph", "value": "Gravity bends spacetime."}]
    }]);
    let mut regular = base;
    regular["article_body"] = serde_json::json!({"html": "<article>Gravity</article>"});
    regular["categories"] = serde_json::json!([{"name": "Physics"}]);
    join_enterprise_article(
        parse_structured_article(&structured.to_string(), "20261010").unwrap(),
        parse_regular_companion(&regular.to_string(), "20261010").unwrap(),
    ).unwrap()
}

#[test]
fn normalizes_revision_and_rejects_unverified_images() {
    let joined = fixture();
    let canonical = normalize_enterprise_article(&joined, false).unwrap();
    assert_eq!(canonical.article.key.page_id, 42);
    assert_eq!(canonical.article.revision.revision_id, 99);
    assert_eq!(canonical.categories, ["Physics"]);
    assert_eq!(canonical.article.sections[0].heading, "Theory");
    assert_eq!(normalize_enterprise_article(&joined, false).unwrap(), canonical);
    canonical.article.validate().unwrap();
    let mut bad = joined;
    bad.rendered_html.push_str("<img src='missing.svg'>");
    assert_eq!(normalize_enterprise_article(&bad, false), Err(NormalizeError::UnresolvedVisual));
}
