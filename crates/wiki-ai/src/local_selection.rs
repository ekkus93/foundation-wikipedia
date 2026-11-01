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
