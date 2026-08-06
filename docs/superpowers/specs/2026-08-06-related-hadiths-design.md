# Related hadiths (`RetrievalService::find_related`)

## Context

This is spec 3 of 4 building the "Sanad" chat UI (see spec 1's narrator
extraction and spec 2's `AnswerService`). The Sanad design's detail
drawer shows 2-3 "related narrations" per hadith — nothing computes that
today. `RetrievalService` already owns the three dependencies this
needs (an `Embedder`, a `VectorStore`, and a `HadithRepository`), so this
adds a second method to it rather than a new service.

Related hadiths are found by re-embedding the source hadith's own
`arabic_text` and querying the vector store again for its nearest
neighbors, excluding itself. This is the same mechanism `retrieve()`
already uses for a user's question — just querying with a hadith's own
text instead of a typed question.

## `find_related`

New method on `src/application/retrieval.rs`'s `RetrievalService`:

```rust
const DEFAULT_RELATED_LIMIT: i64 = 3;

pub async fn find_related(&self, hadith_id: i64, limit: i64) -> Result<Vec<RetrievedHadith>, AppError>
```

Behavior:

1. `self.hadiths.find_by_id(hadith_id)` resolves the source hadith. A
   nonexistent `hadith_id` surfaces as `AppError::NotFound`, same as
   every other by-ID lookup in this codebase.
2. `limit` defaults to `DEFAULT_RELATED_LIMIT` (3, matching the Sanad
   design's 2-3 related cards) when the caller passes `<= 0`, the same
   normalization pattern `RetrievalQuery.limit` already uses inside
   `retrieve()`.
3. Embeds `source.arabic_text` via `self.embedder.embed_batch(...)`
   (single-element slice, same call shape `retrieve()` uses for the
   query text).
4. Queries `self.vector_store.search(vector, None, limit + 1)` — **no
   collection filter** (search spans all collections; a related
   narration in a different collection is a valid, useful cross-
   reference) and **`limit + 1`**, not `limit`, because re-embedding the
   hadith's own already-indexed text makes that same hadith its own
   nearest neighbor in the vector store, so the top match needs to be
   filterable away without shrinking the result count below `limit`.
5. Walks the matches, skipping any whose `hadith_id` equals the source
   hadith's own ID (the expected self-match), resolving the rest via
   `self.hadiths.find_by_id` and stopping once `limit` results have been
   collected.
6. A vector match resolving to a since-deleted hadith
   (`AppError::NotFound` from `find_by_id`) is logged at `warn` and
   skipped, not a hard failure — this exactly mirrors `retrieve()`'s
   existing tolerance for stale vector-store entries. Any other error
   (embedder failure, vector-store failure, or a non-`NotFound` database
   error while resolving a candidate) propagates as `Err` — **this
   method does not swallow errors into an empty `Vec`**, unlike
   `AnswerService`. Whether a route calling this treats a failure as
   "hide the related-narrations section" is that route's decision (spec
   4), not baked into the service.

## Testing

`HadithRepository` is a concrete struct over `PgPool`, not a trait like
`Embedder`/`VectorStore` — there is no fake for it, so (exactly as
`retrieve()`'s own tests already document) any scenario requiring a
*successful* `find_by_id` needs a live database and isn't unit-tested
here. To keep the self-exclusion/limit/truncation logic genuinely
testable without a database, it's factored into two pure functions
(mirroring how `retrieve()` already separates pure `validate_query` from
the DB/network-touching body):

```rust
fn normalize_related_limit(limit: i64) -> i64 {
    if limit <= 0 { DEFAULT_RELATED_LIMIT } else { limit }
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

`find_related` calls both, then resolves whatever `select_related_candidates`
returns via `find_by_id`. Test coverage:

- `normalize_related_limit`: `0` and negative values → `DEFAULT_RELATED_LIMIT`;
  a positive value passes through unchanged.
- `select_related_candidates`: excludes the source hadith's own ID
  regardless of its position in the input; truncates to `limit`;
  preserves input order otherwise.
- `find_related_resolves_source_hadith_before_embedding` — using a
  panicking fake `Embedder` (mirroring `AnswerService`'s
  `PanicsIfCalledCompleter`) and the existing lazy-pool
  `test_repository()` (no live database), asserts `find_related` returns
  `Err` *and* the fake embedder's panic never fires — proving
  `find_by_id` runs, and short-circuits via `?`, before `embed_batch` is
  ever called.
- `find_related_surfaces_a_database_error_instead_of_fabricating_results` —
  mirrors `retrieve_surfaces_a_database_error_instead_of_fabricating_results`
  exactly: a `FakeVectorStore` with a real candidate match, the lazy-pool
  repository, asserts the overall result is `Err`, not fabricated/partial
  results.

## Out of scope

- Calling `find_related` from any page or API route (spec 4).
- Caching the source hadith's embedding (it's re-computed on every call;
  no reuse of whatever vector is already stored for it in Qdrant, since
  `VectorStore::search` takes a query vector, not a stored-point lookup
  — reusing the stored vector would need a new `VectorStore` method,
  which is unnecessary complexity for a feature invoked interactively
  one hadith at a time).
- Any change to `VectorStore::search`'s signature or `QdrantVectorStore`
  — self-exclusion is handled entirely client-side in `RetrievalService`.
