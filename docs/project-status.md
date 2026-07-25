# Project status

## Architecture decision

The application is a single Topcoat `0.4.x` Cargo package. Topcoat serves both
server-rendered pages and the JSON API. PostgreSQL is the source of truth for
Hadith content; Qdrant is a replaceable retrieval index.

```text
browser or API client
  -> Topcoat page / route
  -> application service
  -> SQLx repository
  -> PostgreSQL canonical record

future retrieval request
  -> retrieval policy and scope
  -> Qdrant candidates
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
- Docker Compose dependencies and a production image containing the binary and
  its Topcoat asset bundle.

## Deliberately incomplete

- The Qdrant retrieval pipeline returns `501 Not Implemented`.
- Full-text search over Arabic and English content is not implemented.
- The chat/RAG answer endpoint is not implemented.
- Database-backed integration tests are not yet available.
- The local corpus is ignored until licensing and provenance are reviewed.

## Next milestones

1. Define versioned retrieval chunks and Qdrant payload metadata.
2. Add an embedding ingestion workflow tied to canonical Hadith IDs.
3. Implement scoped candidate retrieval and canonical-record resolution.
4. Return citations and trace information from retrieval.
5. Add database integration fixtures for repositories and ingestion.
6. Add a chat use case only after citation-preserving retrieval is tested.

Topcoat is expected to change rapidly. Framework upgrades should remain isolated
to the `app` layer, startup wiring, and asset build process whenever possible.
