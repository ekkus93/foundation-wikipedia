//! Thin Tauri adapter for explicit, local-only model discovery (AI-002).
//! Source of authority is wiki-ai's pinned-address transport, not web fetch.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use wiki_ai::local_endpoint::LocalEndpoint;
use wiki_ai::{CancellationToken, GenerateRequest, Message, MessageRole, OutboundPolicy, StreamEvent};
use wiki_store::user_state::UserState;
use crate::open_private_bookmark_store;
use wiki_ai::local_selection::{discover_local_models, discover_local_models_cancellable, LocalProtocol, SelectionError};
use wiki_ai::{Locality, ProviderError};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopAiModel {
    id: String,
    context_window: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAiInventory {
    protocol: String,
    endpoint: String,
    locality: &'static str,
    models: Vec<DesktopAiModel>,
}

#[tauri::command]
pub async fn discover_local_ai_models(
    protocol: String,
    endpoint: String,
) -> Result<DesktopAiInventory, String> {
    let selected = parse_local_protocol(&protocol)?;
    if endpoint.len() > 512 {
        return Err("Invalid local endpoint".to_owned());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let inventory = discover_local_models(selected, &endpoint).map_err(|error| match error {
            SelectionError::InvalidEndpoint(_) => {
                "Only explicit localhost or private-IP endpoints are supported".to_owned()
            }
            SelectionError::Provider(ProviderError::Unsupported) => {
                "Authenticated HTTPS is not supported yet".to_owned()
            }
            SelectionError::Provider(ProviderError::Timeout) => {
                "Local model discovery timed out".to_owned()
            }
            _ => "Local model discovery failed".to_owned(),
        })?;
        let locality = match inventory.locality() {
            Locality::Localhost => "localhost",
            Locality::LocalNetwork => "local-network",
            _ => return Err("The selected AI destination is not local".to_owned()),
        };
        Ok(DesktopAiInventory {
            protocol,
            endpoint: inventory.endpoint().to_owned(),
            locality,
            models: inventory.models().iter().map(|model| DesktopAiModel {
                id: model.id.clone(),
                context_window: model.context_window,
            }).collect(),
        })
    }).await.map_err(|_| "Local model discovery worker stopped".to_owned())?
}

fn parse_local_protocol(protocol: &str) -> Result<LocalProtocol, String> {
    match protocol {
        "ollama" => Ok(LocalProtocol::Ollama),
        "openai-compatible" => Ok(LocalProtocol::OpenAiCompatible),
        _ => Err("Unsupported local AI protocol".to_owned()),
    }
}


struct ProbeEntry {
    cancellation: CancellationToken,
    running: bool,
    registered_at: Instant,
}

/// One explicit user-started diagnostic lease. Preparing a lease does not
/// contact a provider. Cancellation removes the lease and signals any worker
/// holding a cloned token; a late run cannot resurrect a cancelled lease.
#[derive(Default)]
pub struct LocalAiProbeRegistry {
    next_id: AtomicU64,
    entries: Mutex<HashMap<u64, ProbeEntry>>,
}

impl LocalAiProbeRegistry {
    fn begin(&self) -> Result<u64, String> {
        let mut entries = self.entries.lock().map_err(|_| "Local test registry unavailable")?;
        // A window that crashes between prepare and run cannot exhaust the
        // registry forever. Running requests are removed on worker completion.
        entries.retain(|_, entry| entry.running || entry.registered_at.elapsed() < Duration::from_secs(300));
        if entries.len() >= 32 {
            return Err("Too many pending local model tests".to_owned());
        }
        // IDs cross JavaScript IPC, so never exceed Number.MAX_SAFE_INTEGER.
        let id = self.next_id.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            (value < 9_007_199_254_740_991).then_some(value + 1)
        }).map_err(|_| "Local test ID space exhausted")? + 1;
        entries.insert(id, ProbeEntry {
            cancellation: CancellationToken::default(),
            running: false,
            registered_at: Instant::now(),
        });
        Ok(id)
    }

    fn claim(&self, id: u64) -> Result<CancellationToken, String> {
        let mut entries = self.entries.lock().map_err(|_| "Local test registry unavailable")?;
        let entry = entries.get_mut(&id).ok_or("Local model test was cancelled or expired")?;
        if entry.running {
            return Err("Local model test already started".to_owned());
        }
        entry.running = true;
        Ok(entry.cancellation.clone())
    }

    fn cancel(&self, id: u64) -> Result<(), String> {
        let entry = self.entries.lock().map_err(|_| "Local test registry unavailable")?.remove(&id);
        if let Some(entry) = entry {
            entry.cancellation.cancel();
        }
        Ok(())
    }

    fn finish(&self, id: u64) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(&id);
        }
    }
}

