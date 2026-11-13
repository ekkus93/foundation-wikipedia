//! AI-003 wire codec for the hosted Anthropic Messages API.
//! (Hosted OpenAI uses the OpenAI stream format in `openai_stream`.)
//!
//! Transport-independent and strict, like `openai_stream`: this module never
//! opens a socket, holds no HTTP client and performs no TLS. It builds request
//! bodies from a provider-neutral `GenerateRequest` and decodes streamed
//! responses into `StreamEvent`s, failing closed on anything other than plain
//! assistant text that ends with an explicit, successful stop. Tool use,
//! "thinking" output, safety blocks and truncation are never presented as a
//! grounded text answer.
//!
//! API keys travel only in headers (never in a URL, body or log line); the
//! `ApiKey` type redacts itself in `Debug` and has no `Display`.

use crate::sse::{SseDecoder, SseEvent};
use crate::{GenerateRequest, MessageRole, ProviderError, StreamEvent};
use serde_json::{json, Value};

pub const MAX_STREAM_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_KEY_BYTES: usize = 512;

/// A hosted-provider credential. Construct from OS secret storage; it is
/// never printed, serialized or placed in a URL by this crate.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Accept a printable-ASCII token without whitespace, as header values
    /// require. Rejecting control characters prevents header injection.
    pub fn new(secret: String) -> Result<Self, ProviderError> {
        if secret.is_empty()
            || secret.len() > MAX_KEY_BYTES
            || !secret.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(ProviderError::InvalidRequest);
        }
        Ok(Self(secret))
    }

    /// The raw key, only for writing it to OS secret storage. Never log,
    /// display, serialize or place it in a URL.
    pub fn expose_for_storage(&self) -> &str {
        &self.0
    }

    /// The single header carrying the key for `provider`.
    pub fn header(&self, provider: HostedProvider) -> (&'static str, String) {
        match provider {
            HostedProvider::Anthropic => ("x-api-key", self.0.clone()),
            HostedProvider::OpenAi => ("authorization", format!("Bearer {}", self.0)),
        }
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostedProvider {
    Anthropic,
    /// Hosted OpenAI Chat Completions (same stream format as local
    /// OpenAI-compatible servers, decoded by `openai_stream`).
    OpenAi,
}

/// Leading system messages become the provider's system field; the rest
/// must alternate user/assistant, starting and ending with a user turn.
/// System text plus (is_user, text) turns in order.
type SplitConversation<'a> = (String, Vec<(bool, &'a str)>);

fn split_messages(request: &GenerateRequest) -> Result<SplitConversation<'_>, ProviderError> {
    if request.model.trim().is_empty() || request.max_output_tokens == 0 {
        return Err(ProviderError::InvalidRequest);
    }
    let mut system = Vec::new();
    let mut turns = Vec::new();
    for message in &request.messages {
        match message.role {
            MessageRole::System if turns.is_empty() => system.push(message.content.as_str()),
            MessageRole::System => return Err(ProviderError::InvalidRequest),
            MessageRole::User => turns.push((true, message.content.as_str())),
            MessageRole::Assistant => turns.push((false, message.content.as_str())),
        }
    }
    let alternates = turns
        .iter()
        .enumerate()
        .all(|(i, (is_user, text))| *is_user == (i % 2 == 0) && !text.is_empty());
    if turns.is_empty() || !alternates || !turns.last().is_some_and(|(is_user, _)| *is_user) {
        return Err(ProviderError::InvalidRequest);
    }
    Ok((system.join("\n\n"), turns))
}

/// Anthropic Messages API streaming request body (`POST /v1/messages`).
pub fn anthropic_messages_body(request: &GenerateRequest) -> Result<String, ProviderError> {
    let (system, turns) = split_messages(request)?;
    let messages: Vec<Value> = turns
        .iter()
        .map(|(is_user, text)| {
            json!({
                "role": if *is_user { "user" } else { "assistant" },
                "content": [{"type": "text", "text": text}],
            })
        })
        .collect();
    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_output_tokens,
        "messages": messages,
        "stream": true,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    serde_json::to_string(&body).map_err(|_| ProviderError::InvalidRequest)
}

