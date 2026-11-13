//! AI-003 hosted provider transport: HTTPS to a fixed provider host.
//!
//! TLS uses rustls with the `ring` provider and the operating system's
//! certificate verifier (`rustls-platform-verifier`); certificate checks are
//! never disabled. Each provider talks only to its own fixed host on port
//! 443 — there is no user-supplied URL, proxy, redirect following or
//! fallback. The HTTP/1.1 exchange reuses the bounded, cancellable reader of
//! the local transport, and responses are decoded by the strict codecs in
//! `hosted_codec` / `openai_stream`.
//!
//! The API key is supplied by the app for each provider instance (the app
//! keeps it in OS secret storage); this module sends it only in its header
//! and never stores, logs or serializes it.
//!
//! Android needs a one-time JNI initialisation of the platform verifier from
//! the app before certificates can be checked; until that bridge exists
//! (MOB-001) construction returns `ProviderError::Unsupported` there.

use std::io::{BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};

use crate::hosted_codec::{
    anthropic_messages_body, AnthropicStreamDecoder, ApiKey, HostedProvider,
};
use crate::local_http_stream::{map_io, openai_chat_body, read_event_stream, ChatStreamDecoder};
use crate::openai_stream::OpenAiStreamDecoder;
use crate::{
    CancellationToken, GenerateRequest, LlmProvider, Locality, ModelInfo, ProviderCapabilities,
    ProviderError, StreamEvent,
};

const MAX_REQUEST_BYTES: usize = 256 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// Short socket read timeout so cancellation is noticed promptly.
const READ_POLL: Duration = Duration::from_millis(250);
const MAX_DURATION: Duration = Duration::from_secs(180);
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The fixed production host for each hosted API.
pub const fn provider_host(provider: HostedProvider) -> &'static str {
    match provider {
        HostedProvider::Anthropic => "api.anthropic.com",
        HostedProvider::OpenAi => "api.openai.com",
    }
}

struct Target {
    host: String,
    port: u16,
    tls: Arc<ClientConfig>,
}

/// A user-selected hosted model provider. Its locality is always `Cloud`,
/// so `stream_selected` refuses it unless the request allows cloud traffic.
pub struct HostedChatProvider {
    provider: HostedProvider,
    key: ApiKey,
    target: Target,
}

impl std::fmt::Debug for HostedChatProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostedChatProvider")
            .field("provider", &self.provider)
            .field("host", &self.target.host)
            .field("key", &self.key)
            .finish()
    }
}

fn ring() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

impl HostedChatProvider {
    /// Connect to `provider`'s fixed production host with OS certificate
    /// verification.
    pub fn new(provider: HostedProvider, key: ApiKey) -> Result<Self, ProviderError> {
        if cfg!(target_os = "android") {
            return Err(ProviderError::Unsupported);
        }
        use rustls_platform_verifier::BuilderVerifierExt;
        let tls = ClientConfig::builder_with_provider(ring())
            .with_safe_default_protocol_versions()
            .map_err(|_| ProviderError::Unsupported)?
            .with_platform_verifier()
            .map_err(|_| ProviderError::Unsupported)?
            .with_no_client_auth();
        Ok(Self {
            provider,
            key,
            target: Target {
                host: provider_host(provider).to_owned(),
                port: 443,
                tls: Arc::new(tls),
            },
        })
    }

    /// Test-only: a local TLS server trusted through an explicit root.
    #[cfg(test)]
    fn with_test_target(
        provider: HostedProvider,
        key: ApiKey,
        host: &str,
        port: u16,
        tls: ClientConfig,
    ) -> Self {
        Self {
            provider,
            key,
            target: Target {
                host: host.to_owned(),
                port,
                tls: Arc::new(tls),
            },
        }
    }

    fn request_parts(
        &self,
        request: &GenerateRequest,
    ) -> Result<(String, String, Box<dyn ChatStreamDecoder>), ProviderError> {
        Ok(match self.provider {
            HostedProvider::Anthropic => (
                "/v1/messages".to_owned(),
                anthropic_messages_body(request)?,
                Box::new(AnthropicStreamDecoder::new()),
            ),
            HostedProvider::OpenAi => (
                "/v1/chat/completions".to_owned(),
                openai_chat_body(request),
                Box::new(OpenAiStreamDecoder::new()),
            ),
        })
    }

    fn connect(&self, deadline: Instant) -> Result<TcpStream, ProviderError> {
        let addresses = (self.target.host.as_str(), self.target.port)
            .to_socket_addrs()
            .map_err(|_| ProviderError::Transport)?;
        let mut last = ProviderError::Transport;
        for address in addresses {
            if Instant::now() >= deadline {
                return Err(ProviderError::Timeout);
            }
            match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                Ok(socket) => return Ok(socket),
                Err(error) => last = map_io(error),
            }
        }
        Err(last)
    }
}