#[tauri::command]
pub fn begin_local_ai_model_test(
    registry: tauri::State<'_, LocalAiProbeRegistry>,
) -> Result<u64, String> {
    registry.begin()
}

#[tauri::command]
pub fn cancel_local_ai_model_test(
    registry: tauri::State<'_, LocalAiProbeRegistry>,
    request_id: u64,
) -> Result<(), String> {
    registry.cancel(request_id)
}

// This explicit diagnostic never receives article text, user prompts,
// bookmarks, history, or credentials. No generated text leaves native Rust.
const LOCAL_TEST_PROMPT: &str = "Reply OK.";

fn probe_local_model(
    protocol: LocalProtocol,
    endpoint: &str,
    model: &str,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    if cancellation.is_cancelled() {
        return Err("Local model test cancelled".to_owned());
    }
    if endpoint.len() > 512 || model.len() > 256 {
        return Err("Invalid local model test selection".to_owned());
    }
    // Re-discover on the native side: a stale or forged UI inventory cannot
    // authorize inference using a model the server no longer advertises.
    let inventory = discover_local_models_cancellable(protocol, endpoint, cancellation)
        .map_err(|error| match error {
            SelectionError::Provider(ProviderError::Cancelled) => "Local model test cancelled".to_owned(),
            _ => "Could not verify the selected local model".to_owned(),
        })?;
    if cancellation.is_cancelled() {
        return Err("Local model test cancelled".to_owned());
    }
    let selected = inventory.select(model)
        .map_err(|_| "Selected model is no longer advertised by this server".to_owned())?;
    let mut saw_text = false;
    selected
        .stream(
            &[Message {
                role: MessageRole::User,
                content: LOCAL_TEST_PROMPT.to_owned(),
            }],
            8,
            cancellation,
            OutboundPolicy::AllowLocalEndpoints,
            &mut |event| {
                if let StreamEvent::TextDelta(text) = event {
                    saw_text |= !text.trim().is_empty();
                }
                Ok(())
            },
        )
        .map_err(|_| "Local model inference test failed".to_owned())?;
    if !saw_text {
        return Err("Local model returned no assistant text".to_owned());
    }
    Ok(())
}

#[tauri::command]
pub async fn test_local_ai_model(
    app: tauri::AppHandle,
    request_id: u64,
    protocol: String,
    endpoint: String,
    model: String,
) -> Result<(), String> {
    let cancellation = app.state::<LocalAiProbeRegistry>().claim(request_id)?;
    let result = match parse_local_protocol(&protocol) {
        Ok(selected) if endpoint.len() <= 512 && model.len() <= 256 => {
            tauri::async_runtime::spawn_blocking(move || {
                probe_local_model(selected, &endpoint, &model, &cancellation)
            })
            .await
            .map_err(|_| "Local model test worker stopped".to_owned())
            .and_then(|outcome| outcome)
        }
        _ => Err("Invalid local model test selection".to_owned()),
    };
    app.state::<LocalAiProbeRegistry>().finish(request_id);
    result
}

/// One non-secret local profile in the private SQLite preferences. A restored
/// model ID is untrusted until explicitly rediscovered from its server.
const LOCAL_PROFILE_KEY: &str = "ai.local_profile.v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopLocalAiProfile {
    protocol: String,
    endpoint: String,
    model: String,
}

