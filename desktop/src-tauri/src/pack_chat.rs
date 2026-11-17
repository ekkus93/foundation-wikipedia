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
use wiki_ai::hosted_codec::HostedProvider;
use wiki_ffi::android::{
    discover_local_ai, ArticleChat, ChatAnswer, ChatHistory, ChatAnswerPart, ChatError, ChatLocality,
    ChatPassage, ChatProgress, ChatProgressUpdate, HostedAiProvider, HostedChatRequest,
    LocalAiError, LocalAiProtocol, LocalChatRequest,
};

use crate::hosted_ai_keys::{load_key, OsKeychain, SecretStore};

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
    /// The exchange will be saved to the private history (history is on).
    saved: bool,
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

/// A sent question's result; `thread_id` is the saved conversation, if any.
#[derive(Serialize)]
pub struct ChatReplyDto {
    answer: AnswerDto,
    thread_id: Option<i64>,
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
        ChatError::Storage => "The answer could not be saved to the private history.",
        ChatError::InvalidKey => {
            "The saved API key is not valid. Replace it in Settings › AI Assistant."
        }
        ChatError::InvalidInput => "Enter a question and a valid model name.",
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

/// Where a question goes: the active local model profile, or a hosted
/// provider (with its model name) whose key is in OS secret storage.
#[derive(Debug, PartialEq, Eq)]
enum ChatProvider {
    Local,
    Hosted(HostedAiProvider, String),
}

fn parse_provider(provider: &str, model: Option<String>) -> Result<ChatProvider, String> {
    let hosted = |provider| -> Result<ChatProvider, String> {
        let model = model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty() && model.len() <= 256)
            .ok_or("Enter the model name for this provider in Settings › AI Assistant.")?;
        Ok(ChatProvider::Hosted(provider, model.to_owned()))
    };
    match provider {
        "local" => Ok(ChatProvider::Local),
        "anthropic" => hosted(HostedAiProvider::Anthropic),
        "openai" => hosted(HostedAiProvider::OpenAi),
        _ => Err("Unknown chat provider.".to_owned()),
    }
}

fn provider_label(provider: HostedAiProvider) -> &'static str {
    match provider {
        HostedAiProvider::Anthropic => "Anthropic",
        HostedAiProvider::OpenAi => "OpenAI",
    }
}

/// The private history to save into, only when the user turned reading
/// history on (the same consent as Android). Saved chats live in the
/// desktop's private user state, so its privacy reset clears them too.
fn chat_history(app: &tauri::AppHandle) -> Result<Option<Arc<ChatHistory>>, String> {
    let state = crate::open_private_bookmark_store(app)?;
    if !state
        .history_enabled()
        .map_err(|_| "Cannot read the history preference")?
    {
        return Ok(None);
    }
    let database = app
        .path()
        .app_data_dir()
        .map_err(|_| "Cannot find the app data directory")?
        .join("reader-state")
        .join("user-state.sqlite");
    ChatHistory::open_database(database)
        .map(Some)
        .map_err(|_| "Cannot open the private chat history".to_owned())
}

/// Prepare a question without sending it. A hosted key is read natively
/// from `store` and kept only in the prepared request's memory.
fn prepare(
    app: &tauri::AppHandle,
    store: &dyn SecretStore,
    provider: ChatProvider,
    project: String,
    page_id: u64,
    question: String,
    selection: Option<String>,
    thread_id: Option<i64>,
) -> Result<(String, Arc<ArticleChat>, bool), String> {
    let library = library(app)?;
    let history = chat_history(app)?;
    let saved = history.is_some();
    // A thread is continued only in the saved history.
    let thread_id = thread_id.filter(|_| saved);
    match provider {
        ChatProvider::Local => {
            let (profile, protocol, endpoint, model) = crate::local_ai_profiles::active_profile(
                app,
            )?
            .ok_or("Chat needs an active local model. Choose one in Settings › AI Assistant.")?;
            let protocol = match protocol.as_str() {
                "ollama" => LocalAiProtocol::Ollama,
                _ => LocalAiProtocol::OpenAiCompatible,
            };
            let server = discover_local_ai(protocol, endpoint).map_err(local_message)?;
            let chat = library
                .prepare_local_chat(
                    LocalChatRequest {
                        project,
                        page_id,
                        question,
                        server,
                        model,
                        thread_id,
                        selection,
                    },
                    history,
                )
                .map_err(chat_message)?;
            Ok((profile, chat, saved))
        }
        ChatProvider::Hosted(provider, model) => {
            let key = load_key(store, HostedProvider::from(provider))?;
            let chat = library
                .prepare_hosted_chat(
                    HostedChatRequest {
                        project,
                        page_id,
                        question,
                        provider,
                        model,
                        thread_id,
                        selection,
                    },
                    key.expose_for_storage().to_owned(),
                    history,
                )
                .map_err(chat_message)?;
            Ok((provider_label(provider).to_owned(), chat, saved))
        }
    }
}