impl LlmProvider for HostedChatProvider {
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
        cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
    ) -> Result<(), ProviderError> {
        request.validate()?;
        if cancellation.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        let (path, body, mut decoder) = self.request_parts(request)?;
        if body.len() > MAX_REQUEST_BYTES {
            return Err(ProviderError::InvalidRequest);
        }
        let (key_name, key_value) = self.key.header(self.provider);
        let mut head = format!(
            "POST {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\nAccept-Encoding: identity\r\nContent-Length: {}\r\nConnection: close\r\n",
            self.target.host,
            body.len()
        );
        if self.provider == HostedProvider::Anthropic {
            head.push_str(&format!("anthropic-version: {ANTHROPIC_VERSION}\r\n"));
        }
        head.push_str(&format!("{key_name}: {key_value}\r\n\r\n"));

        let deadline = Instant::now() + MAX_DURATION;
        let socket = self.connect(deadline)?;
        socket.set_read_timeout(Some(READ_POLL)).map_err(map_io)?;
        socket
            .set_write_timeout(Some(WRITE_TIMEOUT))
            .map_err(map_io)?;
        let name = ServerName::try_from(self.target.host.clone())
            .map_err(|_| ProviderError::InvalidRequest)?;
        let connection = ClientConnection::new(Arc::clone(&self.target.tls), name)
            .map_err(|_| ProviderError::Transport)?;
        let mut tls = StreamOwned::new(connection, socket);
        if cancellation.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        // The handshake (including certificate verification) runs on the
        // first write; a verification failure surfaces as a transport error.
        tls.write_all(head.as_bytes()).map_err(map_io)?;
        tls.write_all(body.as_bytes()).map_err(map_io)?;
        tls.flush().map_err(map_io)?;
        drop(head);
        let mut reader = BufReader::new(tls);
        read_event_stream(
            &mut reader,
            decoder.as_mut(),
            cancellation,
            deadline,
            on_event,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{stream_selected, Message, MessageRole, OutboundPolicy};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::{RootCertStore, ServerConfig, ServerConnection};
    use std::io::Read;
    use std::net::TcpListener;
    use std::thread;

    struct TestServer {
        port: u16,
        client_tls: ClientConfig,
        handle: thread::JoinHandle<String>,
    }

    /// One-shot local TLS server for `localhost` with a fresh test CA.
    fn serve(response: String) -> TestServer {
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let issuer = rcgen::Issuer::new(ca_params, ca_key);
        let leaf_key = rcgen::KeyPair::generate().unwrap();
        let leaf = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
            .unwrap()
            .signed_by(&leaf_key, &issuer)
            .unwrap();

        let server_config = ServerConfig::builder_with_provider(ring())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(leaf.der().to_vec())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
            )
            .unwrap();
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(ca.der().to_vec())).unwrap();
        let client_tls = ClientConfig::builder_with_provider(ring())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let connection = ServerConnection::new(Arc::new(server_config)).unwrap();
            let mut tls = StreamOwned::new(connection, socket);
            let mut bytes = Vec::new();
            let mut buf = [0u8; 4096];
            let request = loop {
                let n = match tls.read(&mut buf) {
                    Ok(n) => n,
                    // The client refused our certificate.
                    Err(_) => return String::new(),
                };
                if n == 0 {
                    return String::from_utf8_lossy(&bytes).into_owned();
                }
                bytes.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&bytes).into_owned();
                if let Some(split) = text.find("\r\n\r\n") {
                    let length: usize = text[..split]
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: "))
                        .and_then(|n| n.trim().parse().ok())
                        .unwrap_or(0);
                    if bytes.len() >= split + 4 + length {
                        break text;
                    }
                }
            };
            tls.write_all(response.as_bytes()).unwrap();
            tls.conn.send_close_notify();
            let _ = tls.flush();
            request
        });
        TestServer {
            port,
            client_tls,
            handle,
        }
    }

    fn event_stream(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
            body.len()
        )
    }

    fn request() -> GenerateRequest {
        GenerateRequest {
            model: "test-model".into(),
            messages: vec![
                Message {
                    role: MessageRole::System,
                    content: "Use only the evidence.".into(),
                },
                Message {
                    role: MessageRole::User,
                    content: "Evidence and question".into(),
                },
            ],
            max_output_tokens: 64,
        }
    }

    fn key() -> ApiKey {
        ApiKey::new("sk-test-secret-value".into()).unwrap()
    }

    fn run(
        provider: HostedProvider,
        response: String,
        policy: OutboundPolicy,
    ) -> (Result<Vec<StreamEvent>, ProviderError>, String) {
        let server = serve(response);
        let hosted = HostedChatProvider::with_test_target(
            provider,
            key(),
            "localhost",
            server.port,
            server.client_tls,
        );
        let mut events = Vec::new();
        let result = stream_selected(
            &hosted,
            &request(),
            &CancellationToken::default(),
            policy,
            &mut |event| {
                events.push(event);
                Ok(())
            },
        )
        .map(|()| events);
        if policy != OutboundPolicy::AllowCloud {
            // No connection was made; release the waiting server thread.
            let _ = TcpStream::connect(("127.0.0.1", server.port));
        }
        (result, server.handle.join().unwrap())
    }

    const ANTHROPIC_OK: &str = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input_tokens\":7}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );

    #[test]
    fn anthropic_over_tls_streams_text_with_key_only_in_its_header() {
        let (result, sent) = run(
            HostedProvider::Anthropic,
            event_stream(ANTHROPIC_OK),
            OutboundPolicy::AllowCloud,
        );
        assert_eq!(
            result.unwrap(),
            vec![
                StreamEvent::TextDelta("Hello".into()),
                StreamEvent::Completed {
                    input_tokens: Some(7),
                    output_tokens: Some(1)
                }
            ]
        );
        let (head, body) = sent.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("POST /v1/messages HTTP/1.1\r\n"));
        assert!(head.contains("\r\nx-api-key: sk-test-secret-value"));
        assert!(head.contains("\r\nanthropic-version: 2023-06-01"));
        assert!(head.contains("\r\nAccept-Encoding: identity"));
        assert!(!body.contains("sk-test-secret-value"));
        assert_eq!(sent.matches("sk-test-secret-value").count(), 1);
    }

    #[test]
    fn openai_uses_its_path_and_bearer_header() {
        let openai_body = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Yo\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let (result, sent) = run(
            HostedProvider::OpenAi,
            event_stream(openai_body),
            OutboundPolicy::AllowCloud,
        );
        assert_eq!(result.unwrap()[0], StreamEvent::TextDelta("Yo".into()));
        assert!(sent.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        assert!(sent.contains("\r\nauthorization: Bearer sk-test-secret-value"));
    }

    #[test]
    fn redirects_errors_and_compressed_bodies_are_refused() {
        for (label, response, expected) in [
            (
                "redirect",
                "HTTP/1.1 302 Found\r\nLocation: https://evil.example/\r\nContent-Length: 0\r\n\r\n".to_owned(),
                ProviderError::Rejected,
            ),
            (
                "unauthorized",
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n".to_owned(),
                ProviderError::Rejected,
            ),
            (
                "gzip",
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Encoding: gzip\r\nContent-Length: 0\r\n\r\n".to_owned(),
                ProviderError::InvalidResponse,
            ),
            (
                "truncated stream",
                event_stream(&ANTHROPIC_OK[..ANTHROPIC_OK.find("event: message_stop").unwrap()]),
                ProviderError::InvalidResponse,
            ),
        ] {
            let (result, _) = run(HostedProvider::Anthropic, response, OutboundPolicy::AllowCloud);
            assert_eq!(result, Err(expected), "{label}");
        }
    }

    #[test]
    fn untrusted_certificates_and_disallowed_cloud_never_send_the_key() {
        // A client that trusts a different CA must refuse the server.
        let server = serve(event_stream(ANTHROPIC_OK));
        let other = serve(String::new());
        let hosted = HostedChatProvider::with_test_target(
            HostedProvider::Anthropic,
            key(),
            "localhost",
            server.port,
            other.client_tls,
        );
        let result = hosted.stream(&request(), &CancellationToken::default(), &mut |_| Ok(()));
        assert_eq!(result, Err(ProviderError::Transport));
        assert!(!server
            .handle
            .join()
            .unwrap()
            .contains("sk-test-secret-value"));
        let _ = TcpStream::connect(("127.0.0.1", other.port));
        let _ = other.handle.join();

        // Cloud locality is refused before any connection without consent.
        for policy in [
            OutboundPolicy::OnDeviceOnly,
            OutboundPolicy::AllowLocalEndpoints,
        ] {
            let (result, sent) = run(
                HostedProvider::Anthropic,
                event_stream(ANTHROPIC_OK),
                policy,
            );
            assert_eq!(result, Err(ProviderError::DestinationNotAllowed));
            assert!(!sent.contains("sk-test-secret-value"));
        }
    }

    #[test]
    fn production_providers_target_fixed_hosts_and_redact_keys() {
        assert_eq!(
            provider_host(HostedProvider::Anthropic),
            "api.anthropic.com"
        );
        assert_eq!(provider_host(HostedProvider::OpenAi), "api.openai.com");
        let provider = HostedChatProvider::new(HostedProvider::Anthropic, key()).unwrap();
        assert_eq!(provider.locality(), Locality::Cloud);
        let debug = format!("{provider:?}");
        assert!(debug.contains("api.anthropic.com"));
        assert!(!debug.contains("sk-test-secret-value"));
    }
}
