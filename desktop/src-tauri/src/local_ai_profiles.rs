//! Non-secret bounded local-model profile management (AI-004, partial).
//! Only the explicitly selected localhost/private-LAN destination is saved.
//! Restoring a profile never performs discovery or inference.

use crate::open_private_bookmark_store;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tauri::Emitter;
use wiki_ai::local_endpoint::LocalEndpoint;
use wiki_ai::{GenerateRequest, Message, MessageRole};
use wiki_store::user_state::UserState;

const PROFILE_KEY: &str = "ai.local_profiles.v2";
const LEGACY_KEY: &str = "ai.local_profile.v1";
const MAX_PROFILES: usize = 8;
const MAX_SAVED_BYTES: usize = 4096;
// Best-effort in-process notification; focus reconciliation covers separate processes.
const PROFILES_CHANGED_EVENT: &str = "ai-local-profiles-changed";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalProfile {
    name: String,
    protocol: String,
    endpoint: String,
    model: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalProfiles {
    active: Option<String>,
    profiles: Vec<LocalProfile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyProfile {
    protocol: String,
    endpoint: String,
    model: String,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name.trim() == name
        && name.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && name.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b' ' | b'.' | b'-' | b'_')
        })
}

fn validate_profile(profile: &LocalProfile) -> Result<(), String> {
    if !valid_name(&profile.name)
        || !matches!(profile.protocol.as_str(), "ollama" | "openai-compatible")
        || profile.endpoint.len() > 512
        || !profile.endpoint.starts_with("http://")
        || LocalEndpoint::parse(&profile.endpoint).is_err()
    {
        return Err("Invalid local AI profile".to_owned());
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
        .map_err(|_| "Invalid local AI model".to_owned())
}

fn validate_profiles(value: &LocalProfiles) -> Result<(), String> {
    if value.profiles.len() > MAX_PROFILES {
        return Err("Too many local AI profiles".to_owned());
    }
    let mut seen = HashSet::new();
    for profile in &value.profiles {
        validate_profile(profile)?;
        if !seen.insert(profile.name.to_ascii_lowercase()) {
            return Err("Duplicate local AI profile name".to_owned());
        }
    }
    if value.active.as_ref().is_some_and(|name| {
        !value.profiles.iter().any(|profile| &profile.name == name)
    }) || (value.active.is_none() && !value.profiles.is_empty())
    {
        return Err("Invalid active local AI profile".to_owned());
    }
    let bytes = serde_json::to_vec(value).map_err(|_| "Cannot encode local AI profiles")?;
    if bytes.len() > MAX_SAVED_BYTES {
        return Err("Local AI profile storage limit exceeded".to_owned());
    }
    Ok(())
}

fn read_profiles(state: &UserState) -> Result<LocalProfiles, String> {
    let saved = state
        .setting(PROFILE_KEY)
        .map_err(|_| "Cannot read local AI profiles")?;
    if let Some(json) = saved {
        let value: LocalProfiles = serde_json::from_str(&json)
            .map_err(|_| "Invalid saved local AI profiles")?;
        validate_profiles(&value)?;
        return Ok(value);
    }
    // No eager migration: a legacy profile remains readable without writing
    // on Settings open. The first explicit mutation creates the v2 record.
    let legacy = state
        .setting(LEGACY_KEY)
        .map_err(|_| "Cannot read legacy local AI profile")?;
    let Some(json) = legacy else {
        return Ok(LocalProfiles::default());
    };
    let old: LegacyProfile =
        serde_json::from_str(&json).map_err(|_| "Invalid legacy local AI profile")?;
    let value = LocalProfiles {
        active: Some("Default".to_owned()),
        profiles: vec![LocalProfile {
            name: "Default".to_owned(),
            protocol: old.protocol,
            endpoint: old.endpoint,
            model: old.model,
        }],
    };
    validate_profiles(&value)?;
    Ok(value)
}

fn write_profiles(
    state: &UserState,
    previous: Option<&str>,
    profiles: &LocalProfiles,
) -> Result<(), String> {
    validate_profiles(profiles)?;
    let json =
        serde_json::to_string(profiles).map_err(|_| "Cannot encode local AI profiles")?;
    let written = state
        .compare_and_swap_setting(PROFILE_KEY, previous, &json)
        .map_err(|_| "Cannot save local AI profiles")?;
    if !written {
        return Err("Local AI profiles changed in another window; reload and retry".to_owned());
    }
    // Once the v2 record is durable, the legacy row is no longer authoritative.
    // v2 stays present even if empty, to prevent resurrecting old selections.
    state
        .delete_setting(LEGACY_KEY)
        .map_err(|_| "Cannot retire legacy AI profile")?;
    Ok(())
}

// A desktop editor must not apply an old form to a newer collection from
// another window. The subsequent SQLite CAS also protects the read/write gap.
fn require_current_profiles(state: &UserState, expected: &LocalProfiles) -> Result<(), String> {
    validate_profiles(expected)?;
    if read_profiles(state)? != *expected {
        return Err("Local AI profiles changed in another window; reload and retry".to_owned());
    }
    Ok(())
}

fn upsert_if_current(
    state: &UserState,
    incoming: LocalProfile,
    expected: &LocalProfiles,
) -> Result<LocalProfiles, String> {
    require_current_profiles(state, expected)?;
    upsert(state, incoming)
}

fn activate_if_current(
    state: &UserState,
    name: &str,
    expected: &LocalProfiles,
) -> Result<LocalProfiles, String> {
    require_current_profiles(state, expected)?;
    activate(state, name)
}

fn remove_if_current(
    state: &UserState,
    name: &str,
    expected: &LocalProfiles,
) -> Result<LocalProfiles, String> {
    require_current_profiles(state, expected)?;
    remove(state, name)
}

fn upsert(state: &UserState, incoming: LocalProfile) -> Result<LocalProfiles, String> {
    validate_profile(&incoming)?;
    let previous = state.setting(PROFILE_KEY).map_err(|_| "Cannot read local AI profiles")?;
    let mut current = read_profiles(state)?;
    if let Some(existing) = current.profiles.iter_mut().find(|p| {
        p.name.eq_ignore_ascii_case(&incoming.name)
    }) {
        if existing.name != incoming.name {
            return Err("Local AI profile names are case-insensitive".to_owned());
        }
        *existing = incoming.clone();
    } else {
        if current.profiles.len() >= MAX_PROFILES {
            return Err("Too many local AI profiles".to_owned());
        }
        current.profiles.push(incoming.clone());
    }
    current.active = Some(incoming.name);
    write_profiles(state, previous.as_deref(), &current)?;
    Ok(current)
}

fn activate(state: &UserState, name: &str) -> Result<LocalProfiles, String> {
    let previous = state.setting(PROFILE_KEY).map_err(|_| "Cannot read local AI profiles")?;
    let mut current = read_profiles(state)?;
    if !current.profiles.iter().any(|p| p.name == name) {
        return Err("Unknown local AI profile".to_owned());
    }
    current.active = Some(name.to_owned());
    write_profiles(state, previous.as_deref(), &current)?;
    Ok(current)
}

fn remove(state: &UserState, name: &str) -> Result<LocalProfiles, String> {
    let previous = state.setting(PROFILE_KEY).map_err(|_| "Cannot read local AI profiles")?;
    let mut current = read_profiles(state)?;
    let Some(index) = current.profiles.iter().position(|p| p.name == name) else {
        return Err("Unknown local AI profile".to_owned());
    };
    current.profiles.remove(index);
    if current.active.as_deref() == Some(name) {
        current.active = current.profiles.first().map(|p| p.name.clone());
    }
    write_profiles(state, previous.as_deref(), &current)?;
    Ok(current)
}

#[tauri::command]
pub async fn read_local_ai_profiles(app: tauri::AppHandle) -> Result<LocalProfiles, String> {
    tauri::async_runtime::spawn_blocking(move || {
        read_profiles(&open_private_bookmark_store(&app)?)
    })
    .await
    .map_err(|_| "Local AI profile worker stopped".to_owned())?
}

#[tauri::command]
pub async fn upsert_local_ai_profile(
    app: tauri::AppHandle,
    profile: LocalProfile,
    expected_profiles: LocalProfiles,
) -> Result<LocalProfiles, String> {
    validate_profile(&profile)?;
    validate_profiles(&expected_profiles)?;
    let notify = app.clone();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        upsert_if_current(&open_private_bookmark_store(&app)?, profile, &expected_profiles)
    })
    .await
    .map_err(|_| "Local AI profile worker stopped".to_owned())??;
    // Only broadcast after a successful durable CAS. Never include endpoints,
    // model IDs or any profile contents in the event payload.
    let _ = notify.emit(PROFILES_CHANGED_EVENT, ());
    Ok(saved)
}

