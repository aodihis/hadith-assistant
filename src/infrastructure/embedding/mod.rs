pub mod openai;

pub use openai::OpenAiEmbedder;

use async_trait::async_trait;

use crate::error::AppError;

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError>;
}
