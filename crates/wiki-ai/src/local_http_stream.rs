//! Bounded, cancellation-aware streaming chat over explicit local HTTP.
//!
//! HTTPS is intentionally unsupported until TLS identity/pinning is implemented.
//! No DNS, proxies, redirects, authentication headers or cloud fallback.

use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::local_endpoint::LocalEndpoint;
use crate::openai_stream::OpenAiStreamDecoder;
use crate::{CancellationToken, GenerateRequest, MessageRole, ProviderError, StreamEvent};

const MAX_REQUEST_BYTES: usize = 256 * 1024;
const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 16 * 1024 * 1024;
const MAX_LINE: usize = 4096;
const READ_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_DURATION: Duration = Duration::from_secs(120);

pub fn stream_chat(
    endpoint: &LocalEndpoint,
    request: &GenerateRequest,
    cancellation: &CancellationToken,
    on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
) -> Result<(), ProviderError> {
    request.validate()?;
    if cancellation.is_cancelled() {
        return Err(ProviderError::Cancelled);
    }
    let authority = endpoint
        .base_url()
        .strip_prefix("http://")
        .ok_or(ProviderError::Unsupported)?
        .split('/')
        .next()
        .ok_or(ProviderError::InvalidRequest)?;
    let messages: Vec<_> = request
        .messages
        .iter()
        .map(|m| {
            json!({
                "role": match m.role {
                    MessageRole::System => "system",
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                },
                "content": m.content,
            })
        })
        .collect();
    let body = json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": request.max_output_tokens,
        "stream": true,
        "stream_options": { "include_usage": true },
    })
    .to_string();
    if body.len() > MAX_REQUEST_BYTES {
        return Err(ProviderError::InvalidRequest);
    }
    let deadline = Instant::now() + MAX_DURATION;
    let mut socket = TcpStream::connect_timeout(&endpoint.connect_addr(), Duration::from_secs(3))
        .map_err(map_io)?;
    socket
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(map_io)?;
    socket
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(map_io)?;
    let header = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).map_err(map_io)?;
    socket.write_all(body.as_bytes()).map_err(map_io)?;
    let mut reader = BufReader::new(socket);
    let mode = read_headers(&mut reader, cancellation, deadline)?;
    let mut decoder = OpenAiStreamDecoder::new();
    let mut total = 0usize;
    match mode {
        BodyMode::Chunked => loop {
            let size_line = read_line(&mut reader, 64, cancellation, deadline)?;
            let size = std::str::from_utf8(&size_line)
                .ok()
                .and_then(|s| usize::from_str_radix(s, 16).ok())
                .ok_or(ProviderError::InvalidResponse)?;
            if size == 0 {
                if !read_line(&mut reader, MAX_LINE, cancellation, deadline)?.is_empty() {
                    return Err(ProviderError::InvalidResponse);
                }
                break;
            }
            if size > MAX_BODY - total {
                return Err(ProviderError::InvalidResponse);
            }
            read_body(
                &mut reader,
                size,
                &mut decoder,
                cancellation,
                deadline,
                on_event,
            )?;
            total += size;
            if !read_line(&mut reader, 2, cancellation, deadline)?.is_empty() {
                return Err(ProviderError::InvalidResponse);
            }
        },
        BodyMode::Length(size) => {
            read_body(
                &mut reader,
                size,
                &mut decoder,
                cancellation,
                deadline,
                on_event,
            )?;
        }
        BodyMode::UntilClose => {
            let mut buffer = [0u8; 8192];
            loop {
                let n = checked_read(&mut reader, &mut buffer, cancellation, deadline)?;
                if n == 0 {
                    break;
                }
                total = total
                    .checked_add(n)
                    .filter(|n| *n <= MAX_BODY)
                    .ok_or(ProviderError::InvalidResponse)?;
                feed(&mut decoder, &buffer[..n], cancellation, on_event)?;
            }
        }
    }
    if cancellation.is_cancelled() {
        return Err(ProviderError::Cancelled);
    }
    decoder.finish()
}

fn read_body<R: Read>(
    reader: &mut R,
    size: usize,
    decoder: &mut OpenAiStreamDecoder,
    cancellation: &CancellationToken,
    deadline: Instant,
    on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
) -> Result<(), ProviderError> {
    let mut remaining = size;
    let mut buffer = [0u8; 8192];
    while remaining > 0 {
        let capacity = remaining.min(buffer.len());
        let n = checked_read(
            reader,
            &mut buffer[..capacity],
            cancellation,
            deadline,
        )?;
        if n == 0 {
            return Err(ProviderError::InvalidResponse);
        }
        feed(decoder, &buffer[..n], cancellation, on_event)?;
        remaining -= n;
    }
    Ok(())
}

fn feed(
    decoder: &mut OpenAiStreamDecoder,
    bytes: &[u8],
    cancellation: &CancellationToken,
    on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
) -> Result<(), ProviderError> {
    for event in decoder.push(bytes)? {
        if cancellation.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        on_event(event)?;
    }
    Ok(())
}

fn checked_read<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Result<usize, ProviderError> {
    loop {
        if cancellation.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(ProviderError::Timeout);
        }
        match reader.read(buffer) {
            Ok(n) => return Ok(n),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(e) => return Err(map_io(e)),
        }
    }
}

