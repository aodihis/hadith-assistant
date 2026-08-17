mod answer;
mod chat;
mod collections;
mod conversation;
mod hadiths;
mod question;
mod retrieval;
mod session;

use std::sync::Arc;

pub use answer::{Answer, AnswerService};
pub use chat::{
    CHAT_SYSTEM_PROMPT, COMPACTION_SYSTEM_PROMPT, ChatReply, ConversationHistory, ConversationTurn,
    HistoryLimits, RefusalReason, ReplyAssembler, StreamEvent, build_compaction_messages,
    build_messages, compose_retrieval_query, needs_compaction, parse_chat_reply, render_narrations,
    split_for_compaction,
};
pub use collections::CollectionService;
pub use conversation::{ConversationConfig, ConversationService, PreparedTurn};
pub use hadiths::HadithService;
pub use question::{AnsweredQuestion, QuestionService};
pub use retrieval::RetrievalService;
pub use session::{SessionConfig, SessionId, SessionService};
use sqlx::PgPool;

use crate::config::{ChatConfig, EmbeddingConfig, VectorConfig};
use crate::infrastructure::completion::{ChatCompleter, CompletionOptions, OpenAiChatClient};
use crate::infrastructure::embedding::{Embedder, OpenAiEmbedder};
use crate::infrastructure::persistence::hadiths::HadithRepository;
use crate::infrastructure::persistence::narrators::NarratorRepository;
use crate::infrastructure::vector::{QdrantVectorStore, VectorStore};

#[derive(Clone)]
pub struct AppServices {
    pub collections: Arc<CollectionService>,
    pub hadiths: Arc<HadithService>,
    pub retrieval: Arc<RetrievalService>,
    /// Generation is only reachable through `questions`, which guarantees an
    /// answer never travels without the records it was grounded in.
    pub questions: Arc<QuestionService>,
    pub sessions: Arc<SessionService>,
}

impl AppServices {
    pub fn new(
        pool: PgPool,
        embedding: EmbeddingConfig,
        vector: VectorConfig,
        chat: ChatConfig,
        session: SessionConfig,
    ) -> Self {
        let hadith_repository = HadithRepository::new(pool.clone());
        let narrator_repository = NarratorRepository::new(pool.clone());

        let embedder: Arc<dyn Embedder> = Arc::new(OpenAiEmbedder::new(embedding));
        let vector_store: Arc<dyn VectorStore> = Arc::new(
            QdrantVectorStore::new(&vector.qdrant_url, vector.qdrant_collection)
                .expect("QDRANT_URL should be a valid Qdrant endpoint URL"),
        );

        let has_chat_api_key = chat.api_key.is_some();
        let answer_options = CompletionOptions::new(chat.temperature, chat.max_tokens);
        let completer: Arc<dyn ChatCompleter> = Arc::new(OpenAiChatClient::new(chat));
        let answers = Arc::new(AnswerService::new(
            completer,
            has_chat_api_key,
            answer_options,
        ));

        let retrieval = Arc::new(RetrievalService::new(
            embedder,
            vector_store,
            hadith_repository,
            narrator_repository,
        ));
        let questions = Arc::new(QuestionService::new(retrieval.clone(), answers));

        Self {
            collections: Arc::new(CollectionService::new(pool.clone())),
            hadiths: Arc::new(HadithService::new(pool)),
            retrieval,
            questions,
            sessions: Arc::new(SessionService::new(session)),
        }
    }
}
