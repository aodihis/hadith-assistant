# Topcoat 0.5.0 upgrade and Qdrant retrieval pipeline

## Context

The project targets Topcoat `0.4.x` and returns `501 Not Implemented` for
`POST /api/retrieval`. Topcoat `0.5.0` is now the latest release and carries
breaking changes. Separately, `docs/project-status.md` lists the Qdrant
retrieval pipeline as the top "next milestone." This spec covers both: the
framework upgrade, and building real retrieval on top of it.

## Part 1 — Topcoat 0.4.0 → 0.5.0

Breaking changes in 0.5.0 that touch this codebase (confirmed against the
GitHub release notes for `v0.5.0`):

1. **Layouts take a rendered `Result`, not a `Slot` future.**
   `src/app.rs`: `root_layout(slot: Slot<'_>) -> Result` becomes
   `root_layout(slot: Result) -> Result`; `(slot.await?)` becomes `(slot?)`;
   the `Slot` import is dropped.
2. **Router errors moved to `router::error`.**
   `src/app/hadiths.rs` imports `bad_request`, `not_found`,
   `internal_server_error` from `topcoat::router::error` instead of
   `topcoat::router`.
3. **`Json` moved to `router::content`.**
   `src/app/api.rs`, `src/app/api/health.rs`, `src/app/api/retrieval.rs`
   import `Json` from `topcoat::router::content` instead of
   `topcoat::router`.

Not affected: `path_param`, `query_params`, `page`, `route`, `layer`,
`asset!`/`Asset` usage (only rendered inside `view!`, which the release notes
confirm is unchanged), `AssetConfig::hosted_at` (unused), `session::Config`
(unused), custom `Route` implementations (none exist), boolean view
attributes (none rendered).

Mechanical changes:

- `Cargo.toml`: `topcoat = "0.5.0"`.
- `Cargo.lock`: regenerated.
- `README.md`: `topcoat-cli --version 0.4.0` → `0.5.0`, and the "targets
  Topcoat `0.4.x`" line updated to `0.5.x`.
- `AGENTS.md`: same version references updated.

No schema, route, or environment variable changes result from this part.

## Part 2 — Qdrant retrieval pipeline

### Storage model

Qdrant stores vectors only. PostgreSQL remains the sole canonical store, per
`AGENTS.md`. A Qdrant point ID is the Hadith's PostgreSQL `id` (cast to
Qdrant's u64 point ID) — no separate embedding-tracking table in Postgres,
so there is nothing that can drift out of sync with the canonical record.
Point payload carries `collection` (the collection slug) so Qdrant can apply
the collection scope filter before ranking, matching the retrieval stage
order in `AGENTS.md` ("scope filters" before "ranking").

Each Hadith is embedded as a single chunk: `arabic_text` and `english_text`
concatenated. One point per Hadith.

### Embedding abstraction

An `Embedder` trait (not a concrete type) is introduced as the one deviation
from this codebase's existing convention of application services holding
concrete infrastructure types directly. This is deliberate: a provider swap
(DeepSeek, OpenRouter, or another OpenAI-compatible host) is an explicit
near-term goal, not a hypothetical one.

```rust
trait Embedder {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError>;
}
```

`OpenAiEmbedder` is the first (and initially only) implementation, calling
any OpenAI-compatible embeddings endpoint over `reqwest`. Configuration is
provider-agnostic rather than OpenAI-specific:

| Variable | Required | Default | Purpose |
| --- | --- | --- | --- |
| `EMBEDDING_BASE_URL` | no | `https://api.openai.com/v1` | Embeddings API base URL |
| `EMBEDDING_API_KEY` | no* | — | Bearer token for the embeddings API |
| `EMBEDDING_MODEL` | no | `text-embedding-3-small` | Embedding model name |

\* Not required at startup — only when retrieval or `import_hadiths --embed`
is actually invoked, consistent with how `VectorConfig` already defaults
rather than hard-failing at startup.

### Ingestion path