fn read_line<R: Read>(
    reader: &mut R,
    limit: usize,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Result<Vec<u8>, ProviderError> {
    let mut line = Vec::new();
    loop {
        let mut byte = [0u8];
        if checked_read(reader, &mut byte, cancellation, deadline)? == 0 {
            return Err(ProviderError::InvalidResponse);
        }
        line.push(byte[0]);
        if line.len() > limit {
            return Err(ProviderError::InvalidResponse);
        }
        if line.ends_with(b"\r\n") {
            line.truncate(line.len() - 2);
            return Ok(line);
        }
    }
}

enum BodyMode {
    Chunked,
    Length(usize),
    UntilClose,
}

fn read_headers<R: Read>(
    reader: &mut R,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Result<BodyMode, ProviderError> {
    let status = read_line(reader, MAX_LINE, cancellation, deadline)?;
    let status = std::str::from_utf8(&status).map_err(|_| ProviderError::InvalidResponse)?;
    let mut words = status.split_ascii_whitespace();
    if !matches!(words.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
        return Err(ProviderError::InvalidResponse);
    }
    if words.next() != Some("200") {
        return Err(ProviderError::Rejected);
    }
    let mut total = status.len() + 2;
    let mut length = None;
    let mut chunked = false;
    let mut event_stream = false;
    loop {
        let line = read_line(reader, MAX_LINE, cancellation, deadline)?;
        total += line.len() + 2;
        if total > MAX_HEADERS {
            return Err(ProviderError::InvalidResponse);
        }
        if line.is_empty() {
            break;
        }
        let line = std::str::from_utf8(&line).map_err(|_| ProviderError::InvalidResponse)?;
        let (name, value) = line.split_once(':').ok_or(ProviderError::InvalidResponse)?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-type") {
            if event_stream || !value.to_ascii_lowercase().starts_with("text/event-stream") {
                return Err(ProviderError::InvalidResponse);
            }
            event_stream = true;
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
    if !event_stream || (chunked && length.is_some()) {
        return Err(ProviderError::InvalidResponse);
    }
    if chunked {
        Ok(BodyMode::Chunked)
    } else if let Some(n) = length {
        if n > MAX_BODY {
            Err(ProviderError::InvalidResponse)
        } else {
            Ok(BodyMode::Length(n))
        }
    } else {
        Ok(BodyMode::UntilClose)
    }
}

fn map_io(error: std::io::Error) -> ProviderError {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => ProviderError::Timeout,
        _ => ProviderError::Transport,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn fixture_server(body: Vec<u8>) -> (LocalEndpoint, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = vec![0u8; 8192];
            let n = socket.read(&mut request).unwrap();
            socket.write_all(&body).unwrap();
            String::from_utf8_lossy(&request[..n]).into_owned()
        });
        (
            LocalEndpoint::parse(&format!("http://127.0.0.1:{port}/v1")).unwrap(),
            handle,
        )
    }

    fn request() -> GenerateRequest {
        GenerateRequest {
            model: "fixture".into(),
            messages: vec![crate::Message {
                role: MessageRole::User,
                content: "Why is the sky blue?".into(),
            }],
            max_output_tokens: 100,
        }
    }

    #[test]
    fn streams_chunked_local_answer_with_explicit_completion() {
        let sse = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Blue\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut response = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        for piece in sse.as_bytes().chunks(19) {
            response.extend_from_slice(format!("{:X}\r\n", piece.len()).as_bytes());
            response.extend_from_slice(piece);
            response.extend_from_slice(b"\r\n");
        }
        response.extend_from_slice(b"0\r\n\r\n");
        let (endpoint, handle) = fixture_server(response);
        let mut events = Vec::new();
        let result = stream_chat(
            &endpoint,
            &request(),
            &CancellationToken::default(),
            &mut |event| {
                events.push(event);
                Ok(())
            },
        );
        assert_eq!(result, Ok(()));
        assert_eq!(
            events,
            vec![
                StreamEvent::TextDelta("Blue".into()),
                StreamEvent::Completed {
                    input_tokens: None,
                    output_tokens: None
                },
            ]
        );
        let request = handle.join().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        assert!(request.contains("\r\nHost: 127.0.0.1:"));
    }

    #[test]
    fn cancellation_prevents_further_delivery() {
        let sse = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"one\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"two\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{sse}",
            sse.len()
        );
        let (endpoint, handle) = fixture_server(response.into_bytes());
        let token = CancellationToken::default();
        let cancel = token.clone();
        let mut seen = Vec::new();
        let result = stream_chat(&endpoint, &request(), &token, &mut |event| {
            seen.push(event);
            cancel.cancel();
            Ok(())
        });
        assert_eq!(result, Err(ProviderError::Cancelled));
        assert_eq!(seen, vec![StreamEvent::TextDelta("one".into())]);
        handle.join().unwrap();
    }

    #[test]
    fn rejects_redirects_without_following_them() {
        let (endpoint, handle) =
            fixture_server(b"HTTP/1.1 302 Found\r\nLocation: http://example.com/\r\n\r\n".to_vec());
        assert_eq!(
            stream_chat(
                &endpoint,
                &request(),
                &CancellationToken::default(),
                &mut |_| Ok(())
            ),
            Err(ProviderError::Rejected)
        );
        handle.join().unwrap();
    }
}
