//! Consent-gated article chat for any explicitly selected model destination.
//!
//! A destination is exactly one user-selected model at one place: a
//! discovered local/LAN server (`local_selection::SelectedLocalModel`) or a
//! hosted provider with an app-supplied key
//! (`hosted_selection::SelectedHostedModel`). The flow is the same for both:
//!
//! 1. [`prepare_article_preview`] builds, without any I/O, the exact passages,
//!    history and destination that a request would send, for disclosure.
//! 2. [`answer_article_with_preview`] sends only if the user consented to
//!    THAT preview: any change of question, settings, destination, model,
//!    revision or packed evidence is refused before connecting.
//!
//! Locality is enforced by the question's `OutboundPolicy` in
//! `stream_selected`, so a hosted destination additionally needs
//! `OutboundPolicy::AllowCloud`. There is never a fallback destination.

use crate::{LlmProvider, Locality, ProviderError};
use wiki_model::Article;
use wiki_search::{EvidenceHit, GroundedAnswer};

pub use crate::local_selection::{
    LocalArticleDisplayAnswer as ArticleDisplayAnswer, LocalArticleError as ArticleInferenceError,
};

/// One explicitly selected model destination.
pub trait ArticleDestination: Clone + PartialEq + std::fmt::Debug {
    /// The exact model identifier requests are sent to.
    fn model(&self) -> &str;
    /// Where the evidence goes (on-device, local, LAN or cloud).
    fn locality(&self) -> Locality;
    /// Human-readable destination to disclose: an endpoint URL or host name.
    fn destination(&self) -> &str;
    /// Build the provider for one request. Must not perform network I/O.
    fn connect(&self) -> Result<Box<dyn LlmProvider>, ProviderError>;
}

/// Exact evidence and selected destination disclosed before consent. A preview
/// does not grant network permission; it is bound to one destination and model.
#[derive(Clone, Debug)]
pub struct ArticlePreview<D> {
    pub(crate) selected: D,
    pub(crate) context: crate::rag::PreparedArticleContext,
    // Consent applies to the entire requested inference, not only excerpts.
    question: String,
    passage_limit: usize,
    context_word_budget: usize,
    selected_evidence: Option<EvidenceHit>,
    max_output_tokens: u32,
    outbound_policy: crate::OutboundPolicy,
}

impl<D: ArticleDestination> ArticlePreview<D> {
    pub fn destination(&self) -> &str {
        self.selected.destination()
    }

    pub fn model(&self) -> &str {
        self.selected.model()
    }

    pub fn locality(&self) -> Locality {
        self.selected.locality()
    }

    pub fn passages(&self) -> &[EvidenceHit] {
        &self.context.evidence
    }

    pub fn user_message(&self) -> &str {
        &self.context.user_message
    }

    /// Earlier (question, citation-free answer) turns that will also be sent.
    pub fn history(&self) -> &[(String, String)] {
        &self.context.history
    }
}

/// Prepare a no-network disclosure of the exact passages and destination.
pub fn prepare_article_preview<D: ArticleDestination>(
    selected: &D,
    article: &Article,
    question: &crate::rag::ArticleQuestion<'_>,
) -> Result<Option<ArticlePreview<D>>, ArticleInferenceError> {
    if question.model != selected.model() {
        return Err(ArticleInferenceError::ModelMismatch);
    }
    crate::rag::prepare_article_context(article, question)
        .map(|context| {
            context.map(|context| ArticlePreview {
                selected: selected.clone(),
                context,
                question: question.question.to_owned(),
                passage_limit: question.passage_limit,
                context_word_budget: question.context_word_budget,
                selected_evidence: question.selected_evidence.cloned(),
                max_output_tokens: question.max_output_tokens,
                outbound_policy: question.outbound_policy,
            })
        })
        .map_err(ArticleInferenceError::Rag)
}

/// Infer only if the affirmative decision applies to the exact preview.
/// Any changed question, revision, destination, model or packed evidence
/// invalidates the preview before provider I/O.
pub fn answer_article_with_preview<D: ArticleDestination>(
    selected: &D,
    article: &Article,
    question: &crate::rag::ArticleQuestion<'_>,
    preview: &ArticlePreview<D>,
    share_article_passages: bool,
    cancellation: &crate::CancellationToken,
) -> Result<GroundedAnswer, ArticleInferenceError> {
    if !share_article_passages {
        return Err(ArticleInferenceError::PassageConsentRequired);
    }
    if question.model != selected.model() {
        return Err(ArticleInferenceError::ModelMismatch);
    }
    if selected != &preview.selected
        || question.question != preview.question
        || question.passage_limit != preview.passage_limit
        || question.context_word_budget != preview.context_word_budget
        || question.selected_evidence != preview.selected_evidence.as_ref()
        || question.max_output_tokens != preview.max_output_tokens
        || question.outbound_policy != preview.outbound_policy
    {
        return Err(ArticleInferenceError::PreviewMismatch);
    }
    let Some(current) = prepare_article_preview(selected, article, question)? else {
        return Err(ArticleInferenceError::PreviewMismatch);
    };
    if current.context.user_message != preview.context.user_message
        || current.context.evidence != preview.context.evidence
        || current.context.history != preview.context.history
    {
        return Err(ArticleInferenceError::PreviewMismatch);
    }
    let provider = selected
        .connect()
        .map_err(|error| ArticleInferenceError::Rag(crate::rag::RagError::Provider(error)))?;
    // Reuse the context just verified against consent. Do not rebuild it
    // again between verification and the actual provider request.
    crate::rag::answer_prepared_article(provider.as_ref(), question, current.context, cancellation)
        .map_err(ArticleInferenceError::Rag)
}

/// Presentation-safe text and canonical citation navigation targets from a
/// *fresh* consent-gated inference. Model output stays buffered until all
/// citation and current-article checks pass.
pub fn answer_article_with_preview_display<D: ArticleDestination>(
    selected: &D,
    article: &Article,
    question: &crate::rag::ArticleQuestion<'_>,
    preview: &ArticlePreview<D>,
    share_article_passages: bool,
    cancellation: &crate::CancellationToken,
) -> Result<ArticleDisplayAnswer, ArticleInferenceError> {
    match answer_article_with_preview(
        selected,
        article,
        question,
        preview,
        share_article_passages,
        cancellation,
    )? {
        GroundedAnswer::InsufficientEvidence => Ok(ArticleDisplayAnswer::InsufficientEvidence),
        supported => crate::rag::answer_navigation_parts(article, &supported)
            .map(ArticleDisplayAnswer::Supported)
            .map_err(ArticleInferenceError::Display),
    }
}
