use wiki_model::{Article, ArticleKey, Block, BlockContent, Revision, ARTICLE_SCHEMA_VERSION};
use wiki_search::{validate_claimed_citations, ArticleLexicalIndex, CitationError};

fn article(project: &str, page_id: u64, revision_id: u64) -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey { project: project.into(), page_id },
        revision: Revision {
            revision_id,
            timestamp: "2026-10-09T00:00:00Z".into(),
            content_sha256: "a".repeat(64),
        },
        title: "Gravité".into(),
        display_title: "Gravité".into(),
        language: "fr".into(),
        wikidata_id: None,
        lead: vec![Block {
            ordinal: 0,
            content: BlockContent::Paragraph("La gravité courbe l’espace-temps.".into()),
        }],
        sections: vec![],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: String::new(),
        is_disambiguation: false,
    }
}

#[test]
fn citations_do_not_cross_article_or_project_boundaries() {
    let original = ArticleLexicalIndex::build(&article("frwiki", 42, 10)).unwrap();
    let hit = original.search("gravité", 1).pop().unwrap();
    for (project, page_id, revision_id) in [
        ("frwiki", 43, 10),
        ("enwiki", 42, 10),
        ("frwiki", 42, 11),
    ] {
        let other = ArticleLexicalIndex::build(&article(project, page_id, revision_id)).unwrap();
        assert!(!other.contains_evidence(&hit));
        assert_eq!(
            validate_claimed_citations(&other, &[hit.clone()], &[hit.block_id.clone()]),
            Err(CitationError::InvalidRetrievedEvidence(hit.block_id.clone()))
        );
    }
}

#[test]
fn unicode_retrieval_preserves_exact_citation_target() {
    let index = ArticleLexicalIndex::build(&article("frwiki", 42, 10)).unwrap();
    let hits = index.search("GRAVITÉ", 5);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].excerpt, "La gravité courbe l’espace-temps.");
    assert_eq!(
        validate_claimed_citations(&index, &hits, &[hits[0].block_id.clone()]),
        Ok(hits)
    );
}
