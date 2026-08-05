# Hadith Assistant

Hadith Assistant is a full-stack Rust application for browsing canonical Hadith
records and, eventually, answering questions with retrieval-augmented generation
(RAG). [Topcoat](https://github.com/tokio-rs/topcoat) renders the web interface
and serves the JSON API from one binary. PostgreSQL remains the source of truth,
while Qdrant is reserved for a future citation-preserving retrieval index.

Topcoat is intentionally early and experimental. This project currently targets
Topcoat `0.5.x` and commits `Cargo.lock` so framework changes are adopted
deliberately.

## What is implemented

- Server-rendered home and Hadith browser pages.
- Read-only JSON endpoints for collections and canonical Hadith records.
- PostgreSQL migrations applied automatically at startup.
- Auditable JSON importer with deterministic Arabic transliteration.
- Qdrant-backed retrieval: embed a query through a swappable `Embedder`,
  search a swappable `VectorStore`, resolve every hit back to its canonical
  Hadith row.
- Docker Compose services for PostgreSQL, Qdrant, and the complete application.

## Project structure

```text
.
├── assets/                         # Topcoat-bundled browser assets
├── data/imports/                   # Local, ignored source datasets
├── docs/                           # API, ingestion, and domain documentation
├── migrations/                     # PostgreSQL schema history
├── src/
│   ├── app.rs                      # Root layout, home page, router assembly
│   ├── app/
│   │   ├── hadiths.rs              # Server-rendered Hadith browser
│   │   ├── api.rs                  # API layer and JSON error adapter
│   │   └── api/                    # Module-derived Topcoat API routes
│   ├── application/                # Use cases and shared application services
│   ├── domain/                     # Canonical entities and query types
│   ├── infrastructure/persistence/ # SQLx repositories
│   ├── ingestion/                  # Dataset parsing, validation, and import
│   ├── transliteration/            # Deterministic Arabic transliteration
│   └── bin/import_hadiths.rs       # Import CLI
├── AGENTS.md                       # Rules for coding agents and contributors
├── docker-compose.yml
└── Cargo.toml
```

The `app` layer owns HTTP and HTML concerns. It calls the same services that
back the JSON API, so there is no separate frontend project and no duplicate
business logic.

## Local development

Requirements:

- A current stable Rust toolchain
- Docker with Docker Compose
- Topcoat CLI `0.5.x`

Install the CLI and create local configuration:

```bash
cargo install topcoat-cli --version 0.5.0 --locked
cp .env.example .env
```

Start PostgreSQL and Qdrant:

```bash
docker compose up -d postgres qdrant
```

Start Topcoat's development server:

```bash
topcoat dev
```

Open <http://127.0.0.1:3000>. Topcoat rebuilds the Rust application, bundles
assets, restarts the server, and reloads browser pages when source files change.

To run without the development server, bundle assets first:

```bash
topcoat asset bundle --bin hadith-assistant
cargo run --bin hadith-assistant
```

Configuration is loaded from `.env`:

| Variable | Required | Default | Purpose |
| --- | --- | --- | --- |
| `DATABASE_URL` | yes | — | PostgreSQL connection string |
| `DATABASE_MAX_CONNECTIONS` | no | `10` | SQLx pool size |
| `HOST` | no | `127.0.0.1` | Topcoat bind host |
| `PORT` | no | `3000` | Topcoat bind port |
| `VECTOR_DB_PROVIDER` | no | `qdrant` | Selected vector backend |
| `QDRANT_URL` | no | `http://localhost:6334` | Qdrant gRPC endpoint (`qdrant-client` speaks gRPC, not REST — port 6333 is REST-only and will not work here) |
| `QDRANT_COLLECTION` | no | `hadith_vectors` | Qdrant collection name |
| `EMBEDDING_BASE_URL` | no | `https://api.openai.com/v1` | Embeddings API base URL |
| `EMBEDDING_API_KEY` | no | — | Bearer token for the embeddings API; required to actually call retrieval or `import_hadiths --embed` |
| `EMBEDDING_MODEL` | no | `text-embedding-3-small` | Embedding model name |
| `RUST_LOG` | no | framework default | Tracing filter |

## Routes

Browser pages:

- `GET /`
- `GET /hadiths`

JSON API:

- `GET /api/health`
- `GET /api/collections`
- `GET /api/collections/{slug}`
- `GET /api/hadiths`
- `GET /api/hadiths/{id}`
- `GET /api/hadiths/by-reference/{collection}/{book_number}/{hadith_number}`
- `POST /api/retrieval`

The old backend routes moved under `/api` during the full-stack migration.
Detailed request and response notes are in [docs/api.md](docs/api.md).

## Importing data

The dataset is intentionally not committed. Place the source JSON under
`data/imports/`, validate it, and then import it:

```bash
cargo run --bin import_hadiths -- data/imports/hadiths.json --validate-only
cargo run --bin import_hadiths -- data/imports/hadiths.json
```

The import runs in one database transaction and preserves canonical source
references. Add `--embed` to also embed the newly imported records into
Qdrant. See [docs/import-hadith-json.md](docs/import-hadith-json.md).

## Verification

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

`make dev`, `make run`, `make infra-up`, `make infra-down`, and `make check`
provide shortcuts for the same workflows.
