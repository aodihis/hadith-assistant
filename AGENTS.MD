# AGENTS.MD

## Purpose

This repository contains the backend for a Hadith assistant chat application. The backend stores Hadith data and supports retrieval augmented generation (RAG) so the chat layer can answer with grounded references.

The primary stack is Rust, Axum, PostgreSQL, and vector search. Favor boring, explicit, production-ready code over clever abstractions.

## Operating Principles

- Treat Hadith text, grading, source, narrator chains, translations, and metadata as authoritative religious content. Do not silently alter, normalize away, paraphrase, or merge Hadith records unless the change is explicitly part of a reviewed ingestion process.
- Every user-facing answer path must preserve traceability back to source records.
- Keep changes small and reversible. Feature updates should extend clear boundaries instead of spreading logic across handlers, SQL, and RAG code at once.
- Prefer deterministic backend behavior. LLM behavior belongs behind explicit interfaces and must not leak into core data integrity rules.
- Do not introduce network services, queues, background workers, embedding providers, or database extensions without documenting the operational reason.

## Expected Architecture

Use a layered service architecture:

```text
src/
  main.rs              # process startup only
  config.rs            # environment parsing and validated settings
  error.rs             # application error type and HTTP mapping
  state.rs             # shared app state
  routes/              # Axum routers and request/response DTOs
  handlers/            # thin HTTP adapters, if separated from routes
  domain/              # Hadith domain types and invariants
  services/            # business use cases
  repositories/        # PostgreSQL persistence
  rag/                 # retrieval, embeddings, ranking, context assembly
  db/                  # migrations, pool helpers, transaction helpers
  observability/       # tracing, metrics, request IDs
  tests/               # integration tests
```

The exact layout may evolve, but keep these boundaries:

- `routes` and `handlers` parse HTTP input, call services, and return typed responses.
- `services` own application use cases and transaction boundaries.
- `repositories` own SQL and database mapping.
- `domain` owns core entities, validation, and invariants.
- `rag` owns retrieval strategy, query expansion, embedding lookup, reranking, and context shaping.
- `main.rs` wires dependencies and starts the server only.

## Design Patterns

### Dependency Injection Through App State

Use an explicit `AppState` shared through Axum state. Store clients and services behind cheap clone handles such as `Arc<T>`.

Handlers should receive dependencies from state instead of constructing pools, clients, or services directly.

### Repository Pattern

All PostgreSQL access belongs in repository modules. Do not write SQL inside route handlers.

Repositories should expose domain-oriented methods such as:

- `find_hadith_by_id`
- `search_hadith_text`
- `find_chunks_for_retrieval`
- `insert_ingestion_batch`

Avoid exposing database table details to HTTP DTOs.

### Service Layer

Services coordinate repositories, transactions, authorization or scope checks, and RAG workflows. Keep services independent from Axum types so they are easy to test.

### Typed Boundaries

Use separate types for:

- HTTP requests and responses
- Domain entities
- Database rows
- RAG chunks and retrieved context
- External provider payloads

Do not reuse database structs as API responses when fields have different privacy, stability, or formatting rules.

### Explicit Transactions

Any multi-write operation must use a transaction. The service should decide the transaction boundary; repositories should accept an executor or transaction reference when needed.

### RAG Pipeline Pattern

Model RAG as a pipeline with replaceable stages:

```text
query input
  -> normalize/query policy
  -> retrieve candidate chunks
  -> apply data scope filters
  -> rank or rerank
  -> assemble context
  -> return citations and trace data
```

Do not mix prompt assembly, vector retrieval, database filtering, and HTTP formatting in one function.

### TODO Policy For Unimplemented Features

When adding a feature boundary before the implementation exists, add an explicit
`TODO` at the service or domain function that owns the missing behavior.

The placeholder must return a clear not-implemented error instead of fake data,
silent fallback behavior, or a misleading empty success response.

## Data Model Guidance

Hadith records should be modeled with enough metadata to support precise retrieval and citation:

- source collection
- book or chapter
- Hadith number or source-local identifier
- Arabic text
- translation text, when available
- narrator chain, when available
- grading and grader, when available
- topic tags
- language
- edition or dataset version
- ingestion batch ID
- canonical source reference

For RAG, store chunks separately from canonical Hadith records. A chunk is a retrieval unit, not the source of truth.

Recommended separation:

