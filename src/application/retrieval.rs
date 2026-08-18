use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::{
    Hadith, Narrator, NarratorRef, RetrievalQuery, RetrievalResult, RetrievedHadith,
};
use crate::error::AppError;
use crate::infrastructure::embedding::Embedder;
use crate::infrastructure::persistence::hadiths::HadithRepository;
use crate::infrastructure::persistence::narrators::NarratorRepository;
use crate::infrastructure::vector::{VectorMatch, VectorStore};

const DEFAULT_LIMIT: i64 = 10;
const MAX_LIMIT: i64 = 20;
const DEFAULT_RELATED_LIMIT: i64 = 3;

#[derive(Clone)]
pub struct RetrievalService {
    embedder: Arc<dyn Embedder>,
    vector_store: Arc<dyn VectorStore>,
    hadiths: HadithRepository,
    narrators: NarratorRepository,
    /// Matches below this cosine score are discarded as irrelevant.
    min_score: f64,
}

impl RetrievalService {
    pub fn new(
        embedder: Arc<dyn Embedder>,
        vector_store: Arc<dyn VectorStore>,
        hadiths: HadithRepository,
        narrators: NarratorRepository,
        min_score: f64,
    ) -> Self {
        Self {
            embedder,
            vector_store,
            hadiths,
            narrators,
            min_score,
        }
    }

