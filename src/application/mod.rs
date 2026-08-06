mod answer;
mod collections;
mod hadiths;
mod retrieval;

use std::sync::Arc;

pub use answer::{Answer, AnswerService};
pub use collections::CollectionService;
pub use hadiths::HadithService;
pub use retrieval::RetrievalService;
use sqlx::PgPool;

use crate::config::{ChatConfig, EmbeddingConfig, VectorConfig};
use crate::infrastructure::completion::{ChatCompleter, OpenAiChatClient};
use crate::infrastructure::embedding::{Embedder, OpenAiEmbedder};
use crate::infrastructure::persistence::hadiths::HadithRepository;
use crate::infrastructure::vector::{QdrantVectorStore, VectorStore};

#[derive(Clone)]
pub struct AppServices {
    pub collections: Arc<CollectionService>,
    pub hadiths: Arc<HadithService>,
    pub retrieval: Arc<RetrievalService>,
    pub answers: Arc<AnswerService>,
}

impl AppServices {
    pub fn new(
        pool: PgPool,
        embedding: EmbeddingConfig,
        vector: VectorConfig,
        chat: ChatConfig,
    ) -> Self {
        let hadith_repository = HadithRepository::new(pool.clone());

        let embedder: Arc<dyn Embedder> = Arc::new(OpenAiEmbedder::new(embedding));
        let vector_store: Arc<dyn VectorStore> = Arc::new(
            QdrantVectorStore::new(&vector.qdrant_url, vector.qdrant_collection)
                .expect("QDRANT_URL should be a valid Qdrant endpoint URL"),
        );

        let has_chat_api_key = chat.api_key.is_some();
        let completer: Arc<dyn ChatCompleter> = Arc::new(OpenAiChatClient::new(chat));
        let answers = Arc::new(AnswerService::new(completer, has_chat_api_key));

        Self {
            collections: Arc::new(CollectionService::new(pool.clone())),
            hadiths: Arc::new(HadithService::new(pool)),
            retrieval: Arc::new(RetrievalService::new(
                embedder,
                vector_store,
                hadith_repository,
            )),
            answers,
        }
    }
}
