use wiki_source::enterprise::{
    join_enterprise_article, parse_regular_companion, parse_structured_article,
};
use wiki_source::enterprise_verified::enterprise_normalize::NormalizeError;
use wiki_source::enterprise_verified::enterprise_normalize_article::normalize_enterprise_article;

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
    structured["infoboxes"] = serde_json::json!([{
        "type": "infobox", "name": "Physics",
        "has_parts": [{"type": "field", "name": "Discipline", "value": "Science"}]
    }]);
    structured["sections"][0]["has_parts"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "type": "list", "has_parts": [{"type": "list_item", "value": "Point one"}]
        }));
    let mut regular = base;
    regular["article_body"] = serde_json::json!({"html": "<article>Gravity</article>"});
    regular["categories"] = serde_json::json!([{"name": "Physics"}]);
    join_enterprise_article(
        parse_structured_article(&structured.to_string(), "20261010").unwrap(),
        parse_regular_companion(&regular.to_string(), "20261010").unwrap(),
    )
    .unwrap()
}

#[test]
fn normalizes_revision_and_rejects_unverified_images() {
    let joined = fixture();
    let canonical = normalize_enterprise_article(&joined, false).unwrap();
    assert_eq!(canonical.article.key.page_id, 42);
    assert_eq!(canonical.article.revision.revision_id, 99);
    assert_eq!(canonical.categories, ["Physics"]);
    assert_eq!(canonical.article.sections[0].heading, "Theory");
    assert!(matches!(
        canonical.article.lead[0].content,
        wiki_model::BlockContent::Infobox(_)
    ));
    assert!(matches!(
        canonical.article.sections[0].blocks[1].content,
        wiki_model::BlockContent::List(_)
    ));
    assert_eq!(
        normalize_enterprise_article(&joined, false).unwrap(),
        canonical
    );
    canonical.article.validate().unwrap();
    let mut changed = joined.clone();
    changed.rendered_html = "<article>Revised content</article>".into();
    let updated = normalize_enterprise_article(&changed, false).unwrap();
    assert_ne!(
        canonical.article.revision.content_sha256,
        updated.article.revision.content_sha256
    );
    let mut bad = joined;
    bad.rendered_html.push_str("<img src='missing.svg'>");
    assert_eq!(
        normalize_enterprise_article(&bad, false),
        Err(NormalizeError::UnresolvedVisual)
    );
}

#[test]
fn preserves_nested_citations_as_revision_scoped_footnotes() {
    use wiki_model::BlockContent;
    use wiki_source::enterprise::{StructuredCitation, StructuredReference};
    let mut joined = fixture();
    joined.structured.references.push(StructuredReference {
        identifier: "cite-1".into(),
        payload: serde_json::json!({
            "identifier": "cite-1",
            "title": "Verified reference title",
            "url": "https://example.org/evidence"
        })
        .as_object()
        .unwrap()
        .clone(),
    });
    let citation = StructuredCitation {
        identifier: "cite-1".into(),
        text: "[1]".into(),
    };
    joined.structured.sections[0].has_parts[0]
        .citations
        .push(citation.clone());
    joined.structured.sections[0].has_parts[1].has_parts[0]
        .citations
        .push(citation.clone());
    joined.structured.infoboxes[0].has_parts[0]
        .citations
        .push(citation);
    let normalized = normalize_enterprise_article(&joined, false).unwrap();
    let notes: Vec<_> = normalized
        .article
        .lead
        .iter()
        .chain(normalized.article.sections[0].blocks.iter())
        .filter_map(|b| match &b.content {
            BlockContent::Footnote(note) => Some(note),
            _ => None,
        })
        .collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].source_id, "cite-1");
    assert_eq!(notes[0].label, "[1]");
    assert_eq!(notes[0].text, "Verified reference title");
    assert_eq!(notes[0].reference_ids, ["cite-1"]);
    normalized.article.validate().unwrap();

    // Repeated identifiers with inconsistent marker text must fail closed.
    joined.structured.sections[0].has_parts[0].citations[0].text = "[different]".into();
    assert_eq!(
        normalize_enterprise_article(&joined, false),
        Err(NormalizeError::InvalidReference("cite-1".into()))
    );
}

#[test]
fn unresolved_structured_page_links_are_not_silently_dropped() {
    use wiki_source::enterprise::StructuredLink;
    let mut joined = fixture();
    joined.structured.infoboxes[0].has_parts[0]
        .links
        .push(StructuredLink {
            url: "https://en.wikipedia.org/wiki/Physics".into(),
            text: "Physics".into(),
        });
    assert_eq!(
        normalize_enterprise_article(&joined, false),
        Err(NormalizeError::UnresolvedLink)
    );
}