#[tauri::command]
pub async fn activate_local_ai_profile(
    app: tauri::AppHandle,
    name: String,
    expected_profiles: LocalProfiles,
) -> Result<LocalProfiles, String> {
    validate_profiles(&expected_profiles)?;
    let notify = app.clone();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        activate_if_current(&open_private_bookmark_store(&app)?, &name, &expected_profiles)
    })
    .await
    .map_err(|_| "Local AI profile worker stopped".to_owned())??;
    // Only broadcast after a successful durable CAS. Never include endpoints,
    // model IDs or any profile contents in the event payload.
    let _ = notify.emit(PROFILES_CHANGED_EVENT, ());
    Ok(saved)
}

#[tauri::command]
pub async fn remove_local_ai_profile(
    app: tauri::AppHandle,
    name: String,
    expected_profiles: LocalProfiles,
) -> Result<LocalProfiles, String> {
    validate_profiles(&expected_profiles)?;
    let notify = app.clone();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        remove_if_current(&open_private_bookmark_store(&app)?, &name, &expected_profiles)
    })
    .await
    .map_err(|_| "Local AI profile worker stopped".to_owned())??;
    // Only broadcast after a successful durable CAS. Never include endpoints,
    // model IDs or any profile contents in the event payload.
    let _ = notify.emit(PROFILES_CHANGED_EVENT, ());
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, endpoint: &str, model: &str) -> LocalProfile {
        LocalProfile {
            name: name.to_owned(),
            protocol: "ollama".to_owned(),
            endpoint: endpoint.to_owned(),
            model: model.to_owned(),
        }
    }

    #[test]
    fn migrating_single_profile_requires_explicit_mutation() {
        let state = UserState::in_memory().unwrap();
        let old = r#"{"protocol":"ollama","endpoint":"http://127.0.0.1:11434","model":"qwen3:8b"}"#;
        state.set_setting(LEGACY_KEY, old).unwrap();
        let saved = read_profiles(&state).unwrap();
        assert_eq!(saved.active.as_deref(), Some("Default"));
        assert_eq!(saved.profiles[0].model, "qwen3:8b");
        assert_eq!(state.setting(PROFILE_KEY).unwrap(), None);
        let updated = upsert(&state, profile("LAN", "http://192.168.1.10:8080", "qwen3:8b"))
            .unwrap();
        assert_eq!(updated.active.as_deref(), Some("LAN"));
        assert_eq!(updated.profiles.len(), 2);
        assert_eq!(state.setting(LEGACY_KEY).unwrap(), None);
        assert_eq!(read_profiles(&state).unwrap(), updated);
    }

    #[test]
    fn create_switch_update_remove_and_empty_do_not_resurrect_legacy() {
        let state = UserState::in_memory().unwrap();
        let first = profile("Desktop", "http://localhost:11434", "qwen3:8b");
        let second = profile("LAN", "http://10.0.0.2:8080", "gemma");
        upsert(&state, first.clone()).unwrap();
        upsert(&state, second).unwrap();
        let picked = activate(&state, "Desktop").unwrap();
        assert_eq!(picked.active.as_deref(), Some("Desktop"));
        assert_eq!(picked.profiles.len(), 2);
        let mut edited = first;
        edited.model = "new-model".to_owned();
        let changed = upsert(&state, edited).unwrap();
        assert_eq!(changed.profiles.len(), 2);
        assert_eq!(changed.profiles[0].model, "new-model");
        let after = remove(&state, "Desktop").unwrap();
        assert_eq!(after.active.as_deref(), Some("LAN"));
        assert_eq!(remove(&state, "LAN").unwrap(), LocalProfiles::default());
        state.set_setting(LEGACY_KEY, "ignored").unwrap();
        assert_eq!(read_profiles(&state).unwrap(), LocalProfiles::default());
    }

    #[test]
    fn rejects_duplicate_names_overbudget_unknown_profiles_and_bad_input() {
        let state = UserState::in_memory().unwrap();
        let bad = [
            profile("bad/name", "http://127.0.0.1", "safe"),
            profile(" bad", "http://127.0.0.1", "safe"),
            profile("safe", "http://example.com", "safe"),
            profile("safe", "http://user:password@localhost", "safe"),
            profile("safe", "https://localhost", "safe"),
            profile("safe", "http://127.0.0.1", "bad\u{202e}name"),
        ];
        for item in bad {
            assert!(upsert(&state, item).is_err());
        }
        upsert(&state, profile("Model", "http://127.0.0.1", "safe")).unwrap();
        assert!(upsert(&state, profile("model", "http://127.0.0.1", "safe")).is_err());
        assert!(activate(&state, "missing").is_err());
        assert!(remove(&state, "missing").is_err());
        for i in 0..7 {
            upsert(&state, profile(&format!("Slot{i}"), "http://127.0.0.1", "safe"))
                .unwrap();
        }
        assert!(upsert(&state, profile("Overflow", "http://127.0.0.1", "safe")).is_err());
        assert_eq!(read_profiles(&state).unwrap().profiles.len(), MAX_PROFILES);
    }


    #[test]
    fn stale_profile_snapshot_cannot_overwrite_newer_saved_collection() {
        let state = UserState::in_memory().unwrap();
        let first = upsert(
            &state,
            profile("First", "http://127.0.0.1:11434", "first-model"),
        )
        .unwrap();
        let stale = state.setting(PROFILE_KEY).unwrap().unwrap();
        upsert(
            &state,
            profile("Second", "http://127.0.0.1:11434", "second-model"),
        )
        .unwrap();
        assert_eq!(first.profiles.len(), 1);
        let result = write_profiles(&state, Some(&stale), &first);
        assert!(result.is_err());
        let after = read_profiles(&state).unwrap();
        assert_eq!(after.profiles.len(), 2);
        assert_eq!(after.active.as_deref(), Some("Second"));
    }

    #[test]
    fn stale_editor_cannot_override_newer_profiles_across_sqlite_handles() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-profile-editor-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("state.sqlite");
        let first = UserState::open(&path).unwrap();
        let second = UserState::open(&path).unwrap();
        let stale_empty = read_profiles(&first).unwrap();
        let saved = upsert(
            &second,
            profile("Work", "http://127.0.0.1:11434", "model-a"),
        )
        .unwrap();
        assert!(upsert_if_current(
            &first,
            profile("Stale", "http://127.0.0.1:11434", "model-b"),
            &stale_empty
        )
        .is_err());
        assert_eq!(read_profiles(&first).unwrap(), saved);
        let refreshed = read_profiles(&first).unwrap();
        let updated = upsert_if_current(
            &first,
            profile("Other", "http://127.0.0.1:11434", "model-c"),
            &refreshed,
        )
        .unwrap();
        assert_eq!(updated.profiles.len(), 2);
        let stale_editor = read_profiles(&second).unwrap();
        activate(&first, "Work").unwrap();
        assert!(activate_if_current(&second, "Other", &stale_editor).is_err());
        assert!(remove_if_current(&second, "Work", &stale_editor).is_err());
        assert_eq!(read_profiles(&second).unwrap().active.as_deref(), Some("Work"));
        let fresh = read_profiles(&second).unwrap();
        assert_eq!(remove_if_current(&second, "Other", &fresh).unwrap().profiles.len(), 1);
        drop(first);
        drop(second);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_or_secret_bearing_storage_fails_closed() {
        let state = UserState::in_memory().unwrap();
        for saved in [
            r#"{"active":null,"profiles":[{"name":"A","protocol":"ollama","endpoint":"http://127.0.0.1","model":"x"}]}"#,
            r#"{"active":"A","profiles":[{"name":"A","protocol":"ollama","endpoint":"http://127.0.0.1","model":"x","apiKey":"secret"}]}"#,
            r#"{"active":"A","profiles":[{"name":"A","protocol":"ollama","endpoint":"http://127.0.0.1","model":"x"},{"name":"a","protocol":"ollama","endpoint":"http://127.0.0.1","model":"x"}]}"#,
            r#"{"active":"missing","profiles":[]}"#,
            "not-json",
        ] {
            state.set_setting(PROFILE_KEY, saved).unwrap();
            assert!(read_profiles(&state).is_err(), "{saved}");
        }
    }
}
