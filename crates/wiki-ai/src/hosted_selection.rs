//! A user-selected hosted model as an article-chat destination (AI-003).
//!
//! The app supplies the provider, the exact model name and the API key from
//! its OS secret storage. The destination always has `Cloud` locality, so
//! article evidence is sent only when the question allows cloud traffic and
//! the user consented to the disclosed preview (see `article_destination`).
//! The key is held only in memory and is redacted from `Debug`.

use crate::article_destination::ArticleDestination;
use crate::hosted_codec::{ApiKey, HostedProvider};
use crate::hosted_http::{provider_host, HostedChatProvider};
use crate::{LlmProvider, Locality, ProviderError};

#[derive(Clone, PartialEq, Eq)]
pub struct SelectedHostedModel {
    provider: HostedProvider,
    model: String,
    key: ApiKey,
}

impl std::fmt::Debug for SelectedHostedModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectedHostedModel")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("key", &self.key)
            .finish()
    }
}

impl SelectedHostedModel {
    /// Select `model` at `provider`. The model name is validated (no
    /// whitespace padding, control or invisible characters); nothing is sent.
    pub fn new(provider: HostedProvider, model: &str, key: ApiKey) -> Result<Self, ProviderError> {
        if !crate::valid_model_id(model) || model.chars().any(char::is_whitespace) {
            return Err(ProviderError::InvalidRequest);
        }
        Ok(Self {
            provider,
            model: model.to_owned(),
            key,
        })
    }

    pub fn provider(&self) -> HostedProvider {
        self.provider
    }
}

impl ArticleDestination for SelectedHostedModel {
    fn model(&self) -> &str {
        &self.model
    }

    fn locality(&self) -> Locality {
        Locality::Cloud
    }

    fn destination(&self) -> &str {
        provider_host(self.provider)
    }

    fn connect(&self) -> Result<Box<dyn LlmProvider>, ProviderError> {
        Ok(Box::new(HostedChatProvider::new(
            self.provider,
            self.key.clone(),
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ApiKey {
        ApiKey::new("sk-test-secret-value".into()).unwrap()
    }

    #[test]
    fn hosted_destination_is_cloud_at_its_fixed_host_and_redacts_the_key() {
        let selected =
            SelectedHostedModel::new(HostedProvider::Anthropic, "claude-haiku-5-5", key()).unwrap();
        assert_eq!(selected.locality(), Locality::Cloud);
        assert_eq!(selected.destination(), "api.anthropic.com");
        assert_eq!(ArticleDestination::model(&selected), "claude-haiku-5-5");
        assert!(!format!("{selected:?}").contains("sk-test-secret-value"));
        let provider = selected.connect().unwrap();
        assert_eq!(provider.locality(), Locality::Cloud);
        let openai = SelectedHostedModel::new(HostedProvider::OpenAi, "gpt-x", key()).unwrap();
        assert_eq!(openai.destination(), "api.openai.com");
    }

    #[test]
    fn model_names_that_could_spoof_or_inject_are_refused() {
        for bad in ["", " padded", "two words", "m\nX: y", "m\u{202e}x"] {
            assert_eq!(
                SelectedHostedModel::new(HostedProvider::OpenAi, bad, key()),
                Err(ProviderError::InvalidRequest),
                "{bad:?}"
            );
        }
    }
}
