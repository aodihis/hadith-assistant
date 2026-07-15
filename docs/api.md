# Backend API

The backend starts an Axum HTTP server using:

- `DATABASE_URL`, required
- `SERVER_HOST`, optional, defaults to `127.0.0.1`
- `SERVER_PORT`, optional, defaults to `3000`
- `VECTOR_DB_PROVIDER`, optional, defaults to `qdrant`
- `QDRANT_URL`, optional, defaults to `http://localhost:6333`
- `QDRANT_COLLECTION`, optional, defaults to `hadith_vectors`

Create a local `.env` from the checked-in example:

```bash
cp .env.example .env
```

The server loads `.env` automatically at startup. Real `.env` files are ignored
by Git.

On startup, the server automatically applies pending SQL files from
`migrations/` using SQLx. Migrations are embedded in the compiled binary, so
the runtime container does not need the migration files mounted separately.
If a migration fails, startup fails before the HTTP server begins accepting
requests.

Start the local PostgreSQL and Qdrant services:

```bash
docker compose up -d postgres qdrant
```

The Rust API is excluded from Docker Compose by default. This keeps the normal
development loop on the host:

```bash
make dev
```

`make dev` starts PostgreSQL and Qdrant in Docker, then runs the API on the host
with `cargo watch`. Rust source and migration changes trigger an incremental
rebuild and server restart. SQLx checks migrations during each startup but
applies only pending versions.

The equivalent commands without Make are:

```bash
docker compose up -d postgres qdrant
cargo watch -x "run --bin hadith-assistant"
```

Other useful targets are `make run` for a single host run, `make infra-up`,
`make infra-down`, and `make check`.

To run the Rust API in Docker too, set the Compose profile in `.env`:

```dotenv
COMPOSE_PROFILES=app
```

Then start the complete stack:

```bash
docker compose up -d --build
```

You can also enable it for a single command without changing `.env`:

```bash
docker compose --profile app up -d --build
```

PostgreSQL is exposed on `localhost:5433` to avoid colliding with a local
PostgreSQL instance on the default `5432` port.

Qdrant is available at `http://localhost:6333`, with the dashboard at
`http://localhost:6333/dashboard`.

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
