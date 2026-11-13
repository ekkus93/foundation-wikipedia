//! AI-003 desktop API-key storage for hosted models (owner decision: keys are
//! app-owned and kept in OS secret storage; the shared Rust core never stores
//! them).
//!
//! Keys live in the macOS Keychain, Windows Credential Manager or the Linux
//! Secret Service via the `keyring` crate. A key is written, checked for
//! presence or deleted from the web UI, but never returned to it. It is read
//! back natively only to build a `HostedChatProvider` for an explicit cloud
//! request. If secure storage is unavailable (for example Linux without a
//! running secret service) saving fails; there is no plain-text fallback.
//! Error strings are fixed and never include key material or platform detail.

use wiki_ai::hosted_codec::{ApiKey, HostedProvider};
use wiki_ai::hosted_http::HostedChatProvider;
use wiki_ai::{
    stream_selected, CancellationToken, GenerateRequest, Message, MessageRole, OutboundPolicy,
    StreamEvent,
};

const SERVICE: &str = "org.foundation-wikipedia.reader";
const UNAVAILABLE: &str =
    "Secure credential storage is unavailable on this system; the key was not saved.";
const READ_FAILED: &str = "The saved key could not be read from secure storage.";

fn parse_provider(provider: &str) -> Result<HostedProvider, String> {
    match provider {
        "anthropic" => Ok(HostedProvider::Anthropic),
        "openai" => Ok(HostedProvider::OpenAi),
        _ => Err("Unknown hosted AI provider".to_owned()),
    }
}

fn account(provider: HostedProvider) -> &'static str {
    match provider {
        HostedProvider::Anthropic => "hosted-ai.anthropic",
        HostedProvider::OpenAi => "hosted-ai.openai",
    }
}

/// Minimal secret store, so tests can run without an OS keychain.
pub(crate) trait SecretStore {
    fn set(&self, account: &str, secret: &str) -> Result<(), String>;
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    /// Returns whether a secret existed.
    fn delete(&self, account: &str) -> Result<bool, String>;
}

/// The OS keychain via `keyring`'s platform backends.
pub(crate) struct OsKeychain;

fn entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, account).map_err(|_| UNAVAILABLE.to_owned())
}

impl SecretStore for OsKeychain {
    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        entry(account)?
            .set_password(secret)
            .map_err(|_| UNAVAILABLE.to_owned())
    }

    fn get(&self, account: &str) -> Result<Option<String>, String> {
        match entry(account)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(READ_FAILED.to_owned()),
        }
    }

    fn delete(&self, account: &str) -> Result<bool, String> {
        match entry(account)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(_) => Err(UNAVAILABLE.to_owned()),
        }
    }
}

pub(crate) fn save_key(store: &dyn SecretStore, provider: &str, key: String) -> Result<(), String> {
    let provider = parse_provider(provider)?;
    // Validate before storage: printable ASCII only, so no header injection.
    let key = ApiKey::new(key.trim().to_owned())
        .map_err(|_| "That does not look like a valid API key".to_owned())?;
    store.set(account(provider), key.expose_for_storage())
}

pub(crate) fn key_configured(store: &dyn SecretStore, provider: &str) -> Result<bool, String> {
    Ok(store.get(account(parse_provider(provider)?))?.is_some())
}

pub(crate) fn delete_key(store: &dyn SecretStore, provider: &str) -> Result<bool, String> {
    store.delete(account(parse_provider(provider)?))
}

/// Read a stored key natively for one hosted request. Never sent to the UI.
pub(crate) fn load_key(
    store: &dyn SecretStore,
    provider: HostedProvider,
) -> Result<ApiKey, String> {
    let secret = store
        .get(account(provider))?
        .ok_or_else(|| "No API key is saved for this provider".to_owned())?;
    ApiKey::new(secret).map_err(|_| READ_FAILED.to_owned())
}

#[tauri::command]
pub async fn save_hosted_ai_key(provider: String, key: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || save_key(&OsKeychain, &provider, key))
        .await
        .map_err(|_| "Credential worker stopped".to_owned())?
}

#[tauri::command]
pub async fn hosted_ai_key_configured(provider: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || key_configured(&OsKeychain, &provider))
        .await
        .map_err(|_| "Credential worker stopped".to_owned())?
}

#[tauri::command]
pub async fn delete_hosted_ai_key(provider: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || delete_key(&OsKeychain, &provider))
        .await
        .map_err(|_| "Credential worker stopped".to_owned())?
}

/// Result of an explicit, user-initiated hosted connection check.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedCheck {
    reply: String,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

