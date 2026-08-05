# Search Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a server-rendered `/search` page that lets a user run a semantic Hadith search through the existing `RetrievalService`, with no backend changes.

**Architecture:** A new Topcoat page module (`src/app/search.rs`), auto-discovered under the URL matching its filename exactly like `src/app/hadiths.rs` → `/hadiths`. It reads typed query params, calls `services.retrieval.retrieve(...)` and `services.collections.list()` directly (the same application services the JSON API already uses), and renders results as cards styled like the existing `/hadiths` page.

**Tech Stack:** Rust, Topcoat 0.5.0 `view!`/`query_params`/`page` macros, existing `RetrievalService`/`CollectionService`.

## Global Constraints

- Server-rendered pages and JSON routes must call the same application services — no duplicated logic, no internal HTTP calls (`AGENTS.md`).
- The stable JSON/page error envelope never leaks raw database or provider errors to the client (`AGENTS.md`).
- Do not display the raw similarity `score` to the user (spec: uncalibrated cosine number, not meaningful to a reader).
- `limit` is fixed at `RetrievalService`'s own default (10) and is not a user-facing control (spec).
- A validation error from `RetrievalService.retrieve` (`AppError::Validation`) renders inline on the still-rendered page — it does not go through Topcoat's `bad_request` page-level error response (spec).
- Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, and `topcoat asset bundle --bin hadith-assistant` before considering this done (`AGENTS.md`).
- Do not run `git commit` — leave the change uncommitted for review (this repo's `AGENTS.MD`, "Agent process" section).

---

## File Structure

- Create: `src/app/search.rs` — the `/search` page. Topcoat's module router discovers pages by file path under `src/app/` with no manual `mod` registration needed — confirmed by `src/app/hadiths.rs` and `src/app/api.rs` already working this way with no corresponding `mod` statement anywhere in `src/app.rs` or `src/lib.rs`.
- Modify: `src/app.rs:40-46` — add a `"Search"` nav link.
- Modify: `assets/app.css` — extend `.filters input` to also style `<select>`, add a `--error` custom property and `.form-error` class for the inline validation message.

---

### Task 1: Add the `/search` page

**Files:**
- Create: `src/app/search.rs`
- Modify: `src/app.rs:40-46`
- Modify: `assets/app.css:1-12` (custom properties), `assets/app.css:191-199` (`.filters input`)

**Interfaces:**
- Consumes: `AppServices.retrieval: Arc<RetrievalService>` with `retrieve(&self, query: RetrievalQuery) -> Result<RetrievalResult, AppError>`; `AppServices.collections: Arc<CollectionService>` with `list(&self) -> Result<Vec<Collection>, AppError>`. `RetrievalQuery { query: String, collection: Option<String>, limit: i64 }`. `RetrievalResult { query: String, results: Vec<RetrievedHadith> }`. `RetrievedHadith { hadith_id: i64, collection: String, book_number: String, hadith_number: String, arabic_text: String, english_text: Option<String>, score: Option<f64> }`. `Collection { id: i64, slug: String, name: String }`. `AppError::{Validation(String), NotFound(String), ...}`. All defined in `src/domain/models.rs` and `src/error.rs` — unchanged by this task.
- Produces: nothing consumed by later work — this is the final task.

This is one task, not split further: a single page with no backend changes is one reviewable, independently testable deliverable — splitting CSS, the page module, and the nav link into separate tasks would create artificial checkpoints with nothing working in between.

- [ ] **Step 1: Add CSS for the collection `<select>` and inline validation error**

In `assets/app.css`, change the `:root` block (currently lines 1-12) from:

```css
:root {
  color-scheme: light;
  --ink: #17211d;
  --muted: #5d6b64;
  --paper: #f7f5ed;
  --surface: #fffdf7;
  --line: #d9ddd3;
  --green: #174c3c;
  --green-light: #dce9df;
  --gold: #b88836;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}
```

to:

```css
:root {
  color-scheme: light;
  --ink: #17211d;
  --muted: #5d6b64;
  --paper: #f7f5ed;
  --surface: #fffdf7;
  --line: #d9ddd3;
  --green: #174c3c;
  --green-light: #dce9df;
  --gold: #b88836;
  --error: #a4373a;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}
```

Change the `.filters input` rule (currently lines 191-199) from:

```css
.filters input {
  width: 100%;
  padding: 0.75rem;
  border: 1px solid var(--line);
  border-radius: 0.35rem;
  color: var(--ink);
  background: white;
  font: inherit;
}
```

to:

```css
.filters input,
.filters select {
  width: 100%;
  padding: 0.75rem;
  border: 1px solid var(--line);
  border-radius: 0.35rem;
  color: var(--ink);
  background: white;
  font: inherit;
}
```

Add a new rule after the `.filters .button, .filters .clear-link` rule (currently ending at line 204):

```css
.form-error {
  margin: 1rem 0 0;
  color: var(--error);
  font-size: 0.85rem;
  font-weight: 700;
}
```

- [ ] **Step 2: Create the search page module**

Create `src/app/search.rs`:

```rust
use topcoat::{
    Error, Result,
    context::{Cx, app_context},
    router::error::{bad_request, internal_server_error, not_found},
    router::{page, query_params},
    view::view,
};

use crate::application::AppServices;
use crate::domain::RetrievalQuery;
use crate::error::AppError;

#[topcoat::router::query_params(error = bad_request)]
struct SearchQuery {
    q: Option<String>,
    collection: Option<String>,
}

#[page]
async fn search(cx: &Cx) -> Result {
    let query = query_params::<SearchQuery>(cx)?;
    let q = query.q.clone().unwrap_or_default();
    let selected_collection = query.collection.clone().unwrap_or_default();
    let submitted = query
        .q
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());

    let services = app_context::<AppServices>(cx);

    let collections = services.collections.list().await.map_err(page_error)?;

    let mut validation_error = None;
    let mut service_error = false;
    let mut results = Vec::new();

    if submitted {
        let retrieval_query = RetrievalQuery {
            query: q.clone(),
            collection: query
                .collection
                .clone()
                .filter(|value| !value.trim().is_empty()),
            limit: 0,
        };

        match services.retrieval.retrieve(retrieval_query).await {
            Ok(result) => results = result.results,
            Err(AppError::Validation(message)) => validation_error = Some(message),
            Err(error) => {
                tracing::error!(error = ?error, "search request failed");
                service_error = true;
            }
        }
    }

    let no_results = submitted && !service_error && validation_error.is_none() && results.is_empty();

    view! {
        <main>
            <section class="page-heading">
                <p class="eyebrow">"Semantic search"</p>
                <h1>"Search Hadiths"</h1>
                <p>
                    "Ask a question or describe a topic. Results are matched by meaning, "
                    "then resolved back to their canonical source records."
                </p>
            </section>

            <form class="filters" action="/search" method="get">
                <label>
                    "Query"
                    <input
                        type="text"
                        name="q"
                        value=(q)
                        placeholder="e.g. the reward of intentions"
                    >
                </label>
                <label>
                    "Collection"
                    <select name="collection">
                        <option value="" selected=(selected_collection.is_empty())>
                            "All collections"
                        </option>
                        for collection in collections {
                            <option
                                value=(collection.slug.clone())
                                selected=(collection.slug == selected_collection)
                            >
                                (collection.name)
                            </option>
                        }
                    </select>
                </label>
                <button class="button primary" type="submit">"Search"</button>
                <a class="clear-link" href="/search">"Clear"</a>
            </form>

            if let Some(message) = validation_error {
                <p class="form-error">(message)</p>
            }

            if service_error {
                <div class="empty-state">
                    <h2>"Search is temporarily unavailable"</h2>
                    <p>"Please try again shortly."</p>
                </div>
            } else if !submitted {
                <div class="empty-state">
                    <h2>"Enter a question or topic to search Hadiths."</h2>
                </div>
            } else if no_results {
                <div class="empty-state">
                    <h2>"No matching Hadiths"</h2>
                    <p>"Try a different phrasing."</p>
                </div>
            } else {
                <section class="hadith-list" aria-label="Search results">
                    for hadith in results {
                        <article class="hadith-card">
                            <div class="hadith-meta">
                                <span class="collection">(hadith.collection)</span>
                                <span>
                                    "Book "
                                    (hadith.book_number)
                                    " · Hadith "
                                    (hadith.hadith_number)
                                </span>
                                <a href=(format!("/api/hadiths/{}", hadith.hadith_id))>
                                    "Record #"
                                    (hadith.hadith_id)
                                </a>
                            </div>
                            <p class="arabic" lang="ar" dir="rtl">
                                (hadith.arabic_text)
                            </p>
                            if let Some(english_text) = hadith.english_text {
                                <p class="translation">(english_text)</p>
                            }
                        </article>
                    }
                </section>
            }
        </main>
    }
}

fn page_error(error: AppError) -> Error {
    match error {
        AppError::Validation(message) => bad_request(message).into(),
        AppError::NotFound(_) => not_found().into(),
        error => {
            tracing::error!(error = ?error, "page request failed");
            internal_server_error(error).into()
        }
    }
}
```

Note: `results` is moved into the final `for hadith in results` branch, and `no_results` is computed from `results.is_empty()` beforehand — this works because the `no_results`/`service_error`/`!submitted` branches are checked, in that order, before the `else` branch that consumes `results` by value, so there is no use-after-move.

- [ ] **Step 3: Add the nav link**

In `src/app.rs`, change the `<nav>` block inside `root_layout` (currently):

```rust
                    <nav aria-label="Primary navigation">
                        <a href="/">"Home"</a>
                        <a href="/hadiths">"Browse Hadiths"</a>
                        <a href="/api/health">"API health"</a>
                    </nav>
```

to:

```rust
                    <nav aria-label="Primary navigation">
                        <a href="/">"Home"</a>
                        <a href="/hadiths">"Browse Hadiths"</a>
                        <a href="/search">"Search"</a>
                        <a href="/api/health">"API health"</a>
                    </nav>
```

- [ ] **Step 4: Build and verify**

Run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Expected: all three succeed. This task adds no new automated tests (per the spec's "Testing" section — a page over existing, already-tested services, and no live DB/Qdrant available for an integration test), so the existing suite's pass count is unchanged; the new page is exercised by the crate simply compiling and `module_router_builds_without_route_conflicts` (in `src/app.rs`) continuing to pass with the new route registered.

If the `topcoat` CLI is available, also run `topcoat asset bundle --bin hadith-assistant` to confirm the CSS changes bundle cleanly.

- [ ] **Step 5: Manual verification**

Not automatable without a live database and Qdrant instance (consistent with the project's existing documented gap). If `docker compose up -d postgres qdrant` and a real `EMBEDDING_API_KEY` are available:

1. `topcoat dev`
2. Visit `http://127.0.0.1:3000/search` — confirm the empty-state prompt shows with no query submitted.
3. Submit a query with no Hadiths embedded yet — confirm the "No matching Hadiths" empty state (not an error).
4. After running `import_hadiths --embed` against some data, repeat the search — confirm result cards render with Arabic/English text, collection badge, book/Hadith numbers, and a working record link.
5. Select a specific collection in the dropdown and confirm the `collection` filter narrows results.

If this environment isn't available, report exactly which verification was skipped, per `AGENTS.md`.

- [ ] **Step 6: Leave the change for review**

Do not commit. Report the files touched and the verification results (Step 4's command output) for the user's review.

---

## Self-Review Notes

- **Spec coverage:** route/handler, collection dropdown, all five page states, nav link, and explicit out-of-scope items (no score display, no limit control, no new automated tests) are all covered by Task 1's single deliverable.
- **Placeholder scan:** none — all code is complete and verified against the actual `RetrievalService`/`CollectionService`/domain types already in the codebase, and against Topcoat 0.5.0's actual `view!` boolean-attribute behavior (`attr=(bool_expr)` omits the attribute on `false`/`None`, confirmed against `topcoat-view-macro-0.5.0`'s docs — this is what makes `selected=(...)` on `<option>` work).
- **Type consistency:** `RetrievalQuery`, `RetrievalResult`, `RetrievedHadith`, `Collection`, and `AppError` field/variant names match `src/domain/models.rs` and `src/error.rs` exactly, verified by reading both files before writing this plan.
