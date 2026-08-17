pub mod qdrant;

pub use qdrant::QdrantVectorStore;

use async_trait::async_trait;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct EmbeddingPoint {
    pub hadith_id: i64,
    pub vector: Vec<f32>,
    pub collection: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorMatch {
    pub hadith_id: i64,
    pub score: f32,
}

#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn ensure_collection(&self, vector_size: u64) -> Result<(), AppError>;
    async fn upsert(&self, points: Vec<EmbeddingPoint>) -> Result<(), AppError>;
    /// Which of `hadith_ids` already have a point in the index.
    ///
    /// Lets an import skip paying for embeddings it already holds, and lets it
    /// close a gap left by an interrupted run — a record can be in Postgres but
    /// missing from the index.
    async fn existing_ids(
        &self,
        hadith_ids: &[i64],
    ) -> Result<std::collections::HashSet<i64>, AppError>;
    async fn search(
        &self,
        vector: Vec<f32>,
        collection_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<VectorMatch>, AppError>;
}
