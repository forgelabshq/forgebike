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

use std::sync::Arc;

use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
    },
    Client,
};
use async_trait::async_trait;
use futures::StreamExt as _;

use forgebike_domain::{
    entities::content_piece::ContentType,
    ports::ai_port::{
        AiContentPort, ChatContext, ChatMessage, ChatReply, ChatRole, ContentContext, ContentDraft,
        ReplyContext, ReplyDraft, SentimentResult,
    },
    DomainError,
};

// ---------------------------------------------------------------------------
// Prompt templates (embedded at compile time)
// ---------------------------------------------------------------------------

const SENTIMENT_PROMPT: &str = include_str!("prompts/sentiment.txt");
const REPLY_PROMPT: &str = include_str!("prompts/reply.txt");
const CONTENT_PROMPT: &str = include_str!("prompts/content.txt");

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct OpenAiClient {
    api_key: String,
    model: String,
    max_sentiment_tokens: u32,
    max_reply_tokens: u32,
    max_content_tokens: u32,
}

impl OpenAiClient {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_sentiment_tokens: u32,
        max_reply_tokens: u32,
        max_content_tokens: u32,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            max_sentiment_tokens,
            max_reply_tokens,
            max_content_tokens,
        }
    }

    // -----------------------------------------------------------------------
    // Content prompt helpers
    // -----------------------------------------------------------------------

    fn build_content_prompt(ctx: &ContentContext) -> String {
        let cuisine_desc = ctx
            .cuisine_type
            .as_deref()
            .map(|c| format!(" {c}"))
            .unwrap_or_default();

        let type_instruction = match ctx.content_type {
            ContentType::SocialPost =>
                "Write a punchy social media post (max 240 characters) for Instagram, Facebook, or X. End with 2-3 relevant hashtags.",
            ContentType::Email =>
                "Write a marketing email. Line 1: the subject line. Blank line. Then the body (3 concise paragraphs).",
            ContentType::MenuDescription =>
                "Write an appetising 2-sentence menu item description (max 60 words) highlighting flavours and making the dish sound unmissable.",
            ContentType::BlogIntro =>
                "Write an engaging blog introduction (2 paragraphs, max 120 words) that hooks the reader and previews what the post will cover.",
        };

        let topic_line = ctx
            .topic
            .as_deref()
            .map(|t| format!("Topic / focus: {t}\n"))
            .unwrap_or_default();

        let tone_line = ctx
            .tone
            .as_deref()
            .map(|t| format!("Tone: {t}\n"))
            .unwrap_or_default();

        CONTENT_PROMPT
            .replace("{{BUSINESS_NAME}}", &ctx.business_name)
            .replace("{{CUISINE_DESC}}", &cuisine_desc)
            .replace("{{CONTENT_TYPE_INSTRUCTION}}", type_instruction)
            .replace("{{TOPIC_LINE}}", &topic_line)
            .replace("{{TONE_LINE}}", &tone_line)
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

    async fn generate_content(
        &self,
        context: &ContentContext,
    ) -> Result<ContentDraft, DomainError> {
        if self.api_key.is_empty() {
            return Err(DomainError::ExternalService(
                "OpenAI API key is not configured — set APP__AI__OPENAI_API_KEY".into(),
            ));
        }

        let prompt = Self::build_content_prompt(context);

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .max_tokens(self.max_content_tokens)
            .messages([
                ChatCompletionRequestSystemMessageArgs::default()
                    .content("You are a skilled marketing copywriter specialising in the restaurant industry.")
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
            DomainError::ExternalService(format!("OpenAI content call failed: {e}"))
        })?;

        let tokens_used = response.usage.map_or(0, |u| u64::from(u.total_tokens));
        let raw = response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default()
            .trim()
            .to_string();

        if raw.is_empty() {
            return Err(DomainError::ExternalService(
                "OpenAI returned empty content".into(),
            ));
        }

        Ok(split_title_body(&context.content_type, raw, tokens_used))
    }

    async fn chat(
        &self,
        context: &ChatContext,
        messages: &[ChatMessage],
    ) -> Result<ChatReply, DomainError> {
        if self.api_key.is_empty() {
            return Err(DomainError::ExternalService(
                "OpenAI API key is not configured — set APP__AI__OPENAI_API_KEY".into(),
            ));
        }

        let cuisine_desc = context
            .cuisine_type
            .as_deref()
            .map_or_else(|| " restaurant".to_string(), |c| format!(" {c} restaurant"));

        let persona = context
            .persona
            .as_deref()
            .unwrap_or("friendly, knowledgeable, and concise");

        let system_content = format!(
            "You are a helpful AI assistant for {}{cuisine_desc} called '{name}'. \
             Your persona is {persona}. \
             Help customers with questions about the restaurant including menu, hours, \
             reservations, and general information. \
             Keep replies brief and warm. \
             If you don't know something specific, say so and offer to help in another way.",
            context.business_name,
            name = context.business_name,
        );

        let mut api_messages = vec![ChatCompletionRequestSystemMessageArgs::default()
            .content(system_content)
            .build()
            .map_err(|e| DomainError::ExternalService(e.to_string()))?
            .into()];

        for msg in messages {
            let api_msg = match msg.role {
                ChatRole::User => ChatCompletionRequestUserMessageArgs::default()
                    .content(msg.content.clone())
                    .build()
                    .map_err(|e| DomainError::ExternalService(e.to_string()))?
                    .into(),
                ChatRole::Assistant => ChatCompletionRequestAssistantMessageArgs::default()
                    .content(msg.content.clone())
                    .build()
                    .map_err(|e| DomainError::ExternalService(e.to_string()))?
                    .into(),
            };
            api_messages.push(api_msg);
        }

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .max_tokens(512u32)
            .messages(api_messages)
            .build()
            .map_err(|e| DomainError::ExternalService(e.to_string()))?;

        let response =
            self.client().chat().create(request).await.map_err(|e| {
                DomainError::ExternalService(format!("OpenAI chat call failed: {e}"))
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
                "OpenAI returned an empty chat reply".into(),
            ));
        }

        Ok(ChatReply { text, tokens_used })
    }

    async fn stream_content(
        &self,
        context: &ContentContext,
        on_chunk: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<ContentDraft, DomainError> {
        if self.api_key.is_empty() {
            return Err(DomainError::ExternalService(
                "OpenAI API key is not configured — set APP__AI__OPENAI_API_KEY".into(),
            ));
        }

        let prompt = Self::build_content_prompt(context);

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .max_tokens(self.max_content_tokens)
            .messages([
                ChatCompletionRequestSystemMessageArgs::default()
                    .content("You are a skilled marketing copywriter specialising in the restaurant industry.")
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

        let mut stream = self
            .client()
            .chat()
            .create_stream(request)
            .await
            .map_err(|e| DomainError::ExternalService(format!("OpenAI stream failed: {e}")))?;

        let mut full_text = String::new();
        let mut tokens_used = 0u64;

        while let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    for choice in response.choices {
                        if let Some(delta) = choice.delta.content {
                            full_text.push_str(&delta);
                            on_chunk(delta);
                        }
                    }
                    if let Some(usage) = response.usage {
                        tokens_used = u64::from(usage.total_tokens);
                    }
                }
                Err(e) => {
                    return Err(DomainError::ExternalService(format!(
                        "Stream chunk error: {e}"
                    )));
                }
            }
        }

        let trimmed = full_text.trim().to_string();
        if trimmed.is_empty() {
            return Err(DomainError::ExternalService(
                "OpenAI stream produced empty content".into(),
            ));
        }

        Ok(split_title_body(
            &context.content_type,
            trimmed,
            tokens_used,
        ))
    }
}

// ---------------------------------------------------------------------------
// Content helper: split email subject / body
// ---------------------------------------------------------------------------

/// For `Email` content the model is instructed to put the subject on line 1.
/// This helper splits it out; all other types return the raw text as the body.
fn split_title_body(content_type: &ContentType, raw: String, tokens_used: u64) -> ContentDraft {
    if *content_type == ContentType::Email {
        let mut lines = raw.splitn(2, '\n');
        let subject = lines.next().unwrap_or("").trim().to_string();
        let body = lines.next().unwrap_or("").trim().to_string();
        let (title, body) = if body.is_empty() {
            (None, subject)
        } else {
            (Some(subject), body)
        };
        return ContentDraft {
            title,
            body,
            tokens_used,
        };
    }
    ContentDraft {
        title: None,
        body: raw,
        tokens_used,
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
