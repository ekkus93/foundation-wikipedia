//! UX-002 / DUI-002: grounded article chat on the desktop, for articles of
//! the active installed pack, through the same shared code as Android.
//!
//! A question is first *prepared* with no network use: the result discloses
//! the destination, its locality, the model and every passage that would be
//! sent. Only an explicit send transmits it, to the user's active local
//! model profile (never anything else). Progress, including an unchecked
//! draft whose citations are placeholders, is emitted as events; the final
//! answer's citations are validated against the exact article revision.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{Emitter, Manager};
use wiki_ffi::android::{
    discover_local_ai, ArticleChat, ChatAnswer, ChatAnswerPart, ChatError, ChatLocality,
    ChatPassage, ChatProgress, ChatProgressUpdate, LocalAiError, LocalAiProtocol, LocalChatRequest,
};

use crate::offline_packs::library;

/// Event carrying `ChatProgressDto` while a request runs.
pub const PROGRESS_EVENT: &str = "pack-chat-progress";
/// Prepared but unsent questions kept at once (older ones are discarded).
const MAX_PENDING: usize = 8;
const MAX_QUESTION_CHARS: usize = 2000;
const MAX_SELECTION_CHARS: usize = 2000;

#[derive(Default)]
pub struct PackChatState {
    next: AtomicU64,
    chats: Mutex<HashMap<u64, Arc<ArticleChat>>>,
}

#[derive(Serialize)]
pub struct PassageDto {
    block_id: String,
    heading_path: Vec<String>,
    excerpt: String,
}

impl From<ChatPassage> for PassageDto {
    fn from(passage: ChatPassage) -> Self {
        Self {
            block_id: passage.block_id,
            heading_path: passage.heading_path,
            excerpt: passage.excerpt,
        }
    }
}

/// Everything the request would send, shown before the user confirms.
#[derive(Serialize)]
pub struct ChatPreviewDto {
    id: u64,
    destination: String,
    /// "this device", "local network" or "internet".
    locality: &'static str,
    model: String,
    profile: String,
    article_title: String,
    revision_id: u64,
    passages: Vec<PassageDto>,
    selected_passage: Option<PassageDto>,
}

#[derive(Serialize, Clone)]
pub struct ChatProgressDto {
    id: u64,
    elapsed_ms: u64,
    first_output_ms: Option<u64>,
    finished: bool,
    /// Unchecked draft text; citation markers are placeholders.
    draft: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnswerPartDto {
    Text { text: String },
    Citation { passage: PassageDto },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnswerDto {
    Supported { parts: Vec<AnswerPartDto> },
    InsufficientEvidence,
}

fn locality(locality: ChatLocality) -> &'static str {
    match locality {
        ChatLocality::ThisDevice => "this device",
        ChatLocality::LocalNetwork => "local network",
        ChatLocality::Cloud => "internet",
    }
}

/// Plain-language text for a failed chat request; never includes a key.
fn chat_message(error: ChatError) -> String {
    match error {
        ChatError::NoEvidence => {
            "No part of this article matches the question, so nothing was sent. Try rephrasing."
        }
        ChatError::SelectionNotFound => {
            "The selected text was not found within one paragraph of this article. Select text inside a single paragraph."
        }
        ChatError::NotInstalled => "This article is no longer in the active offline pack.",
        ChatError::VerificationFailed => "The installed article failed verification.",
        ChatError::Cancelled => "Stopped. No answer was shown.",
        ChatError::Timeout => "The model did not respond in time.",
        ChatError::Network => "Could not reach the model server. Is it running?",
        ChatError::Rejected => "The model refused the request or stopped before finishing an answer.",
        ChatError::InvalidResponse => "The model sent an unexpected response.",
        ChatError::Unverifiable => {
            "The answer's citations did not match this article, so it is not shown."
        }
        ChatError::PreviewChanged | ChatError::AlreadySent => {
            "The article or question changed. Review the request again."
        }
        ChatError::ConsentRequired => "Nothing was sent.",
        _ => "Something went wrong with the chat request.",
    }
    .to_owned()
}

fn local_message(error: LocalAiError) -> String {
    match error {
        LocalAiError::InvalidEndpoint => "The active local model's address is not valid.",
        LocalAiError::Unreachable => {
            "Nothing answered at the local model's address. Is the server running?"
        }
        LocalAiError::Timeout => "The local model server did not answer in time.",
        LocalAiError::NoModels => "The local model server has no models.",
        _ => "Could not reach the local model server.",
    }
    .to_owned()
}

fn bounded(text: &str, max: usize) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max {
        return Err("Enter a question of at most 2000 characters.".to_owned());
    }
    Ok(trimmed.to_owned())
}

