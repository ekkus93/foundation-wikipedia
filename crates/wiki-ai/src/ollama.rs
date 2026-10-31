//! Explicit Ollama adapter (AI-002): native /api/tags model discovery and
//! OpenAI-compatible /v1/chat/completions streaming on the same pinned host.
//!
//! No DNS, proxy, redirect, credentials, cloud fallback or implicit endpoint
//! discovery. HTTPS remains unsupported until authenticated TLS is available.

use std::collections::HashSet;

use serde_json::Value;

use crate::local_endpoint::LocalEndpoint;
use crate::local_http::fetch_local_json;
use crate::{
    valid_model_id, CancellationToken, GenerateRequest, LlmProvider, Locality, ModelInfo,
    ProviderCapabilities, ProviderError, StreamEvent,
};

const MAX_MODELS: usize = 256;

pub struct LocalOllamaProvider {
    endpoint: LocalEndpoint,
}

impl LocalOllamaProvider {
    pub fn new(endpoint: LocalEndpoint) -> Self {
        Self { endpoint }
    }
}

impl LlmProvider for LocalOllamaProvider {
    fn locality(&self) -> Locality {
        self.endpoint.locality()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        let supported = self.endpoint.base_url().starts_with("http://");
        ProviderCapabilities {
            discover_models: supported,
            streaming: supported,
            cancellable: supported,
        }
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        discover_ollama_models(&self.endpoint)
    }

    fn test_connection(&self) -> Result<(), ProviderError> {
        discover_ollama_models(&self.endpoint).map(|_| ())
    }

    fn stream(
        &self,
        request: &GenerateRequest,
        cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
    ) -> Result<(), ProviderError> {
        // Ollama exposes the OpenAI-compatible chat endpoint alongside its
        // native model inventory. Keep one explicit, pinned destination.
        crate::local_http_stream::stream_chat(&self.endpoint, request, cancellation, on_event)
    }
}

/// Native Ollama model discovery. /api/tags does not report a reliable
/// context-window limit, so the optional value is intentionally unknown.
pub fn discover_ollama_models(endpoint: &LocalEndpoint) -> Result<Vec<ModelInfo>, ProviderError> {
    let json = fetch_local_json(endpoint, "/api/tags")?;
    parse_tags(&json)
}

fn parse_tags(json: &Value) -> Result<Vec<ModelInfo>, ProviderError> {
    let data = json
        .get("models")
        .and_then(Value::as_array)
        .ok_or(ProviderError::InvalidResponse)?;
    if data.len() > MAX_MODELS {
        return Err(ProviderError::InvalidResponse);
    }
    let mut seen = HashSet::new();
    let mut models = Vec::with_capacity(data.len());
    for item in data {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .ok_or(ProviderError::InvalidResponse)?;
        if !valid_model_id(name) || !seen.insert(name)
        {
            return Err(ProviderError::InvalidResponse);
        }
        if let Some(model) = item.get("model") {
            if model.as_str() != Some(name) {
                return Err(ProviderError::InvalidResponse);
            }
        }
        models.push(ModelInfo {
            id: name.to_owned(),
            context_window: None,
        });
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(response: Vec<u8>) -> (LocalEndpoint, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = [0u8; 4096];
            let len = socket.read(&mut request).unwrap();
            socket.write_all(&response).unwrap();
            String::from_utf8(request[..len].to_vec()).unwrap()
        });
        (
            LocalEndpoint::parse(&format!("http://127.0.0.1:{port}")).unwrap(),
            handle,
        )
    }

    #[test]
    fn native_ollama_tags_are_discovered_from_a_pinned_loopback_host() {
        let body = r#"{"models":[{"name":"qwen3:8b","model":"qwen3:8b","size":1024},{"name":"llama3.2:latest"}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (endpoint, handle) = serve_once(response.into_bytes());
        let provider = LocalOllamaProvider::new(endpoint);
        assert_eq!(provider.locality(), Locality::Localhost);
        assert!(provider.capabilities().streaming);
        assert_eq!(
            provider.list_models().unwrap(),
            vec![
                ModelInfo {
                    id: "qwen3:8b".into(),
                    context_window: None,
                },
                ModelInfo {
                    id: "llama3.2:latest".into(),
                    context_window: None,
                }
            ]
        );
        assert!(handle
            .join()
            .unwrap()
            .starts_with("GET /api/tags HTTP/1.1\r\n"));
    }

    #[test]
    fn native_ollama_tags_reject_ambiguous_or_malformed_inventory() {
        for json in [
            r#"{}"#,
            r#"{"models":{}}"#,
            r#"{"models":[{"name":"a"},{"name":"a"}]}"#,
            r#"{"models":[{"name":"a","model":"b"}]}"#,
            r#"{"models":[{"name":" a"}]}"#,
            r#"{"models":[{"name":"a\nb"}]}"#,
            r#"{"models":[{"name":""}]}"#,
        ] {
            let value: Value = serde_json::from_str(json).unwrap();
            assert_eq!(
                parse_tags(&value),
                Err(ProviderError::InvalidResponse),
                "{json}"
            );
        }
        let large = serde_json::json!({
            "models": (0..=MAX_MODELS).map(|i| serde_json::json!({"name": format!("model-{i}")})).collect::<Vec<_>>()
        });
        assert_eq!(parse_tags(&large), Err(ProviderError::InvalidResponse));
    }

    #[test]
    fn native_tags_reject_invisible_and_directional_model_aliases() {
        for name in ["qwen\u{202e}3:8b", "qwen\u{200b}3:8b", "qwen\u{2066}3:8b"] {
            let value = serde_json::json!({"models": [{"name": name}]});
            assert_eq!(parse_tags(&value), Err(ProviderError::InvalidResponse), "{name:?}");
        }
        let valid = serde_json::json!({"models": [{"name": "modèle-1"}]});
        assert_eq!(parse_tags(&valid).unwrap()[0].id, "modèle-1");
    }

    #[test]
    fn ollama_chat_uses_selected_model_and_openai_stream_on_same_host() {
        let body = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (endpoint, handle) = serve_once(response.into_bytes());
        let provider = LocalOllamaProvider::new(endpoint);
        let request = GenerateRequest {
            model: "qwen3:8b".into(),
            messages: vec![crate::Message {
                role: crate::MessageRole::User,
                content: "hello".into(),
            }],
            max_output_tokens: 64,
        };
        let mut events = Vec::new();
        provider
            .stream(&request, &CancellationToken::default(), &mut |event| {
                events.push(event);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            events,
            vec![
                StreamEvent::TextDelta("Hi".into()),
                StreamEvent::Completed {
                    input_tokens: None,
                    output_tokens: None,
                }
            ]
        );
        let sent = handle.join().unwrap();
        assert!(sent.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
    }

    #[test]
    fn native_ollama_never_downgrades_https_or_public_hosts() {
        assert!(LocalEndpoint::parse("http://api.ollama.com").is_err());
        let provider =
            LocalOllamaProvider::new(LocalEndpoint::parse("https://127.0.0.1:443").unwrap());
        assert!(!provider.capabilities().discover_models);
        assert_eq!(provider.list_models(), Err(ProviderError::Unsupported));
    }
}
