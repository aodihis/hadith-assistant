use std::sync::Arc;

use crate::domain::{RetrievalQuery, RetrievalResult, RetrievedHadith};
use crate::error::AppError;
use crate::infrastructure::embedding::Embedder;
use crate::infrastructure::persistence::hadiths::HadithRepository;
use crate::infrastructure::vector::{VectorMatch, VectorStore};

const DEFAULT_LIMIT: i64 = 10;
const MAX_LIMIT: i64 = 20;
const DEFAULT_RELATED_LIMIT: i64 = 3;

#[derive(Clone)]
pub struct RetrievalService {
    embedder: Arc<dyn Embedder>,
    vector_store: Arc<dyn VectorStore>,
    hadiths: HadithRepository,
}

impl RetrievalService {
    pub fn new(
        embedder: Arc<dyn Embedder>,
        vector_store: Arc<dyn VectorStore>,
        hadiths: HadithRepository,
    ) -> Self {
        Self {
            embedder,
            vector_store,
            hadiths,
        }
    }

    pub async fn retrieve(&self, query: RetrievalQuery) -> Result<RetrievalResult, AppError> {
        let query = validate_query(query)?;

        let mut vectors = self
            .embedder
            .embed_batch(std::slice::from_ref(&query.query))
            .await?;
        let vector = vectors.pop().ok_or_else(|| {
            AppError::Internal("embedding provider returned no vector for the query".to_owned())
        })?;

        let matches = self
            .vector_store
            .search(vector, query.collection.as_deref(), query.limit)
            .await?;

        let mut results = Vec::with_capacity(matches.len());
        for candidate in matches {
            match self.hadiths.find_by_id(candidate.hadith_id).await {
                Ok(hadith) => results.push(RetrievedHadith {
                    hadith_id: hadith.id,
                    collection: hadith.collection,
                    book_number: hadith.book_number,
                    hadith_number: hadith.hadith_number,
                    arabic_text: hadith.arabic_text,
                    english_text: hadith.english_text,
                    score: Some(candidate.score as f64),
                }),
                Err(AppError::NotFound(_)) => {
                    tracing::warn!(
                        hadith_id = candidate.hadith_id,
                        "retrieval candidate no longer resolves to a canonical record"
                    );
                }
                Err(error) => return Err(error),
            }
        }

        Ok(RetrievalResult {
            query: query.query,
            results,
        })
    }

    pub async fn find_related(
        &self,
        hadith_id: i64,
        limit: i64,
    ) -> Result<Vec<RetrievedHadith>, AppError> {
        let source = self.hadiths.find_by_id(hadith_id).await?;
        let limit = normalize_related_limit(limit);

        let mut vectors = self
            .embedder
            .embed_batch(std::slice::from_ref(&source.arabic_text))
            .await?;
        let vector = vectors.pop().ok_or_else(|| {
            AppError::Internal(
                "embedding provider returned no vector for the source hadith".to_owned(),
            )
        })?;

        let matches = self.vector_store.search(vector, None, limit + 1).await?;
        let candidates = select_related_candidates(matches, hadith_id, limit);

        let mut results = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            match self.hadiths.find_by_id(candidate.hadith_id).await {
                Ok(hadith) => results.push(RetrievedHadith {
                    hadith_id: hadith.id,
                    collection: hadith.collection,
                    book_number: hadith.book_number,
                    hadith_number: hadith.hadith_number,
                    arabic_text: hadith.arabic_text,
                    english_text: hadith.english_text,
                    score: Some(candidate.score as f64),
                }),
                Err(AppError::NotFound(_)) => {
                    tracing::warn!(
                        hadith_id = candidate.hadith_id,
                        "related candidate no longer resolves to a canonical record"
                    );
                }
                Err(error) => return Err(error),
            }
        }

        Ok(results)
    }
}

