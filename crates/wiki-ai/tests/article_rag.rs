use std::sync::atomic::{AtomicUsize, Ordering};
use wiki_ai::rag::{answer_article, ArticleQuestion, RagError};
use wiki_ai::{
    CancellationToken, GenerateRequest, LlmProvider, Locality, ModelInfo, OutboundPolicy,
    ProviderCapabilities, ProviderError, StreamEvent,
};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, Revision, Section, ARTICLE_SCHEMA_VERSION,
};
use wiki_search::{CitationError, GroundedAnswer};

struct FixtureProvider<'a> {
    output: &'a str,
    locality: Locality,
    calls: &'a AtomicUsize,
}

impl LlmProvider for FixtureProvider<'_> {
    fn locality(&self) -> Locality {
        self.locality
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            discover_models: false,
            streaming: true,
            cancellable: true,
        }
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }

    fn test_connection(&self) -> Result<(), ProviderError> {
        Ok(())
    }

    fn stream(
        &self,
        request: &GenerateRequest,
        _: &CancellationToken,
        on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
    ) -> Result<(), ProviderError> {
        assert!(request.messages[1].content.contains("revision: 12"));
        assert!(request.messages[1]
            .content
            .contains("Evidence ID: wkb:enwiki:9:12:1:b0"));
        self.calls.fetch_add(1, Ordering::Relaxed);
        on_event(StreamEvent::TextDelta(self.output.to_string()))?;
        on_event(StreamEvent::Completed {
            input_tokens: Some(20),
            output_tokens: Some(8),
        })
    }
}

fn sample() -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id: 9,
        },
        revision: Revision {
            revision_id: 12,
            timestamp: "2026-10-09T00:00:00Z".into(),
            content_sha256: "a".repeat(64),
        },
        title: "Physics".into(),
        display_title: "Physics".into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![Block {
            ordinal: 0,
            content: BlockContent::Paragraph("Physics studies matter.".into()),
        }],
        sections: vec![Section {
            ordinal: 1,
            heading: "Gravitation".into(),
            blocks: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph("Gravity bends spacetime.".into()),
            }],
            subsections: vec![],
        }],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: "<article>Physics</article>".into(),
        is_disambiguation: false,
    }
}

fn question<'a>() -> ArticleQuestion<'a> {
    ArticleQuestion {
        question: "How does gravity affect spacetime?",
        model: "fixture-model",
        passage_limit: 2,
        context_word_budget: 32,
        max_output_tokens: 80,
        outbound_policy: OutboundPolicy::OnDeviceOnly,
    }
}

#[test]
fn exact_retrieved_revision_can_be_cited() {
    let calls = AtomicUsize::new(0);
    let provider = FixtureProvider {
        output: "Gravity bends spacetime. [[cite:wkb:enwiki:9:12:1:b0]]",
        locality: Locality::OnDevice,
        calls: &calls,
    };
    let answer = answer_article(
        &sample(),
        &provider,
        &question(),
        &CancellationToken::default(),
    )
    .unwrap();
    match answer {
        GroundedAnswer::Supported { citations, text } => {
            assert_eq!(citations.len(), 1);
            assert_eq!(citations[0].block_id, "wkb:enwiki:9:12:1:b0");
            assert!(text.contains("Gravity bends"));
        }
        GroundedAnswer::InsufficientEvidence => panic!("expected supported result"),
    }
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn invented_and_old_revision_citations_are_rejected() {
    for claim in ["invented", "wkb:enwiki:9:11:1:b0", "wkb:frwiki:9:12:1:b0"] {
        let calls = AtomicUsize::new(0);
        let output = format!("Unsupported [[cite:{claim}]]");
        let provider = FixtureProvider {
            output: &output,
            locality: Locality::OnDevice,
            calls: &calls,
        };
        let error = answer_article(
            &sample(),
            &provider,
            &question(),
            &CancellationToken::default(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            RagError::Citation(CitationError::UnretrievedCitation(claim.into()))
        );
    }
}

#[test]
fn missing_evidence_blocks_dispatch_and_hosted_fallback() {
    let calls = AtomicUsize::new(0);
    let provider = FixtureProvider {
        output: "Result without citations",
        locality: Locality::Cloud,
        calls: &calls,
    };
    let mut absent = question();
    absent.question = "nonexistent quantumflibber";
    assert_eq!(
        answer_article(&sample(), &provider, &absent, &CancellationToken::default()),
        Ok(GroundedAnswer::InsufficientEvidence)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        answer_article(
            &sample(),
            &provider,
            &question(),
            &CancellationToken::default()
        ),
        Err(RagError::Provider(ProviderError::DestinationNotAllowed))
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn no_claims_or_unclosed_claims_cannot_be_displayed_as_grounded() {
    for (output, error) in [
        (
            "Factual-looking prose with no supporting references.",
            RagError::Citation(CitationError::UnsupportedAnswer),
        ),
        ("Malformed [[cite:incomplete", RagError::MalformedCitation),
    ] {
        let calls = AtomicUsize::new(0);
        let provider = FixtureProvider {
            output,
            locality: Locality::OnDevice,
            calls: &calls,
        };
        assert_eq!(
            answer_article(
                &sample(),
                &provider,
                &question(),
                &CancellationToken::default()
            ),
            Err(error)
        );
    }
}

#[test]
fn provider_abstention_has_explicit_insufficient_evidence_state() {
    let calls = AtomicUsize::new(0);
    let provider = FixtureProvider {
        output: "[NO_EVIDENCE]",
        locality: Locality::OnDevice,
        calls: &calls,
    };
    assert_eq!(
        answer_article(
            &sample(),
            &provider,
            &question(),
            &CancellationToken::default()
        ),
        Ok(GroundedAnswer::InsufficientEvidence)
    );
}
