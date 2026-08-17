pub mod openai;

pub use openai::OpenAiChatClient;

use async_trait::async_trait;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

impl ChatRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
        }
    }
}

/// Decoding profile for a completion request.
///
/// Clamped at construction so no caller can request a profile unsuitable for
/// religious content, whatever path the values arrive by. Configuration
/// rejects out-of-range values loudly at startup; this clamp is the
/// defence-in-depth behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompletionOptions {
    temperature: f32,
    max_tokens: u32,
}

impl CompletionOptions {
    pub const MAX_TEMPERATURE: f32 = 0.7;
    pub const MIN_MAX_TOKENS: u32 = 64;
    pub const MAX_MAX_TOKENS: u32 = 1200;

    pub fn new(temperature: f32, max_tokens: u32) -> Self {
        Self {
            temperature: temperature.clamp(0.0, Self::MAX_TEMPERATURE),
            max_tokens: max_tokens.clamp(Self::MIN_MAX_TOKENS, Self::MAX_MAX_TOKENS),
        }
    }

    pub fn temperature(&self) -> f32 {
        self.temperature
    }

    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }
}

/// A chunk of a streamed completion.
pub type CompletionChunk = Result<String, AppError>;

/// A stream of text deltas, in order.
pub type CompletionStream =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = CompletionChunk> + Send>>;

#[async_trait]
pub trait ChatCompleter: Send + Sync {
    async fn complete_messages(
        &self,
        messages: &[ChatMessage],
        options: CompletionOptions,
    ) -> Result<String, AppError>;

    /// Streams a completion as it is generated.
    ///
    /// Defaults to running the non-streaming call and yielding it as one chunk,
    /// so implementations that cannot stream — and every test fake — stay
    /// correct without extra code. Callers must not assume more than one chunk.
    async fn stream_messages(
        &self,
        messages: &[ChatMessage],
        options: CompletionOptions,
    ) -> Result<CompletionStream, AppError> {
        let whole = self.complete_messages(messages, options).await?;
        Ok(Box::pin(futures_util::stream::once(
            async move { Ok(whole) },
        )))
    }

    /// Convenience for the single-shot case. Provided rather than required, so
    /// implementors only ever write `complete_messages`.
    async fn complete(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        options: CompletionOptions,
    ) -> Result<String, AppError> {
        self.complete_messages(
            &[
                ChatMessage::system(system_prompt),
                ChatMessage::user(user_prompt),
            ],
            options,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_options_clamp_temperature_to_the_safe_ceiling() {
        assert_eq!(CompletionOptions::new(2.0, 400).temperature(), 0.7);
        assert_eq!(CompletionOptions::new(-1.0, 400).temperature(), 0.0);
        assert_eq!(CompletionOptions::new(0.3, 400).temperature(), 0.3);
    }

    #[test]
    fn completion_options_clamp_max_tokens_to_the_supported_range() {
        assert_eq!(CompletionOptions::new(0.3, 0).max_tokens(), 64);
        assert_eq!(CompletionOptions::new(0.3, 99_999).max_tokens(), 1200);
        assert_eq!(CompletionOptions::new(0.3, 400).max_tokens(), 400);
    }
}
