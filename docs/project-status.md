# Project status

## Architecture decision

The application is a single Topcoat `0.5.x` Cargo package. Topcoat serves both
server-rendered pages and the JSON API. PostgreSQL is the source of truth for
Hadith content; Qdrant is a replaceable retrieval index.

```text
browser or API client
  -> Topcoat page / route
  -> application service
  -> SQLx repository
  -> PostgreSQL canonical record

retrieval request
  -> retrieval policy and scope
  -> embed query (Embedder trait, OpenAI-compatible by default)
  -> Qdrant candidates (VectorStore trait, Qdrant by default)
  -> PostgreSQL canonical resolution
  -> cited context
```

## Implemented

- Topcoat root layout, home page, Hadith browser, asset bundle, and module-based
  routing.
- JSON API under `/api` with typed path, query, and body parsing.
- Stable JSON error codes without leaked database details.
- Typed application context containing shared services.
- PostgreSQL schema and automatic startup migrations.
- Collection and Hadith repositories and services.
- Hadith filters and lookup by internal ID or published reference.
- JSON import CLI with validation, one-transaction import, source checksum, and
  deterministic Arabic transliteration.
- Qdrant retrieval: embed a query through a swappable `Embedder`, search a
  swappable `VectorStore`, resolve every hit back to its canonical Hadith row.
  Hadiths are embedded via `import_hadiths --embed`.
- Docker Compose dependencies and a production image containing the binary and
  its Topcoat asset bundle.

## Deliberately incomplete

- Full-text search over Arabic and English content is not implemented.
- The chat/RAG answer endpoint is not implemented.
- Database-backed integration tests are not yet available.
- The local corpus is ignored until licensing and provenance are reviewed.

## Next milestones

1. Add database-backed integration fixtures for repositories, ingestion, and
   retrieval, replacing the fake-double unit tests where a real database and
   Qdrant instance can assert on end-to-end behavior.
2. Full-text search over Arabic and English content, likely PostgreSQL-native
   (`tsvector`), as a complement to vector retrieval rather than a
   replacement.
3. Commentary/explanation corpus: a future corpus of Hadith commentary (sharh)
   books, embedded and retrievable the same way as Hadith text, to provide
   explanatory context alongside or instead of raw citations. Reusable
   through the existing `Embedder`/`VectorStore` traits, since collection
   name and source text are already parameters rather than hardcoded
   assumptions — implementation will decide between a second Qdrant
   collection and a payload `kind` discriminator when that work starts.
4. LLM-agnostic chat/RAG endpoint, built only after retrieval is proven in
   production use, per the sequencing already established in `AGENTS.md`.
   Should follow the same base-URL/API-key/model-configurable pattern as
   `Embedder` so DeepSeek, OpenRouter, or other OpenAI-compatible chat
   backends can be swapped without rewriting the use case.

Topcoat is expected to change rapidly. Framework upgrades should remain isolated
to the `app` layer, startup wiring, and asset build process whenever possible.
