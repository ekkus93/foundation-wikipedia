//! Strict, bounded OpenAI-compatible chat-completion SSE decoding.
//!
//! This module never opens a socket. The future transport must pin the
//! validated local endpoint, reject redirects, bound reads and honor cancel.

use crate::sse::{SseDecoder, SseEvent};
use crate::{ProviderError, StreamEvent};
use serde_json::Value;

pub const MAX_STREAM_TEXT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
pub struct OpenAiStreamDecoder {
    sse: SseDecoder,
    saw_choice: bool,
    saw_stop: bool,
    done: bool,
    text_bytes: usize,
    usage: (Option<u32>, Option<u32>),
}

impl OpenAiStreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>, ProviderError> {
        let mut result = Vec::new();
        for event in self
            .sse
            .push(chunk)
            .map_err(|_| ProviderError::InvalidResponse)?
        {
            self.accept(event, &mut result)?;
        }
        Ok(result)
    }

    /// A clean EOF is not a completed response without both stop and [DONE].
    pub fn finish(&self) -> Result<(), ProviderError> {
        self.sse
            .finish()
            .map_err(|_| ProviderError::InvalidResponse)?;
        if self.done {
            Ok(())
        } else {
            Err(ProviderError::InvalidResponse)
        }
    }

    fn accept(&mut self, event: SseEvent, out: &mut Vec<StreamEvent>) -> Result<(), ProviderError> {
        if self.done {
            return Err(ProviderError::InvalidResponse);
        }
        if !matches!(event.event_type.as_deref(), None | Some("message")) {
            return Err(ProviderError::Rejected);
        }
        if event.data == "[DONE]" {
            if !self.saw_choice || !self.saw_stop {
                return Err(ProviderError::InvalidResponse);
            }
            self.done = true;
            out.push(StreamEvent::Completed {
                input_tokens: self.usage.0,
                output_tokens: self.usage.1,
            });
            return Ok(());
        }
        let value: Value =
            serde_json::from_str(&event.data).map_err(|_| ProviderError::InvalidResponse)?;
        if value.get("error").is_some() {
            return Err(ProviderError::Rejected);
        }
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .ok_or(ProviderError::InvalidResponse)?;
        if let Some(usage) = value.get("usage").filter(|v| !v.is_null()) {
            self.usage = parse_usage(usage)?;
        }
        if choices.is_empty() {
            if !self.saw_stop || value.get("usage").is_none_or(Value::is_null) {
                return Err(ProviderError::InvalidResponse);
            }
            return Ok(());
        }
        if self.saw_stop || choices.len() != 1 {
            return Err(ProviderError::InvalidResponse);
        }
        let choice = &choices[0];
        if choice.get("index").and_then(Value::as_u64) != Some(0) {
            return Err(ProviderError::InvalidResponse);
        }
        let delta = choice
            .get("delta")
            .and_then(Value::as_object)
            .ok_or(ProviderError::InvalidResponse)?;
        // Streaming text must originate from the assistant role. A malicious
        // or incompatible endpoint cannot inject system, user or tool output
        // into the validated answer stream by changing delta.role.
        if delta
            .get("role")
            .is_some_and(|role| role.as_str() != Some("assistant"))
        {
            return Err(ProviderError::InvalidResponse);
        }
        // Never disguise tool execution or refusal as a grounded text answer.
        if delta.contains_key("tool_calls")
            || delta.contains_key("function_call")
            || delta.get("refusal").is_some_and(|v| !v.is_null())
        {
            return Err(ProviderError::Rejected);
        }
        if let Some(content) = delta.get("content").filter(|v| !v.is_null()) {
            let content = content.as_str().ok_or(ProviderError::InvalidResponse)?;
            self.text_bytes = self
                .text_bytes
                .checked_add(content.len())
                .filter(|size| *size <= MAX_STREAM_TEXT_BYTES)
                .ok_or(ProviderError::InvalidResponse)?;
            if !content.is_empty() {
                out.push(StreamEvent::TextDelta(content.to_owned()));
            }
        }
        self.saw_choice = true;
        match choice.get("finish_reason").filter(|v| !v.is_null()) {
            None => {}
            Some(Value::String(s)) if s == "stop" => self.saw_stop = true,
            Some(Value::String(_)) => return Err(ProviderError::Rejected),
            _ => return Err(ProviderError::InvalidResponse),
        }
        Ok(())
    }
}

