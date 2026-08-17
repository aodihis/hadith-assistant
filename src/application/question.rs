use std::sync::Arc;

use crate::application::{Answer, AnswerService, RetrievalService};
use crate::domain::{RetrievalQuery, RetrievedHadith};
use crate::error::AppError;

/// A retrieval-grounded answer together with the canonical records it was
/// generated from.
///
/// `answer` is `None` whenever generation is unavailable (no configured API
/// key), the provider failed, or the model returned unusable output. The
/// citations are returned either way, so a caller never receives generated
/// text without its sources and never receives a fabricated answer in place
/// of a missing one.
#[derive(Debug)]
pub struct AnsweredQuestion {
    pub query: String,
    pub answer: Option<Answer>,
    pub citations: Vec<RetrievedHadith>,
}

/// Coordinates the two stages behind a grounded answer: candidate retrieval,
/// then generation constrained to the retrieved records.
pub struct QuestionService {
    retrieval: Arc<RetrievalService>,
    answers: Arc<AnswerService>,
}

impl QuestionService {
    pub fn new(retrieval: Arc<RetrievalService>, answers: Arc<AnswerService>) -> Self {
        Self { retrieval, answers }
    }

    pub async fn ask(&self, query: RetrievalQuery) -> Result<AnsweredQuestion, AppError> {
        let retrieved = self.retrieval.retrieve(query).await?;

        let answer = self
            .answers
            .generate(&retrieved.query, &retrieved.results)
            .await;

        Ok(AnsweredQuestion {
            query: retrieved.query,
            answer,
            citations: retrieved.results,
        })
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::infrastructure::completion::{ChatCompleter, ChatMessage, CompletionOptions};
    use crate::infrastructure::embedding::Embedder;
    use crate::infrastructure::persistence::hadiths::HadithRepository;
    use crate::infrastructure::vector::{EmbeddingPoint, VectorMatch, VectorStore};

    struct FakeEmbedder;

    #[async_trait]
    impl Embedder for FakeEmbedder {
        async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
            Ok(texts.iter().map(|_| vec![0.1, 0.2, 0.3]).collect())
        }
    }

    struct EmptyVectorStore;

    #[async_trait]
    impl VectorStore for EmptyVectorStore {
        async fn ensure_collection(&self, _vector_size: u64) -> Result<(), AppError> {
            Ok(())
        }

        async fn upsert(&self, _points: Vec<EmbeddingPoint>) -> Result<(), AppError> {
            Ok(())
        }

        async fn search(
            &self,
            _vector: Vec<f32>,
            _collection_filter: Option<&str>,
            _limit: i64,
        ) -> Result<Vec<VectorMatch>, AppError> {
            Ok(vec![])
        }
    }

    struct PanicsIfCalledCompleter;

    #[async_trait]
    impl ChatCompleter for PanicsIfCalledCompleter {
        async fn complete_messages(
            &self,
            _messages: &[ChatMessage],
            _options: CompletionOptions,
        ) -> Result<String, AppError> {
            panic!("completer should not be called");
        }
    }

    fn test_repository() -> HadithRepository {
        use sqlx::postgres::PgPoolOptions;

        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/hadiths")
            .expect("test database URL should parse");

        HadithRepository::new(pool)
    }

    fn service_with_no_matches() -> QuestionService {
        let retrieval = Arc::new(RetrievalService::new(
            Arc::new(FakeEmbedder),
            Arc::new(EmptyVectorStore),
            test_repository(),
        ));
        let answers = Arc::new(AnswerService::new(
            Arc::new(PanicsIfCalledCompleter),
            true,
            CompletionOptions::new(0.3, 400),
        ));

        QuestionService::new(retrieval, answers)
    }

    #[tokio::test]
    async fn ask_returns_no_answer_and_no_citations_when_retrieval_finds_nothing() {
        let service = service_with_no_matches();

        let answered = service
            .ask(RetrievalQuery {
                query: "a question with no matching narrations".to_owned(),
                collection: None,
                limit: 0,
            })
            .await
            .expect("retrieval returning nothing is not an error");

        assert_eq!(answered.query, "a question with no matching narrations");
        assert_eq!(answered.answer, None);
        assert!(answered.citations.is_empty());
    }
}
