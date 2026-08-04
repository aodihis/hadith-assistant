# Web and JSON API

Topcoat serves the web interface and JSON API from the same Rust binary.
`DATABASE_URL` is required; the rest of the configuration is documented in the
root [README](../README.md).

Pending SQLx migrations run before the server begins accepting requests.

## Local server

Start dependencies and the Topcoat development server:

```bash
docker compose up -d postgres qdrant
topcoat dev
```

The application listens on <http://127.0.0.1:3000> by default. Set `HOST` and
`PORT` to override the bind address.

## Browser pages

```http
GET /
GET /hadiths
```

`/hadiths` accepts the same filters as the JSON list endpoint and renders
canonical records on the server.

## Error envelope

Application errors use a stable JSON shape:

```json
{
  "code": "validation_error",
  "message": "validation failed: limit must be between 1 and 200"
}
```

Database details and internal stack traces are never returned to clients.

## Health

```http
GET /api/health
```

```json
{
  "status": "ok"
}
```

## Collections

```http
GET /api/collections
GET /api/collections/{slug}
```

## Hadiths

```http
GET /api/hadiths
GET /api/hadiths/{id}
GET /api/hadiths/by-reference/{collection}/{book_number}/{hadith_number}
```

Supported list filters:

```http
GET /api/hadiths?collection=bukhari&book_number=1&hadith_number=1&grade=Sahih&limit=50&offset=0
```

- The default `limit` is `50`.
- The maximum `limit` is `200`.
- `offset` must be zero or greater.

Reference lookup:

```http
GET /api/hadiths/by-reference/bukhari/1/1
```

Reference lookup returns an array because some collections assign the same
published Hadith number to multiple independently sourced records or variants.
The combination of collection, book number, and Hadith number is searchable but
is not a canonical record identifier. Use each result's `id`, `arabic_urn`, or
`english_urn` for record-level traceability.

Canonical data is imported through the CLI, so the HTTP API remains read-only.

## Retrieval

```http
POST /api/retrieval
Content-Type: application/json
```

```json
{
  "query": "intentions",
  "collection": "bukhari",
  "limit": 10
}
```

`collection` and `limit` are optional. `limit` defaults to `10` and is capped
at `20`.

Response:

```json
{
  "query": "intentions",
  "results": [
    {
      "hadith_id": 1,
      "collection": "bukhari",
      "book_number": "1",
      "hadith_number": "1",
      "arabic_text": "...",
      "english_text": "...",
      "score": 0.83
    }
  ]
}
```

Retrieval embeds the query text through the configured embedding provider,
searches Qdrant for the nearest indexed Hadith vectors (optionally scoped to
one collection), and resolves every match back to its canonical PostgreSQL
record before returning it. A vector match that no longer resolves to a
canonical record (e.g. a stale point after a Hadith was removed) is dropped
from the response rather than fabricated; it is logged as a warning.

Hadiths are indexed into Qdrant via `import_hadiths --embed`, documented in
[docs/import-hadith-json.md](import-hadith-json.md).

## Migration from the backend-only layout

The full-stack migration moved the former endpoints under `/api`:

| Previous | Current |
| --- | --- |
| `/health` | `/api/health` |
| `/collections` | `/api/collections` |
| `/hadiths` | `/api/hadiths` |
| `/retrieval` | `/api/retrieval` |

The `/hadiths` path is now the server-rendered browser page.
