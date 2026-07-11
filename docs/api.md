# Backend API

The backend starts an Axum HTTP server using:

- `DATABASE_URL`, required
- `SERVER_HOST`, optional, defaults to `127.0.0.1`
- `SERVER_PORT`, optional, defaults to `3000`
- `VECTOR_DB_PROVIDER`, optional, defaults to `qdrant`
- `QDRANT_URL`, optional, defaults to `http://localhost:6333`
- `QDRANT_COLLECTION`, optional, defaults to `hadith_vectors`

Run:

```bash
cargo run
```

## Health

```http
GET /health
```

Response:

```json
{
  "status": "ok"
}
```

## Collections

```http
GET /collections
GET /collections/{slug}
```

## Hadiths

```http
GET /hadiths
GET /hadiths/{id}
GET /hadiths/by-reference/{collection}/{book_number}/{hadith_number}
```

Supported list filters:

```http
GET /hadiths?collection=bukhari&book_number=1&hadith_number=1&grade=Sahih&limit=50&offset=0
```

Reference lookup:

```http
GET /hadiths/by-reference/bukhari/1/1
```

Hadith data is imported through the CLI, so the HTTP API is read-only for
canonical data.

## Retrieval

```http
POST /retrieval
```

Request body:

```json
{
  "query": "intentions",
  "collection": "bukhari",
  "limit": 10
}
```

Current behavior:

```json
{
  "code": "not_implemented",
  "message": "not implemented: retrieval is not implemented yet for query `intentions`"
}
```

The endpoint exists so clients can integrate against the API shape. The actual
Qdrant retrieval pipeline is still marked with a TODO in the service layer.