fn validate_query(query: RetrievalQuery) -> Result<RetrievalQuery, AppError> {
    let text = query.query.trim();
    if text.is_empty() {
        return Err(AppError::Validation("query is required".to_owned()));
    }

    let limit = if query.limit == 0 {
        DEFAULT_LIMIT
    } else {
        query.limit
    };

    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(AppError::Validation(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }

    Ok(RetrievalQuery {
        query: text.to_owned(),
        collection: query
            .collection
            .map(|collection| collection.trim().to_owned())
            .filter(|collection| !collection.is_empty()),
        limit,
    })
}

fn normalize_related_limit(limit: i64) -> i64 {
    if limit <= 0 {
        DEFAULT_RELATED_LIMIT
    } else {
        limit
    }
}

fn select_related_candidates(
    matches: Vec<VectorMatch>,
    exclude_hadith_id: i64,
    limit: i64,
) -> Vec<VectorMatch> {
    matches
        .into_iter()
        .filter(|candidate| candidate.hadith_id != exclude_hadith_id)
        .take(limit as usize)
        .collect()
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::infrastructure::vector::{EmbeddingPoint, VectorMatch};

    struct FakeEmbedder;

    #[async_trait]
    impl Embedder for FakeEmbedder {
        async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
            Ok(texts.iter().map(|_| vec![0.1, 0.2, 0.3]).collect())
        }
    }

    struct FakeVectorStore {
        matches: Vec<VectorMatch>,
    }

    #[async_trait]
    impl VectorStore for FakeVectorStore {
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
            Ok(self.matches.clone())
        }
    }

    struct PanicsIfCalledEmbedder;

    #[async_trait]
    impl Embedder for PanicsIfCalledEmbedder {
        async fn embed_batch(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
            panic!("embedder should not be called");
        }
    }

    fn test_repository() -> HadithRepository {
        use sqlx::postgres::PgPoolOptions;

        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/hadiths")
            .expect("test database URL should parse");

        HadithRepository::new(pool)
    }

    #[tokio::test]
    async fn retrieve_returns_validation_error_for_empty_query() {
        let service = RetrievalService::new(
            Arc::new(FakeEmbedder),
            Arc::new(FakeVectorStore { matches: vec![] }),
            test_repository(),
        );

        let error = service
            .retrieve(RetrievalQuery {
                query: "   ".to_owned(),
                collection: None,
                limit: 0,
            })
            .await
            .expect_err("empty query should be invalid");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "query is required"
        ));
    }

    #[tokio::test]
    async fn retrieve_surfaces_a_database_error_instead_of_fabricating_results() {
        let service = RetrievalService::new(
            Arc::new(FakeEmbedder),
            Arc::new(FakeVectorStore {
                matches: vec![VectorMatch {
                    hadith_id: 999_999,
                    score: 0.9,
                }],
            }),
            test_repository(),
        );

        // No live database in this test; find_by_id against a lazy pool with no
        // reachable server surfaces as AppError::Database, not AppError::NotFound,
        // so this test only exercises the validation + embed + search wiring path
        // without asserting on database connectivity. Full end-to-end resolution
        // is exercised manually against `docker compose up -d postgres qdrant`.
        let result = service
            .retrieve(RetrievalQuery {
                query: "intentions".to_owned(),
                collection: None,
                limit: 5,
            })
            .await;

        assert!(
            result.is_err(),
            "unreachable database should surface as an error, not fabricated results"
        );
    }

    #[test]
    fn validate_query_trims_query_and_collection_and_defaults_limit() {
        let query = validate_query(RetrievalQuery {
            query: " intentions ".to_owned(),
            collection: Some(" bukhari ".to_owned()),
            limit: 0,
        })
        .expect("valid query should normalize");

        assert_eq!(query.query, "intentions");
        assert_eq!(query.collection.as_deref(), Some("bukhari"));
        assert_eq!(query.limit, DEFAULT_LIMIT);
    }

    #[test]
    fn validate_query_drops_empty_collection() {
        let query = validate_query(RetrievalQuery {
            query: "intentions".to_owned(),
            collection: Some(" ".to_owned()),
            limit: 3,
        })
        .expect("empty optional collection should be ignored");

        assert_eq!(query.collection, None);
        assert_eq!(query.limit, 3);
    }

    #[test]
    fn validate_query_rejects_empty_query() {
        let error = validate_query(RetrievalQuery {
            query: " ".to_owned(),
            collection: None,
            limit: 1,
        })
        .expect_err("empty query should be invalid");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "query is required"
        ));
    }

    #[test]
    fn validate_query_rejects_out_of_range_limit() {
        let error = validate_query(RetrievalQuery {
            query: "intentions".to_owned(),
            collection: None,
            limit: MAX_LIMIT + 1,
        })
        .expect_err("limit above max should be invalid");

        assert!(matches!(
            error,
            AppError::Validation(message)
                if message == format!("limit must be between 1 and {MAX_LIMIT}")
        ));
    }

    #[test]
    fn normalize_related_limit_defaults_non_positive_values() {
        assert_eq!(normalize_related_limit(0), DEFAULT_RELATED_LIMIT);
        assert_eq!(normalize_related_limit(-5), DEFAULT_RELATED_LIMIT);
        assert_eq!(normalize_related_limit(7), 7);
    }

    #[test]
    fn select_related_candidates_excludes_the_source_hadith_regardless_of_position() {
        let matches = vec![
            VectorMatch {
                hadith_id: 1,
                score: 0.99,
            },
            VectorMatch {
                hadith_id: 2,
                score: 0.9,
            },
            VectorMatch {
                hadith_id: 3,
                score: 0.8,
            },
        ];

        let selected = select_related_candidates(matches, 2, 5);

        assert_eq!(
            selected,
            vec![
                VectorMatch {
                    hadith_id: 1,
                    score: 0.99,
                },
                VectorMatch {
                    hadith_id: 3,
                    score: 0.8,
                },
            ]
        );
    }

    #[test]
    fn select_related_candidates_truncates_to_the_limit_after_excluding_the_source() {
        let matches = vec![
            VectorMatch {
                hadith_id: 10,
                score: 1.0,
            },
            VectorMatch {
                hadith_id: 1,
                score: 0.95,
            },
            VectorMatch {
                hadith_id: 2,
                score: 0.9,
            },
            VectorMatch {
                hadith_id: 3,
                score: 0.8,
            },
        ];

        let selected = select_related_candidates(matches, 10, 2);

        assert_eq!(
            selected,
            vec![
                VectorMatch {
                    hadith_id: 1,
                    score: 0.95,
                },
                VectorMatch {
                    hadith_id: 2,
                    score: 0.9,
                },
            ]
        );
    }

    #[tokio::test]
    async fn find_related_resolves_source_hadith_before_embedding() {
        let service = RetrievalService::new(
            Arc::new(PanicsIfCalledEmbedder),
            Arc::new(FakeVectorStore { matches: vec![] }),
            test_repository(),
        );

        let result = service.find_related(1, 3).await;

        assert!(
            result.is_err(),
            "unreachable database should surface as an error"
        );
    }

    #[tokio::test]
    async fn find_related_surfaces_a_database_error_instead_of_fabricating_results() {
        let service = RetrievalService::new(
            Arc::new(FakeEmbedder),
            Arc::new(FakeVectorStore {
                matches: vec![VectorMatch {
                    hadith_id: 999_999,
                    score: 0.9,
                }],
            }),
            test_repository(),
        );

        let result = service.find_related(1, 3).await;

        assert!(
            result.is_err(),
            "unreachable database should surface as an error, not fabricated results"
        );
    }
}
