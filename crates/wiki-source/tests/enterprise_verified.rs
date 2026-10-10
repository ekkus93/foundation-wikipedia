use std::io::Cursor;

use wiki_source::enterprise_integrity::IntegrityError;
use wiki_source::enterprise_verified::{
    import_canonical_enterprise_ndjson, import_verified_enterprise_ndjson, VerifiedImportError,
};

fn base() -> serde_json::Value {
    serde_json::json!({
        "name": "Example",
        "identifier": 42,
        "version": {"identifier": 99},
        "is_part_of": {"identifier": "enwiki"},
        "in_language": {"identifier": "en"},
        "namespace": {"identifier": 0},
        "date_modified": "2026-10-10T00:00:00Z"
    })
}

#[test]
fn verified_import_rejects_missing_citation_and_accepts_matching_one() {
    let mut structured = base();
    structured["sections"] = serde_json::json!([{
        "type": "section",
        "name": "Theory",
        "has_parts": [{
            "type": "paragraph",
            "value": "Text",
            "citations": [{"identifier": "cite1", "text": "[1]"}]
        }]
    }]);
    let mut regular = base();
    regular["article_body"] = serde_json::json!({"html": "<article>Text</article>"});
    let run = |source: &serde_json::Value| {
        import_verified_enterprise_ndjson(
            Cursor::new(source.to_string()),
            Cursor::new(regular.to_string()),
            "20261010",
            vec![],
        )
    };
    assert_eq!(
        run(&structured),
        Err(VerifiedImportError::Integrity(
            IntegrityError::UnknownCitation("cite1".into())
        ))
    );
    structured["references"] = serde_json::json!([{"identifier": "cite1"}]);
    let joined = run(&structured).unwrap();
    assert_eq!(joined.len(), 1);
    assert_eq!(joined[0].structured.revision_id, 99);
    let canonical = import_canonical_enterprise_ndjson(
        Cursor::new(structured.to_string()),
        Cursor::new(regular.to_string()),
        "20261010",
        vec![],
    ).unwrap();
    assert_eq!(canonical[0].article.revision.revision_id, 99);
    assert_eq!(canonical[0].article.references[0].id, "cite1");
}