/// Prepare (no request is sent; a local provider's model list is read) a
/// question about an article of the active pack.
#[tauri::command]
pub async fn prepare_pack_chat(
    app: tauri::AppHandle,
    project: String,
    page_id: u64,
    question: String,
    selection: Option<String>,
    provider: String,
    model: Option<String>,
    thread_id: Option<i64>,
) -> Result<ChatPreviewDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let question = bounded(&question, MAX_QUESTION_CHARS)?;
        let selection = match selection {
            Some(text) => Some(bounded(&text, MAX_SELECTION_CHARS)?),
            None => None,
        };
        let provider = parse_provider(&provider, model)?;
        let (profile, chat, saved) = prepare(
            &app,
            &OsKeychain,
            provider,
            project,
            page_id,
            question,
            selection,
            thread_id,
        )?;
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
            saved,
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
pub async fn send_pack_chat(app: tauri::AppHandle, id: u64) -> Result<ChatReplyDto, String> {
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
    let answer = match reply.answer {
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
    };
    Ok(ChatReplyDto {
        answer,
        thread_id: reply.thread_id,
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
            desktop_image_sources(body),
            cited.located,
        )))
    })
    .await
    .map_err(|_| "Chat worker stopped".to_owned())?
}

/// The shared reader document points pack images at Android's local URL;
/// the sanitizer writes that URL only into image sources it mapped, so the
/// desktop's `packimage` scheme can take its place exactly there.
fn desktop_image_sources(body: &str) -> String {
    body.replace(
        &format!("src=\"{}", wiki_source::reader_document::READER_IMAGE_BASE),
        &format!("src=\"{}", crate::offline_packs::PACK_IMAGE_BASE),
    )
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
        let digest = "ab".repeat(32);
        assert_eq!(
            desktop_image_sources(&format!(
                "<img src=\"https://foundation.invalid/media/{digest}\" alt=\"x\">"
            )),
            format!(
                "<img src=\"{}{digest}\" alt=\"x\">",
                crate::offline_packs::PACK_IMAGE_BASE
            )
        );
    }

    #[test]
    fn providers_are_explicit_and_hosted_ones_need_a_model() {
        assert_eq!(parse_provider("local", None), Ok(ChatProvider::Local));
        assert_eq!(
            parse_provider("anthropic", Some(" claude-x ".into())),
            Ok(ChatProvider::Hosted(
                HostedAiProvider::Anthropic,
                "claude-x".into()
            ))
        );
        assert_eq!(
            parse_provider("openai", Some("gpt-x".into())),
            Ok(ChatProvider::Hosted(
                HostedAiProvider::OpenAi,
                "gpt-x".into()
            ))
        );
        assert!(parse_provider("anthropic", None).is_err());
        assert!(parse_provider("openai", Some("  ".into())).is_err());
        assert!(parse_provider("cloud", Some("m".into())).is_err());
        assert!(parse_provider("", None).is_err());
    }

    #[test]
    fn questions_and_selections_are_bounded() {
        assert!(bounded("  ", 10).is_err());
        assert!(bounded(&"x".repeat(11), 10).is_err());
        assert_eq!(bounded(" What? ", 10).unwrap(), "What?");
    }
}