fn validate_local_profile(profile: &DesktopLocalAiProfile) -> Result<(), String> {
    if !matches!(profile.protocol.as_str(), "ollama" | "openai-compatible")
        || profile.endpoint.len() > 512
        || !profile.endpoint.starts_with("http://")
        || LocalEndpoint::parse(&profile.endpoint).is_err()
    {
        return Err("Invalid local provider or endpoint".to_owned());
    }
    let request = GenerateRequest {
        model: profile.model.clone(),
        messages: vec![Message {
            role: MessageRole::User,
            content: "validation only".to_owned(),
        }],
        max_output_tokens: 1,
    };
    request
        .validate()
        .map_err(|_| "Invalid local model identifier".to_owned())
}

fn load_local_profile(state: &UserState) -> Result<Option<DesktopLocalAiProfile>, String> {
    let saved = state
        .setting(LOCAL_PROFILE_KEY)
        .map_err(|_| "Cannot read local AI preference".to_owned())?;
    saved
        .map(|json| {
            let profile: DesktopLocalAiProfile = serde_json::from_str(&json)
                .map_err(|_| "Invalid saved local AI preference".to_owned())?;
            validate_local_profile(&profile)?;
            Ok(profile)
        })
        .transpose()
}

#[tauri::command]
pub async fn read_local_ai_profile(
    app: tauri::AppHandle,
) -> Result<Option<DesktopLocalAiProfile>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = open_private_bookmark_store(&app)?;
        load_local_profile(&state)
    })
    .await
    .map_err(|_| "Local AI preference worker stopped".to_owned())?
}

#[tauri::command]
pub async fn save_local_ai_profile(
    app: tauri::AppHandle,
    profile: DesktopLocalAiProfile,
) -> Result<(), String> {
    validate_local_profile(&profile)?;
    tauri::async_runtime::spawn_blocking(move || {
        let serialized = serde_json::to_string(&profile)
            .map_err(|_| "Cannot serialize local AI preference".to_owned())?;
        open_private_bookmark_store(&app)?
            .set_setting(LOCAL_PROFILE_KEY, &serialized)
            .map_err(|_| "Cannot save local AI preference".to_owned())
    })
    .await
    .map_err(|_| "Local AI preference worker stopped".to_owned())?
}

#[cfg(test)]
mod local_profile_tests {
    use super::*;

    fn profile(protocol: &str, endpoint: &str, model: &str) -> DesktopLocalAiProfile {
        DesktopLocalAiProfile {
            protocol: protocol.to_owned(),
            endpoint: endpoint.to_owned(),
            model: model.to_owned(),
        }
    }


