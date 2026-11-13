//! AI-003: hosted (cloud) destinations reuse the consent-gated article chat
//! flow. A scripted cloud provider stands in for the HTTPS transport.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use wiki_ai::article_destination::{
    answer_article_with_preview_display, prepare_article_preview, ArticleDestination,
    ArticleDisplayAnswer, ArticleInferenceError,
};
use wiki_ai::rag::{ArticleAnswerDisplayPart, ArticleQuestion, RagError};
use wiki_ai::{
    CancellationToken, GenerateRequest, LlmProvider, Locality, ModelInfo, OutboundPolicy,
    ProviderCapabilities, ProviderError, StreamEvent,
};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, Revision, Section, ARTICLE_SCHEMA_VERSION,
};

const CITED: &str = "Gravity bends spacetime. [[cite:wkb:enwiki:9:12:1:b0]]";

/// A cloud destination whose provider replays one scripted reply.
#[derive(Clone, Debug)]
struct FakeCloud {
    model: String,
    credential: String,
    reply: &'static str,
    requests: Arc<AtomicUsize>,
}

impl PartialEq for FakeCloud {
    fn eq(&self, other: &Self) -> bool {
        self.model == other.model && self.credential == other.credential
    }
}

impl FakeCloud {
    fn new(reply: &'static str) -> Self {
        Self {
            model: "cloud-model".into(),
            credential: "key-a".into(),
            reply,
            requests: Arc::new(AtomicUsize::new(0)),
        }
    }
}

struct ScriptedProvider {
    reply: &'static str,
    requests: Arc<AtomicUsize>,
}

impl LlmProvider for ScriptedProvider {
    fn locality(&self) -> Locality {
        Locality::Cloud
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            discover_models: false,
            streaming: true,
            cancellable: true,
        }
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Err(ProviderError::Unsupported)
    }

    fn test_connection(&self) -> Result<(), ProviderError> {
        Err(ProviderError::Unsupported)
    }

    fn stream(
        &self,
        request: &GenerateRequest,
        _: &CancellationToken,
        on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
    ) -> Result<(), ProviderError> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.model, "cloud-model");
        assert!(request
            .messages
            .last()
            .unwrap()
            .content
            .contains("Evidence ID: wkb:enwiki:9:12:1:b0"));
        on_event(StreamEvent::TextDelta(self.reply.into()))?;
        on_event(StreamEvent::Completed {
            input_tokens: None,
            output_tokens: None,
        })
    }
}

impl ArticleDestination for FakeCloud {
    fn model(&self) -> &str {
        &self.model
    }

    fn locality(&self) -> Locality {
        Locality::Cloud
    }

    fn destination(&self) -> &str {
        "api.example.test"
    }

    fn connect(&self) -> Result<Box<dyn LlmProvider>, ProviderError> {
        Ok(Box::new(ScriptedProvider {
            reply: self.reply,
            requests: self.requests.clone(),
        }))
    }
}

fn article() -> Article {
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

fn question(policy: OutboundPolicy) -> ArticleQuestion<'static> {
    ArticleQuestion {
        question: "How does gravity affect spacetime?",
        model: "cloud-model",
        passage_limit: 2,
        context_word_budget: 32,
        selected_evidence: None,
        max_output_tokens: 80,
        outbound_policy: policy,
        prior_turns: &[],
    }
}

#[test]
fn consented_cloud_request_returns_validated_citations() {
    let cloud = FakeCloud::new(CITED);
    let question = question(OutboundPolicy::AllowCloud);
    let article = article();
    let preview = prepare_article_preview(&cloud, &article, &question)
        .unwrap()
        .unwrap();
    assert_eq!(preview.destination(), "api.example.test");
    assert_eq!(preview.locality(), Locality::Cloud);
    assert_eq!(preview.passages()[0].block_id, "wkb:enwiki:9:12:1:b0");
    let answer = answer_article_with_preview_display(
        &cloud,
        &article,
        &question,
        &preview,
        true,
        &CancellationToken::default(),
    )
    .unwrap();
    let ArticleDisplayAnswer::Supported(parts) = answer else {
        panic!("expected a supported answer");
    };
    assert!(parts.iter().any(|part| matches!(
        part,
        ArticleAnswerDisplayPart::Citation(target) if target.evidence.block_id == "wkb:enwiki:9:12:1:b0"
    )));
    assert_eq!(cloud.requests.load(Ordering::SeqCst), 1);
}

#[test]
fn a_cloud_destination_needs_allow_cloud_and_consent_before_any_request() {
    let article = article();
    let cloud = FakeCloud::new(CITED);
    let local_only = question(OutboundPolicy::AllowLocalEndpoints);
    let preview = prepare_article_preview(&cloud, &article, &local_only)
        .unwrap()
        .unwrap();
    assert_eq!(
        answer_article_with_preview_display(
            &cloud,
            &article,
            &local_only,
            &preview,
            true,
            &CancellationToken::default()
        ),
        Err(ArticleInferenceError::Rag(RagError::Provider(
            ProviderError::DestinationNotAllowed
        )))
    );

    let cloud_ok = question(OutboundPolicy::AllowCloud);
    let preview = prepare_article_preview(&cloud, &article, &cloud_ok)
        .unwrap()
        .unwrap();
    assert_eq!(
        answer_article_with_preview_display(
            &cloud,
            &article,
            &cloud_ok,
            &preview,
            false,
            &CancellationToken::default()
        ),
        Err(ArticleInferenceError::PassageConsentRequired)
    );
    assert_eq!(cloud.requests.load(Ordering::SeqCst), 0);
}

#[test]
fn consent_does_not_transfer_to_another_destination_or_credential() {
    let article = article();
    let question = question(OutboundPolicy::AllowCloud);
    let consented = FakeCloud::new(CITED);
    let preview = prepare_article_preview(&consented, &article, &question)
        .unwrap()
        .unwrap();
    let mut other_key = consented.clone();
    other_key.credential = "key-b".into();
    assert_eq!(
        answer_article_with_preview_display(
            &other_key,
            &article,
            &question,
            &preview,
            true,
            &CancellationToken::default()
        ),
        Err(ArticleInferenceError::PreviewMismatch)
    );
    let mut other_model = consented.clone();
    other_model.model = "other-model".into();
    let renamed = ArticleQuestion {
        model: "other-model",
        ..question.clone()
    };
    assert_eq!(
        answer_article_with_preview_display(
            &other_model,
            &article,
            &renamed,
            &preview,
            true,
            &CancellationToken::default()
        )
        .unwrap_err(),
        ArticleInferenceError::PreviewMismatch
    );
    assert_eq!(consented.requests.load(Ordering::SeqCst), 0);
}

#[test]
fn an_uncited_cloud_reply_is_never_displayed_as_grounded() {
    let article = article();
    let question = question(OutboundPolicy::AllowCloud);
    let cloud = FakeCloud::new("Gravity bends spacetime, trust me.");
    let preview = prepare_article_preview(&cloud, &article, &question)
        .unwrap()
        .unwrap();
    assert!(answer_article_with_preview_display(
        &cloud,
        &article,
        &question,
        &preview,
        true,
        &CancellationToken::default()
    )
    .is_err());
}
