//! Opt-in, DNS-free OpenAI-compatible local model discovery (AI-002).
//!
//! The read-only HTTP transport pins the already validated socket address,
//! never follows redirects, and sends no article passages or credentials.
//! Plain HTTP chat streaming is supported; HTTPS remains explicitly unavailable
//! until authenticated TLS transport is implemented.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::Value;

use crate::local_endpoint::LocalEndpoint;
use crate::{
    CancellationToken, GenerateRequest, LlmProvider, Locality, ModelInfo, ProviderCapabilities,
    ProviderError, StreamEvent,
};

const MAX_HTTP_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
const MAX_MODELS: usize = 256;
const TIMEOUT: Duration = Duration::from_secs(3);

pub struct LocalOpenAiProvider {
    endpoint: LocalEndpoint,
}

impl LocalOpenAiProvider {
    pub fn new(endpoint: LocalEndpoint) -> Self {
        Self { endpoint }
    }
}

impl LlmProvider for LocalOpenAiProvider {
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
        discover_models(&self.endpoint)
    }

    fn test_connection(&self) -> Result<(), ProviderError> {
        discover_models(&self.endpoint).map(|_| ())
    }

    fn stream(
        &self,
        request: &GenerateRequest,
        cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
    ) -> Result<(), ProviderError> {
        crate::local_http_stream::stream_chat(&self.endpoint, request, cancellation, on_event)
    }
}

/// HTTP-only initial transport: no DNS, no proxies, no redirects, no secrets.
pub fn discover_models(endpoint: &LocalEndpoint) -> Result<Vec<ModelInfo>, ProviderError> {
    let authority = endpoint
        .base_url()
        .strip_prefix("http://")
        .ok_or(ProviderError::Unsupported)?
        .split('/')
        .next()
        .ok_or(ProviderError::InvalidRequest)?;
    let mut socket =
        TcpStream::connect_timeout(&endpoint.connect_addr(), TIMEOUT).map_err(map_io)?;
    socket.set_read_timeout(Some(TIMEOUT)).map_err(map_io)?;
    socket.set_write_timeout(Some(TIMEOUT)).map_err(map_io)?;
    let request = format!(
        "GET /v1/models HTTP/1.1\r\nHost: {authority}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    socket.write_all(request.as_bytes()).map_err(map_io)?;
    let mut response = Vec::new();
    socket
        .take((MAX_HTTP_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut response)
        .map_err(map_io)?;
    if response.len() > MAX_HTTP_RESPONSE_BYTES {
        return Err(ProviderError::InvalidResponse);
    }
    let body = parse_http_response(&response)?;
    let json: Value = serde_json::from_slice(&body).map_err(|_| ProviderError::InvalidResponse)?;
    let data = json
        .get("data")
        .and_then(Value::as_array)
        .ok_or(ProviderError::InvalidResponse)?;
    if data.len() > MAX_MODELS {
        return Err(ProviderError::InvalidResponse);
    }
    let mut seen = HashSet::new();
    let mut models = Vec::with_capacity(data.len());
    for item in data {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .ok_or(ProviderError::InvalidResponse)?;
        if id.trim() != id
            || id.is_empty()
            || id.len() > 256
            || id.bytes().any(|b| b.is_ascii_control())
            || !seen.insert(id)
        {
            return Err(ProviderError::InvalidResponse);
        }
        let context_window = match item.get("context_window") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0)
                    .ok_or(ProviderError::InvalidResponse)?,
            ),
        };
        models.push(ModelInfo {
            id: id.to_owned(),
            context_window,
        });
    }
    Ok(models)
}

fn map_io(error: std::io::Error) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => ProviderError::Timeout,
        _ => ProviderError::Transport,
    }
}

fn parse_http_response(bytes: &[u8]) -> Result<Vec<u8>, ProviderError> {
    let split = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(ProviderError::InvalidResponse)?;
    if split > MAX_HTTP_HEADER_BYTES {
        return Err(ProviderError::InvalidResponse);
    }
    let headers =
        std::str::from_utf8(&bytes[..split]).map_err(|_| ProviderError::InvalidResponse)?;
    let mut lines = headers.split("\r\n");
    let mut status = lines
        .next()
        .ok_or(ProviderError::InvalidResponse)?
        .split_ascii_whitespace();
    if !matches!(status.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
        return Err(ProviderError::InvalidResponse);
    }
    let code = status.next().ok_or(ProviderError::InvalidResponse)?;
    if code != "200" {
        return Err(ProviderError::Rejected);
    }
    let mut length = None;
    let mut chunked = false;
    let mut content_type = false;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(ProviderError::InvalidResponse)?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-type") {
            let media_type = value.split(';').next().unwrap_or("").trim();
            if content_type || !media_type.eq_ignore_ascii_case("application/json") {
                return Err(ProviderError::InvalidResponse);
            }
            content_type = true;
        } else if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(ProviderError::InvalidResponse);
            }
            length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| ProviderError::InvalidResponse)?,
            );
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if chunked || !value.eq_ignore_ascii_case("chunked") {
                return Err(ProviderError::InvalidResponse);
            }
            chunked = true;
        } else if name.eq_ignore_ascii_case("content-encoding")
            && !value.eq_ignore_ascii_case("identity")
        {
            return Err(ProviderError::InvalidResponse);
        }
    }
    if chunked && length.is_some() {
        return Err(ProviderError::InvalidResponse);
    }
    let body = &bytes[split + 4..];
    if chunked {
        return decode_chunked(body);
    }
    if length.is_some_and(|expected| expected != body.len()) {
        return Err(ProviderError::InvalidResponse);
    }
    Ok(body.to_vec())
}

