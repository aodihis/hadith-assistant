use std::sync::Arc;

use crate::application::RetrievalService;
use crate::application::chat::{
    ChatReply, ConversationHistory, ConversationTurn, HistoryLimits, build_compaction_messages,
    build_messages, compose_retrieval_query, needs_compaction, split_for_compaction,
};
use crate::domain::{RetrievalQuery, RetrievedHadith};
use crate::error::AppError;
use crate::infrastructure::completion::{
    ChatCompleter, ChatMessage, CompletionOptions, CompletionStream,
};

/// How long a composed retrieval query may get before it is truncated.
const MAX_RETRIEVAL_QUERY_CHARS: usize = 400;

pub struct ConversationConfig {
    pub limits: HistoryLimits,
    pub answer_options: CompletionOptions,
    pub summary_options: CompletionOptions,
    pub retrieval_limit: i64,
}

/// A turn that has been retrieved for but not yet generated.
///
/// Citations are held here rather than emitted, because whether the turn is an
/// answer or a refusal is only known once the model's first line arrives — and
/// a refusal must carry none.
pub struct PreparedTurn {
    pub question: String,
    pub citations: Vec<RetrievedHadith>,
    messages: Vec<ChatMessage>,
}

/// Coordinates one conversational turn: retrieve, generate, then compact.
///
/// Split into `prepare` and `finish` so the transport can stream the middle.
/// Compaction has to happen after generation completes, which in a stream means
/// after the last delta.
pub struct ConversationService {
    retrieval: Arc<RetrievalService>,
    completer: Arc<dyn ChatCompleter>,
    /// Compaction runs on its own handle so a recap — internal notes the reader
    /// never sees — can be produced by a cheaper model than the answer.
    summariser: Arc<dyn ChatCompleter>,
    config: ConversationConfig,
    has_api_key: bool,
}

impl ConversationService {
    pub fn new(
        retrieval: Arc<RetrievalService>,
        completer: Arc<dyn ChatCompleter>,
        summariser: Arc<dyn ChatCompleter>,
        config: ConversationConfig,
        has_api_key: bool,
    ) -> Self {
        Self {
            retrieval,
            completer,
            summariser,
            config,
            has_api_key,
        }
    }

    pub fn limits(&self) -> &HistoryLimits {
        &self.config.limits
    }

    /// Validates the request and retrieves candidates for it.
    pub async fn prepare(
        &self,
        message: &str,
        collection: Option<String>,
        history: &ConversationHistory,
    ) -> Result<PreparedTurn, AppError> {
        let question = message.trim().to_owned();
        if question.is_empty() {
            return Err(AppError::Validation("a question is required".to_owned()));
        }
        if question.chars().count() > self.config.limits.max_question_chars {
            return Err(AppError::Validation(format!(
                "a question may not exceed {} characters",
                self.config.limits.max_question_chars
            )));
        }

        history.validate(&self.config.limits)?;

        if !self.has_api_key {
            return Err(AppError::NotImplemented(
                "answer generation is not configured".to_owned(),
            ));
        }

        let retrieval_query =
            compose_retrieval_query(history, &question, MAX_RETRIEVAL_QUERY_CHARS);

        let retrieved = self
            .retrieval
            .retrieve(RetrievalQuery {
                query: retrieval_query,
                collection,
                limit: self.config.retrieval_limit,
            })
            .await?;

        let messages = build_messages(history, &question, &retrieved.results);

        Ok(PreparedTurn {
            question,
            citations: retrieved.results,
            messages,
        })
    }

    /// Starts generation for a prepared turn.
    pub async fn stream(&self, turn: &PreparedTurn) -> Result<CompletionStream, AppError> {
        self.completer
            .stream_messages(&turn.messages, self.config.answer_options)
            .await
    }

