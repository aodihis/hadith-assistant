# Semantic search page (`/search`)

## Context

`POST /api/retrieval` (implemented in the Topcoat 0.5.0 / Qdrant retrieval
pass) embeds a query, searches Qdrant, and resolves matches back to
canonical Hadith rows — but the application has no browser UI for it.
`src/app/hadiths.rs`'s `/hadiths` page is the closest existing pattern: a
server-rendered page reading typed query params, calling an application
service directly, and rendering results as cards.

This spec adds `/search`, a server-rendered page over the same
`RetrievalService` the API route already uses. No backend changes: this is
presentation-only.

## Route and request handling

New module `src/app/search.rs`, registered the same way `src/app/hadiths.rs`
is (Topcoat's module router discovers pages by file path under `src/app/`).

```rust
#[topcoat::router::query_params(error = bad_request)]
struct SearchQuery {
    q: Option<String>,
    collection: Option<String>,
}
```

- `q` is optional so the page has a valid unfiltered initial state (no
  request made) rather than requiring a query string to render at all —
  matching how `/hadiths` renders with no filters applied.
- `limit` is fixed at `RetrievalService`'s own default (10) and is not a
  user-facing control, matching `/hadiths`' treatment of `limit` as a fixed
  value rather than an exposed input.
- `collection`, when present and non-empty, is passed straight through to
  `RetrievalQuery.collection`.

The page handler calls `services.retrieval.retrieve(...)` directly — the
same application-layer call `src/app/api/retrieval.rs`'s `POST
/api/retrieval` route makes — not an internal HTTP request to that route.
This follows `AGENTS.md`'s rule that server-rendered pages and JSON routes
call the same application services rather than duplicating logic or
routing through each other.

## Collection dropdown

Populated from `services.collections.list()` (the existing
`CollectionService`, already backing `GET /api/collections`), so the
`<select>` always lists real, currently-existing collection slugs. An
"All collections" option maps to no `collection` filter (i.e. `None` is
sent to `RetrievalQuery`).

## Page states

All error mapping reuses the exact `page_error` pattern already defined in
`src/app/hadiths.rs` (`AppError::Validation` → `bad_request`,
`AppError::NotFound` → `not_found`, everything else → logged and
`internal_server_error`), duplicated into `search.rs` rather than factored
out — the existing codebase already tolerates one copy of this mapping per
page module, and extracting a shared helper is out of scope for a
one-page addition.

1. **Initial / empty query** (`q` absent or blank on first load): render the
   form and a neutral prompt ("Enter a question or topic to search
   Hadiths."); no call to `RetrievalService` is made.
2. **Validation error** (`q` present but empty/whitespace-only, or any other
   `AppError::Validation` from the service): render the form with an inline
   error message near the query input. Does not use Topcoat's `bad_request`
   page-level error response — the page still renders (form + message), the
   same way `/hadiths` doesn't hard-error on a technically-invalid filter
   combination that its own service already normalizes. Only truly
   malformed query params (a param Topcoat's query-parsing itself rejects)
   go through `bad_request`.
3. **No matches** (valid query, `RetrievalResult.results` is empty): render
   an empty-state block styled like `/hadiths`' `.empty-state`
   ("No matching Hadiths. Try a different phrasing.").
4. **Service error** (embedding provider unreachable/misconfigured, Qdrant
   unreachable, or any other non-Validation `AppError`): render a generic
   "Search is temporarily unavailable — please try again shortly." message.
   The underlying error is logged server-side via `tracing::error!` exactly
   as `page_error` already does; nothing about the failure (provider name,
   connection details, stack trace) reaches the response, consistent with
   `AGENTS.md`'s stable-error-envelope rule.
5. **Results**: one card per `RetrievedHadith`, modeled on `/hadiths`'
   `.hadith-card` — collection slug, book number, Hadith number, a link to
   the canonical record (`/api/hadiths/{hadith_id}`), Arabic text (`lang="ar"
   dir="rtl"`), and English text when present. The similarity `score` is
   deliberately not displayed: an uncalibrated cosine similarity number
   (e.g. `0.83`) isn't meaningful to a reader browsing Hadith and would read
   as unexplained noise.

## Navigation

Add a `"Search"` link to `root_layout`'s `<nav>` in `src/app.rs`, positioned
between `"Browse Hadiths"` and `"API health"`.

## Out of scope

- Any change to `RetrievalService`, `Embedder`, `VectorStore`, or the JSON
  API — this page is a pure consumer of existing application services.
- A user-facing `limit` control.
- Displaying the raw similarity score.
- Pagination of search results (the API itself caps at 20; the page simply
  renders what it gets back).
- Page-level integration tests requiring a live database/Qdrant instance —
  consistent with the project's existing, documented gap
  (`docs/project-status.md`: "Database-backed integration tests are not yet
  available").

## Testing

- No new application-service tests (behavior is unchanged; `search.rs` only
  renders what `RetrievalService`/`CollectionService` already return).
- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
  warnings`, `cargo test`, and `topcoat asset bundle --bin hadith-assistant`
  before handoff, per `AGENTS.md`.
- Manual verification against `docker compose up -d postgres qdrant` plus a
  real `EMBEDDING_API_KEY`, since no automated test exercises the live
  retrieval path.