fn decode_chunked(bytes: &[u8]) -> Result<Vec<u8>, ProviderError> {
    let mut cursor = 0;
    let mut output = Vec::new();
    loop {
        let end = bytes[cursor..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or(ProviderError::InvalidResponse)?
            + cursor;
        if end - cursor > 16 {
            return Err(ProviderError::InvalidResponse);
        }
        let size = std::str::from_utf8(&bytes[cursor..end])
            .ok()
            .and_then(|s| usize::from_str_radix(s, 16).ok())
            .ok_or(ProviderError::InvalidResponse)?;
        cursor = end + 2;
        if size == 0 {
            if bytes.get(cursor..) != Some(b"\r\n".as_slice()) {
                return Err(ProviderError::InvalidResponse);
            }
            return Ok(output);
        }
        if size > MAX_HTTP_RESPONSE_BYTES - output.len() {
            return Err(ProviderError::InvalidResponse);
        }
        let after = cursor
            .checked_add(size)
            .ok_or(ProviderError::InvalidResponse)?;
        if bytes.get(after..after + 2) != Some(b"\r\n".as_slice()) {
            return Err(ProviderError::InvalidResponse);
        }
        output.extend_from_slice(&bytes[cursor..after]);
        cursor = after + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(response: Vec<u8>) -> (LocalEndpoint, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(TIMEOUT)).unwrap();
            let mut request = [0u8; 512];
            let len = socket.read(&mut request).unwrap();
            socket.write_all(&response).unwrap();
            String::from_utf8(request[..len].to_vec()).unwrap()
        });
        (
            LocalEndpoint::parse(&format!("http://127.0.0.1:{port}/v1")).unwrap(),
            handle,
        )
    }

    #[test]
    fn discovers_real_loopback_models_without_proxy_or_dns() {
        let json =
            r#"{"object":"list","data":[{"id":"local-1","context_window":8192},{"id":"local-2"}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{json}",
            json.len()
        );
        let (endpoint, handle) = serve_once(response.into_bytes());
        let provider = LocalOpenAiProvider::new(endpoint);
        assert_eq!(provider.locality(), Locality::Localhost);
        assert!(provider.capabilities().streaming);
        assert!(provider.capabilities().cancellable);
        let models = provider.list_models().unwrap();
        assert_eq!(models[0].id, "local-1");
        assert_eq!(models[0].context_window, Some(8192));
        assert_eq!(models[1].context_window, None);
        let request = handle.join().unwrap();
        assert!(request.starts_with("GET /v1/models HTTP/1.1\r\n"));
        assert!(request.contains("\r\nHost: 127.0.0.1:"));
    }

    #[test]
    fn rejects_redirects_and_truncated_responses() {
        for response in [
            b"HTTP/1.1 302 Found\r\nLocation: http://example.com/\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 999\r\n\r\n{}".to_vec(),
        ] {
            let (endpoint, handle) = serve_once(response);
            assert!(discover_models(&endpoint).is_err());
            handle.join().unwrap();
        }
    }

    #[test]
    fn decodes_chunked_and_rejects_conflicting_framing() {
        let json = br#"{"data":[{"id":"ollama"}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n",
            json.len()
        );
        let mut response = response.into_bytes();
        response.extend_from_slice(json);
        response.extend_from_slice(b"\r\n0\r\n\r\n");
        let (endpoint, handle) = serve_once(response);
        assert_eq!(discover_models(&endpoint).unwrap()[0].id, "ollama");
        handle.join().unwrap();
        assert_eq!(parse_http_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 1\r\n\r\n0\r\n\r\n"),
            Err(ProviderError::InvalidResponse));
    }

    #[test]
    fn https_is_explicitly_unsupported_not_downgraded() {
        let endpoint = LocalEndpoint::parse("https://127.0.0.1:443/v1").unwrap();
        assert_eq!(discover_models(&endpoint), Err(ProviderError::Unsupported));
        let provider = LocalOpenAiProvider::new(endpoint);
        assert_eq!(
            provider.capabilities(),
            ProviderCapabilities {
                discover_models: false,
                streaming: false,
                cancellable: false,
            }
        );
        assert_eq!(provider.list_models(), Err(ProviderError::Unsupported));
        assert_eq!(provider.test_connection(), Err(ProviderError::Unsupported));
    }
}
