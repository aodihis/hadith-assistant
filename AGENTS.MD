# AGENTS.md

## Purpose

This repository is a full-stack Hadith research application built with Rust,
Topcoat, PostgreSQL, and SQLx. It serves HTML and JSON from one application.
Qdrant is planned as a retrieval index, never as the source of truth.

Topcoat is early-stage software. The application currently targets `0.5.x`.
Before changing framework APIs, consult the documentation for the exact locked
version and expect module, macro, or asset behavior to change between releases.

## Non-negotiable content rules

- Treat Hadith text, translations, grades, narrator data, numbering, and source
  metadata as authoritative religious content.
- Never silently paraphrase, merge, truncate, normalize away, or overwrite
  canonical records.
- Every retrieved or displayed record must retain a path to its collection,
  book number, Hadith number, and stable database or source identifier.
- Embeddings, chunks, generated text, and transliterations are derived data.
  They must never become the only copy of the source text.
- Dataset changes must be explicit, auditable, and covered by a reviewed
  ingestion path.

## Architecture

```text
src/
  web.rs                       root Topcoat router, layout, and home page
  web/                         server-rendered pages and JSON route adapters
    api.rs                     shared API layer and error response mapping
    api/                       module-derived /api routes
  application/                 use cases and dependency composition
  domain/                      canonical entities and query types
  infrastructure/persistence/ SQLx repositories and database mapping
  ingestion/                   source parsing, validation, and transactions
  transliteration/             deterministic derived text
  bin/                         operational CLIs
```

Keep these boundaries:

- `web` owns Topcoat macros, request parsing, HTML, HTTP status codes, and DTOs.
- `application` owns validation and use-case coordination. It must not depend on
  Topcoat request or response types.
- `domain` owns core data shapes and invariants. Do not put SQL or HTML here.
- `infrastructure/persistence` owns SQL and row mapping. No SQL belongs in pages
  or API handlers.
- `ingestion` owns import parsing, validation, audit data, and transaction
  behavior.
- `main.rs` only loads configuration, connects dependencies, migrates the
  database, constructs services, and starts Topcoat.

The server-rendered pages and JSON routes should call the same application
services. Do not create a second frontend project or duplicate validation in a
page-specific data layer.

## Topcoat conventions

- Use `topcoat::router::module_router!()` from `src/web.rs`.
- Keep route modules aligned with URLs. A file under `src/web/api/` normally
  becomes a route under `/api`.
- Register long-lived values once through `.app_context(...)`; access them with
  `app_context::<T>(cx)`.
- Use `#[page]` for full pages, `#[layout]` for shared document structure, and
  `#[route(METHOD)]` for API endpoints.
- Use typed `#[query_params]` and `#[path_param]` values rather than parsing URLs
  by hand.
- Use `view!` for markup. Rust control flow belongs in page/component code, not
  string-built HTML.
- Declare browser files with `asset!` and load the matching `AssetBundle` on the
  router. Production images must ship the bundle next to the executable.
- Keep the stable JSON error envelope in `src/web/api.rs`; do not expose raw SQL
  errors, stack traces, secrets, or provider payloads.
- When a Topcoat upgrade changes public APIs, update `Cargo.lock`, README
  instructions, Docker asset building, and affected tests in the same change.

## Application and persistence rules

- Inject `AppServices` through Topcoat app context. Do not construct pools or
  repositories in route handlers.
- Services validate and normalize input before repositories execute a query.
- Repositories expose domain-oriented operations and return `AppError`.
- Any multi-write operation must use an explicit transaction.
- Store timestamps in UTC and keep database constraints aligned with domain
  invariants.
- Bound free-text inputs, result limits, and offsets.
- Keep canonical HTTP writes disabled unless a reviewed editing workflow is
  deliberately added.

## Retrieval and RAG

Model retrieval as replaceable stages:

```text
query policy
  -> candidate retrieval
  -> collection/language/version scope filters
  -> ranking or reranking
  -> canonical-record resolution
  -> context and citations
```

- Apply scope filters before final context assembly.
- Every vector or chunk match must resolve to a canonical PostgreSQL record.
- Preserve original retrieval scores if reranking is added.
- Version chunking and embedding models.
- Never return generated content without its source citations.
- Until a stage exists, return an explicit not-implemented error; do not return
  fabricated, silently empty, or misleading success data.

## Configuration and operations

- Commit `.env.example`, never `.env` or credentials.
- Required configuration must fail fast at startup.
- Topcoat owns `HOST` and `PORT`; application-specific configuration belongs in
  `src/config.rs`.
- Migrations run before the server accepts traffic.
- Keep Docker Compose usable both for dependency-only development and the
  complete application profile.
- Do not add queues, providers, database extensions, or background services
  without documenting the operational reason.

## Testing and verification

Run the narrowest useful test while developing, then before handoff run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

When browser assets or routes change, also run:

```bash
topcoat asset bundle --bin hadith-assistant
```

Relevant tests should cover:

- service validation and normalization
- API error-code stability and safe public messages
- source reference and citation preservation
- retrieval scope and empty-result behavior
- ingestion validation and transaction semantics
- deterministic transliteration

If database or Topcoat CLI infrastructure is unavailable, report exactly which
verification was skipped.

## Change workflow

1. Read the nearest code and documentation.
2. Identify the affected boundary.
3. Add or change types at that boundary rather than leaking framework or SQL
   concerns across layers.
4. Preserve canonical text and traceability.
5. Add focused tests where the behavior can be captured.
6. Run formatting, linting, tests, and asset bundling as applicable.
7. Update README or `docs/` for routes, environment variables, ingestion
   formats, framework upgrades, and operational assumptions.

Keep changes simple and reversible. Do not revert unrelated user work in a dirty
worktree.

## Agent process

- Never run `git commit` without an explicit, in-the-moment request from the
  user — even when following a plan or skill whose steps say to commit after
  each task. Implement the change, verify it, and leave it uncommitted for
  the user to review, then wait to be asked.
- Scale process to task size. Reserve heavy multi-agent workflows (a fresh
  implementer subagent plus a separate reviewer subagent per task) for
  large, complex, or judgment-heavy work. Small or fully-mechanical tasks
  (dependency bumps, renames, single-file config additions) should be
  implemented directly in the working session instead.
- Do not create git worktrees for implementation work. Work directly in the
  main checkout so changes are visible without switching folders.
- If you find an issue in Topcoat or any other dependency (a bug, an
  undocumented breaking change, missing functionality) do not silently
  write a workaround. Stop and report it to the user first — what's
  broken, what you found, and the options — before choosing a fix. The
  right fix may belong upstream in the framework's own repository rather
  than as a local patch, and that's the user's call to make.
