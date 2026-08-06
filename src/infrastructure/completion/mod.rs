pub mod openai;

pub use openai::OpenAiChatClient;

use async_trait::async_trait;

use crate::error::AppError;

#[async_trait]
pub trait ChatCompleter: Send + Sync {
    async fn complete(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError>;
}