fn parse_usage(value: &Value) -> Result<(Option<u32>, Option<u32>), ProviderError> {
    let object = value.as_object().ok_or(ProviderError::InvalidResponse)?;
    fn count(value: Option<&Value>) -> Result<Option<u32>, ProviderError> {
        match value {
            None => Ok(None),
            Some(v) => v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .map(Some)
                .ok_or(ProviderError::InvalidResponse),
        }
    }
    Ok((
        count(object.get("prompt_tokens"))?,
        count(object.get("completion_tokens"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: &str = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",",
        "\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n"
    );
    const STOP: &str =
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\n";

    #[test]
    fn fragmented_stream_emits_text_and_single_terminal_event() {
        let mut parser = OpenAiStreamDecoder::new();
        let body = format!("{FIRST}{STOP}data: {{\"choices\":[],\"usage\":{{\"prompt_tokens\":9,\"completion_tokens\":2}}}}\n\ndata: [DONE]\n\n");
        let mut events = Vec::new();
        for chunk in body.as_bytes().chunks(7) {
            events.extend(parser.push(chunk).unwrap());
        }
        assert_eq!(
            events,
            vec![
                StreamEvent::TextDelta("Hello".into()),
                StreamEvent::TextDelta(" world".into()),
                StreamEvent::Completed {
                    input_tokens: Some(9),
                    output_tokens: Some(2)
                }
            ]
        );
        assert_eq!(parser.finish(), Ok(()));
        assert_eq!(
            parser.push(b"data: [DONE]\n\n"),
            Err(ProviderError::InvalidResponse)
        );
    }

    #[test]
    fn rejects_truncated_stream_or_premature_done() {
        let mut parser = OpenAiStreamDecoder::new();
        parser.push(FIRST.as_bytes()).unwrap();
        assert_eq!(parser.finish(), Err(ProviderError::InvalidResponse));
        assert_eq!(
            parser.push(b"data: [DONE]\n\n"),
            Err(ProviderError::InvalidResponse)
        );
        let mut parser = OpenAiStreamDecoder::new();
        parser.push(STOP.as_bytes()).unwrap();
        assert_eq!(parser.finish(), Err(ProviderError::InvalidResponse));
        parser.push(b"data: [DONE]\n").unwrap();
        assert_eq!(parser.finish(), Err(ProviderError::InvalidResponse));
    }

    #[test]
    fn rejects_refusals_tools_nonstop_and_server_errors() {
        for body in [
            "event: error\ndata: {\"error\":{\"message\":\"denied\"}}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"refusal\":\"no\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
        ] {
            assert_eq!(
                OpenAiStreamDecoder::new().push(body.as_bytes()),
                Err(ProviderError::Rejected),
                "{body}"
            );
        }
    }

    #[test]
    fn rejects_non_assistant_stream_roles() {
        for role in ["system", "user", "tool", "function"] {
            let data = format!(
                "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"{role}\",\"content\":\"unsafe\"}},\"finish_reason\":null}}]}}\n\n"
            );
            assert_eq!(
                OpenAiStreamDecoder::new().push(data.as_bytes()),
                Err(ProviderError::InvalidResponse),
                "{role}"
            );
        }
        for role in ["null", "42", "true"] {
            let data = format!(
                "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"role\":{role},\"content\":\"unsafe\"}},\"finish_reason\":null}}]}}\n\n"
            );
            assert_eq!(
                OpenAiStreamDecoder::new().push(data.as_bytes()),
                Err(ProviderError::InvalidResponse),
                "{role}"
            );
        }
        let mut parser = OpenAiStreamDecoder::new();
        assert_eq!(
            parser.push(FIRST.as_bytes()).unwrap(),
            vec![StreamEvent::TextDelta("Hello".into())]
        );
    }

    #[test]
    fn rejects_invalid_choices_usage_and_post_stop_data() {
        for body in [
            "data: {\"choices\":[{\"index\":1,\"delta\":{}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":42}}]}\n\n",
            "data: {\"choices\":[]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":4294967296}}\n\n",
        ] {
            assert_eq!(
                OpenAiStreamDecoder::new().push(body.as_bytes()),
                Err(ProviderError::InvalidResponse),
                "{body}"
            );
        }
        let mut parser = OpenAiStreamDecoder::new();
        parser.push(STOP.as_bytes()).unwrap();
        assert_eq!(
            parser.push(FIRST.as_bytes()),
            Err(ProviderError::InvalidResponse)
        );
        assert_eq!(
            OpenAiStreamDecoder::new().push(b"data: \xff\n\n"),
            Err(ProviderError::InvalidResponse)
        );
    }
}