/// Send one tiny prompt to the user's hosted model. This IS cloud traffic
/// and must only be invoked by an explicit user action that discloses it.
#[tauri::command]
pub async fn check_hosted_ai_connection(
    provider: String,
    model: String,
) -> Result<HostedCheck, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let provider = parse_provider(&provider)?;
        let key = load_key(&OsKeychain, provider)?;
        let hosted = HostedChatProvider::new(provider, key)
            .map_err(|_| "Hosted models are not supported on this platform yet".to_owned())?;
        run_check(&hosted, &model)
    })
    .await
    .map_err(|_| "Hosted AI check worker stopped".to_owned())?
}

fn run_check(provider: &dyn wiki_ai::LlmProvider, model: &str) -> Result<HostedCheck, String> {
    let request = GenerateRequest {
        model: model.to_owned(),
        messages: vec![Message {
            role: MessageRole::User,
            content: "Reply with the single word OK.".into(),
        }],
        max_output_tokens: 16,
    };
    let mut reply = String::new();
    let mut usage = (None, None);
    stream_selected(
        provider,
        &request,
        &CancellationToken::default(),
        OutboundPolicy::AllowCloud,
        &mut |event| {
            match event {
                StreamEvent::TextDelta(text) => reply.push_str(&text),
                StreamEvent::Completed {
                    input_tokens,
                    output_tokens,
                } => usage = (input_tokens, output_tokens),
            }
            Ok(())
        },
    )
    .map_err(|error| format!("Hosted model check failed: {error:?}"))?;
    Ok(HostedCheck {
        reply,
        input_tokens: usage.0,
        output_tokens: usage.1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct MemoryStore {
        secrets: RefCell<BTreeMap<String, String>>,
        unavailable: bool,
    }

    impl SecretStore for MemoryStore {
        fn set(&self, account: &str, secret: &str) -> Result<(), String> {
            if self.unavailable {
                return Err(UNAVAILABLE.to_owned());
            }
            self.secrets
                .borrow_mut()
                .insert(account.to_owned(), secret.to_owned());
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            Ok(self.secrets.borrow().get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<bool, String> {
            Ok(self.secrets.borrow_mut().remove(account).is_some())
        }
    }

    #[test]
    fn keys_are_saved_per_provider_and_never_echoed() {
        let store = MemoryStore::default();
        assert!(!key_configured(&store, "anthropic").unwrap());
        save_key(&store, "anthropic", "  sk-ant-secret-1  ".into()).unwrap();
        save_key(&store, "openai", "sk-openai-secret-2".into()).unwrap();
        assert!(key_configured(&store, "anthropic").unwrap());
        assert_eq!(
            store.secrets.borrow().get("hosted-ai.anthropic").unwrap(),
            "sk-ant-secret-1"
        );
        assert_eq!(
            store.secrets.borrow().get("hosted-ai.openai").unwrap(),
            "sk-openai-secret-2",
            "the stored OpenAI key has no Bearer prefix"
        );
        let loaded = load_key(&store, HostedProvider::OpenAi).unwrap();
        assert_eq!(
            loaded.header(HostedProvider::OpenAi).1,
            "Bearer sk-openai-secret-2"
        );
        assert!(!format!("{loaded:?}").contains("secret"));
        assert!(delete_key(&store, "anthropic").unwrap());
        assert!(!delete_key(&store, "anthropic").unwrap());
        assert!(load_key(&store, HostedProvider::Anthropic).is_err());
    }

    #[test]
    fn invalid_input_and_unavailable_storage_fail_without_leaking() {
        let store = MemoryStore::default();
        for bad in ["", "two words", "line\r\nX-Injected: 1", "é"] {
            let error = save_key(&store, "anthropic", bad.into()).unwrap_err();
            assert!(!error.contains("Injected"));
        }
        assert!(save_key(&store, "gemini", "sk-1".into()).is_err());
        assert!(store.secrets.borrow().is_empty());

        let unavailable = MemoryStore {
            unavailable: true,
            ..MemoryStore::default()
        };
        let error = save_key(&unavailable, "openai", "sk-hidden-value".into()).unwrap_err();
        assert_eq!(error, UNAVAILABLE);
        assert!(!error.contains("sk-hidden-value"));
    }

    /// keyring falls back to an in-memory mock store when no platform
    /// backend feature is enabled, which would silently lose saved keys.
    #[test]
    fn os_keychain_entries_are_not_the_mock_store() {
        let entry = keyring::Entry::new(SERVICE, "hosted-ai.feature-check").unwrap();
        assert!(entry
            .get_credential()
            .downcast_ref::<keyring::mock::MockCredential>()
            .is_none());
    }
}