fn count(value: Option<&Value>) -> Result<Option<u32>, ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or(ProviderError::InvalidResponse),
    }
}

fn add_text(total: &mut usize, text: &str) -> Result<(), ProviderError> {
    *total = total
        .checked_add(text.len())
        .filter(|size| *size <= MAX_STREAM_TEXT_BYTES)
        .ok_or(ProviderError::InvalidResponse)?;
    Ok(())
}

#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
enum AnthropicPhase {
    #[default]
    Start,
    Message,
    Block,
    BlockDone,
    Stopped,
    Done,
}

/// Strict decoder for Anthropic Messages SSE: `message_start`, one text
/// content block at index 0, `message_delta` with a normal stop reason, then
/// `message_stop`. `ping` is ignored; `error` and anything else fail closed.
#[derive(Default)]
pub struct AnthropicStreamDecoder {
    sse: SseDecoder,
    phase: AnthropicPhase,
    text_bytes: usize,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

impl AnthropicStreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>, ProviderError> {
        let mut out = Vec::new();
        for event in self
            .sse
            .push(chunk)
            .map_err(|_| ProviderError::InvalidResponse)?
        {
            self.accept(event, &mut out)?;
        }
        Ok(out)
    }

    /// EOF is a completed answer only after `message_stop`.
    pub fn finish(&self) -> Result<(), ProviderError> {
        self.sse
            .finish()
            .map_err(|_| ProviderError::InvalidResponse)?;
        if self.phase == AnthropicPhase::Done {
            Ok(())
        } else {
            Err(ProviderError::InvalidResponse)
        }
    }

    fn accept(&mut self, event: SseEvent, out: &mut Vec<StreamEvent>) -> Result<(), ProviderError> {
        use AnthropicPhase::*;
        if self.phase == Done {
            return Err(ProviderError::InvalidResponse);
        }
        let kind = event
            .event_type
            .as_deref()
            .ok_or(ProviderError::InvalidResponse)?;
        let value: Value =
            serde_json::from_str(&event.data).map_err(|_| ProviderError::InvalidResponse)?;
        // The SSE event name and the JSON type must agree.
        if value.get("type").and_then(Value::as_str) != Some(kind) {
            return Err(ProviderError::InvalidResponse);
        }
        match (kind, self.phase) {
            ("ping", _) => {}
            ("error", _) => return Err(ProviderError::Rejected),
            ("message_start", Start) => {
                let message = value.get("message").ok_or(ProviderError::InvalidResponse)?;
                if message.get("role").and_then(Value::as_str) != Some("assistant") {
                    return Err(ProviderError::InvalidResponse);
                }
                let usage = message.get("usage");
                self.input_tokens = count(usage.and_then(|u| u.get("input_tokens")))?;
                self.phase = Message;
            }
            ("content_block_start", Message) => {
                let block = value
                    .get("content_block")
                    .ok_or(ProviderError::InvalidResponse)?;
                if value.get("index").and_then(Value::as_u64) != Some(0) {
                    return Err(ProviderError::InvalidResponse);
                }
                // tool_use, thinking and other block kinds are not answers.
                if block.get("type").and_then(Value::as_str) != Some("text") {
                    return Err(ProviderError::Rejected);
                }
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    add_text(&mut self.text_bytes, text)?;
                    if !text.is_empty() {
                        out.push(StreamEvent::TextDelta(text.to_owned()));
                    }
                }
                self.phase = Block;
            }
            ("content_block_delta", Block) => {
                if value.get("index").and_then(Value::as_u64) != Some(0) {
                    return Err(ProviderError::InvalidResponse);
                }
                let delta = value.get("delta").ok_or(ProviderError::InvalidResponse)?;
                if delta.get("type").and_then(Value::as_str) != Some("text_delta") {
                    return Err(ProviderError::Rejected);
                }
                let text = delta
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or(ProviderError::InvalidResponse)?;
                add_text(&mut self.text_bytes, text)?;
                if !text.is_empty() {
                    out.push(StreamEvent::TextDelta(text.to_owned()));
                }
            }
            ("content_block_stop", Block) => {
                if value.get("index").and_then(Value::as_u64) != Some(0) {
                    return Err(ProviderError::InvalidResponse);
                }
                self.phase = BlockDone;
            }
            ("message_delta", BlockDone) => {
                match value
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    Some("end_turn" | "stop_sequence") => {}
                    // max_tokens truncation, tool_use, refusal, pause_turn…
                    Some(_) => return Err(ProviderError::Rejected),
                    None => return Err(ProviderError::InvalidResponse),
                }
                self.output_tokens =
                    count(value.get("usage").and_then(|u| u.get("output_tokens")))?;
                self.phase = Stopped;
            }
            ("message_stop", Stopped) => {
                self.phase = Done;
                out.push(StreamEvent::Completed {
                    input_tokens: self.input_tokens,
                    output_tokens: self.output_tokens,
                });
            }
            _ => return Err(ProviderError::InvalidResponse),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    fn request(messages: Vec<(MessageRole, &str)>) -> GenerateRequest {
        GenerateRequest {
            model: "test-model".into(),
            messages: messages
                .into_iter()
                .map(|(role, content)| Message {
                    role,
                    content: content.into(),
                })
                .collect(),
            max_output_tokens: 256,
        }
    }

