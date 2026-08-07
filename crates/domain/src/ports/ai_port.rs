//! Port trait for AI content generation (sentiment analysis + reply drafts).
//!
//! The concrete implementation lives in
//! `forgebike_infrastructure::ai::OpenAiClient`.  Tests use simple in-memory
//! mocks.

use async_trait::async_trait;

use crate::{entities::review::ReviewPlatform, DomainError};

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
}
