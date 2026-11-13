//! Explicit, discovered local model selection shared by desktop and Android.
//!
//! A UI supplies the protocol and an IP-literal/localhost endpoint, displays
//! the discovered inventory and destination, then selects an exact model ID.
//! Nothing in this module persists profiles, supplies cloud fallbacks or
//! turns a LAN server into an "offline/on-device" model.

use crate::local_endpoint::{EndpointError, LocalEndpoint};
use crate::local_http::LocalOpenAiProvider;
use crate::ollama::LocalOllamaProvider;
use crate::{
    stream_selected, CancellationToken, GenerateRequest, LlmProvider, Locality, Message, ModelInfo,
    OutboundPolicy, ProviderError, StreamEvent,
};
use wiki_model::Article;
use wiki_search::GroundedAnswer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalProtocol {
    Ollama,
    OpenAiCompatible,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionError {
    InvalidEndpoint(EndpointError),
    Provider(ProviderError),
    UnknownModel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalArticleError {
    PassageConsentRequired,
    ModelMismatch,
    PreviewMismatch,
    Rag(crate::rag::RagError),
    Display(crate::rag::ArticleAnswerDisplayError),
}

/// Only fresh, consent-gated, citation-validated inference can produce
/// these typed navigation targets. Historical saved answers cannot enter
/// this API as an input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalArticleDisplayAnswer {
    Supported(Vec<crate::rag::ArticleAnswerDisplayPart>),
    InsufficientEvidence,
}

#[derive(Clone, Debug)]
pub struct LocalModelInventory {
    protocol: LocalProtocol,
    endpoint: LocalEndpoint,
    models: Vec<ModelInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedLocalModel {
    protocol: LocalProtocol,
    endpoint: LocalEndpoint,
    model: String,
}

/// A disclosed request to a discovered local model (see `article_destination`).
pub type LocalArticlePreview = crate::article_destination::ArticlePreview<SelectedLocalModel>;

impl LocalArticlePreview {
    pub fn endpoint(&self) -> &str {
        self.selected.endpoint()
    }
}

impl crate::article_destination::ArticleDestination for SelectedLocalModel {
    fn model(&self) -> &str {
        &self.model
    }

    fn locality(&self) -> Locality {
        self.endpoint.locality()
    }

    fn destination(&self) -> &str {
        SelectedLocalModel::endpoint(self)
    }

    fn connect(&self) -> Result<Box<dyn LlmProvider>, ProviderError> {
        Ok(provider(self.protocol, self.endpoint.clone()))
    }
}

fn provider(protocol: LocalProtocol, endpoint: LocalEndpoint) -> Box<dyn LlmProvider> {
    match protocol {
        LocalProtocol::Ollama => Box::new(LocalOllamaProvider::new(endpoint)),
        LocalProtocol::OpenAiCompatible => Box::new(LocalOpenAiProvider::new(endpoint)),
    }
}

/// Connect once to the *explicitly* selected local endpoint, with no DNS,
/// credentials, redirects or cloud fallback. Reject bad endpoints before I/O.
/// HTTPS is currently unsupported rather than silently downgraded.
pub fn discover_local_models(
    protocol: LocalProtocol,
    endpoint_url: &str,
) -> Result<LocalModelInventory, SelectionError> {
    discover_local_models_cancellable(protocol, endpoint_url, &CancellationToken::default())
}

/// The explicit diagnostic can withdraw local-network permission during GET.
/// This retains the same endpoint parser, inventory validation and model IDs.
pub fn discover_local_models_cancellable(
    protocol: LocalProtocol,
    endpoint_url: &str,
    cancellation: &CancellationToken,
) -> Result<LocalModelInventory, SelectionError> {
    let endpoint = LocalEndpoint::parse(endpoint_url).map_err(SelectionError::InvalidEndpoint)?;
    if cancellation.is_cancelled() {
        return Err(SelectionError::Provider(ProviderError::Cancelled));
    }
    let models = match protocol {
        LocalProtocol::Ollama => {
            crate::ollama::discover_ollama_models_cancellable(&endpoint, cancellation)
        }
        LocalProtocol::OpenAiCompatible => {
            crate::local_http::discover_models_cancellable(&endpoint, cancellation)
        }
    }
    .map_err(SelectionError::Provider)?;
    if cancellation.is_cancelled() {
        return Err(SelectionError::Provider(ProviderError::Cancelled));
    }
    Ok(LocalModelInventory {
        protocol,
        endpoint,
        models,
    })
}

impl LocalModelInventory {
    pub fn protocol(&self) -> LocalProtocol {
        self.protocol
    }

    pub fn endpoint(&self) -> &str {
        self.endpoint.base_url()
    }

    /// A LAN-hosted LLM is network-dependent, not an on-device model.
    pub fn locality(&self) -> Locality {
        self.endpoint.locality()
    }

    pub fn models(&self) -> &[ModelInfo] {
        &self.models
    }

    /// Only models returned by the pinned endpoint are selectable. A caller
    /// cannot silently substitute a different ID after presenting inventory.
    pub fn select(&self, model: &str) -> Result<SelectedLocalModel, SelectionError> {
        if !self.models.iter().any(|available| available.id == model) {
            return Err(SelectionError::UnknownModel);
        }
        Ok(SelectedLocalModel {
            protocol: self.protocol,
            endpoint: self.endpoint.clone(),
            model: model.to_owned(),
        })
    }
}

impl SelectedLocalModel {
    pub fn protocol(&self) -> LocalProtocol {
        self.protocol
    }

    pub fn endpoint(&self) -> &str {
        self.endpoint.base_url()
    }

    pub fn locality(&self) -> Locality {
        self.endpoint.locality()
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Stream only to the previously selected destination and model.
    /// The outbound policy is checked before sending any conversation text.
    pub fn stream(
        &self,
        messages: &[Message],
        max_output_tokens: u32,
        cancellation: &CancellationToken,
        policy: OutboundPolicy,
        on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
    ) -> Result<(), ProviderError> {
        let request = GenerateRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            max_output_tokens,
        };
        stream_selected(
            provider(self.protocol, self.endpoint.clone()).as_ref(),
            &request,
            cancellation,
            policy,
            on_event,
        )
    }

    /// Prepare a no-network disclosure of the exact passages and destination.
    /// A later invocation must recheck the article, question, endpoint and
    /// selected model; a preview alone is never passage-sharing consent.
    pub fn prepare_article_preview(
        &self,
        article: &Article,
        question: &crate::rag::ArticleQuestion<'_>,
    ) -> Result<Option<LocalArticlePreview>, LocalArticleError> {
        crate::article_destination::prepare_article_preview(self, article, question)
    }

    /// Infer only if the affirmative decision applies to the exact preview.
    /// Any changed question, revision, selected endpoint/model or packed
    /// evidence invalidates the preview before provider I/O.
    pub fn answer_article_with_preview(
        &self,
        article: &Article,
        question: &crate::rag::ArticleQuestion<'_>,
        preview: &LocalArticlePreview,
        share_article_passages: bool,
        cancellation: &CancellationToken,
    ) -> Result<GroundedAnswer, LocalArticleError> {
        crate::article_destination::answer_article_with_preview(
            self,
            article,
            question,
            preview,
            share_article_passages,
            cancellation,
        )
    }

    /// Return presentation-safe text and canonical citation navigation
    /// targets from a *fresh* consent-gated local inference, not from an
    /// arbitrary caller-constructed or persisted GroundedAnswer. Model output
    /// stays buffered until all citation and current-article checks pass.
    pub fn answer_article_with_preview_display(
        &self,
        article: &Article,
        question: &crate::rag::ArticleQuestion<'_>,
        preview: &LocalArticlePreview,
        share_article_passages: bool,
        cancellation: &CancellationToken,
    ) -> Result<LocalArticleDisplayAnswer, LocalArticleError> {
        crate::article_destination::answer_article_with_preview_display(
            self,
            article,
            question,
            preview,
            share_article_passages,
            cancellation,
        )
    }

    /// Use this exact discovered local model for revision-scoped RAG.
    ///
    /// The caller must obtain per-request consent before sharing article
    /// passages. Saving a profile or discovering models is not consent.
    /// Responses are buffered until citation validation succeeds; this
    /// checks provenance, not factual entailment.
    pub fn answer_article(
        &self,
        article: &Article,
        question: &crate::rag::ArticleQuestion<'_>,
        share_article_passages: bool,
        cancellation: &CancellationToken,
    ) -> Result<GroundedAnswer, LocalArticleError> {
        if !share_article_passages {
            return Err(LocalArticleError::PassageConsentRequired);
        }
        if question.model != self.model {
            return Err(LocalArticleError::ModelMismatch);
        }
        crate::rag::answer_article(
            article,
            provider(self.protocol, self.endpoint.clone()).as_ref(),
            question,
            cancellation,
        )
        .map_err(LocalArticleError::Rag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageRole;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    fn serve(responses: Vec<Vec<u8>>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let handle = thread::spawn(move || {
            let mut seen = Vec::new();
            for response in responses {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut data = Vec::new();
                loop {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    data.extend_from_slice(&buffer[..count]);
                    let Some(split) = data.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let header = std::str::from_utf8(&data[..split]).unwrap();
                    let length = header
                        .lines()
                        .find_map(|line| line.strip_prefix("Content-Length: "))
                        .map(|number| number.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if data.len() >= split + 4 + length {
                        break;
                    }
                }
                socket.write_all(&response).unwrap();
                seen.push(String::from_utf8(data).unwrap());
            }
            seen
        });
        (url, handle)
    }

    fn http(body: &str, media_type: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {media_type}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn messages() -> Vec<Message> {
        vec![Message {
            role: MessageRole::User,
            content: "explain gravity".into(),
        }]
    }

    #[test]
    fn ollama_discovery_selects_only_an_advertised_model_and_streams_to_same_host() {
        let tags = r#"{"models":[{"name":"qwen3:8b"},{"name":"llama3.2:latest"}]}"#;
        let sse = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Gravity\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let (url, server) = serve(vec![
            http(tags, "application/json"),
            http(sse, "text/event-stream"),
        ]);
        let inventory = discover_local_models(LocalProtocol::Ollama, &url).unwrap();
        assert_eq!(inventory.locality(), Locality::Localhost);
        assert_eq!(inventory.protocol(), LocalProtocol::Ollama);
        assert_eq!(inventory.endpoint(), url);
        assert_eq!(inventory.models().len(), 2);
        assert_eq!(
            inventory.select("Qwen3:8b").unwrap_err(),
            SelectionError::UnknownModel
        );
        let selected = inventory.select("qwen3:8b").unwrap();
        assert_eq!(selected.model(), "qwen3:8b");
        assert_eq!(selected.endpoint(), url);
        assert_eq!(selected.protocol(), LocalProtocol::Ollama);
        let mut events = Vec::new();
        selected
            .stream(
                &messages(),
                64,
                &CancellationToken::default(),
                OutboundPolicy::AllowLocalEndpoints,
                &mut |event| {
                    events.push(event);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(events[0], StreamEvent::TextDelta("Gravity".into()));
        assert!(matches!(events[1], StreamEvent::Completed { .. }));
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /api/tags HTTP/1.1\r\n"));
        assert!(requests[1].starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        assert!(requests[1].contains(r#""model":"qwen3:8b""#));
        assert!(requests[1].contains("explain gravity"));
    }

    #[test]
    fn openai_compatible_discovery_preserves_explicit_model_and_locality() {
        let (url, server) = serve(vec![http(
            r#"{"data":[{"id":"modèle-1","context_window":8192}]}"#,
            "application/json",
        )]);
        let inventory = discover_local_models(LocalProtocol::OpenAiCompatible, &url).unwrap();
        assert_eq!(inventory.protocol(), LocalProtocol::OpenAiCompatible);
        assert_eq!(inventory.models()[0].context_window, Some(8192));
        assert_eq!(inventory.select("modèle-1").unwrap().model(), "modèle-1");
        assert_eq!(
            inventory.select("fallback-cloud"),
            Err(SelectionError::UnknownModel)
        );
        let selected = inventory.select("modèle-1").unwrap();
        assert_eq!(
            selected.stream(
                &messages(),
                64,
                &CancellationToken::default(),
                OutboundPolicy::OnDeviceOnly,
                &mut |_| Ok(())
            ),
            Err(ProviderError::DestinationNotAllowed)
        );
        assert!(server.join().unwrap()[0].starts_with("GET /v1/models HTTP/1.1\r\n"));
    }

    #[test]
    fn selected_local_article_rag_requires_consent_and_checks_exact_citations() {
        use crate::rag::{
            ArticleAnswerDisplayPart, ArticleQuestion, CitationNavigationTarget, RagError,
        };
        use wiki_model::{ArticleKey, Block, BlockContent, Revision, ARTICLE_SCHEMA_VERSION};
        use wiki_search::{ArticleLexicalIndex, CitationError};

        let article = Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 9,
            },
            revision: Revision {
                revision_id: 12,
                timestamp: "2026-10-12T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Gravity".into(),
            display_title: "Gravity".into(),
            language: "en".into(),
            namespace: 0,
            aliases: Vec::new(),
            wikidata_id: None,
            lead: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph(
                    "Gravity is the attraction between masses.".into(),
                ),
            }],
            sections: Vec::new(),
            references: Vec::new(),
            links: Vec::new(),
            media: Vec::new(),
            rendered_html: "<article/>".into(),
            is_disambiguation: false,
        };
        // Expected evidence comes from the same structural retriever that
        // chat context preparation ranks with (including its rank score).
        let hit = match ArticleLexicalIndex::build(&article).unwrap().retrieve(
            &wiki_search::retrieval::RetrievalRequest {
                query: "gravity".into(),
                expected_revision: article.revision.revision_id,
                current_block_id: None,
                selected_text: None,
                limit: 1,
                max_context_tokens: usize::MAX,
            },
        ) {
            wiki_search::retrieval::Retrieval::Evidence { blocks, .. } => blocks[0].hit.clone(),
            other => panic!("expected evidence, got {other:?}"),
        };
        let answer = format!("Gravity attracts masses [[cite:{}]]", hit.block_id);
        let valid_chunk = serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": answer},
                "finish_reason": null
            }]
        });
        let forged_chunk = serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": "Unverified [[cite:forged]]"},
                "finish_reason": null
            }]
        });
        let response = |chunk: serde_json::Value| {
            let sse = format!(
                "data: {chunk}\n\ndata: {stop}\n\ndata: [DONE]\n\n",
                stop = r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#
            );
            http(&sse, "text/event-stream")
        };
        let (url, server) = serve(vec![
            http(r#"{"models":[{"name":"qwen3:8b"}]}"#, "application/json"),
            response(valid_chunk.clone()),
            response(valid_chunk),
            response(forged_chunk),
        ]);
        let inventory = discover_local_models(LocalProtocol::Ollama, &url).unwrap();
        let selected = inventory.select("qwen3:8b").unwrap();
        let question = ArticleQuestion {
            question: "gravity",
            model: "qwen3:8b",
            passage_limit: 2,
            context_word_budget: 64,
            selected_evidence: None,
            max_output_tokens: 64,
            outbound_policy: OutboundPolicy::AllowLocalEndpoints,
            prior_turns: &[],
        };
        let cancellation = CancellationToken::default();
        let preview = selected
            .prepare_article_preview(&article, &question)
            .unwrap()
            .unwrap();
        assert_eq!(preview.endpoint(), url);
        assert_eq!(preview.model(), "qwen3:8b");
        assert_eq!(preview.locality(), Locality::Localhost);
        assert_eq!(preview.passages(), &[hit.clone()]);
        assert!(preview.user_message().contains("Gravity is the attraction"));
        let mut changed_article = article.clone();
        changed_article.revision.revision_id += 1;
        assert_eq!(
            selected.answer_article_with_preview(
                &changed_article,
                &question,
                &preview,
                true,
                &cancellation
            ),
            Err(LocalArticleError::PreviewMismatch)
        );
        let changed_question = ArticleQuestion {
            question: "What is gravity?",
            ..question.clone()
        };
        assert_eq!(
            selected.answer_article_with_preview(
                &article,
                &changed_question,
                &preview,
                true,
                &cancellation
            ),
            Err(LocalArticleError::PreviewMismatch)
        );
        let different_endpoint = SelectedLocalModel {
            endpoint: LocalEndpoint::parse("http://127.0.0.1:11434").unwrap(),
            ..selected.clone()
        };
        assert_eq!(
            different_endpoint.answer_article_with_preview(
                &article,
                &question,
                &preview,
                true,
                &cancellation
            ),
            Err(LocalArticleError::PreviewMismatch)
        );
        assert_eq!(
            selected.answer_article_with_preview(
                &article,
                &question,
                &preview,
                false,
                &cancellation
            ),
            Err(LocalArticleError::PassageConsentRequired)
        );
        // Changes to generation limits or locality permission invalidate
        // prior consent even when the evidence bytes happen to be identical.
        let altered_questions = [
            ArticleQuestion {
                max_output_tokens: 128,
                ..question.clone()
            },
            ArticleQuestion {
                outbound_policy: OutboundPolicy::OnDeviceOnly,
                ..question.clone()
            },
            ArticleQuestion {
                passage_limit: 3,
                ..question.clone()
            },
            ArticleQuestion {
                context_word_budget: 128,
                ..question.clone()
            },
            ArticleQuestion {
                selected_evidence: Some(&hit),
                ..question.clone()
            },
        ];
        for altered in &altered_questions {
            assert_eq!(
                selected.answer_article_with_preview(
                    &article,
                    altered,
                    &preview,
                    true,
                    &cancellation
                ),
                Err(LocalArticleError::PreviewMismatch)
            );
        }

        assert_eq!(
            selected.answer_article(&article, &question, false, &cancellation),
            Err(LocalArticleError::PassageConsentRequired)
        );
        let wrong_model = ArticleQuestion {
            model: "not-selected",
            ..question.clone()
        };
        assert_eq!(
            selected.answer_article(&article, &wrong_model, true, &cancellation),
            Err(LocalArticleError::ModelMismatch)
        );
        let denied = ArticleQuestion {
            outbound_policy: OutboundPolicy::OnDeviceOnly,
            ..question.clone()
        };
        assert_eq!(
            selected.answer_article(&article, &denied, true, &cancellation),
            Err(LocalArticleError::Rag(RagError::Provider(
                ProviderError::DestinationNotAllowed
            )))
        );
        let cancelled = CancellationToken::default();
        cancelled.cancel();
        assert_eq!(
            selected.answer_article(&article, &question, true, &cancelled),
            Err(LocalArticleError::Rag(RagError::Provider(
                ProviderError::Cancelled
            )))
        );
        assert_eq!(
            selected.answer_article_with_preview(
                &article,
                &question,
                &preview,
                true,
                &cancellation
            ),
            Ok(GroundedAnswer::Supported {
                text: answer,
                citations: vec![hit.clone()],
            })
        );
        assert_eq!(
            selected.answer_article_with_preview_display(
                &article,
                &question,
                &preview,
                false,
                &cancellation,
            ),
            Err(LocalArticleError::PassageConsentRequired)
        );
        assert_eq!(
            selected.answer_article_with_preview_display(
                &article,
                &question,
                &preview,
                true,
                &cancellation,
            ),
            Ok(LocalArticleDisplayAnswer::Supported(vec![
                ArticleAnswerDisplayPart::Text("Gravity attracts masses ".into()),
                ArticleAnswerDisplayPart::Citation(CitationNavigationTarget {
                    article: article.key.clone(),
                    revision_id: 12,
                    evidence: hit,
                }),
            ]))
        );
        assert_eq!(
            selected.answer_article(&article, &question, true, &cancellation),
            Err(LocalArticleError::Rag(RagError::Citation(
                CitationError::UnretrievedCitation("forged".into())
            )))
        );

        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[0].starts_with("GET /api/tags HTTP/1.1\r\n"));
        for request in &requests[1..] {
            assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
            assert!(request.contains(r#""model":"qwen3:8b""#));
            assert!(request.contains("Gravity is the attraction between masses."));
            assert!(request.contains(&format!(
                "Evidence ID: {}",
                article.key.block_id(12, &[], 0)
            )));
        }
    }

    #[test]
    fn endpoint_rejection_and_https_unsupported_never_trigger_cloud_fallback() {
        assert!(matches!(
            discover_local_models(LocalProtocol::Ollama, "https://api.openai.com/v1"),
            Err(SelectionError::InvalidEndpoint(
                EndpointError::UnapprovedHost
            ))
        ));
        assert!(matches!(
            discover_local_models(LocalProtocol::Ollama, "http://example.com"),
            Err(SelectionError::InvalidEndpoint(
                EndpointError::UnapprovedHost
            ))
        ));
        assert!(matches!(
            discover_local_models(LocalProtocol::Ollama, "https://127.0.0.1:11434"),
            Err(SelectionError::Provider(ProviderError::Unsupported))
        ));
        let lan = LocalEndpoint::parse("http://192.168.1.42:11434").unwrap();
        assert_eq!(lan.locality(), Locality::LocalNetwork);
    }
}
