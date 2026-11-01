//! Thin Tauri adapter for explicit, local-only model discovery (AI-002).
//! Source of authority is wiki-ai's pinned-address transport, not web fetch.
use serde::{Deserialize, Serialize};
use wiki_ai::local_endpoint::LocalEndpoint;
use wiki_ai::{GenerateRequest, Message, MessageRole};
use wiki_store::user_state::UserState;
use crate::open_private_bookmark_store;
use wiki_ai::local_selection::{discover_local_models, LocalProtocol, SelectionError};
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
    let selected = match protocol.as_str() {
        "ollama" => LocalProtocol::Ollama,
        "openai-compatible" => LocalProtocol::OpenAiCompatible,
        _ => return Err("Unsupported local AI protocol".to_owned()),
    };
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
