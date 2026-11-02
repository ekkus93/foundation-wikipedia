//! Platform-neutral, consent-gated local article chat with atomic persistence.
//!
//! This use case requires a canonical exact-revision Article from a verified
//! source. It never constructs evidence from an HTML preview, guesses a model,
//! silently switches endpoints, or upgrades historical saved citations.
//! UI adapters must disclose preview.passages(), model() and endpoint() and
//! obtain affirmative consent for THIS request before invoking it.

use wiki_ai::local_selection::{
    LocalArticleDisplayAnswer, LocalArticleError, LocalArticlePreview, SelectedLocalModel,
};
use wiki_ai::rag::{ArticleAnswerDisplayPart, ArticleQuestion};
use wiki_ai::CancellationToken;
use wiki_model::Article;
use wiki_store::user_state::{UserState, UserStateError};

#[derive(Debug)]
pub enum ArticleChatError {
    Inference(LocalArticleError),
    Store(UserStateError),
    StalePersonalData,
    Cancelled,
    InvalidSavedMessage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SavedArticleChatOutcome {
    /// A conservative abstention is displayed, but never creates a thread.
    InsufficientEvidence,
    /// These are fresh, validated navigation parts, not historical citations.
    Supported {
        parts: Vec<ArticleAnswerDisplayPart>,
        thread_id: i64,
        question_id: i64,
        answer_id: i64,
    },
}

/// Persist only after the consented provider has completed and its citations
/// have passed the shared RAG and canonical navigation checks. This private
/// boundary is not an API for arbitrary caller-constructed "verified" text.
fn persist_validated_display(
    state: &mut UserState,
    generation: u64,
    article: &Article,
    question: &str,
    thread_id: Option<i64>,
    answer: LocalArticleDisplayAnswer,
    cancellation: &CancellationToken,
) -> Result<SavedArticleChatOutcome, ArticleChatError> {
    if cancellation.is_cancelled() {
        return Err(ArticleChatError::Cancelled);
    }
    if state
        .personal_generation()
        .map_err(ArticleChatError::Store)?
        != generation
    {
        return Err(ArticleChatError::StalePersonalData);
    }
    let LocalArticleDisplayAnswer::Supported(parts) = answer else {
        return Ok(SavedArticleChatOutcome::InsufficientEvidence);
    };
    let mut stored = String::new();
    for part in &parts {
        match part {
            ArticleAnswerDisplayPart::Text(text) => stored.push_str(text),
            ArticleAnswerDisplayPart::Citation(target) => {
                stored.push_str("[[cite:");
                stored.push_str(&target.evidence.block_id);
                stored.push_str("]]");
            }
        }
        if stored.len() > 65_536 {
            return Err(ArticleChatError::InvalidSavedMessage);
        }
    }
    if stored.is_empty() || stored.contains('\0') {
        return Err(ArticleChatError::InvalidSavedMessage);
    }
    // The SQLite IMMEDIATE transaction checks generation and exact
    // article/revision/thread identity again. A new thread and both turns are
    // inserted together, so cancellation/errors leave no empty conversation.
    let saved = state
        .save_chat_exchange_for_generation(
            generation,
            &article.key,
            article.revision.revision_id,
            thread_id,
            question,
            &stored,
        )
        .map_err(ArticleChatError::Store)?;
    let Some((thread_id, question_id, answer_id)) = saved else {
        return Err(ArticleChatError::StalePersonalData);
    };
    Ok(SavedArticleChatOutcome::Supported {
        parts,
        thread_id,
        question_id,
        answer_id,
    })
}

/// Run one article-scoped local request and persist a completed supported Q&A.
///
/// The selected model and preview must originate from the SAME explicit
/// discovery/selection and canonical article. The shared AI layer revalidates
/// the complete preview, selected endpoint, model, passages, question and
/// generation settings before sending any evidence. This function performs
/// no network request on missing consent, stale privacy generation, invalid
/// existing thread, or pre-cancellation.
///
/// This is a blocking shared-core API; desktop and Android adapters must run
/// it off their UI thread and cancel when article/revision or consent changes.
/// The UI must still compare citation target identities before navigating.
pub struct ArticleChatRequest<'a> {
    pub article: &'a Article,
    pub question: &'a ArticleQuestion<'a>,
    pub selected: &'a SelectedLocalModel,
    pub preview: &'a LocalArticlePreview,
    pub share_article_passages: bool,
    pub existing_thread: Option<i64>,
    pub cancellation: &'a CancellationToken,
}

