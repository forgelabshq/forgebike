//! `OpenAI` adapter implementing [`AiContentPort`].
//!
//! ## Prompt templates
//! Prompts are embedded at compile-time via `include_str!` from the adjacent
//! `prompts/` directory.  This keeps them in version control as plain text
//! files that can be edited without touching Rust code.
//!
//! ## Graceful degradation
//! When `api_key` is empty:
//! - `analyse_sentiment` returns `Ok(None)` — callers skip silently.
//! - `generate_reply_draft` returns `Err(ExternalService)` — callers surface
//!   a 503 to the user.

use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
        CreateChatCompletionRequestArgs,
    },
    Client,
};
use async_trait::async_trait;

use forgebike_domain::{
    ports::ai_port::{AiContentPort, ReplyContext, ReplyDraft, SentimentResult},
    DomainError,
};

// ---------------------------------------------------------------------------
// Prompt templates (embedded at compile time)
// ---------------------------------------------------------------------------

const SENTIMENT_PROMPT: &str = include_str!("prompts/sentiment.txt");
const REPLY_PROMPT: &str = include_str!("prompts/reply.txt");

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct OpenAiClient {
    api_key: String,
    model: String,
    max_sentiment_tokens: u32,
    max_reply_tokens: u32,
}

impl OpenAiClient {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_sentiment_tokens: u32,
        max_reply_tokens: u32,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            max_sentiment_tokens,
            max_reply_tokens,
        }
    }

    /// Build a configured `OpenAI` client from the stored API key.
    fn client(&self) -> Client<OpenAIConfig> {
        Client::with_config(OpenAIConfig::new().with_api_key(&self.api_key))
    }
}

// ---------------------------------------------------------------------------
// Port implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl AiContentPort for OpenAiClient {
    async fn analyse_sentiment(&self, text: &str) -> Result<Option<SentimentResult>, DomainError> {
        if self.api_key.is_empty() {
            tracing::debug!("OpenAI API key not configured — skipping sentiment analysis");
            return Ok(None);
        }

        let prompt = SENTIMENT_PROMPT.replace("{{REVIEW_TEXT}}", text);

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .max_tokens(self.max_sentiment_tokens)
            .messages([
                ChatCompletionRequestSystemMessageArgs::default()
                    .content("You are a precise sentiment analysis assistant. Always respond with only valid JSON.")
                    .build()
                    .map_err(|e| DomainError::ExternalService(e.to_string()))?
                    .into(),
                ChatCompletionRequestUserMessageArgs::default()
                    .content(prompt)
                    .build()
                    .map_err(|e| DomainError::ExternalService(e.to_string()))?
                    .into(),
            ])
            .build()
            .map_err(|e| DomainError::ExternalService(e.to_string()))?;

        let response = self.client().chat().create(request).await.map_err(|e| {
            DomainError::ExternalService(format!("OpenAI sentiment call failed: {e}"))
        })?;

        let tokens_used = response.usage.map_or(0, |u| u64::from(u.total_tokens));

        let content = response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default();

        let score = parse_sentiment_score(&content)?;

        Ok(Some(SentimentResult { score, tokens_used }))
    }

    async fn generate_reply_draft(
        &self,
        context: &ReplyContext,
    ) -> Result<ReplyDraft, DomainError> {
        if self.api_key.is_empty() {
            return Err(DomainError::ExternalService(
                "OpenAI API key is not configured — set APP__AI__OPENAI_API_KEY".into(),
            ));
        }

        let prompt = REPLY_PROMPT
            .replace("{{BUSINESS_NAME}}", &context.business_name)
            .replace("{{PLATFORM}}", &context.platform.to_string())
            .replace("{{RATING}}", &context.rating.to_string())
            .replace("{{REVIEW_TEXT}}", &context.review_text);

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .max_tokens(self.max_reply_tokens)
            .messages([
                ChatCompletionRequestSystemMessageArgs::default()
                    .content("You are a professional customer relations assistant for a restaurant. Write genuine, helpful replies.")
                    .build()
                    .map_err(|e| DomainError::ExternalService(e.to_string()))?
                    .into(),
                ChatCompletionRequestUserMessageArgs::default()
                    .content(prompt)
                    .build()
                    .map_err(|e| DomainError::ExternalService(e.to_string()))?
                    .into(),
            ])
            .build()
            .map_err(|e| DomainError::ExternalService(e.to_string()))?;

        let response =
            self.client().chat().create(request).await.map_err(|e| {
                DomainError::ExternalService(format!("OpenAI reply call failed: {e}"))
            })?;

        let tokens_used = response.usage.map_or(0, |u| u64::from(u.total_tokens));

        let text = response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default()
            .trim()
            .to_string();

        if text.is_empty() {
            return Err(DomainError::ExternalService(
                "OpenAI returned an empty reply draft".into(),
            ));
        }

        Ok(ReplyDraft { text, tokens_used })
    }
}

// ---------------------------------------------------------------------------
// Sentiment score parsing
// ---------------------------------------------------------------------------

/// Parse the score from the AI's JSON response.
///
/// Tries strict JSON first, then falls back to extracting any bare float so
/// that minor model deviations from the prompt don't cause hard failures.
fn parse_sentiment_score(content: &str) -> Result<f32, DomainError> {
    let trimmed = content.trim();

    // Happy path: well-formed JSON {"score": 0.5}
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(score) = v["score"].as_f64() {
            #[allow(clippy::cast_possible_truncation)]
            return Ok((score as f32).clamp(-1.0, 1.0));
        }
    }

    // Fallback: bare float (e.g. the model returned "0.75" without JSON)
    if let Ok(score) = trimmed.parse::<f32>() {
        return Ok(score.clamp(-1.0, 1.0));
    }

    Err(DomainError::ExternalService(format!(
        "Could not parse sentiment score from AI response: {trimmed:?}"
    )))
}