`import_hadiths` gains a `--embed` flag. Plain `import_hadiths` behaves
exactly as it does today — no forced external API cost. With `--embed`:

1. The existing transactional import runs unchanged, except the `INSERT`
   gains `RETURNING id` so the newly inserted Hadith IDs are known.
2. After commit, the newly inserted rows are re-fetched by ID.
3. Their combined Arabic+English text is embedded in batches (~96 texts per
   embedding API call, well under typical provider batch limits).
4. Vectors are upserted into Qdrant, creating the collection on first use if
   it does not already exist (dimension taken from the embedding model's
   output size; distance metric is cosine).

### Query path

`RetrievalService::retrieve`:

1. Validate the query (existing logic, unchanged).
2. Embed the query text via `Embedder`.
3. Search Qdrant with the query vector, an optional collection payload
   filter, and the requested limit.
4. Resolve each hit's point ID back to a canonical `Hadith` via the existing
   `HadithRepository`.
5. Build `RetrievedHadith` entries carrying the Qdrant similarity score,
   assemble `RetrievalResult`.

If Qdrant or the embedding provider is unreachable, the service returns
`AppError::Internal`/`AppError::Database`-shaped errors through the existing
stable JSON error envelope — never a fabricated or silently empty result,
per `AGENTS.md`.

### New modules

- `src/infrastructure/embedding/mod.rs` + `openai.rs` — `Embedder` trait and
  `OpenAiEmbedder`.
- `src/infrastructure/vector/mod.rs` + `qdrant.rs` — `QdrantVectorStore`:
  `ensure_collection`, `upsert`, `search`.

`RetrievalService` gains `Arc<dyn Embedder>`, `QdrantVectorStore`, and
`HadithRepository` (or `Arc<HadithService>`) fields, wired through
`AppServices::new`.

### New dependencies

- `qdrant-client = "1.18.0"`
- `reqwest = "0.13.4"` (features: `json`; `rustls-tls` to match the existing
  SQLx TLS backend rather than pulling in a second TLS stack)

### Documentation updates

- `README.md`: new environment variables table entries, retrieval route
  behavior once implemented.
- `docs/api.md`: `POST /api/retrieval` response contract updated from the
  `501` example to a real response shape.
- `docs/project-status.md`: move Qdrant retrieval from "deliberately
  incomplete" to "implemented"; add two new forward-looking milestones:
  - **Commentary/explanation corpus.** A future corpus of Hadith commentary
    (sharh) books, embedded and retrievable the same way, to provide
    explanatory context alongside or instead of raw citations. Reusable
    through the same `Embedder`/`QdrantVectorStore` types since collection
    name and source text are already parameters, not hardcoded assumptions
    — likely a second Qdrant collection or a payload `kind` discriminator,
    decided when that work starts.
  - **LLM-agnostic chat/RAG endpoint.** Built only after this retrieval
    pipeline is tested, per `AGENTS.md`'s existing sequencing rule. Should
    follow the same base-URL/API-key/model-configurable pattern as
    `Embedder` so DeepSeek, OpenRouter, or other OpenAI-compatible chat
    backends can be swapped without rewriting the use case.

## Out of scope for this change

- Full-text search over Arabic/English content.
- The chat/RAG answer endpoint itself.
- Database-backed integration tests.
- The commentary corpus and any multi-provider LLM chat abstraction — noted
  above as documented future milestones only.

## Testing

- Unit tests for `Embedder`/`OpenAiEmbedder` request/response mapping using
  a mocked HTTP transport (no live API calls in CI).
- Unit tests for `QdrantVectorStore` point ID mapping and payload
  construction where feasible without a live Qdrant instance; integration
  behavior documented as manually verified against local `docker compose`
  Qdrant, consistent with the project's current lack of DB-backed
  integration tests.
- `RetrievalService` validation tests (existing) retained; new tests cover
  the success path with fake `Embedder`/store doubles.
- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
  warnings`, `cargo test`, and `topcoat asset bundle --bin hadith-assistant`
  run before handoff, per `AGENTS.md`.