    fn chat() -> GenerateRequest {
        request(vec![
            (MessageRole::System, "Use only the evidence."),
            (MessageRole::User, "Earlier question"),
            (MessageRole::Assistant, "Earlier answer"),
            (MessageRole::User, "Evidence and question"),
        ])
    }

    fn sse(events: &[(&str, &str)]) -> String {
        use std::fmt::Write;
        events.iter().fold(String::new(), |mut out, (kind, data)| {
            let _ = write!(
                out,
                "event: {kind}
data: {data}

"
            );
            out
        })
    }

    fn decode_anthropic(body: &str, chunk: usize) -> Result<Vec<StreamEvent>, ProviderError> {
        let mut decoder = AnthropicStreamDecoder::new();
        let mut events = Vec::new();
        for piece in body.as_bytes().chunks(chunk) {
            events.extend(decoder.push(piece)?);
        }
        decoder.finish()?;
        Ok(events)
    }

    fn anthropic_ok(stop: &str) -> String {
        sse(&[
            (
                "message_start",
                r#"{"type":"message_start","message":{"role":"assistant","usage":{"input_tokens":12}}}"#,
            ),
            ("ping", r#"{"type":"ping"}"#),
            (
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            ),
            (
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Gravity "}}"#,
            ),
            (
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"attracts."}}"#,
            ),
            (
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
            (
                "message_delta",
                &format!(
                    r#"{{"type":"message_delta","delta":{{"stop_reason":"{stop}"}},"usage":{{"output_tokens":4}}}}"#
                ),
            ),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ])
    }

