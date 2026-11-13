//! AI-003 host tests of the Kotlin-callable hosted-model API. No network:
//! every case fails before connecting or uses a fake cloud provider.

use std::sync::Mutex;

use wiki_ai::{
    CancellationToken, GenerateRequest, LlmProvider, Locality, ModelInfo, ProviderCapabilities,
    ProviderError, StreamEvent,
};
use wiki_ffi::android::{
    check_hosted_ai_connection, hosted_ai_key_is_valid, hosted_ai_ready, run_hosted_check,
    HostedAiError, HostedAiProvider,
};

const SECRET: &str = "sk-test-secret-value";

struct FakeCloud {
    outcome: Result<(), ProviderError>,
    seen_model: Mutex<Option<String>>,
}

impl FakeCloud {
    fn new(outcome: Result<(), ProviderError>) -> Self {
        Self {
            outcome,
            seen_model: Mutex::new(None),
        }
    }
}

impl LlmProvider for FakeCloud {
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
        _cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
    ) -> Result<(), ProviderError> {
        *self.seen_model.lock().unwrap() = Some(request.model.clone());
        self.outcome.clone()?;
        on_event(StreamEvent::TextDelta("O".into()))?;
        on_event(StreamEvent::TextDelta("K".into()))?;
        on_event(StreamEvent::Completed {
            input_tokens: Some(12),
            output_tokens: Some(1),
        })
    }
}

#[test]
fn hosts_other_than_android_need_no_verifier_initialisation() {
    assert!(hosted_ai_ready());
}

#[test]
fn key_shape_is_validated_before_the_app_stores_it() {
    assert!(hosted_ai_key_is_valid(SECRET.to_owned()));
    for bad in ["", "sk with space", "sk-\r\nX-Injected: 1", "sk-\u{e9}"] {
        assert!(!hosted_ai_key_is_valid(bad.to_owned()), "{bad:?}");
    }
    assert!(!hosted_ai_key_is_valid("k".repeat(4096)));
}

#[test]
fn bad_keys_and_models_are_refused_before_any_connection() {
    assert_eq!(
        check_hosted_ai_connection(
            HostedAiProvider::Anthropic,
            "bad key".to_owned(),
            "claude-haiku-5-5".to_owned()
        ),
        Err(HostedAiError::InvalidKey)
    );
    for model in ["", "model with space", "m\nX: y"] {
        assert_eq!(
            check_hosted_ai_connection(HostedAiProvider::OpenAi, SECRET.to_owned(), model.into()),
            Err(HostedAiError::InvalidRequest),
            "{model:?}"
        );
    }
    assert_eq!(
        check_hosted_ai_connection(HostedAiProvider::OpenAi, SECRET.to_owned(), "m".repeat(129)),
        Err(HostedAiError::InvalidRequest)
    );
}

#[test]
fn a_connection_check_streams_the_reply_and_usage() {
    let cloud = FakeCloud::new(Ok(()));
    let check = run_hosted_check(&cloud, "claude-haiku-5-5".to_owned()).unwrap();
    assert_eq!(check.reply, "OK");
    assert_eq!(check.input_tokens, Some(12));
    assert_eq!(check.output_tokens, Some(1));
    assert_eq!(
        cloud.seen_model.lock().unwrap().as_deref(),
        Some("claude-haiku-5-5")
    );
}

#[test]
fn provider_failures_map_to_typed_errors_without_detail() {
    for (failure, expected) in [
        (ProviderError::Rejected, HostedAiError::Rejected),
        (ProviderError::Timeout, HostedAiError::Timeout),
        (ProviderError::Transport, HostedAiError::Network),
        (
            ProviderError::InvalidResponse,
            HostedAiError::InvalidResponse,
        ),
        (ProviderError::Unsupported, HostedAiError::NotReady),
    ] {
        let error = run_hosted_check(&FakeCloud::new(Err(failure)), "m".into()).unwrap_err();
        assert_eq!(error, expected);
        assert!(!format!("{error} {error:?}").contains(SECRET));
    }
}
