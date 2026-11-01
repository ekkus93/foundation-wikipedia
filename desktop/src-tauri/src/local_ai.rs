//! Thin Tauri adapter for explicit, local-only model discovery (AI-002).
//! Source of authority is wiki-ai's pinned-address transport, not web fetch.
use serde::Serialize;
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
