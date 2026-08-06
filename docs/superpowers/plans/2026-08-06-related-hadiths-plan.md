# Related Hadiths (find_related) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `RetrievalService::find_related(hadith_id, limit)` that returns semantically related hadiths by re-embedding a hadith's own text and querying the vector store again, excluding itself.

**Architecture:** One new method on the existing `RetrievalService`, backed by two pure helper functions (`normalize_related_limit`, `select_related_candidates`) that make the limit-normalization and self-exclusion/truncation logic unit-testable without a live database — the same pure/impure split `retrieve()`/`validate_query()` already use in this file.

**Tech Stack:** Rust, existing `Embedder`/`VectorStore`/`HadithRepository` traits and structs — no new dependencies.

## Global Constraints

- `find_related` propagates errors (`Result<Vec<RetrievedHadith>, AppError>`) — it does not swallow failures into an empty `Vec` the way `AnswerService::generate` does.
- No collection filter on the related-hadiths vector search — it spans all collections.
- Default related-hadiths limit is 3 (`DEFAULT_RELATED_LIMIT`), used when the caller passes `limit <= 0`.
- `find_related` is added to `RetrievalService` in this plan but not called from any route — that's spec 4.

---

### Task 1: `find_related` on `RetrievalService`

**Files:**
- Modify: `src/application/retrieval.rs`

**Interfaces:**
- Consumes: existing `RetrievalService` fields (`embedder: Arc<dyn Embedder>`, `vector_store: Arc<dyn VectorStore>`, `hadiths: HadithRepository`), existing `RetrievedHadith` domain struct, existing `infrastructure::vector::VectorMatch`.
- Produces: `pub async fn find_related(&self, hadith_id: i64, limit: i64) -> Result<Vec<RetrievedHadith>, AppError>` on `RetrievalService` — consumed by a future route (out of scope here, spec 4).

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `src/application/retrieval.rs`
(the existing `use async_trait::async_trait;`, `use super::*;`, `use
crate::infrastructure::vector::{EmbeddingPoint, VectorMatch};`,
`FakeEmbedder`, `FakeVectorStore`, and `test_repository()` all stay as
they are — these tests reuse them):

```rust
    struct PanicsIfCalledEmbedder;

    #[async_trait]
    impl Embedder for PanicsIfCalledEmbedder {
        async fn embed_batch(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
            panic!("embedder should not be called");
        }
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
            VectorMatch { hadith_id: 1, score: 0.99 },
            VectorMatch { hadith_id: 2, score: 0.9 },
            VectorMatch { hadith_id: 3, score: 0.8 },
        ];

        let selected = select_related_candidates(matches, 2, 5);

        assert_eq!(
            selected,
            vec![
                VectorMatch { hadith_id: 1, score: 0.99 },
                VectorMatch { hadith_id: 3, score: 0.8 },
            ]
        );
    }

    #[test]
    fn select_related_candidates_truncates_to_the_limit_after_excluding_the_source() {
        let matches = vec![
            VectorMatch { hadith_id: 10, score: 1.0 },
            VectorMatch { hadith_id: 1, score: 0.95 },
            VectorMatch { hadith_id: 2, score: 0.9 },
            VectorMatch { hadith_id: 3, score: 0.8 },
        ];

        let selected = select_related_candidates(matches, 10, 2);

        assert_eq!(
            selected,
            vec![
                VectorMatch { hadith_id: 1, score: 0.95 },
                VectorMatch { hadith_id: 2, score: 0.9 },
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
                matches: vec![VectorMatch { hadith_id: 999_999, score: 0.9 }],
            }),
            test_repository(),
        );

        let result = service.find_related(1, 3).await;

        assert!(
            result.is_err(),
            "unreachable database should surface as an error, not fabricated results"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib application::retrieval`
Expected: compile errors — `normalize_related_limit`,
`select_related_candidates`, `DEFAULT_RELATED_LIMIT`, and
`RetrievalService::find_related` don't exist yet.

- [ ] **Step 3: Implement**

In `src/application/retrieval.rs`, add the new constant next to the
existing ones:

```rust
const DEFAULT_LIMIT: i64 = 10;
const MAX_LIMIT: i64 = 20;
const DEFAULT_RELATED_LIMIT: i64 = 3;
```

Add the import needed for the pure helper (the file already imports
`crate::domain::{RetrievalQuery, RetrievalResult, RetrievedHadith}` and
`crate::infrastructure::vector::VectorStore` — add `VectorMatch`
alongside it):

```rust
use crate::infrastructure::vector::{VectorMatch, VectorStore};
```

Add `find_related` as a new method on `impl RetrievalService`, right
after the existing `retrieve` method:

```rust
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
```

Add the two pure helper functions after `validate_query` (same file,
module-level, not inside `impl RetrievalService`):

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib application::retrieval`
Expected: all tests pass, including the 5 new ones and the pre-existing
`retrieve`/`validate_query` tests (unaffected).

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --lib`
Expected: all tests pass.

- [ ] **Step 6: Format and lint**

Run: `cargo fmt` then `cargo fmt --check`
Expected: no diff.

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add src/application/retrieval.rs
git commit -m "feat: add RetrievalService::find_related for related hadiths"
```

---

## Self-Review Notes

- **Spec coverage:** limit normalization, self-exclusion via `limit + 1`
  overfetch, no-collection-filter search, stale-match tolerance
  (`NotFound` skip vs. other-error propagation), and the pure-function
  split for testability are all in Task 1's single method — the spec is
  small enough that one task covers it fully, unlike specs 1/2 which
  needed multiple tasks.
- **Out of scope reminder:** no route calls `find_related` yet — that's
  spec 4, which will also need to decide how the UI reacts to an `Err`
  from this method (the spec deliberately left that decision to the
  route, not baked into the service).
- **Type consistency check:** `find_related(hadith_id: i64, limit: i64)
  -> Result<Vec<RetrievedHadith>, AppError>` matches the spec's
  signature exactly; `normalize_related_limit`/`select_related_candidates`
  signatures match between the spec's Testing section and this plan's
  Step 3 implementation.