    fn serve_probe_fixture(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::time::Duration;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (body, media_type) in responses {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let header = String::from_utf8_lossy(&bytes[..split]);
                    let length = header.lines()
                        .find_map(|line| line.strip_prefix("Content-Length: "))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if bytes.len() >= split + 4 + length {
                        break;
                    }
                }
                requests.push(String::from_utf8(bytes).unwrap());
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {media_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(reply.as_bytes()).unwrap();
            }
            requests
        });
        (endpoint, worker)
    }

    #[test]
    fn explicit_local_inference_probe_sends_only_fixed_prompt_and_discards_output() {
        let sse = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"OK\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let (endpoint, worker) = serve_probe_fixture(vec![
            (r#"{"models":[{"name":"qwen3:8b"}]}"#, "application/json"),
            (sse, "text/event-stream"),
        ]);
        probe_local_model(LocalProtocol::Ollama, &endpoint, "qwen3:8b", &CancellationToken::default()).unwrap();
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /api/tags HTTP/1.1"));
        assert!(requests[1].starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(requests[1].contains(r#""content":"Reply OK.""#));
        assert!(requests[1].contains(r#""model":"qwen3:8b""#));
        assert!(!requests[1].contains("article"));
    }

    #[test]
    fn local_inference_probe_rejects_unknown_model_before_post_and_public_hosts() {
        let (endpoint, worker) = serve_probe_fixture(vec![
            (r#"{"models":[{"name":"qwen3:8b"}]}"#, "application/json"),
        ]);
        assert!(probe_local_model(LocalProtocol::Ollama, &endpoint, "unadvertised", &CancellationToken::default()).is_err());
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET /api/tags HTTP/1.1"));
        assert!(probe_local_model(LocalProtocol::Ollama, "http://8.8.8.8", "qwen3:8b", &CancellationToken::default()).is_err());
        assert!(probe_local_model(LocalProtocol::Ollama, "http://user:secret@127.0.0.1", "qwen3:8b", &CancellationToken::default()).is_err());
        assert!(probe_local_model(LocalProtocol::Ollama, &endpoint, &"x".repeat(257), &CancellationToken::default()).is_err());
    }


    #[test]
    fn diagnostic_leases_cancel_before_run_and_during_stream() {
        let registry = LocalAiProbeRegistry::default();
        let first = registry.begin().unwrap();
        let running = registry.claim(first).unwrap();
        assert!(!running.is_cancelled());
        assert!(registry.claim(first).is_err(), "one lease cannot run twice");
        registry.cancel(first).unwrap();
        assert!(running.is_cancelled(), "an in-flight worker observes cancellation");
        assert!(registry.claim(first).is_err(), "cancelled leases cannot restart");
        registry.cancel(first).unwrap(); // idempotent

        let second = registry.begin().unwrap();
        assert_ne!(first, second);
        registry.cancel(second).unwrap();
        let already_cancelled = CancellationToken::default();
        already_cancelled.cancel();
        assert!(probe_local_model(
            LocalProtocol::Ollama,
            "http://127.0.0.1:1",
            "qwen3:8b",
            &already_cancelled
        ).is_err(), "pre-cancelled probe must not open a socket");
        registry.finish(first);
    }

    #[test]
    fn diagnostic_lease_capacity_is_bounded_and_reusable() {
        let registry = LocalAiProbeRegistry::default();
        let mut ids = Vec::new();
        for _ in 0..32 {
            ids.push(registry.begin().unwrap());
        }
        assert!(registry.begin().is_err());
        registry.cancel(ids[0]).unwrap();
        assert!(registry.begin().is_ok());
        registry.cancel(ids[1]).unwrap();
        // Abandoned prepared leases are reclaimed without canceling running
        // workers, so a crashed window cannot permanently consume capacity.
        let stale = registry.begin().unwrap();
        registry.entries.lock().unwrap().get_mut(&stale).unwrap().registered_at =
            Instant::now() - Duration::from_secs(301);
        let _fresh = registry.begin().unwrap();
        assert!(registry.claim(stale).is_err());
    }

    #[test]
    fn non_secret_profile_roundtrip_requires_explicit_inventory_revalidation() {
        let state = UserState::in_memory().unwrap();
        assert_eq!(load_local_profile(&state).unwrap(), None);
        let valid = profile("ollama", "http://127.0.0.1:11434", "qwen3:8b");
        validate_local_profile(&valid).unwrap();
        let raw = serde_json::to_string(&valid).unwrap();
        state.set_setting(LOCAL_PROFILE_KEY, &raw).unwrap();
        assert_eq!(load_local_profile(&state).unwrap(), Some(valid));
    }

    #[test]
    fn refuses_secret_url_public_network_invalid_id_and_corrupt_saved_data() {
        for bad in [
            profile("cloud", "http://127.0.0.1:11434", "safe"),
            profile("ollama", "https://127.0.0.1:11434", "safe"),
            profile("ollama", "http://example.com", "safe"),
            profile("ollama", "http://user:password@127.0.0.1", "safe"),
            profile("ollama", "http://127.0.0.1:11434", "bad\u{202e}model"),
            profile("ollama", "http://127.0.0.1:11434", " "),
        ] {
            assert!(validate_local_profile(&bad).is_err(), "{bad:?}");
        }
        let state = UserState::in_memory().unwrap();
        for json in [
            r#"{"protocol":"ollama","endpoint":"http://127.0.0.1","model":"qwen","key":"secret"}"#,
            r#"{"protocol":"cloud","endpoint":"http://127.0.0.1","model":"qwen"}"#,
            "not-json",
        ] {
            state.set_setting(LOCAL_PROFILE_KEY, json).unwrap();
            assert!(load_local_profile(&state).is_err());
        }
    }
}
