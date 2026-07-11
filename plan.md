# Hadith Assistant Backend Plan

## Architecture Decision

Use PostgreSQL as the source of truth for Hadith records.

For the current version, keep the schema intentionally small and close to the
JSON data we have:

```text
PostgreSQL
  collections          collection slugs such as bukhari, muslim, mishkat
  hadiths              one row per Hadith JSON record

External vector database
  Qdrant               vectors keyed back to PostgreSQL hadiths.id or Hadith reference
```

The vector database is an index, not the source of truth. The canonical Arabic
text, English translation, grades, numbering, and chapter metadata live in
PostgreSQL.

## Import Artifact

`HadithTable.sql` was converted into a local JSON artifact at
`data/imports/hadiths.json`.

`data/imports/` is ignored by Git because the dataset comes from an external
source. The app has importer code, but the corpus itself should stay local unless
licensing and provenance are reviewed.

## Status Legend

- `Done`: implemented and checked into the backend.
- `Partial`: boundary exists, but there is a known missing piece.
- `Not started`: no implementation yet.

## Implementation Steps

### 1. Create The PostgreSQL Schema - Done

Implemented in `migrations/0001_create_hadith_schema.sql`.

- Created `collections`.
- Created `hadiths`.
- Kept Hadith fields close to the JSON source.
- Put grades directly on `hadiths`.
- Added nullable `arabic_transliteration`.
- Added `UNIQUE (collection_id, book_number, hadith_number)` for reference
  lookup and retrieval.

Note: the migration file exists, but we have not yet run migrations against a
real database in this workspace.

### 2. Import Local `hadiths.json` - Partial

Implemented:

- JSON parser and validator in `src/import/hadith_json.rs`.
- Import CLI in `src/bin/import_hadiths.rs`.
- Local import folder is ignored by Git.
- Local JSON validation passed for 44,896 records.
- Import docs exist in `docs/import-hadith-json.md`.

Still missing:

- Run the import against a real PostgreSQL database after migrations are set up.
- Add database-backed integration tests for import behavior.

### 3. Add Deterministic Arabic Transliteration - Partial

Implemented:

- Added `simple-readable-v1` transliteration module.
- Added tests for Arabic letters, diacritics, markup preservation, and non-Arabic text.
- JSON import calls the transliteration library and stores
  `hadiths.arabic_transliteration` during insert.
- Added docs in `docs/transliteration.md`.

Still missing:

- Run the importer against a real PostgreSQL database and verify stored
  transliteration values.
- Review transliteration quality on real Hadith samples.
- Decide whether the simple scheme is enough or if a scholarly scheme is needed later.

### 4. Build The Axum Backend Foundation - Done

Implemented:

- Environment config in `src/config.rs`.
- Qdrant vector backend config defaults in `src/config.rs`.
- Shared `AppState` in `src/state.rs`.
- PostgreSQL connection pool startup in `src/main.rs`.
- Application error mapping in `src/error.rs`.
- Health route at `GET /health`.
- API docs in `docs/api.md`.

Note: the server requires `DATABASE_URL`, and it has not been smoke-tested
against a live database in this workspace.

### 5. Add Repository And Service Layers - Done

Implemented:

- Collection repository/service.
- Hadith repository/service.
- Thin Axum route handlers.
- Validation in services before repository calls.
- Unit tests for validation and error behavior.
- HTTP routes are read-only for canonical data because writes happen through
  the CLI import path.

Still missing:

- Database-backed repository integration tests.

### 6. Add Basic Search Before RAG - Partial

Implemented:

- Hadith list filters for collection, book number, Hadith number, and grade.
- Hadith lookup by internal `id`.
- Hadith lookup by reference:
  `GET /hadiths/by-reference/{collection}/{book_number}/{hadith_number}`.

Still missing:

- Full text search over Arabic and English text.
- Citation-specific response DTOs separate from full Hadith rows.
- Integration tests against PostgreSQL.

### 7. Add External Vector Search - Partial

Implemented:

- Retrieval API boundary at `POST /retrieval`.
- Retrieval service exists.
- Service validates request shape.
- Service returns `501 Not Implemented`.
- Missing retrieval behavior is marked with an explicit `TODO`.
- Qdrant is selected as the vector database provider.
- Qdrant config variables are defined:
  `VECTOR_DB_PROVIDER`, `QDRANT_URL`, and `QDRANT_COLLECTION`.

Still missing:

- Define vector record metadata.
- Generate chunks or embedding input outside the canonical schema.
- Store embeddings in Qdrant.
- Resolve vector matches back to Hadith records.
- Add retrieval tests for scope filtering, no-result behavior, and citations.

### 8. Add The Chat/RAG Endpoint - Not Started

Planned:

- Accept a user query.
- Apply validation and collection/language filters.
- Retrieve matching Hadith records through search and vector lookup.
- Return cited references alongside any generated answer.
