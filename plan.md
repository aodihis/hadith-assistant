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
  embeddings           vectors keyed back to PostgreSQL hadiths.id
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

## Implementation Steps

### 1. Create The PostgreSQL Schema

- Create `collections`.
- Create `hadiths`.
- Keep Hadith fields close to the JSON source.
- Put grades directly on `hadiths`.
- Keep `arabic_transliteration` nullable for a later deterministic
  transliteration pass.

### 2. Import Local `hadiths.json`

- Parse and validate `data/imports/hadiths.json`.
- Insert collection slugs into `collections`.
- Insert one `hadiths` row per JSON record.
- Preserve Arabic and English source text without changing it.
- Keep external source data out of Git.

### 3. Add Deterministic Arabic Transliteration

- Choose a simple deterministic transliteration rule set.
- Generate transliteration from `hadiths.arabic_text`.
- Store the result in `hadiths.arabic_transliteration`.
- Do not overwrite `arabic_text`.

### 4. Build The Axum Backend Foundation

- Add configuration parsing.
- Add validated environment settings.
- Add shared `AppState`.
- Add PostgreSQL connection pool.
- Add application error type and HTTP error mapping.
- Add basic health route.

### 5. Add Repository And Service Layers

- Keep SQL inside repository modules.
- Keep HTTP handlers thin.
- Add lookup and search methods for Hadith records.

### 6. Add Basic Search Before RAG

- Support lookup by collection, book number, Hadith number, and grade.
- Add text search where useful.
- Return citation metadata with every result.

### 7. Add External Vector Search

- Chunk Hadith text for embedding outside the canonical schema.
- Store embeddings in the selected external vector database.
- Key vector records back to PostgreSQL `hadiths.id`.
- Treat vector data as rebuildable.

### 8. Add The Chat/RAG Endpoint

- Accept a user query.
- Apply validation and collection/language filters.
- Retrieve matching Hadith records through search and vector lookup.
- Return cited references alongside any generated answer.
