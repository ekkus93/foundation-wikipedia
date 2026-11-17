//! SEC-003: with a local-only model selected, the app makes no hosted-AI
//! connection, whether the local answer succeeds, finds no evidence, or the
//! local server is gone (no fallback). A separate test binary, so the
//! process-wide hosted connection counter is not shared with hosted tests.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use wiki_ai::hosted_http::hosted_connection_attempts;
use wiki_ffi::android::{
    discover_local_ai, ChatAnswer, ChatError, LocalAiProtocol, LocalChatRequest, OfflineLibrary,
};

mod support;

fn read_request(socket: &mut TcpStream) -> String {
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = socket.read(&mut buf).unwrap();
        data.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&data).into_owned();
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .map_or(0, |value| value.trim().parse::<usize>().unwrap());
            if data.len() >= end + 4 + length || n == 0 {
                return text;
            }
        }
        if n == 0 {
            return text;
        }
    }
}

/// A fake Ollama on loopback: the model list, then `replies` chat answers.
fn fake_ollama(replies: Vec<String>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let handle = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        assert!(read_request(&mut socket).starts_with("GET /api/tags "));
        let body = r#"{"models":[{"name":"qwen2.5:3b"}]}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        drop(socket);
        for reply in replies {
            let (mut socket, _) = listener.accept().unwrap();
            assert!(read_request(&mut socket).starts_with("POST /v1/chat/completions "));
            // Test replies contain no quotes or backslashes.
            let content = format!("\"{reply}\"");
            let sse = format!(
                "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{content}}},\"finish_reason\":null}}]}}\n\n\
                 data: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n"
            );
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{sse}",
                sse.len()
            )
            .unwrap();
        }
        // Dropping the listener: the server is gone for later requests.
    });
    (endpoint, handle)
}

fn ask(
    library: &OfflineLibrary,
    server: &Arc<wiki_ffi::android::LocalAiServer>,
    question: &str,
) -> Result<ChatAnswer, ChatError> {
    let chat = library.prepare_local_chat(
        LocalChatRequest {
            project: "enwiki".into(),
            page_id: 2,
            question: question.into(),
            server: server.clone(),
            model: "qwen2.5:3b".into(),
            thread_id: None,
            selection: None,
        },
        None,
    )?;
    chat.send(true, None).map(|reply| reply.answer)
}

#[test]
fn a_local_only_provider_never_contacts_a_hosted_provider() {
    let dir = support::test_dir("local-only-traffic");
    let library = support::library(&dir.0);
    let pack = support::build_pack(&dir.0, "moon-pack");
    library
        .install_pack(pack.to_string_lossy().into_owned(), true)
        .unwrap();
    library.activate("moon-pack".into()).unwrap();
    let lead = wiki_model::ArticleKey {
        project: "enwiki".into(),
        page_id: 2,
    }
    .block_id(7002, &[], 0);
    let (endpoint, server) = fake_ollama(vec![
        format!("Tides follow the Moon's pull. [[cite:{lead}]]"),
        "[NO_EVIDENCE]".to_owned(),
    ]);
    let local = discover_local_ai(LocalAiProtocol::Ollama, endpoint).unwrap();

    let supported = ask(&library, &local, "What do tides follow?").unwrap();
    assert!(matches!(supported, ChatAnswer::Supported { .. }));
    let unsupported = ask(&library, &local, "Who was the first emperor of Rome?").unwrap();
    assert!(matches!(unsupported, ChatAnswer::InsufficientEvidence));
    server.join().unwrap();

    // The local server is gone: the request fails; it is not sent elsewhere.
    assert!(ask(&library, &local, "What do tides follow?").is_err());

    assert_eq!(hosted_connection_attempts(), 0, "no hosted-AI connection");
}