- `hadiths`: canonical record and stable metadata
- `hadith_texts`: language-specific text variants, if multiple languages are supported
- `hadith_chunks`: retrieval chunks with offsets and chunk strategy metadata
- `hadith_embeddings`: embedding vectors tied to chunk IDs and embedding model version
- `ingestion_batches`: source, checksum, version, and import audit data

Never make embeddings the only path back to the original Hadith. Every retrieved chunk must resolve to a canonical record.

## Data Scoping Rules

Every read path must define its scope explicitly.

Scope dimensions may include:

- language
- collection
- dataset version
- grading policy
- public or private dataset visibility
- tenant or application namespace, if multi-tenant support is added
- ingestion batch, for admin review workflows

RAG retrieval must apply scope filters before final context assembly. If vector search happens before all filters can be applied, the service must filter and rerank afterward, then record that behavior in code comments or tests.

Do not let a chat request retrieve:

- unpublished ingestion data
- records outside the requested language or collection scope
- records from deprecated dataset versions unless explicitly requested
- private tenant data from another tenant

## Error Handling

Use one application error type, for example `AppError`, and map it to HTTP responses in one place.

Recommended categories:

- `Validation`: malformed input, invalid pagination, unsupported language
- `NotFound`: missing Hadith, chunk, collection, or ingestion batch
- `Conflict`: duplicate source identifiers, ingestion version conflicts
- `Unauthorized`: missing or invalid authentication
- `Forbidden`: authenticated but outside allowed scope
- `Database`: PostgreSQL failures
- `External`: embedding or LLM provider failures
- `Unavailable`: dependency timeout or degraded service
- `Internal`: unexpected server errors

Rules:

- Do not expose raw SQL errors, credentials, prompts, provider payloads, or stack traces in HTTP responses.
- Include a stable error code in API responses.
- Log internal details with `tracing`, including request ID and relevant safe identifiers.
- Prefer `thiserror` for error definitions.
- Use `anyhow` only at application edges such as startup, migrations, or one-off tooling. Do not use `anyhow` as the main service error type.

## HTTP API Rules

- Use Axum extractors for path, query, JSON, and state.
- Validate request payloads before calling services.
- Use typed response structs. Avoid returning ad hoc JSON maps.
- Use consistent pagination for list and search endpoints.
- Include source IDs and citation metadata in RAG responses.
- Do not stream LLM responses from this service unless the streaming contract is documented and tested.
- API behavior must be stable enough for the chat application to depend on it.

## PostgreSQL Rules

- Use migrations for all schema changes.
- Prefer compile-time checked SQL when practical, such as `sqlx` query macros, once the project chooses a database access library.
- Use indexes intentionally for retrieval filters, source IDs, language, collection, and vector search.
- Keep database constraints aligned with domain invariants.
- Store timestamps in UTC.
- Use `uuid` or clear stable identifiers where records need public references.
- Do not rely on application-only uniqueness for canonical Hadith identifiers.

## RAG Rules

- Retrieval must return citations, not just text.
- Store embedding model name and version with each embedding.
- Re-embed chunks when the chunking strategy, text normalization, or embedding model changes.
- Keep chunking deterministic and versioned.
- Prefer smaller, citation-friendly chunks over large opaque blobs.
- Do not allow generated content to overwrite canonical Hadith data.
- If reranking is added, keep the original retrieval score and rerank score for debugging.
- Add tests for scope filtering, citation preservation, and empty-result behavior.

## Ingestion Rules

Hadith ingestion should be idempotent and auditable.

Each ingestion batch should record:

- source name
- source version or file checksum
- import timestamp
- record count
- chunking strategy version
- embedding model version, if embeddings are generated
- success or failure status

Do not partially publish a failed ingestion batch. Prefer staging imported data and promoting it only after validation passes.

Validation should check:

- required identifiers
- duplicate source references
- empty Arabic or translation fields, depending on dataset expectations
- invalid grading values
- broken chunk-to-Hadith references

## Observability

- Use `tracing` for structured logs.
- Add request IDs to logs and responses.
- Log retrieval counts, selected scope, and latency for RAG requests.
- Do not log full Hadith corpora, full prompts, secrets, tokens, or user private messages by default.
- Add metrics for request latency, database latency, retrieval latency, error rates, and ingestion status when metrics infrastructure exists.

## Testing Rules