    #[test]
    fn api_keys_are_redacted_and_header_safe() {
        let key = ApiKey::new("sk-ant-secret-123".into()).unwrap();
        assert_eq!(format!("{key:?}"), "ApiKey(<redacted>)");
        assert!(!format!("{:?}", Some(&key)).contains("secret"));
        assert_eq!(
            key.header(HostedProvider::Anthropic),
            ("x-api-key", "sk-ant-secret-123".to_owned())
        );
        assert_eq!(
            key.header(HostedProvider::OpenAi),
            ("authorization", "Bearer sk-ant-secret-123".to_owned())
        );
        for bad in ["", "has space", "line\r\nX-Injected: 1", "tab\t", "é"] {
            assert_eq!(
                ApiKey::new(bad.into()),
                Err(ProviderError::InvalidRequest),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn request_bodies_map_roles_and_keep_keys_out() {
        let anthropic: Value =
            serde_json::from_str(&anthropic_messages_body(&chat()).unwrap()).unwrap();
        assert_eq!(anthropic["system"], "Use only the evidence.");
        assert_eq!(anthropic["stream"], true);
        assert_eq!(anthropic["max_tokens"], 256);
        let roles: Vec<_> = anthropic["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["user", "assistant", "user"]);
        assert_eq!(
            anthropic["messages"][2]["content"][0]["text"],
            "Evidence and question"
        );
    }

    #[test]
    fn malformed_conversations_are_refused() {
        for (label, messages) in [
            ("no turns", vec![(MessageRole::System, "s")]),
            (
                "ends with assistant",
                vec![(MessageRole::User, "q"), (MessageRole::Assistant, "a")],
            ),
            (
                "starts with assistant",
                vec![(MessageRole::Assistant, "a"), (MessageRole::User, "q")],
            ),
            (
                "two users",
                vec![(MessageRole::User, "q"), (MessageRole::User, "q2")],
            ),
            (
                "late system",
                vec![(MessageRole::User, "q"), (MessageRole::System, "s")],
            ),
            ("empty turn", vec![(MessageRole::User, "")]),
        ] {
            let req = request(messages);
            assert_eq!(
                anthropic_messages_body(&req),
                Err(ProviderError::InvalidRequest),
                "{label}"
            );
        }
        let mut zero = chat();
        zero.max_output_tokens = 0;
        assert!(anthropic_messages_body(&zero).is_err());
    }

    #[test]
    fn anthropic_stream_yields_text_and_one_completion() {
        let body = anthropic_ok("end_turn");
        for chunk in [1, 7, body.len()] {
            assert_eq!(
                decode_anthropic(&body, chunk).unwrap(),
                vec![
                    StreamEvent::TextDelta("Gravity ".into()),
                    StreamEvent::TextDelta("attracts.".into()),
                    StreamEvent::Completed {
                        input_tokens: Some(12),
                        output_tokens: Some(4)
                    },
                ],
                "chunk {chunk}"
            );
        }
        assert!(decode_anthropic(&anthropic_ok("stop_sequence"), 9).is_ok());
    }

    #[test]
    fn anthropic_stream_fails_closed() {
        let ok = anthropic_ok("end_turn");
        let cases: Vec<(&str, String, ProviderError)> = vec![
            (
                "truncated by max_tokens",
                anthropic_ok("max_tokens"),
                ProviderError::Rejected,
            ),
            (
                "tool use stop",
                anthropic_ok("tool_use"),
                ProviderError::Rejected,
            ),
            (
                "refusal stop",
                anthropic_ok("refusal"),
                ProviderError::Rejected,
            ),
            (
                "no message_stop",
                ok.replace(
                    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                    "",
                ),
                ProviderError::InvalidResponse,
            ),
            (
                "tool_use block",
                ok.replace(
                    r#""content_block":{"type":"text","text":""}"#,
                    r#""content_block":{"type":"tool_use","id":"t","name":"x","input":{}}"#,
                ),
                ProviderError::Rejected,
            ),
            (
                "thinking delta",
                ok.replace(
                    r#""delta":{"type":"text_delta","text":"Gravity "}"#,
                    r#""delta":{"type":"thinking_delta","thinking":"hmm"}"#,
                ),
                ProviderError::Rejected,
            ),
            (
                "second block",
                ok.replace(
                    r#"{"type":"content_block_stop","index":0}"#,
                    r#"{"type":"content_block_stop","index":1}"#,
                ),
                ProviderError::InvalidResponse,
            ),
            (
                "mismatched type",
                ok.replace(
                    r#"data: {"type":"ping"}"#,
                    r#"data: {"type":"message_stop"}"#,
                ),
                ProviderError::InvalidResponse,
            ),
            (
                "error event",
                format!(
                    "{}{}",
                    &ok[..ok.find("event: content_block_start").unwrap()],
                    sse(&[(
                        "error",
                        r#"{"type":"error","error":{"type":"overloaded_error"}}"#
                    )])
                ),
                ProviderError::Rejected,
            ),
            (
                "user role",
                ok.replace(r#""role":"assistant""#, r#""role":"user""#),
                ProviderError::InvalidResponse,
            ),
            (
                "after stop",
                format!("{ok}{}", sse(&[("ping", r#"{"type":"ping"}"#)])),
                ProviderError::InvalidResponse,
            ),
        ];
        for (label, body, expected) in cases {
            assert_eq!(decode_anthropic(&body, 5), Err(expected), "{label}");
        }
    }
}
