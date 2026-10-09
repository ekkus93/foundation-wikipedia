//! Provider-independent, UI-free LLM contract (partial AI-001).
//!
//! Adapters implement this protocol without choosing a fallback provider.
//! Streaming is callback-based; platform adapters must schedule blocking
//! transports off the UI thread and honor the cancellation token.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locality {
    OnDevice,
    Localhost,
    LocalNetwork,
    Cloud,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub discover_models: bool,
    pub streaming: bool,
    pub cancellable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub context_window: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerateRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_output_tokens: u32,
}

impl GenerateRequest {
    pub fn validate(&self) -> Result<(), ProviderError> {
        if self.model.trim().is_empty()
            || self.messages.is_empty()
            || self.max_output_tokens == 0
            || self
                .messages
                .iter()
                .any(|msg| msg.content.trim().is_empty())
        {
            return Err(ProviderError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta(String),
    Completed {
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderError {
    InvalidRequest,
    Unsupported,
    Cancelled,
    Timeout,
    Transport,
    InvalidResponse,
    Rejected,
}

/// Cooperative cancellation shared across UI and adapter threads.
/// A transport must check this during streaming and abort in-flight I/O.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Object-safe provider interface. The caller deliberately selects exactly
/// one provider; this contract does not silently try a cloud endpoint.
pub trait LlmProvider: Send + Sync {
    fn locality(&self) -> Locality;
    fn capabilities(&self) -> ProviderCapabilities;
    fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    fn test_connection(&self) -> Result<(), ProviderError>;

    fn stream(
        &self,
        request: &GenerateRequest,
        cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
    ) -> Result<(), ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    enum Outcome {
        Success,
        Timeout,
        BadResponse,
    }

    struct FakeProvider(Outcome);

    impl LlmProvider for FakeProvider {
        fn locality(&self) -> Locality {
            Locality::LocalNetwork
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                discover_models: true,
                streaming: true,
                cancellable: true,
            }
        }

        fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![ModelInfo {
                id: "fixture-model".into(),
                context_window: Some(4096),
            }])
        }

        fn test_connection(&self) -> Result<(), ProviderError> {
            Ok(())
        }

        fn stream(
            &self,
            request: &GenerateRequest,
            cancellation: &CancellationToken,
            on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
        ) -> Result<(), ProviderError> {
            request.validate()?;
            match self.0 {
                Outcome::Timeout => return Err(ProviderError::Timeout),
                Outcome::BadResponse => return Err(ProviderError::InvalidResponse),
                Outcome::Success => {}
            }
            for token in ["Grounded", " answer"] {
                if cancellation.is_cancelled() {
                    return Err(ProviderError::Cancelled);
                }
                on_event(StreamEvent::TextDelta(token.into()))?;
            }
            if cancellation.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            on_event(StreamEvent::Completed {
                input_tokens: Some(12),
                output_tokens: Some(3),
            })
        }
    }

    fn request() -> GenerateRequest {
        GenerateRequest {
            model: "fixture-model".into(),
            messages: vec![Message {
                role: MessageRole::User,
                content: "What is gravity?".into(),
            }],
            max_output_tokens: 48,
        }
    }

    #[test]
    fn streams_typed_events_without_ui_or_provider_specific_protocols() {
        let provider: Box<dyn LlmProvider> = Box::new(FakeProvider(Outcome::Success));
        assert_eq!(provider.locality(), Locality::LocalNetwork);
        assert!(provider.capabilities().streaming);
        assert_eq!(provider.list_models().unwrap()[0].id, "fixture-model");
        assert_eq!(provider.test_connection(), Ok(()));
        let mut events = Vec::new();
        let result = provider.stream(&request(), &CancellationToken::default(), &mut |event| {
            events.push(event);
            Ok(())
        });
        assert_eq!(result, Ok(()));
        assert_eq!(
            events,
            vec![
                StreamEvent::TextDelta("Grounded".into()),
                StreamEvent::TextDelta(" answer".into()),
                StreamEvent::Completed {
                    input_tokens: Some(12),
                    output_tokens: Some(3),
                },
            ]
        );
    }

    #[test]
    fn cancellation_interrupts_further_stream_events() {
        let provider = FakeProvider(Outcome::Success);
        let token = CancellationToken::default();
        let another_thread = token.clone();
        let mut events = Vec::new();
        let result = provider.stream(&request(), &token, &mut |event| {
            events.push(event);
            another_thread.cancel();
            Ok(())
        });
        assert_eq!(result, Err(ProviderError::Cancelled));
        assert_eq!(events, vec![StreamEvent::TextDelta("Grounded".into())]);
    }

    #[test]
    fn typed_timeout_and_bad_response_are_not_swallowed() {
        for (outcome, error) in [
            (Outcome::Timeout, ProviderError::Timeout),
            (Outcome::BadResponse, ProviderError::InvalidResponse),
        ] {
            assert_eq!(
                FakeProvider(outcome).stream(
                    &request(),
                    &CancellationToken::default(),
                    &mut |_| Ok(())
                ),
                Err(error)
            );
        }
    }

    #[test]
    fn invalid_prompts_fail_before_any_provider_invocation() {
        let mut input = request();
        input.model = " ".into();
        assert_eq!(input.validate(), Err(ProviderError::InvalidRequest));
        input = request();
        input.messages.clear();
        assert_eq!(input.validate(), Err(ProviderError::InvalidRequest));