    /// Hydrates vector matches into canonical records in two queries rather
    /// than one per hit, then restores the vector ranking that the batch
    /// lookups discard.
    async fn resolve_matches(
        &self,
        matches: Vec<VectorMatch>,
    ) -> Result<Vec<RetrievedHadith>, AppError> {
        if matches.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<i64> = matches
            .iter()
            .map(|candidate| candidate.hadith_id)
            .collect();
        // Neither query needs the other's result, and this runs on every chat
        // turn, so paying two round trips in series is a round trip wasted.
        let (hadiths, narrators) = tokio::try_join!(
            self.hadiths.find_by_ids(&ids),
            self.narrators.find_primary_by_hadith_ids(&ids)
        )?;

        Ok(assemble(matches, hadiths, narrators))
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

        // Weak matches are dropped rather than shown. A narration that merely
        // shares vocabulary with the question is worse than no answer here,
        // because the model is instructed to ground its reply in whatever it
        // is handed.
        let matches = filter_by_score(matches, self.min_score);

        let results = self.resolve_matches(matches).await?;

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

        self.resolve_matches(candidates).await
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

/// Joins vector matches to their canonical records.
///
/// Iterates `matches` rather than `hadiths` because the batch lookup returns
/// rows in database order — walking the matches is what preserves the vector
/// ranking. Scores come from the match, never recomputed. A match whose record
/// no longer exists is dropped and logged rather than fabricated, and dropping
/// it does not disturb the order of the rest.
fn assemble(
    matches: Vec<VectorMatch>,
    hadiths: Vec<Hadith>,
    mut narrators: HashMap<i64, Narrator>,
) -> Vec<RetrievedHadith> {
    let mut by_id: HashMap<i64, Hadith> = hadiths
        .into_iter()
        .map(|hadith| (hadith.id, hadith))
        .collect();

    let mut results = Vec::with_capacity(matches.len());
    for candidate in matches {
        let Some(hadith) = by_id.remove(&candidate.hadith_id) else {
            tracing::warn!(
                hadith_id = candidate.hadith_id,
                "retrieval candidate no longer resolves to a canonical record"
            );
            continue;
        };

        results.push(RetrievedHadith {
            hadith_id: hadith.id,
            collection: hadith.collection,
            book_number: hadith.book_number,
            hadith_number: hadith.hadith_number,
            arabic_text: hadith.arabic_text,
            english_text: hadith.english_text,
            arabic_grade: hadith.arabic_grade,
            english_grade: hadith.english_grade,
            narrator: narrators
                .remove(&candidate.hadith_id)
                .map(|narrator| NarratorRef {
                    name: narrator.name,
                    role: narrator.role,
                }),
            score: Some(candidate.score as f64),
        });
    }

    results
}

fn filter_by_score(matches: Vec<VectorMatch>, min_score: f64) -> Vec<VectorMatch> {
    let before = matches.len();
    let kept: Vec<VectorMatch> = matches
        .into_iter()
        .filter(|candidate| f64::from(candidate.score) >= min_score)
        .collect();

    if kept.len() < before {
        tracing::debug!(
            dropped = before - kept.len(),
            min_score,
            "discarded retrieval matches below the relevance threshold"
        );
    }

    kept
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

    fn test_repositories() -> (HadithRepository, NarratorRepository) {
        use sqlx::postgres::PgPoolOptions;

        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/hadiths")
            .expect("test database URL should parse");

        (
            HadithRepository::new(pool.clone()),
            NarratorRepository::new(pool),
        )
    }

    fn test_service(
        embedder: Arc<dyn Embedder>,
        vector_store: Arc<dyn VectorStore>,
    ) -> RetrievalService {
        let (hadiths, narrators) = test_repositories();
        RetrievalService::new(embedder, vector_store, hadiths, narrators, 0.0)
    }

    #[tokio::test]
    async fn retrieve_returns_validation_error_for_empty_query() {
        let service = test_service(
            Arc::new(FakeEmbedder),
            Arc::new(FakeVectorStore { matches: vec![] }),
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
        let service = test_service(
            Arc::new(FakeEmbedder),
            Arc::new(FakeVectorStore {
                matches: vec![VectorMatch {
                    hadith_id: 999_999,
                    score: 0.9,
                }],
            }),
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

    fn hadith_row(id: i64) -> Hadith {
        Hadith {
            id,
            collection_id: 1,
            collection: "bukhari".to_owned(),
            book_number: "1".to_owned(),
            bab_id: 1.0,
            english_bab_number: None,
            arabic_bab_number: None,
            hadith_number: id.to_string(),
            our_hadith_number: id as i32,
            arabic_urn: id,
            arabic_bab_name: None,
            arabic_text: format!("arabic {id}"),
            arabic_transliteration: None,
            arabic_grade: "صحيح".to_owned(),
            english_urn: id,
            english_bab_name: None,
            english_text: Some(format!("english {id}")),
            english_grade: "Sahih".to_owned(),
            last_updated: None,
            xrefs: String::new(),
        }
    }

    fn narrator_row(hadith_id: i64, name: &str) -> Narrator {
        Narrator {
            id: hadith_id,
            hadith_id,
            external_id: None,
            role: "sahabi".to_owned(),
            name: name.to_owned(),
            position: 0,
        }
    }

    fn vector_match(hadith_id: i64, score: f32) -> VectorMatch {
        VectorMatch { hadith_id, score }
    }

    #[test]
    fn matches_below_the_threshold_are_discarded() {
        let matches = vec![
            vector_match(1, 0.62),
            vector_match(2, 0.51),
            vector_match(3, 0.44),
        ];

        let kept = filter_by_score(matches, 0.5);

        assert_eq!(
            kept.iter().map(|m| m.hadith_id).collect::<Vec<_>>(),
            vec![1, 2],
            "a narration that merely shares vocabulary is worse than no answer"
        );
    }

    #[test]
    fn a_threshold_above_every_score_yields_nothing_rather_than_a_best_effort() {
        // Deliberate: retrieval reports honestly that it found nothing relevant
        // instead of handing the model its least-bad guess to ground an answer in.
        let matches = vec![vector_match(1, 0.66), vector_match(2, 0.52)];

        assert!(filter_by_score(matches, 0.7).is_empty());
    }

    #[test]
    fn a_zero_threshold_keeps_everything() {
        let matches = vec![vector_match(1, 0.2), vector_match(2, 0.01)];

        assert_eq!(filter_by_score(matches, 0.0).len(), 2);
    }

    #[test]
    fn assemble_preserves_vector_rank_even_though_the_batch_lookup_returns_id_order() {
        // The vector ranks 30 first, then 10, then 20 — but find_by_ids ends in
        // ORDER BY h.id, so relying on the row order would silently reorder
        // results by id and destroy the ranking.
        let matches = vec![
            vector_match(30, 0.91),
            vector_match(10, 0.75),
            vector_match(20, 0.60),
        ];
        let hadiths = vec![hadith_row(10), hadith_row(20), hadith_row(30)];

        let results = assemble(matches, hadiths, HashMap::new());

        assert_eq!(
            results
                .iter()
                .map(|hadith| hadith.hadith_id)
                .collect::<Vec<_>>(),
            vec![30, 10, 20]
        );
        assert_eq!(results[0].score, Some(0.91f32 as f64));
    }

    #[test]
    fn assemble_skips_a_candidate_with_no_canonical_record_without_shifting_the_rest() {
        let matches = vec![
            vector_match(10, 0.9),
            vector_match(999, 0.8),
            vector_match(20, 0.7),
        ];
        let hadiths = vec![hadith_row(10), hadith_row(20)];

        let results = assemble(matches, hadiths, HashMap::new());

        assert_eq!(
            results
                .iter()
                .map(|hadith| hadith.hadith_id)
                .collect::<Vec<_>>(),
            vec![10, 20],
            "the unresolvable candidate is dropped, never fabricated"
        );
    }

    #[test]
    fn assemble_carries_grades_verbatim_and_attaches_the_primary_narrator() {
        let matches = vec![vector_match(10, 0.9), vector_match(20, 0.8)];
        let hadiths = vec![hadith_row(10), hadith_row(20)];
        let narrators = HashMap::from([(10, narrator_row(10, "Umar ibn al-Khattab"))]);

        let results = assemble(matches, hadiths, narrators);

        assert_eq!(results[0].english_grade, "Sahih");
        assert_eq!(results[0].arabic_grade, "صحيح");
        let narrator = results[0]
            .narrator
            .as_ref()
            .expect("hadith 10 has a narrator row");
        assert_eq!(narrator.name, "Umar ibn al-Khattab");
        assert_eq!(narrator.role, "sahabi");
        assert!(
            results[1].narrator.is_none(),
            "a hadith with no narrator row reports None rather than a placeholder"
        );
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
        let service = test_service(
            Arc::new(PanicsIfCalledEmbedder),
            Arc::new(FakeVectorStore { matches: vec![] }),
        );

        let result = service.find_related(1, 3).await;

        assert!(
            result.is_err(),
            "unreachable database should surface as an error"
        );
    }

    #[tokio::test]
    async fn find_related_surfaces_a_database_error_instead_of_fabricating_results() {
        let service = test_service(
            Arc::new(FakeEmbedder),
            Arc::new(FakeVectorStore {
                matches: vec![VectorMatch {
                    hadith_id: 999_999,
                    score: 0.9,
                }],
            }),
        );

        let result = service.find_related(1, 3).await;

        assert!(
            result.is_err(),
            "unreachable database should surface as an error, not fabricated results"
        );
    }
}
