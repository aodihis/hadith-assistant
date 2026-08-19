# Sanad

Sanad is a full-stack Rust application for browsing canonical Hadith records and
answering questions about them with retrieval-augmented generation (RAG), where
every answer is cited back to the narrations it was built from. A *sanad* is the
chain of transmission that supports a hadith, which is what the project is about.

Hadith text, translations, and gradings come from [sunnah.com](https://sunnah.com/).

[Topcoat](https://github.com/tokio-rs/topcoat) renders the web interface and
serves the JSON API from one binary. PostgreSQL remains the source of truth, with
Qdrant holding the citation-preserving retrieval index.

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
- Multi-turn chat grounded in retrieved narrations, streamed over server-sent
  events. Off-topic and uncovered questions are declined rather than answered
  from weak matches, and a refusal never carries citations.
- Conversation memory held by the browser and compacted server-side. Nothing
  about a conversation is stored on the server or in the database.
- Per-session rate limiting on the chat endpoint, keyed by a signed session
  token that carries no user identity.
- Docker Compose services for PostgreSQL, Qdrant, and the complete application.

## Project structure

```text
.
├── assets/                         # Topcoat-bundled browser assets
├── data/imports/                   # Local, ignored source datasets
├── docs/                           # API, ingestion, and domain documentation
├── migrations/                     # PostgreSQL schema history
├── src/
│   ├── web.rs                      # Root layout, home page, router assembly
│   ├── web/
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

The `web` layer owns HTTP and HTML concerns. It calls the same services that
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
topcoat asset bundle --bin sanad
cargo run --bin sanad
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
| `OPEN_ROUTER_API_KEY` | no | — | Bearer token shared by the embedding and chat-completion clients; required to actually call retrieval, `import_hadiths --embed`, or answer generation |
| `EMBEDDING_BASE_URL` | no | `https://api.openai.com/v1` | Embeddings API base URL |
| `EMBEDDING_MODEL` | no | `text-embedding-3-small` | Embedding model name |
| `CHAT_BASE_URL` | no | `https://openrouter.ai/api/v1` | Chat-completion API base URL |
| `CHAT_MODEL` | no | `deepseek/deepseek-v4-flash` | Chat-completion model used to answer |
| `CHAT_SUMMARY_MODEL` | no | value of `CHAT_MODEL` | Model used to compact history; set it to run recaps on something cheaper |
| `CHAT_TEMPERATURE` | no | `0.3` | Answer sampling temperature; rejected at startup outside `0.0`-`0.7` |
| `CHAT_MAX_TOKENS` | no | `700` | Answer length cap; rejected outside `64`-`1200` |
| `CHAT_SUMMARY_TEMPERATURE` | no | `0.1` | Compaction temperature, colder than answers on purpose |
| `CHAT_SUMMARY_MAX_TOKENS` | no | `300` | Compaction length cap |
| `CHAT_HISTORY_MAX_TURNS` | no | `8` | Turns before compaction fires |
| `CHAT_HISTORY_KEEP_TURNS` | no | `4` | Turns kept verbatim after compaction; must be less than the max |
| `CHAT_HISTORY_MAX_CHARS` | no | `6000` | Size-based compaction trigger |
| `CHAT_MAX_QUESTION_CHARS` | no | `1000` | Upper bound on one question |
| `RETRIEVAL_MIN_SCORE` | no | `0.45` | Minimum cosine score for a match to count as relevant |
| `SESSION_SECRET` | no | generated per run | Signs chat session tokens; unset means sessions end at restart |
| `RUST_LOG` | no | `ERROR` only | Tracing filter — set to `info` or warnings stay invisible |

## Routes

Browser pages:

- `GET /`
- `GET /hadiths`
- `GET /chat` — the Sanad chat interface

JSON API:

- `GET /api/health`
- `GET /api/collections`
- `GET /api/collections/{slug}`
- `GET /api/hadiths`
- `GET /api/hadiths/{id}`
- `GET /api/hadiths/{id}/related`
- `GET /api/hadiths/by-reference/{collection}/{book_number}/{hadith_number}`
- `POST /api/retrieval`
- `POST /api/answers` — single-shot grounded answer
- `POST /api/chat/session` — issues the session token `/api/chat` requires
- `POST /api/chat` — streams one conversational turn over server-sent events

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
references. Records already present are skipped, matched on the source dump's
own `arabicURN` and `englishURN`, so re-running never duplicates canonical
text. Add `--embed` to also embed the imported records into Qdrant; anything
already indexed is skipped, so a re-run costs nothing for work already done.

To build the vector index for a collection imported earlier:

```bash
cargo run --bin import_hadiths -- --embed-collection bukhari
cargo run --bin import_hadiths -- --embed-collection bukhari --limit 200
```

Source markup is stripped from the text an embedding is built from, while the
stored record keeps its original content. See
[docs/import-hadith-json.md](docs/import-hadith-json.md).

## Verification

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

`make dev`, `make run`, `make infra-up`, `make infra-down`, and `make check`
provide shortcuts for the same workflows.