    /// Appends the completed turn and compacts the history if it has outgrown
    /// its budget.
    ///
    /// Compaction runs *after* the user already has their answer, so a failure
    /// here must never fail the turn. On failure it falls back to dropping the
    /// oldest turns and reports `compacted: false`.
    pub async fn finish(
        &self,
        mut history: ConversationHistory,
        question: String,
        reply: &ChatReply,
    ) -> (ConversationHistory, bool) {
        let (answer, refused) = match reply {
            ChatReply::Answered(answer) => (answer.answer.clone(), false),
            ChatReply::Refused { message, .. } => (message.clone(), true),
        };

        history.turns.push(ConversationTurn {
            question,
            answer,
            refused,
        });

        if !needs_compaction(&history, &self.config.limits) {
            return (history, false);
        }

        let (folded, kept) = split_for_compaction(&history, &self.config.limits);
        if folded.is_empty() {
            return (history, false);
        }

        let messages = build_compaction_messages(history.summary.as_deref(), &folded);

        match self
            .summariser
            .complete_messages(&messages, self.config.summary_options)
            .await
        {
            Ok(summary) if !summary.trim().is_empty() => (
                ConversationHistory {
                    summary: Some(summary.trim().to_owned()),
                    summarized_turns: history.summarized_turns + folded.len(),
                    turns: kept,
                },
                true,
            ),
            Ok(_) | Err(_) => {
                // Deterministic fallback: drop the oldest turns and keep the
                // previous recap unchanged. The user's answer is already
                // delivered; losing older context is far better than failing.
                tracing::warn!("history compaction produced no usable summary, truncating instead");
                (
                    ConversationHistory {
                        summary: history.summary,
                        summarized_turns: history.summarized_turns + folded.len(),
                        turns: kept,
                    },
                    false,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::application::Answer;

    struct ScriptedCompleter {
        responses: Mutex<Vec<Result<String, ()>>>,
        calls: Mutex<usize>,
    }

    impl ScriptedCompleter {
        fn new(responses: Vec<Result<String, ()>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                calls: Mutex::new(0),
            }
        }

        fn calls(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl ChatCompleter for ScriptedCompleter {
        async fn complete_messages(
            &self,
            _messages: &[ChatMessage],
            _options: CompletionOptions,
        ) -> Result<String, AppError> {
            *self.calls.lock().unwrap() += 1;
            let mut responses = self.responses.lock().unwrap();
            match responses.is_empty() {
                true => Err(AppError::Internal("no scripted response".to_owned())),
                false => responses
                    .remove(0)
                    .map_err(|()| AppError::Internal("scripted failure".to_owned())),
            }
        }
    }

    fn limits() -> HistoryLimits {
        HistoryLimits {
            max_question_chars: 1_000,
            max_answer_chars: 4_000,
            max_summary_chars: 4_000,
            max_turns: 12,
            compact_after_turns: 4,
            keep_turns: 2,
            max_history_chars: 6_000,
        }
    }

    fn service(completer: Arc<dyn ChatCompleter>) -> ConversationService {
        use crate::infrastructure::persistence::hadiths::HadithRepository;
        use crate::infrastructure::persistence::narrators::NarratorRepository;
        use sqlx::postgres::PgPoolOptions;

        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/hadiths")
            .expect("test database URL should parse");

        // Retrieval is never exercised by these tests; they cover the finish
        // half of a turn, which is pure apart from the summarizer call.
        let retrieval = Arc::new(RetrievalService::new(
            Arc::new(NoopEmbedder),
            Arc::new(NoopVectorStore),
            HadithRepository::new(pool.clone()),
            NarratorRepository::new(pool),
            0.0,
        ));

        ConversationService::new(
            retrieval,
            completer.clone(),
            // These tests script the summarizer, so both handles are the same
            // fake — matching the single-handle behaviour they were written
            // against.
            completer,
            ConversationConfig {
                limits: limits(),
                answer_options: CompletionOptions::new(0.3, 400),
                summary_options: CompletionOptions::new(0.1, 300),
                retrieval_limit: 5,
            },
            true,
        )
    }

    struct NoopEmbedder;

    #[async_trait]
    impl crate::infrastructure::embedding::Embedder for NoopEmbedder {
        async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
            Ok(texts.iter().map(|_| vec![0.0]).collect())
        }
    }

    struct NoopVectorStore;

    #[async_trait]
    impl crate::infrastructure::vector::VectorStore for NoopVectorStore {
        async fn ensure_collection(&self, _vector_size: u64) -> Result<(), AppError> {
            Ok(())
        }
        async fn upsert(
            &self,
            _points: Vec<crate::infrastructure::vector::EmbeddingPoint>,
        ) -> Result<(), AppError> {
            Ok(())
        }
        async fn existing_ids(
            &self,
            _hadith_ids: &[i64],
        ) -> Result<std::collections::HashSet<i64>, AppError> {
            Ok(std::collections::HashSet::new())
        }
        async fn search(
            &self,
            _vector: Vec<f32>,
            _collection_filter: Option<&str>,
            _limit: i64,
        ) -> Result<Vec<crate::infrastructure::vector::VectorMatch>, AppError> {
            unreachable!("retrieval is not exercised in these tests")
        }
    }

    fn answered(text: &str) -> ChatReply {
        ChatReply::Answered(Answer {
            title: "T".to_owned(),
            answer: text.to_owned(),
        })
    }

    fn history_of(count: usize) -> ConversationHistory {
        ConversationHistory {
            summary: None,
            summarized_turns: 0,
            turns: (0..count)
                .map(|i| ConversationTurn {
                    question: format!("q{i}"),
                    answer: format!("a{i}"),
                    refused: false,
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn a_short_history_is_not_compacted_and_costs_no_extra_call() {
        let completer = Arc::new(ScriptedCompleter::new(vec![]));
        let service = service(completer.clone());

        let (history, compacted) = service
            .finish(history_of(1), "q".to_owned(), &answered("a"))
            .await;

        assert!(!compacted);
        assert_eq!(history.turns.len(), 2);
        assert_eq!(
            completer.calls(),
            0,
            "no summarizer call may be made below the threshold"
        );
    }

    #[tokio::test]
    async fn an_overlong_history_is_compacted_into_a_summary() {
        let completer = Arc::new(ScriptedCompleter::new(vec![Ok(
            "The user is asking about fasting.".to_owned(),
        )]));
        let service = service(completer.clone());

        let (history, compacted) = service
            .finish(history_of(4), "q4".to_owned(), &answered("a4"))
            .await;

        assert!(compacted);
        assert_eq!(completer.calls(), 1);
        assert_eq!(
            history.summary.as_deref(),
            Some("The user is asking about fasting.")
        );
        assert_eq!(history.turns.len(), 2, "only keep_turns survive verbatim");
        assert_eq!(history.summarized_turns, 3);
    }

    #[tokio::test]
    async fn a_failed_compaction_still_returns_the_turn() {
        let completer = Arc::new(ScriptedCompleter::new(vec![Err(())]));
        let service = service(completer.clone());

        let (history, compacted) = service
            .finish(history_of(4), "q4".to_owned(), &answered("a4"))
            .await;

        assert!(
            !compacted,
            "a failed summarizer must be reported honestly, not as a success"
        );
        assert_eq!(
            history.turns.len(),
            2,
            "history still shrinks, so the next turn is not oversized"
        );
        assert_eq!(history.summary, None);
    }

    #[tokio::test]
    async fn a_refused_turn_is_recorded_as_refused() {
        let completer = Arc::new(ScriptedCompleter::new(vec![]));
        let service = service(completer);

        let (history, _) = service
            .finish(
                ConversationHistory::default(),
                "what is the weather?".to_owned(),
                &ChatReply::Refused {
                    reason: crate::application::RefusalReason::OffTopic,
                    message: "I can only help with hadith.".to_owned(),
                },
            )
            .await;

        assert!(
            history.turns[0].refused,
            "refused turns must be marked so they are not used as retrieval anchors"
        );
    }
}
