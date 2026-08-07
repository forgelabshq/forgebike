//! Port trait for AI content generation (sentiment analysis + reply drafts).
//!
//! The concrete implementation lives in
//! `forgebike_infrastructure::ai::OpenAiClient`.  Tests use simple in-memory
//! mocks.

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    entities::{content_piece::ContentType, review::ReviewPlatform},
    DomainError,
};

// ---------------------------------------------------------------------------
// Value types
// ---------------------------------------------------------------------------

/// Output of a sentiment analysis call.
#[derive(Debug, Clone)]
pub struct SentimentResult {
    /// Score in the range −1.0 (very negative) … +1.0 (very positive).
    pub score: f32,
    /// `OpenAI` tokens consumed by this call.
    pub tokens_used: u64,
}

/// Context passed to the reply-draft generator.
#[derive(Debug, Clone)]
pub struct ReplyContext {
    pub review_text: String,
    pub rating: i16,
    pub platform: ReviewPlatform,
    pub business_name: String,
}

/// Output of a reply-draft generation call.
#[derive(Debug, Clone)]
pub struct ReplyDraft {
    pub text: String,
    /// `OpenAI` tokens consumed by this call.
    pub tokens_used: u64,
}

// ---------------------------------------------------------------------------
// Content generation types (Phase 5)
// ---------------------------------------------------------------------------

/// Context used to generate marketing content for a restaurant.
#[derive(Debug, Clone)]
pub struct ContentContext {
    pub content_type: ContentType,
    pub business_name: String,
    /// e.g. `"Italian"` — included in the prompt when set.
    pub cuisine_type: Option<String>,
    /// e.g. `"summer menu launch"`, `"pasta carbonara"`, `"Mother's Day offer"`.
    pub topic: Option<String>,
    /// Optional tone / style guidance from the owner.
    pub tone: Option<String>,
}

/// Output of a content-generation call (sync or streaming).
#[derive(Debug, Clone)]
pub struct ContentDraft {
    /// Separate title when the content type calls for one (e.g. email subject).
    pub title: Option<String>,
    pub body: String,
    /// `OpenAI` tokens consumed by this call.
    pub tokens_used: u64,
}

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

#[async_trait]
pub trait AiContentPort: Send + Sync {
    /// Analyse the sentiment of a review body text.
    ///
    /// Returns `Ok(None)` when the client is not configured (empty `API` key)
    /// so callers can skip gracefully without an error.
    async fn analyse_sentiment(&self, text: &str) -> Result<Option<SentimentResult>, DomainError>;

    /// Generate a reply draft for a review given its context.
    ///
    /// Returns `Err(DomainError::ExternalService)` when the API key is empty
    /// or the `OpenAI` call fails.
    async fn generate_reply_draft(&self, context: &ReplyContext)
        -> Result<ReplyDraft, DomainError>;

    /// Generate marketing content synchronously.
    ///
    /// Returns `Err(DomainError::ExternalService)` when the `API` key is empty.
    async fn generate_content(&self, context: &ContentContext)
        -> Result<ContentDraft, DomainError>;

    /// Generate marketing content with **streaming** output.
    ///
    /// Each token chunk is forwarded to `on_chunk` as it arrives from the
    /// `OpenAI` API.  The complete accumulated text is returned in
    /// [`ContentDraft`] once the stream ends.
    ///
    /// Returns `Err(DomainError::ExternalService)` when the `API` key is empty.
    async fn stream_content(
        &self,
        context: &ContentContext,
        on_chunk: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<ContentDraft, DomainError>;
}