Before completing backend changes, run the narrowest useful verification first, then broader checks when available:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

When working on a feature or bug fix, prefer adding a failing test first when the behavior can be captured cleanly.

For database changes, add migration tests or integration tests when the project has test database support.

For RAG changes, test:

- scope filters
- citation IDs
- no-result behavior
- ranking stability for fixed fixtures
- chunk-to-source traceability

If documenting a known bug in tests, the test should still pass and clearly mark the undesirable behavior. When the bug is fixed, update the expectation and remove the marker or comment.

Do not mark work complete if tests are skipped due to missing infrastructure without saying so clearly.

## Feature Update Workflow

For any feature update:

1. Identify the affected boundary: HTTP, service, repository, domain, RAG, database, or ingestion.
2. Read the nearby code before editing.
3. Add or update types at the correct boundary.
4. Keep handlers thin and push logic into services or domain modules.
5. Add migrations before repository code when schema changes are needed.
6. Add focused tests for the changed behavior.
7. Run formatting and relevant tests.
8. Document new environment variables, operational requirements, or data assumptions.

## Security And Privacy

- Load secrets from environment variables or a secret manager, not source files.
- Never commit real credentials, API keys, `.env` files, database dumps, or private Hadith datasets.
- Treat user chat messages as private data.
- Avoid logging user messages unless there is a deliberate redaction strategy.
- Validate and bound all search inputs, pagination values, and free-text query sizes.
- Use timeouts for database and external provider calls.

## Configuration

Configuration should be explicit and validated at startup.

Expected configuration categories:

- server host and port
- PostgreSQL connection URL
- database pool limits
- embedding provider and model
- vector dimensions
- request timeouts
- allowed dataset version
- logging level

Fail fast on missing required configuration.

## Code Style

- Prefer simple Rust types and clear ownership.
- Keep the concept count low. Avoid adding helpers, traits, modules, or generic abstractions until they remove real complexity across multiple call sites.
- Balance KISS and DRY. If removing duplication requires a heavyweight helper, first look for a simpler design that avoids the duplicated work.
- Avoid global mutable state.
- Keep async functions cancellation-safe where possible.
- Use `Result<T, AppError>` in service and repository paths.
- Use `serde` DTOs only at serialization boundaries.
- Keep module names lowercase and domain-specific.
- Avoid large files. Split by responsibility once a file becomes hard to scan.
- Put `use` imports at the top of the file instead of scattering inline fully-qualified paths, except when a name collision makes the qualified path clearer.
- Do not add silent fallbacks for states that should be impossible. Use `unreachable!("reason")` or `.expect("reason")` for invariant-protected states, and use normal error handling for states that can happen at runtime.
- Comments should explain invariants, tradeoffs, or non-obvious safety constraints. Do not duplicate what the code already says.

## Invariants And Fallbacks

Be explicit about the difference between invalid user input, dependency failure, and impossible internal state.

- Invalid user input should become a validation error.
- Missing data that can happen in normal operation should become a typed not-found or conflict error.
- Dependency failures should become database, external, timeout, or unavailable errors.
- Impossible internal states should fail loudly with a clear invariant message.

Never hide broken assumptions with `_ => default`, `.unwrap_or_default()`, empty context assembly, or broad catch-all behavior when doing so could produce an incorrect Hadith answer or incorrect citation.

## Documentation Rules

Update documentation when adding:

- environment variables
- database tables or migrations
- ingestion formats
- public API endpoints
- RAG behavior or retrieval policy
- operational dependencies

Architecture decisions with long-term impact should be captured in an ADR-style note under `docs/adr/` once that directory exists.

## Source Control And Commits

Do not assume source-control commands are available or healthy. Check the repository state before using Git, and report clearly if the checkout is blocked by local configuration such as safe-directory ownership.

Commit messages should explain intent rather than list every edited file:

- why the change is needed
- what high-level approach was taken
- why the approach works

Before committing, amending, or handing code to a human for review, run formatting and linting when the relevant tools are available. If linting fails because of pre-existing issues, do not introduce new lint failures in files touched by the change.

## Done Definition

A change is done when:

- the behavior is implemented at the right architectural boundary
- source traceability is preserved
- data scope is explicit
- errors are mapped safely
- relevant tests or verification commands pass
- operational assumptions are documented
- no unrelated files or user changes are reverted
