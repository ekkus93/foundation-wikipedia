//! Platform-neutral, consent-gated article chat (local or hosted model) with
//! atomic persistence.
//!
//! This use case requires a canonical exact-revision Article from a verified
//! source. It never constructs evidence from an HTML preview, guesses a model,
//! silently switches endpoints, or upgrades historical saved citations.
//! UI adapters must disclose preview.passages(), model() and endpoint() and
//! obtain affirmative consent for THIS request before invoking it.

use wiki_ai::article_destination::{
    answer_article_with_preview_display, ArticleDestination, ArticlePreview,
};
use wiki_ai::local_selection::{LocalArticleDisplayAnswer, LocalArticleError, SelectedLocalModel};
use wiki_ai::rag::{ArticleAnswerDisplayPart, ArticleQuestion, MAX_PRIOR_TURNS};
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
    /// The question's prior turns are not exactly the thread's stored
    /// recent exchanges (or a new thread was sent history).
    HistoryMismatch,
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
#[allow(clippy::too_many_arguments)]
fn persist_validated_display(
    state: &mut UserState,
    generation: u64,
    article: &Article,
    question: &str,
    thread_id: Option<i64>,
    answer: LocalArticleDisplayAnswer,
    evidence_snapshot: Option<&str>,
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
    // The exact evidence each distinct cited block came from, in first-cited
    // order, saved with the answer so history can show it later (schema v3).
    let mut citations: Vec<wiki_store::user_state::StoredCitation> = Vec::new();
    for part in &parts {
        match part {
            ArticleAnswerDisplayPart::Text(text) => stored.push_str(text),
            ArticleAnswerDisplayPart::Citation(target) => {
                if !citations
                    .iter()
                    .any(|saved| saved.block_id == target.evidence.block_id)
                {
                    let saved = wiki_search::saved_citation::SavedCitation::from_evidence(
                        &article.key,
                        &target.evidence,
                        evidence_snapshot,
                    )
                    .map_err(|_| ArticleChatError::InvalidSavedMessage)?;
                    citations.push(saved.to_stored());
                }
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
        .save_cited_chat_exchange_for_generation(
            generation,
            &article.key,
            article.revision.revision_id,
            thread_id,
            question,
            &stored,
            &citations,
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
///
/// `D` is the explicitly selected destination: a discovered local model
/// (`SelectedLocalModel`) or a hosted one (`SelectedHostedModel`, which also
/// needs `OutboundPolicy::AllowCloud`).
pub struct ArticleChatRequest<'a, D: ArticleDestination = SelectedLocalModel> {
    pub article: &'a Article,
    pub question: &'a ArticleQuestion<'a>,
    pub selected: &'a D,
    pub preview: &'a ArticlePreview<D>,
    pub share_article_passages: bool,
    pub existing_thread: Option<i64>,
    /// Installed snapshot the article was read from, if any; recorded with
    /// each saved citation.
    pub evidence_snapshot: Option<&'a str>,
    pub cancellation: &'a CancellationToken,
}

/// The last `MAX_PRIOR_TURNS` stored exchanges of an exact-revision thread,
/// oldest first, as (question, answer) pairs for building `PriorTurn`s.
/// Adapters pass these back through `ArticleQuestion::prior_turns`.
pub fn prior_turns_for_thread(
    state: &UserState,
    article: &Article,
    thread_id: i64,
) -> Result<Vec<(String, String)>, ArticleChatError> {
    Ok(state
        .recent_chat_exchanges(
            &article.key,
            article.revision.revision_id,
            thread_id,
            MAX_PRIOR_TURNS,
        )
        .map_err(ArticleChatError::Store)?
        .into_iter()
        .map(|exchange| (exchange.question.content, exchange.answer.content))
        .collect())
}

pub fn answer_and_save_local_article(
    state: &mut UserState,
    generation: u64,
    request: ArticleChatRequest<'_>,
) -> Result<SavedArticleChatOutcome, ArticleChatError> {
    answer_and_save_article(state, generation, request)
}

/// [`answer_and_save_local_article`] for any explicitly selected destination.
pub fn answer_and_save_article<D: ArticleDestination>(
    state: &mut UserState,
    generation: u64,
    request: ArticleChatRequest<'_, D>,
) -> Result<SavedArticleChatOutcome, ArticleChatError> {
    let ArticleChatRequest {
        article,
        question,
        selected,
        preview,
        share_article_passages,
        existing_thread,
        evidence_snapshot,
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
    // Reject wrong-article/revision/deleted threads, and history that is not
    // exactly the thread's stored recent exchanges, before any provider I/O.
    let stored = match existing_thread {
        Some(id) => prior_turns_for_thread(state, article, id)?,
        None => Vec::new(),
    };
    let supplied: Vec<(&str, &str)> = question
        .prior_turns
        .iter()
        .map(|turn| (turn.question, turn.answer))
        .collect();
    let expected: Vec<(&str, &str)> = stored
        .iter()
        .map(|(question, answer)| (question.as_str(), answer.as_str()))
        .collect();
    if supplied != expected {
        return Err(ArticleChatError::HistoryMismatch);
    }
    let answer = answer_article_with_preview_display(
        selected,
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
        evidence_snapshot,
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
    use wiki_ai::rag::PriorTurn;
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
                None,
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
                None,
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
            None,
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
        // The cited evidence is stored with the answer for later display.
        let citations = state
            .chat_citations(&article.key, 12, thread_id, answer_id)
            .unwrap();
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].block_id, hit.block_id);
        assert_eq!(citations[0].excerpt, hit.excerpt);
        assert_eq!(citations[0].snapshot_id, None);
        assert!(matches!(
            persist_validated_display(
                &mut state,
                generation,
                &article,
                "gravity",
                Some(thread_id + 1),
                LocalArticleDisplayAnswer::Supported(parts),
                None,
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
                None,
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
        // Expected evidence is ranked by the same structural retriever that
        // chat context preparation uses (including its rank score).
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
            prior_turns: &[],
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
                    evidence_snapshot: None,
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
                evidence_snapshot: None,
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

        // Bounded history must be exactly the thread's stored exchanges. The
        // server has stopped, so these mismatches prove no provider I/O.
        let stored = prior_turns_for_thread(&state, &article, thread_id).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].0, "gravity");
        let stored_turns: Vec<PriorTurn<'_>> = stored
            .iter()
            .map(|(question, answer)| PriorTurn { question, answer })
            .collect();
        let fabricated = [PriorTurn {
            question: "gravity",
            answer: "Something the thread never said.",
        }];
        for (label, turns, thread, expect_mismatch) in [
            ("missing history", &[][..], Some(thread_id), true),
            ("fabricated history", &fabricated[..], Some(thread_id), true),
            ("history on a new thread", &stored_turns[..], None, true),
            (
                "exact stored history",
                &stored_turns[..],
                Some(thread_id),
                false,
            ),
        ] {
            let follow_up = ArticleQuestion {
                prior_turns: turns,
                ..question.clone()
            };
            let follow_up_preview = selected
                .prepare_article_preview(&article, &follow_up)
                .unwrap()
                .unwrap();
            assert_eq!(follow_up_preview.history().len(), turns.len(), "{label}");
            let result = answer_and_save_local_article(
                &mut state,
                generation,
                ArticleChatRequest {
                    article: &article,
                    question: &follow_up,
                    selected: &selected,
                    preview: &follow_up_preview,
                    share_article_passages: true,
                    existing_thread: thread,
                    evidence_snapshot: None,
                    cancellation: &token,
                },
            );
            if expect_mismatch {
                assert!(
                    matches!(result, Err(ArticleChatError::HistoryMismatch)),
                    "{label}: {result:?}"
                );
            } else {
                // Passed the history check and reached the (stopped) server.
                assert!(
                    matches!(result, Err(ArticleChatError::Inference(_))),
                    "{label}: {result:?}"
                );
            }
        }
        assert_eq!(
            state
                .chat_messages(&article.key, 12, thread_id, 10)
                .unwrap()
                .len(),
            2,
            "refused follow-ups write nothing"
        );
    }
}
