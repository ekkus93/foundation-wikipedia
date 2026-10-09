use std::sync::atomic::{AtomicUsize, Ordering};
use wiki_ai::{
    stream_selected, CancellationToken, GenerateRequest, LlmProvider, Locality, Message,
    MessageRole, ModelInfo, OutboundPolicy, ProviderCapabilities, ProviderError, StreamEvent,
};

struct FakeProvider<'a> {
    locality: Locality,
    calls: &'a AtomicUsize,
}

impl LlmProvider for FakeProvider<'_> {
    fn locality(&self) -> Locality {
        self.locality
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            discover_models: false,
            streaming: true,
            cancellable: true,
        }
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }

    fn test_connection(&self) -> Result<(), ProviderError> {
        Ok(())
    }

    fn stream(
        &self,
        _: &GenerateRequest,
        _: &CancellationToken,
        on_event: &mut dyn FnMut(StreamEvent) -> Result<(), ProviderError>,
    ) -> Result<(), ProviderError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        on_event(StreamEvent::TextDelta("safe response".into()))
    }
}

fn request() -> GenerateRequest {
    GenerateRequest {
        model: "chosen-model".into(),
        messages: vec![Message {
            role: MessageRole::User,
            content: "Local article excerpt".into(),
        }],
        max_output_tokens: 32,
    }
}

#[test]
fn on_device_only_never_dispatches_lan_or_cloud_context() {
    for locality in [Locality::Localhost, Locality::LocalNetwork, Locality::Cloud] {
        let calls = AtomicUsize::new(0);
        let provider = FakeProvider {
            locality,
            calls: &calls,
        };
        let result = stream_selected(
            &provider,
            &request(),
            &CancellationToken::default(),
            OutboundPolicy::OnDeviceOnly,
            &mut |_| Ok(()),
        );
        assert_eq!(result, Err(ProviderError::DestinationNotAllowed));
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn lan_permission_does_not_confer_cloud_permission() {
    let calls = AtomicUsize::new(0);
    let provider = FakeProvider {
        locality: Locality::Cloud,
        calls: &calls,
    };
    assert_eq!(
        stream_selected(
            &provider,
            &request(),
            &CancellationToken::default(),
            OutboundPolicy::AllowLocalEndpoints,
            &mut |_| Ok(())
        ),
        Err(ProviderError::DestinationNotAllowed)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn permitted_provider_receives_exactly_one_dispatch() {
    for (locality, policy) in [
        (Locality::OnDevice, OutboundPolicy::OnDeviceOnly),
        (Locality::LocalNetwork, OutboundPolicy::AllowLocalEndpoints),
        (Locality::Cloud, OutboundPolicy::AllowCloud),
    ] {
        let calls = AtomicUsize::new(0);
        let provider = FakeProvider {
            locality,
            calls: &calls,
        };
        let result = stream_selected(
            &provider,
            &request(),
            &CancellationToken::default(),
            policy,
            &mut |_| Ok(()),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn cancellation_prevents_any_provider_dispatch() {
    let calls = AtomicUsize::new(0);
    let provider = FakeProvider {
        locality: Locality::OnDevice,
        calls: &calls,
    };
    let token = CancellationToken::default();
    token.cancel();
    let mut events = Vec::new();
    assert_eq!(
        stream_selected(
            &provider,
            &request(),
            &token,
            OutboundPolicy::OnDeviceOnly,
            &mut |event| {
                events.push(event);
                Ok(())
            }
        ),
        Err(ProviderError::Cancelled)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(events.is_empty());
}

#[test]
fn invalid_request_does_not_start_provider_stream() {
    let calls = AtomicUsize::new(0);
    let provider = FakeProvider {
        locality: Locality::OnDevice,
        calls: &calls,
    };
    let mut request = request();
    request.model.clear();
    assert_eq!(
        stream_selected(
            &provider,
            &request,
            &CancellationToken::default(),
            OutboundPolicy::OnDeviceOnly,
            &mut |_| Ok(())
        ),
        Err(ProviderError::InvalidRequest)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}