pub fn answer_and_save_local_article(
    state: &mut UserState,
    generation: u64,
    request: ArticleChatRequest<'_>,
) -> Result<SavedArticleChatOutcome, ArticleChatError> {
    let ArticleChatRequest {
        article,
        question,
        selected,
        preview,
        share_article_passages,
        existing_thread,
        cancellation,
    } = request;
    if !share_article_passages {
        return Err(ArticleChatError::Inference(
            LocalArticleError::PassageConsentRequired,
        ));
    }
    if cancellation.is_cancelled() {
        return Err(ArticleChatError::Cancelled);
    }
    if question.question.contains('\0') || question.question.len() > 65_536 {
        return Err(ArticleChatError::InvalidSavedMessage);
    }
    if state
        .personal_generation()
        .map_err(ArticleChatError::Store)?
        != generation
    {
        return Err(ArticleChatError::StalePersonalData);
    }
    if let Some(id) = existing_thread {
        // Reject wrong-article/revision/deleted threads before any provider I/O.
        state
            .recent_chat_exchanges(&article.key, article.revision.revision_id, id, 0)
            .map_err(ArticleChatError::Store)?;
    }
    let answer = selected
        .answer_article_with_preview_display(
            article,
            question,
            preview,
            share_article_passages,
            cancellation,
        )
        .map_err(ArticleChatError::Inference)?;
    persist_validated_display(
        state,
        generation,
        article,
        question.question,
        existing_thread,
        answer,
        cancellation,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;
    use wiki_ai::local_selection::{discover_local_models, LocalProtocol};
    use wiki_ai::OutboundPolicy;
    use wiki_model::{ArticleKey, Block, BlockContent, Revision, ARTICLE_SCHEMA_VERSION};
    use wiki_search::ArticleLexicalIndex;

    fn article() -> Article {
        Article {
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
                content: BlockContent::Paragraph("Gravity is attraction between masses.".into()),
            }],
            sections: Vec::new(),
            references: Vec::new(),
            links: Vec::new(),
            media: Vec::new(),
            rendered_html: "<article/>".into(),
            is_disambiguation: false,
        }
    }

    #[test]
    fn saved_answer_is_atomic_revision_scoped_and_never_retrusted_on_replay() {
        let article = article();
        let hit = ArticleLexicalIndex::build(&article)
            .unwrap()
            .search("gravity", 1)
            .pop()
            .unwrap();
        let parts = vec![
            ArticleAnswerDisplayPart::Text("Gravity attracts ".into()),
            ArticleAnswerDisplayPart::Citation(wiki_ai::rag::CitationNavigationTarget {
                article: article.key.clone(),
                revision_id: article.revision.revision_id,
                evidence: hit.clone(),
            }),
        ];
        let mut state = UserState::in_memory().unwrap();
        let generation = state.personal_generation().unwrap();
        let cancelled = CancellationToken::default();
        cancelled.cancel();
        assert!(matches!(
            persist_validated_display(
                &mut state,
                generation,
                &article,
                "gravity",
                None,
                LocalArticleDisplayAnswer::Supported(parts.clone()),
                &cancelled,
            ),
            Err(ArticleChatError::Cancelled)
        ));
        assert!(state
            .list_chat_threads(&article.key, 12, 10)
            .unwrap()
            .is_empty());
        let active = CancellationToken::default();
        assert!(matches!(
            persist_validated_display(
                &mut state,
                generation,
                &article,
                "gravity",
                None,
                LocalArticleDisplayAnswer::InsufficientEvidence,
                &active,
            )
            .unwrap(),
            SavedArticleChatOutcome::InsufficientEvidence
        ));
        let saved = persist_validated_display(
            &mut state,
            generation,
            &article,
            "gravity",
            None,
            LocalArticleDisplayAnswer::Supported(parts.clone()),
            &active,
        )
        .unwrap();
        let SavedArticleChatOutcome::Supported {
            thread_id,
            question_id,
            answer_id,
            parts: verified,
        } = saved
        else {
            panic!("expected a supported answer");
        };
        assert_eq!(verified, parts);
        assert!(answer_id > question_id);
        let messages = state
            .chat_messages(&article.key, 12, thread_id, 10)
            .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "gravity");
        assert_eq!(
            messages[1].content,
            format!("Gravity attracts [[cite:{}]]", hit.block_id)
        );
        assert!(matches!(
            persist_validated_display(
                &mut state,
                generation,
                &article,
                "gravity",
                Some(thread_id + 1),
                LocalArticleDisplayAnswer::Supported(parts),
                &active,
            ),
            Err(ArticleChatError::Store(UserStateError::InvalidValue))
        ));
        assert_eq!(
            state
                .chat_messages(&article.key, 12, thread_id, 10)
                .unwrap()
                .len(),
            2
        );
        state.clear_personal_reading_data().unwrap();
        assert!(matches!(
            persist_validated_display(
                &mut state,
                generation,
                &article,
                "gravity",
                None,
                LocalArticleDisplayAnswer::InsufficientEvidence,
                &active,
            ),
            Err(ArticleChatError::StalePersonalData)
        ));
        assert!(state
            .list_chat_threads(&article.key, 12, 10)
            .unwrap()
            .is_empty());
    }

    fn http(body: &str, content_type: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn loopback_discovery_consent_inference_and_persistence_are_one_use_case() {
        let article = article();
        let hit = ArticleLexicalIndex::build(&article)
            .unwrap()
            .search("gravity", 1)
            .pop()
            .unwrap();
        let answer = format!("Gravity attracts masses [[cite:{}]]", hit.block_id);
        let chunk = format!(
            "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\",\"content\":\"{answer}\"}},\"finish_reason\":null}}]}}\n\n"
        );
        let sse = format!(
            "{chunk}data: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n"
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in [
                http(r#"{"models":[{"name":"qwen3:8b"}]}"#, "application/json"),
                http(&sse, "text/event-stream"),
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buf = [0; 4096];
                    let n = socket.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    let Some(split) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
                        continue;
                    };
                    let headers = std::str::from_utf8(&bytes[..split]).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("Content-Length: "))
                        .map(|n| n.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if bytes.len() >= split + 4 + length {
                        break;
                    }
                }
                socket.write_all(&response).unwrap();
                requests.push(String::from_utf8(bytes).unwrap());
            }
            requests
        });
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
        };
        let preview = selected
            .prepare_article_preview(&article, &question)
            .unwrap()
            .unwrap();
        assert_eq!(preview.passages(), &[hit]);
        let mut state = UserState::in_memory().unwrap();
        let generation = state.personal_generation().unwrap();
        let token = CancellationToken::default();
        assert!(matches!(
            answer_and_save_local_article(
                &mut state,
                generation,
                ArticleChatRequest {
                    article: &article,
                    question: &question,
                    selected: &selected,
                    preview: &preview,
                    share_article_passages: false,
                    existing_thread: None,
                    cancellation: &token,
                },
            ),
            Err(ArticleChatError::Inference(
                LocalArticleError::PassageConsentRequired
            ))
        ));
        assert!(state
            .list_chat_threads(&article.key, 12, 10)
            .unwrap()
            .is_empty());
        let saved = answer_and_save_local_article(
            &mut state,
            generation,
            ArticleChatRequest {
                article: &article,
                question: &question,
                selected: &selected,
                preview: &preview,
                share_article_passages: true,
                existing_thread: None,
                cancellation: &token,
            },
        )
        .unwrap();
        let SavedArticleChatOutcome::Supported {
            thread_id, parts, ..
        } = saved
        else {
            panic!("expected supported answer");
        };
        assert_eq!(parts.len(), 2);
        assert_eq!(
            state
                .chat_messages(&article.key, 12, thread_id, 10)
                .unwrap()
                .len(),
            2
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /api/tags HTTP/1.1\r\n"));
        assert!(requests[1].starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        assert!(requests[1].contains("Gravity is attraction between masses."));
    }
}