/// Prepare (no network beyond reading the active local server's model list)
/// a question about an article of the active pack.
#[tauri::command]
pub async fn prepare_pack_chat(
    app: tauri::AppHandle,
    project: String,
    page_id: u64,
    question: String,
    selection: Option<String>,
) -> Result<ChatPreviewDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let question = bounded(&question, MAX_QUESTION_CHARS)?;
        let selection = match selection {
            Some(text) => Some(bounded(&text, MAX_SELECTION_CHARS)?),
            None => None,
        };
        let (profile, protocol, endpoint, model) = crate::local_ai_profiles::active_profile(&app)?
            .ok_or("Chat needs an active local model. Choose one in Settings › AI Assistant.")?;
        let protocol = match protocol.as_str() {
            "ollama" => LocalAiProtocol::Ollama,
            _ => LocalAiProtocol::OpenAiCompatible,
        };
        let server = discover_local_ai(protocol, endpoint).map_err(local_message)?;
        let chat = library(&app)?
            .prepare_local_chat(
                LocalChatRequest {
                    project,
                    page_id,
                    question,
                    server,
                    model,
                    thread_id: None,
                    selection,
                },
                None,
            )
            .map_err(chat_message)?;
        let state = app.state::<PackChatState>();
        let id = state.next.fetch_add(1, Ordering::Relaxed) + 1;
        let preview = ChatPreviewDto {
            id,
            destination: chat.destination(),
            locality: locality(chat.locality()),
            model: chat.model(),
            profile,
            article_title: chat.article_title(),
            revision_id: chat.revision_id(),
            passages: chat.passages().into_iter().map(Into::into).collect(),
            selected_passage: chat.selected_passage().map(Into::into),
        };
        let mut chats = state.chats.lock().map_err(|_| "Chat unavailable")?;
        if chats.len() >= MAX_PENDING {
            let oldest = chats.keys().min().copied();
            if let Some(oldest) = oldest {
                chats.remove(&oldest);
            }
        }
        chats.insert(id, chat);
        Ok(preview)
    })
    .await
    .map_err(|_| "Chat worker stopped".to_owned())?
}

struct EmitProgress {
    app: tauri::AppHandle,
    id: u64,
}

impl ChatProgress for EmitProgress {
    fn on_progress(&self, update: ChatProgressUpdate) {
        let _ = self.app.emit(
            PROGRESS_EVENT,
            ChatProgressDto {
                id: self.id,
                elapsed_ms: update.elapsed_ms,
                first_output_ms: update.first_output_ms,
                finished: update.finished,
                draft: update.draft,
            },
        );
    }
}

/// Send a prepared question after the user reviewed and confirmed it.
#[tauri::command]
pub async fn send_pack_chat(app: tauri::AppHandle, id: u64) -> Result<AnswerDto, String> {
    let chat = app
        .state::<PackChatState>()
        .chats
        .lock()
        .map_err(|_| "Chat unavailable")?
        .get(&id)
        .cloned()
        .ok_or("Review the request again.")?;
    let progress: Arc<dyn ChatProgress> = Arc::new(EmitProgress {
        app: app.clone(),
        id,
    });
    let result = tauri::async_runtime::spawn_blocking(move || chat.send(true, Some(progress)))
        .await
        .map_err(|_| "Chat worker stopped".to_owned())?;
    if let Ok(mut chats) = app.state::<PackChatState>().chats.lock() {
        chats.remove(&id);
    }
    let reply = result.map_err(chat_message)?;
    Ok(match reply.answer {
        ChatAnswer::InsufficientEvidence => AnswerDto::InsufficientEvidence,
        ChatAnswer::Supported { parts } => AnswerDto::Supported {
            parts: parts
                .into_iter()
                .map(|part| match part {
                    ChatAnswerPart::Text { text } => AnswerPartDto::Text { text },
                    ChatAnswerPart::Citation { passage } => AnswerPartDto::Citation {
                        passage: passage.into(),
                    },
                })
                .collect(),
        },
    })
}

/// Stop a request in progress (or discard an unsent one).
#[tauri::command]
pub fn cancel_pack_chat(app: tauri::AppHandle, id: u64) {
    if let Ok(chats) = app.state::<PackChatState>().chats.lock() {
        if let Some(chat) = chats.get(&id) {
            chat.cancel();
        }
    }
}

/// The exact article revision a citation points to, as sanitized body
/// HTML with the cited passage highlighted at `#foundation-cited`:
/// `(page ID, revision ID, title, body, located)`. `None` if the active
/// pack no longer has that revision.
#[tauri::command]
pub async fn open_offline_citation(
    app: tauri::AppHandle,
    project: String,
    page_id: u64,
    revision_id: u64,
    block_id: String,
) -> Result<Option<(u64, u64, String, String, bool)>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(cited) = library(&app)?
            .open_reader_citation(project, page_id, revision_id, block_id, false)
            .map_err(|_| "The cited article could not be opened.")?
        else {
            return Ok(None);
        };
        let body =
            article_body(&cited.page.document).ok_or("The cited article could not be opened.")?;
        Ok(Some((
            cited.page.page_id,
            cited.page.revision_id,
            cited.page.display_title,
            body.to_owned(),
            cited.located,
        )))
    })
    .await
    .map_err(|_| "Chat worker stopped".to_owned())?
}

/// The sanitized article body of a shared reader document (between its
/// single `<main>` and `</main>`; article markup cannot contain either, as
/// `main` elements inside it are re-sanitized text).
fn article_body(document: &str) -> Option<&str> {
    let start = document.find("<main>")? + "<main>".len();
    let end = document.rfind("</main>")?;
    (start <= end).then(|| &document[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_article_body_is_taken_from_the_reader_document() {
        assert_eq!(
            article_body("<html><h1>T</h1><main><p>Body</p></main></body></html>"),
            Some("<p>Body</p>")
        );
        assert_eq!(article_body("<p>no main</p>"), None);
        assert_eq!(article_body("</main><main>"), None);
    }

    #[test]
    fn questions_and_selections_are_bounded() {
        assert!(bounded("  ", 10).is_err());
        assert!(bounded(&"x".repeat(11), 10).is_err());
        assert_eq!(bounded(" What? ", 10).unwrap(), "What?");
    }
}
