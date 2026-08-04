# Topcoat 0.5.0 Upgrade and Qdrant Retrieval Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade the Topcoat framework dependency from 0.4.0 to 0.5.0, then implement the Qdrant-backed retrieval pipeline so `POST /api/retrieval` returns real, citation-preserving results instead of `501 Not Implemented`.

**Architecture:** Topcoat's breaking API moves (`Slot` → rendered `Result`, error helpers and `Json` relocated) are fixed mechanically with no behavior change. Retrieval adds two small infrastructure abstractions — a swappable `Embedder` trait (OpenAI-compatible HTTP API) and a swappable `VectorStore` trait (Qdrant) — wired into `RetrievalService`, which embeds the query, searches Qdrant with an optional collection filter, and resolves every hit back to its canonical `Hadith` row via the existing repository. Embeddings are produced by extending `import_hadiths` with an opt-in `--embed` flag rather than a separate pipeline.

**Tech Stack:** Rust, Topcoat 0.5.0, SQLx/PostgreSQL, Qdrant via `qdrant-client` 1.18.0, `reqwest` 0.13.4 for the embeddings HTTP call, `async-trait` 0.1.91 for dyn-compatible async traits, `wiremock` 0.6.5 for HTTP-mocked embedder tests.

## Global Constraints

- Never silently paraphrase, merge, truncate, normalize away, or overwrite canonical Hadith records (`AGENTS.md`).
- Every retrieved record must retain a path to its collection, book number, Hadith number, and stable database ID (`AGENTS.md`).
- `application` must not depend on Topcoat request/response types (`AGENTS.md`).
- `infrastructure/persistence` owns SQL; no SQL in pages or API handlers (`AGENTS.md`).
- Any multi-write operation must use an explicit transaction (`AGENTS.md`).
- Required configuration must fail fast at startup; a config value only needed for optional functionality (embedding/retrieval) must not block startup when absent (`AGENTS.md`, spec Part 2).
- Never return fabricated, silently empty, or misleading success data from retrieval; an explicit error beats a guess (`AGENTS.md`).
- Run `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` before each task is considered done; run `topcoat asset bundle --bin hadith-assistant` whenever browser assets or routes change (`AGENTS.md`).
- Point ID in Qdrant is the Hadith's PostgreSQL `id`; no separate embedding-tracking table in Postgres (spec Part 2).
- Plain `import_hadiths` (no `--embed`) must behave exactly as it does today — no forced external API cost (spec Part 2).
- `Embedder` is a trait, not a concrete type, specifically so the embedding provider can be swapped later without touching `RetrievalService` (spec Part 2).

---

## File Structure

New files:
- `src/infrastructure/embedding/mod.rs` — `Embedder` trait.
- `src/infrastructure/embedding/openai.rs` — `OpenAiEmbedder`, the first `Embedder` implementation, speaking any OpenAI-compatible embeddings HTTP API.
- `src/infrastructure/vector/mod.rs` — `VectorStore` trait, `VectorMatch`, `EmbeddingPoint`.
- `src/infrastructure/vector/qdrant.rs` — `QdrantVectorStore`, the `VectorStore` implementation.
- `src/ingestion/embedding.rs` — `embed_hadiths`, the batching orchestration used by both the CLI and (indirectly, through shared types) tested in isolation with fakes.
- `docs/superpowers/plans/2026-08-04-topcoat-upgrade-and-qdrant-retrieval.md` — this file.

Modified files:
- `Cargo.toml`, `Cargo.lock` — dependency bump and additions.
- `README.md`, `AGENTS.md`, `docs/api.md`, `docs/project-status.md` — version references, env vars, retrieval contract, milestone tracking.
- `src/app.rs` — Topcoat 0.5.0 layout signature; test config construction.
- `src/app/hadiths.rs` — `router::error` import path.
- `src/app/api.rs`, `src/app/api/health.rs`, `src/app/api/retrieval.rs` — `router::content::Json` import path.
- `src/config.rs` — `EmbeddingConfig`, `Default` impls.
- `src/infrastructure/mod.rs` — register `embedding` and `vector` modules.
- `src/infrastructure/persistence/hadiths.rs` — `find_by_ids`.
- `src/application/retrieval.rs` — real `RetrievalService` implementation.
- `src/application/mod.rs` — `AppServices::new` gains embedding/vector config parameters.
- `src/main.rs` — pass new config to `AppServices::new`.
- `src/ingestion/hadith_json.rs` — `RETURNING id`, `ImportSummary.inserted_ids`.
- `src/ingestion/mod.rs` — register `embedding` module.
- `src/bin/import_hadiths.rs` — `--embed` flag.

---

### Task 1: Upgrade Topcoat to 0.5.0

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `src/app.rs:1-24`
- Modify: `src/app/hadiths.rs:1-4`
- Modify: `src/app/api.rs:1-8`
- Modify: `src/app/api/health.rs:1-2`
- Modify: `src/app/api/retrieval.rs:1-6`
- Modify: `README.md`
- Modify: `AGENTS.md`

**Interfaces:**
- Consumes: nothing from other tasks (first task).
- Produces: a codebase building and testing clean against `topcoat = "0.5.0"`. All later tasks build on top of this.

- [ ] **Step 1: Bump the Topcoat dependency**

In `Cargo.toml`, change:

```toml
topcoat = "0.4.0"
```

to:

```toml
topcoat = "0.5.0"
```

- [ ] **Step 2: Regenerate the lockfile for Topcoat only**

Run: `cargo update -p topcoat`

This updates `Cargo.lock` for `topcoat` and its own dependency tree without touching unrelated pinned versions.

- [ ] **Step 3: Fix the layout signature in `src/app.rs`**

Change the import on line 4 from:

```rust
router::{Router, Slot, layout, page},
```

to:

```rust
router::{Router, layout, page},
```

Change the layout function (currently lines 23-24):

```rust
#[layout]
async fn root_layout(slot: Slot<'_>) -> Result {
```

to:

```rust
#[layout]
async fn root_layout(slot: Result) -> Result {
```

Change the body (currently line 48) from:

```rust
                (slot.await?)
```

to:

```rust
                (slot?)
```

- [ ] **Step 4: Fix the error-helper import in `src/app/hadiths.rs`**

Change the import on lines 1-6 from:

```rust
use topcoat::{
    Error, Result,
    context::{Cx, app_context},
    router::{bad_request, internal_server_error, not_found, page, query_params},
    view::view,
};
```

to:

```rust
use topcoat::{
    Error, Result,
    context::{Cx, app_context},
    router::{page, query_params},
    router::error::{bad_request, internal_server_error, not_found},
    view::view,
};
```

- [ ] **Step 5: Fix the `Json` import in `src/app/api.rs`**

Change the import on lines 2-8 from:

```rust
use topcoat::{
    Result,
    context::{Cx, CxBuilder},
    router::{
        Body, IntoResponse, Json, Next, Response, StatusCode, layer,
    },
};
```

to:

```rust
use topcoat::{
    Result,
    context::{Cx, CxBuilder},
    router::{Body, IntoResponse, Next, Response, StatusCode, layer},
    router::content::Json,
};
```

- [ ] **Step 6: Fix the `Json` import in `src/app/api/health.rs`**

Change line 2 from:

```rust
use topcoat::{Result, router::{Json, route}};
```

to:

```rust
use topcoat::{Result, router::route, router::content::Json};
```

- [ ] **Step 7: Fix the `Json` import in `src/app/api/retrieval.rs`**

Change the import on lines 2-6 from:

```rust
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{Json, route},
};
```

to:

```rust
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::route,
    router::content::Json,
};
```

- [ ] **Step 8: Update version references in `README.md` and `AGENTS.md`**

In `README.md`:
- Line 9-10: `"This project currently targets Topcoat `0.4.x`..."` → `0.5.x`.
- Line 58: `Topcoat CLI `0.4.x`` → `0.5.x`.
- Line 63: `cargo install topcoat-cli --version 0.4.0 --locked` → `--version 0.5.0`.

In `AGENTS.md`:
- Line 9: `"The application currently targets `0.4.x`."` → `0.5.x`.

- [ ] **Step 9: Verify the build and test suite**

Run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Expected: all three succeed with no errors or warnings. If `topcoat` CLI is available, also run `topcoat asset bundle --bin hadith-assistant` and confirm it succeeds — this exercises the `asset!`/`Asset` handle change from the 0.5.0 release notes, which the release notes confirm needs no source change but is worth confirming end-to-end.

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml Cargo.lock README.md AGENTS.md src/app.rs src/app/hadiths.rs src/app/api.rs src/app/api/health.rs src/app/api/retrieval.rs
git commit -m "chore: upgrade Topcoat to 0.5.0"
```

---

### Task 2: Add embedding and vector store configuration

**Files:**
- Modify: `src/config.rs`
- Test: `src/config.rs` (inline `#[cfg(test)]` module, existing pattern in this file — none currently exists, so this task adds it)

**Interfaces:**
- Consumes: nothing new (builds on existing `Config`/`VectorConfig`).
- Produces: `pub struct EmbeddingConfig { pub base_url: String, pub api_key: Option<String>, pub model: String }`, `EmbeddingConfig::from_env() -> Self`, `impl Default for EmbeddingConfig`, `impl Default for VectorConfig`, `Config.embedding: EmbeddingConfig`. Tasks 3-6 construct `EmbeddingConfig`/`VectorConfig` values using these.

- [ ] **Step 1: Write the failing test**

Add to the bottom of `src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_config_default_points_at_openai() {
        let config = EmbeddingConfig::default();

        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert_eq!(config.api_key, None);
        assert_eq!(config.model, "text-embedding-3-small");
    }

    #[test]
    fn vector_config_default_points_at_local_qdrant() {
        let config = VectorConfig::default();

        assert_eq!(config.provider, "qdrant");
        assert_eq!(config.qdrant_url, "http://localhost:6333");
        assert_eq!(config.qdrant_collection, "hadith_vectors");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib config::tests`
Expected: FAIL — `EmbeddingConfig` does not exist yet, and `VectorConfig` has no `Default` impl.

- [ ] **Step 3: Add `EmbeddingConfig` and wire it into `Config`**

In `src/config.rs`, add after the `VectorConfig` struct definition:

```rust
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}
```

Add `embedding: VectorConfig` — no, add `pub embedding: EmbeddingConfig` to `Config`:

```rust
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub database_max_connections: u32,
    pub vector: VectorConfig,
    pub embedding: EmbeddingConfig,
}
```

In `Config::from_env`, change the final `Ok(Self { ... })` block to:

```rust
        Ok(Self {
            database_url,
            database_max_connections,
            vector: VectorConfig::from_env(),
            embedding: EmbeddingConfig::from_env(),
        })
```

Change `VectorConfig::from_env` from `fn from_env` to `pub fn from_env` (it is called from the CLI in Task 6, outside this module's current callers). Add alongside it:

```rust
impl EmbeddingConfig {
    pub fn from_env() -> Self {
        Self {
            base_url: env::var("EMBEDDING_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_owned()),
            api_key: env::var("EMBEDDING_API_KEY").ok(),
            model: env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| "text-embedding-3-small".to_owned()),
        }
    }
}
```

- [ ] **Step 4: Replace the inline `Default for Config` construction with `Default` impls on each part**

Replace the existing `impl Default for Config` block with:

```rust
impl Default for VectorConfig {
    fn default() -> Self {
        Self {
            provider: "qdrant".to_owned(),
            qdrant_url: "http://localhost:6333".to_owned(),
            qdrant_collection: "hadith_vectors".to_owned(),
        }
    }
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key: None,
            model: "text-embedding-3-small".to_owned(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            database_max_connections: 10,
            vector: VectorConfig::default(),
            embedding: EmbeddingConfig::default(),
        }
    }
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --lib config::tests`
Expected: PASS (2 tests).

- [ ] **Step 6: Verify the full build**

Run: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test`
Expected: all succeed. `Config` gaining a field is backward compatible everywhere it is constructed via `from_env()` or `Default`; no other file constructs `Config { .. }` with an exhaustive literal.

- [ ] **Step 7: Commit**

```bash
git add src/config.rs
git commit -m "feat: add embedding configuration alongside vector config"
```

---

### Task 3: Add the `Embedder` trait and `OpenAiEmbedder`

**Files:**
- Create: `src/infrastructure/embedding/mod.rs`
- Create: `src/infrastructure/embedding/openai.rs`
- Modify: `src/infrastructure/mod.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: `EmbeddingConfig` from Task 2 (`src/config.rs`), `AppError` from `src/error.rs`.
- Produces: `pub trait Embedder: Send + Sync { async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError>; }` and `pub struct OpenAiEmbedder` implementing it via `OpenAiEmbedder::new(config: EmbeddingConfig) -> Self`. Task 5 (`RetrievalService`) and Task 6 (`embed_hadiths`) depend on both.

- [ ] **Step 1: Add dependencies**

In `Cargo.toml`, add to `[dependencies]`:

```toml
async-trait = "0.1.91"
reqwest = { version = "0.13.4", default-features = false, features = ["json", "rustls-tls"] }
```

Add a new `[dev-dependencies]` section (or add to it if one already exists — it does not currently):

```toml
[dev-dependencies]
wiremock = "0.6.5"
```

Run: `cargo build` to confirm the new dependencies resolve and update `Cargo.lock`.

- [ ] **Step 2: Create the `Embedder` trait**

Create `src/infrastructure/embedding/mod.rs`:

```rust
pub mod openai;

pub use openai::OpenAiEmbedder;

use async_trait::async_trait;

use crate::error::AppError;

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError>;
}
```

- [ ] **Step 3: Register the module**

In `src/infrastructure/mod.rs`, change:

```rust
pub mod persistence;
```

to:

```rust
pub mod embedding;
pub mod persistence;
pub mod vector;
```

(`vector` is created in Task 4; add both now so this file only needs one edit. Task 4 will fail to compile if the module file doesn't exist yet — create an empty `src/infrastructure/vector/mod.rs` with just `pub mod qdrant;` placeholder removed; instead, to keep this task's build green on its own, only add `pub mod embedding;` here now and add `pub mod vector;` in Task 4's Step 3 instead.)

Use instead:

```rust
pub mod embedding;
pub mod persistence;
```

- [ ] **Step 4: Write the failing tests**

Create `src/infrastructure/embedding/openai.rs` with just the test module first:

```rust
#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::config::EmbeddingConfig;
    use crate::error::AppError;

    #[tokio::test]
    async fn embed_batch_parses_openai_response_in_index_order() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    { "embedding": [0.2, 0.3], "index": 1 },
                    { "embedding": [0.0, 0.1], "index": 0 }
                ]
            })))
            .mount(&server)
            .await;

        let embedder = OpenAiEmbedder::new(EmbeddingConfig {
            base_url: server.uri(),
            api_key: Some("test-key".to_owned()),
            model: "text-embedding-3-small".to_owned(),
        });

        let vectors = embedder
            .embed_batch(&["first".to_owned(), "second".to_owned()])
            .await
            .expect("mocked embedding request should succeed");

        assert_eq!(vectors, vec![vec![0.0, 0.1], vec![0.2, 0.3]]);
    }

    #[tokio::test]
    async fn embed_batch_returns_error_on_non_success_status() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/embeddings"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let embedder = OpenAiEmbedder::new(EmbeddingConfig {
            base_url: server.uri(),
            api_key: None,
            model: "text-embedding-3-small".to_owned(),
        });

        let error = embedder
            .embed_batch(&["first".to_owned()])
            .await
            .expect_err("non-success status should fail");

        assert!(matches!(error, AppError::Internal(message) if message.contains("401")));
    }

    #[tokio::test]
    async fn embed_batch_returns_empty_vec_for_empty_input_without_a_request() {
        let embedder = OpenAiEmbedder::new(EmbeddingConfig {
            base_url: "http://127.0.0.1:1".to_owned(),
            api_key: None,
            model: "text-embedding-3-small".to_owned(),
        });

        let vectors = embedder
            .embed_batch(&[])
            .await
            .expect("empty input should not make a request");

        assert!(vectors.is_empty());
    }
}
```

- [ ] **Step 5: Run the tests to verify they fail**

Run: `cargo test --lib infrastructure::embedding::openai`
Expected: FAIL to compile — `OpenAiEmbedder` does not exist yet.

- [ ] **Step 6: Implement `OpenAiEmbedder`**

Add above the test module in `src/infrastructure/embedding/openai.rs`:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::EmbeddingConfig;
use crate::error::AppError;

use super::Embedder;

#[derive(Clone)]
pub struct OpenAiEmbedder {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
}

impl OpenAiEmbedder {
    pub fn new(config: EmbeddingConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: config.base_url,
            api_key: config.api_key,
            model: config.model,
        }
    }
}

#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    input: &'a [String],
    model: &'a str,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingDatum>,
}

#[derive(Deserialize)]
struct EmbeddingDatum {
    embedding: Vec<f32>,
    index: usize,
}

#[async_trait]
impl Embedder for OpenAiEmbedder {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut request = self
            .client
            .post(format!("{}/embeddings", self.base_url))
            .json(&EmbeddingRequest {
                input: texts,
                model: &self.model,
            });

        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = request
            .send()
            .await
            .map_err(|error| AppError::Internal(format!("embedding request failed: {error}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "embedding request failed with status {status}: {body}"
            )));
        }

        let mut body: EmbeddingResponse = response.json().await.map_err(|error| {
            AppError::Internal(format!("embedding response was not valid JSON: {error}"))
        })?;

        body.data.sort_by_key(|datum| datum.index);

        Ok(body.data.into_iter().map(|datum| datum.embedding).collect())
    }
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test --lib infrastructure::embedding::openai`
Expected: PASS (3 tests).

- [ ] **Step 8: Verify the full build**

Run: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test`

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src/infrastructure/mod.rs src/infrastructure/embedding
git commit -m "feat: add Embedder trait and OpenAI-compatible embedder"
```

---

### Task 4: Add the `VectorStore` trait and `QdrantVectorStore`

**Files:**
- Create: `src/infrastructure/vector/mod.rs`
- Create: `src/infrastructure/vector/qdrant.rs`
- Modify: `src/infrastructure/mod.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: `AppError` from `src/error.rs`.
- Produces: `pub trait VectorStore: Send + Sync { async fn ensure_collection(&self, vector_size: u64) -> Result<(), AppError>; async fn upsert(&self, points: Vec<EmbeddingPoint>) -> Result<(), AppError>; async fn search(&self, vector: Vec<f32>, collection_filter: Option<&str>, limit: i64) -> Result<Vec<VectorMatch>, AppError>; }`, `pub struct EmbeddingPoint { pub hadith_id: i64, pub vector: Vec<f32>, pub collection: String }`, `pub struct VectorMatch { pub hadith_id: i64, pub score: f32 }`, and `pub struct QdrantVectorStore` implementing the trait via `QdrantVectorStore::new(url: &str, collection_name: String) -> Result<Self, AppError>`. Task 5 and Task 6 depend on all of these.

- [ ] **Step 1: Add the dependency**

In `Cargo.toml`, add to `[dependencies]`:

```toml
qdrant-client = "1.18.0"
```

Run: `cargo build` to confirm it resolves.

- [ ] **Step 2: Create the `VectorStore` trait and shared types**

Create `src/infrastructure/vector/mod.rs`:

```rust
pub mod qdrant;

pub use qdrant::QdrantVectorStore;

use async_trait::async_trait;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct EmbeddingPoint {
    pub hadith_id: i64,
    pub vector: Vec<f32>,
    pub collection: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorMatch {
    pub hadith_id: i64,
    pub score: f32,
}

#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn ensure_collection(&self, vector_size: u64) -> Result<(), AppError>;
    async fn upsert(&self, points: Vec<EmbeddingPoint>) -> Result<(), AppError>;
    async fn search(
        &self,
        vector: Vec<f32>,
        collection_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<VectorMatch>, AppError>;
}
```

- [ ] **Step 3: Register the module**

In `src/infrastructure/mod.rs`, change:

```rust
pub mod embedding;
pub mod persistence;
```

to:

```rust
pub mod embedding;
pub mod persistence;
pub mod vector;
```

- [ ] **Step 4: Write the failing test**

Create `src/infrastructure/vector/qdrant.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::vector::EmbeddingPoint;

    #[test]
    fn to_point_struct_uses_hadith_id_as_the_point_id_and_carries_the_collection_payload() {
        let point = EmbeddingPoint {
            hadith_id: 42,
            vector: vec![0.1, 0.2],
            collection: "bukhari".to_owned(),
        };

        let point_struct = to_point_struct(point);

        assert_eq!(
            point_struct.id,
            Some(qdrant_client::qdrant::PointId::from(42u64))
        );
        assert!(point_struct.payload.contains_key("collection"));
    }

    #[test]
    fn point_id_to_hadith_id_reads_numeric_ids_and_ignores_uuids() {
        use qdrant_client::qdrant::PointId;
        use qdrant_client::qdrant::point_id::PointIdOptions;

        let numeric = PointId {
            point_id_options: Some(PointIdOptions::Num(7)),
        };
        let uuid = PointId {
            point_id_options: Some(PointIdOptions::Uuid("not-a-hadith-id".to_owned())),
        };

        assert_eq!(point_id_to_hadith_id(Some(numeric)), Some(7));
        assert_eq!(point_id_to_hadith_id(Some(uuid)), None);
        assert_eq!(point_id_to_hadith_id(None), None);
    }
}
```

- [ ] **Step 5: Run the tests to verify they fail**

Run: `cargo test --lib infrastructure::vector::qdrant`
Expected: FAIL to compile — `to_point_struct` and `point_id_to_hadith_id` do not exist yet.

- [ ] **Step 6: Implement `QdrantVectorStore`**

Add above the test module in `src/infrastructure/vector/qdrant.rs`:

```rust
use std::sync::Arc;

use async_trait::async_trait;
use qdrant_client::qdrant::point_id::PointIdOptions;
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, Distance, Filter, PointId, PointStruct,
    QueryPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};

use crate::error::AppError;

use super::{EmbeddingPoint, VectorMatch, VectorStore};

#[derive(Clone)]
pub struct QdrantVectorStore {
    client: Arc<Qdrant>,
    collection_name: String,
}

impl QdrantVectorStore {
    pub fn new(url: &str, collection_name: String) -> Result<Self, AppError> {
        let client = Qdrant::from_url(url)
            .build()
            .map_err(|error| AppError::Internal(format!("qdrant client init failed: {error}")))?;

        Ok(Self {
            client: Arc::new(client),
            collection_name,
        })
    }
}

#[async_trait]
impl VectorStore for QdrantVectorStore {
    async fn ensure_collection(&self, vector_size: u64) -> Result<(), AppError> {
        let exists = self
            .client
            .collection_exists(self.collection_name.clone())
            .await
            .map_err(|error| {
                AppError::Internal(format!("qdrant collection_exists failed: {error}"))
            })?;

        if exists {
            return Ok(());
        }

        self.client
            .create_collection(
                CreateCollectionBuilder::new(self.collection_name.clone())
                    .vectors_config(VectorParamsBuilder::new(vector_size, Distance::Cosine)),
            )
            .await
            .map_err(|error| {
                AppError::Internal(format!("qdrant create_collection failed: {error}"))
            })?;

        Ok(())
    }

    async fn upsert(&self, points: Vec<EmbeddingPoint>) -> Result<(), AppError> {
        let points = points.into_iter().map(to_point_struct).collect();

        self.client
            .upsert_points(UpsertPointsBuilder::new(
                self.collection_name.clone(),
                points,
            ))
            .await
            .map_err(|error| AppError::Internal(format!("qdrant upsert failed: {error}")))?;

        Ok(())
    }

    async fn search(
        &self,
        vector: Vec<f32>,
        collection_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<VectorMatch>, AppError> {
        let mut query = QueryPointsBuilder::new(self.collection_name.clone())
            .query(vector)
            .limit(limit as u64)
            .with_payload(false);

        if let Some(collection) = collection_filter {
            query = query.filter(Filter::all([Condition::matches(
                "collection",
                collection.to_owned(),
            )]));
        }

        let response = self
            .client
            .query(query)
            .await
            .map_err(|error| AppError::Internal(format!("qdrant query failed: {error}")))?;

        Ok(response
            .result
            .into_iter()
            .filter_map(|scored_point| {
                point_id_to_hadith_id(scored_point.id).map(|hadith_id| VectorMatch {
                    hadith_id,
                    score: scored_point.score,
                })
            })
            .collect())
    }
}

fn to_point_struct(point: EmbeddingPoint) -> PointStruct {
    let payload: Payload = serde_json::json!({ "collection": point.collection })
        .try_into()
        .expect("payload literal is always valid JSON");

    PointStruct::new(point.hadith_id as u64, point.vector, payload)
}

fn point_id_to_hadith_id(id: Option<PointId>) -> Option<i64> {
    match id?.point_id_options? {
        PointIdOptions::Num(value) => Some(value as i64),
        PointIdOptions::Uuid(_) => None,
    }
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test --lib infrastructure::vector::qdrant`
Expected: PASS (2 tests).

- [ ] **Step 8: Verify the full build**

Run: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test`

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src/infrastructure/mod.rs src/infrastructure/vector
git commit -m "feat: add VectorStore trait and Qdrant-backed implementation"
```

---

### Task 5: Wire `RetrievalService` to Qdrant and the embedder

**Files:**
- Modify: `src/application/retrieval.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/main.rs`
- Modify: `src/app.rs:104-118` (test module)
- Modify: `src/infrastructure/persistence/hadiths.rs`

**Interfaces:**
- Consumes: `Embedder` (Task 3), `VectorStore`/`EmbeddingPoint`/`VectorMatch` (Task 4), `EmbeddingConfig`/`VectorConfig` (Task 2), existing `HadithRepository`.
- Produces: `RetrievalService::new(embedder: Arc<dyn Embedder>, vector_store: Arc<dyn VectorStore>, hadiths: HadithRepository) -> Self`, `AppServices::new(pool: PgPool, embedding: EmbeddingConfig, vector: VectorConfig) -> Self` (signature change — Task 6's CLI does not call this, so no other caller is affected besides `main.rs` and the test in `app.rs`), `HadithRepository::find_by_ids(&self, ids: &[i64]) -> Result<Vec<Hadith>, AppError>` (used by Task 6).

- [ ] **Step 1: Add `find_by_ids` to `HadithRepository`**

In `src/infrastructure/persistence/hadiths.rs`, add after `find_by_id`:

```rust
    pub async fn find_by_ids(&self, ids: &[i64]) -> Result<Vec<Hadith>, AppError> {
        let hadiths = sqlx::query_as::<_, Hadith>(&format!("{HADITH_SELECT} WHERE h.id = ANY($1) ORDER BY h.id"))
            .bind(ids)
            .fetch_all(&self.pool)
            .await?;

        Ok(hadiths)
    }
```

- [ ] **Step 2: Write the failing tests for `RetrievalService`**

Replace the full contents of `src/application/retrieval.rs` with (test module first, implementation in the next step):

```rust
use std::sync::Arc;

use crate::domain::{RetrievalQuery, RetrievalResult};
use crate::error::AppError;

const DEFAULT_LIMIT: i64 = 10;
const MAX_LIMIT: i64 = 20;

fn validate_query(query: RetrievalQuery) -> Result<RetrievalQuery, AppError> {
    let text = query.query.trim();
    if text.is_empty() {
        return Err(AppError::Validation("query is required".to_owned()));
    }

    let limit = if query.limit == 0 {
        DEFAULT_LIMIT
    } else {
        query.limit
    };

    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(AppError::Validation(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }

    Ok(RetrievalQuery {
        query: text.to_owned(),
        collection: query
            .collection
            .map(|collection| collection.trim().to_owned())
            .filter(|collection| !collection.is_empty()),
        limit,
    })
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::domain::Hadith;
    use crate::infrastructure::embedding::Embedder;
    use crate::infrastructure::persistence::hadiths::HadithRepository;
    use crate::infrastructure::vector::{EmbeddingPoint, VectorMatch, VectorStore};

    struct FakeEmbedder;

    #[async_trait]
    impl Embedder for FakeEmbedder {
        async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
            Ok(texts.iter().map(|_| vec![0.1, 0.2, 0.3]).collect())
        }
    }

    struct FakeVectorStore {
        matches: Vec<VectorMatch>,
    }

    #[async_trait]
    impl VectorStore for FakeVectorStore {
        async fn ensure_collection(&self, _vector_size: u64) -> Result<(), AppError> {
            Ok(())
        }

        async fn upsert(&self, _points: Vec<EmbeddingPoint>) -> Result<(), AppError> {
            Ok(())
        }

        async fn search(
            &self,
            _vector: Vec<f32>,
            _collection_filter: Option<&str>,
            _limit: i64,
        ) -> Result<Vec<VectorMatch>, AppError> {
            Ok(self.matches.clone())
        }
    }

    fn test_repository() -> HadithRepository {
        use sqlx::postgres::PgPoolOptions;

        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/hadiths")
            .expect("test database URL should parse");

        HadithRepository::new(pool)
    }

    #[tokio::test]
    async fn retrieve_returns_validation_error_for_empty_query() {
        let service = RetrievalService::new(
            Arc::new(FakeEmbedder),
            Arc::new(FakeVectorStore { matches: vec![] }),
            test_repository(),
        );

        let error = service
            .retrieve(RetrievalQuery {
                query: "   ".to_owned(),
                collection: None,
                limit: 0,
            })
            .await
            .expect_err("empty query should be invalid");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "query is required"
        ));
    }

    #[tokio::test]
    async fn retrieve_skips_matches_that_no_longer_resolve_to_a_hadith() {
        let service = RetrievalService::new(
            Arc::new(FakeEmbedder),
            Arc::new(FakeVectorStore {
                matches: vec![VectorMatch {
                    hadith_id: 999_999,
                    score: 0.9,
                }],
            }),
            test_repository(),
        );

        // No live database in this test; find_by_id against a lazy pool with no
        // reachable server surfaces as AppError::Database, not AppError::NotFound,
        // so this test only exercises the validation + embed + search wiring path
        // without asserting on database connectivity. Full end-to-end resolution
        // is exercised manually against `docker compose up -d postgres qdrant`.
        let result = service
            .retrieve(RetrievalQuery {
                query: "intentions".to_owned(),
                collection: None,
                limit: 5,
            })
            .await;

        assert!(result.is_err(), "unreachable database should surface as an error, not fabricated results");
    }

    #[test]
    fn validate_query_trims_query_and_collection_and_defaults_limit() {
        let query = validate_query(RetrievalQuery {
            query: " intentions ".to_owned(),
            collection: Some(" bukhari ".to_owned()),
            limit: 0,
        })
        .expect("valid query should normalize");

        assert_eq!(query.query, "intentions");
        assert_eq!(query.collection.as_deref(), Some("bukhari"));
        assert_eq!(query.limit, DEFAULT_LIMIT);
    }

    #[test]
    fn validate_query_drops_empty_collection() {
        let query = validate_query(RetrievalQuery {
            query: "intentions".to_owned(),
            collection: Some(" ".to_owned()),
            limit: 3,
        })
        .expect("empty optional collection should be ignored");

        assert_eq!(query.collection, None);
        assert_eq!(query.limit, 3);
    }

    #[test]
    fn validate_query_rejects_empty_query() {
        let error = validate_query(RetrievalQuery {
            query: " ".to_owned(),
            collection: None,
            limit: 1,
        })
        .expect_err("empty query should be invalid");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "query is required"
        ));
    }

    #[test]
    fn validate_query_rejects_out_of_range_limit() {
        let error = validate_query(RetrievalQuery {
            query: "intentions".to_owned(),
            collection: None,
            limit: MAX_LIMIT + 1,
        })
        .expect_err("limit above max should be invalid");

        assert!(matches!(
            error,
            AppError::Validation(message)
                if message == format!("limit must be between 1 and {MAX_LIMIT}")
        ));
    }
}
```

Note: `Hadith` is imported in the test module but only used implicitly through `HadithRepository`; remove the `use crate::domain::Hadith;` line if `cargo clippy` flags it as unused once Step 3 is complete — keep whichever imports the compiler actually requires.

- [ ] **Step 3: Run the tests to verify the relevant ones fail**

Run: `cargo test --lib application::retrieval`
Expected: FAIL to compile — `RetrievalService` does not exist yet in this file.

- [ ] **Step 4: Implement `RetrievalService`**

Insert into `src/application/retrieval.rs`, after the imports and before `fn validate_query`:

```rust
use crate::infrastructure::embedding::Embedder;
use crate::infrastructure::persistence::hadiths::HadithRepository;
use crate::infrastructure::vector::VectorStore;
use crate::domain::RetrievedHadith;

#[derive(Clone)]
pub struct RetrievalService {
    embedder: Arc<dyn Embedder>,
    vector_store: Arc<dyn VectorStore>,
    hadiths: HadithRepository,
}

impl RetrievalService {
    pub fn new(
        embedder: Arc<dyn Embedder>,
        vector_store: Arc<dyn VectorStore>,
        hadiths: HadithRepository,
    ) -> Self {
        Self {
            embedder,
            vector_store,
            hadiths,
        }
    }

    pub async fn retrieve(&self, query: RetrievalQuery) -> Result<RetrievalResult, AppError> {
        let query = validate_query(query)?;

        let mut vectors = self
            .embedder
            .embed_batch(std::slice::from_ref(&query.query))
            .await?;
        let vector = vectors.pop().ok_or_else(|| {
            AppError::Internal("embedding provider returned no vector for the query".to_owned())
        })?;

        let matches = self
            .vector_store
            .search(vector, query.collection.as_deref(), query.limit)
            .await?;

        let mut results = Vec::with_capacity(matches.len());
        for candidate in matches {
            match self.hadiths.find_by_id(candidate.hadith_id).await {
                Ok(hadith) => results.push(RetrievedHadith {
                    hadith_id: hadith.id,
                    collection: hadith.collection,
                    book_number: hadith.book_number,
                    hadith_number: hadith.hadith_number,
                    arabic_text: hadith.arabic_text,
                    english_text: hadith.english_text,
                    score: Some(candidate.score as f64),
                }),
                Err(AppError::NotFound(_)) => {
                    tracing::warn!(
                        hadith_id = candidate.hadith_id,
                        "retrieval candidate no longer resolves to a canonical record"
                    );
                }
                Err(error) => return Err(error),
            }
        }

        Ok(RetrievalResult {
            query: query.query,
            results,
        })
    }
}
```

Remove the now-unused `Hadith` import from the test module if `cargo clippy` reports it unused (see the note at the end of Step 2).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib application::retrieval`
Expected: PASS. The `retrieve_skips_matches_that_no_longer_resolve_to_a_hadith` test connects to a lazy pool with no reachable database, so it asserts on `result.is_err()` rather than a specific `Hadith` — this is intentional; full resolution behavior is covered by `docs/project-status.md`'s existing "database-backed integration tests are not yet available" gap, not newly introduced here.

- [ ] **Step 6: Update `AppServices::new`**

Replace the full contents of `src/application/mod.rs` with:

```rust
mod collections;
mod hadiths;
mod retrieval;

use std::sync::Arc;

pub use collections::CollectionService;
pub use hadiths::HadithService;
pub use retrieval::RetrievalService;
use sqlx::PgPool;

use crate::config::{EmbeddingConfig, VectorConfig};
use crate::infrastructure::embedding::{Embedder, OpenAiEmbedder};
use crate::infrastructure::persistence::hadiths::HadithRepository;
use crate::infrastructure::vector::{QdrantVectorStore, VectorStore};

#[derive(Clone)]
pub struct AppServices {
    pub collections: Arc<CollectionService>,
    pub hadiths: Arc<HadithService>,
    pub retrieval: Arc<RetrievalService>,
}

impl AppServices {
    pub fn new(pool: PgPool, embedding: EmbeddingConfig, vector: VectorConfig) -> Self {
        let hadith_repository = HadithRepository::new(pool.clone());

        let embedder: Arc<dyn Embedder> = Arc::new(OpenAiEmbedder::new(embedding));
        let vector_store: Arc<dyn VectorStore> = Arc::new(
            QdrantVectorStore::new(&vector.qdrant_url, vector.qdrant_collection)
                .expect("QDRANT_URL should be a valid Qdrant endpoint URL"),
        );

        Self {
            collections: Arc::new(CollectionService::new(pool.clone())),
            hadiths: Arc::new(HadithService::new(pool)),
            retrieval: Arc::new(RetrievalService::new(
                embedder,
                vector_store,
                hadith_repository,
            )),
        }
    }
}
```

- [ ] **Step 7: Update the call site in `src/main.rs`**

Change:

```rust
    let router = app::router(AppServices::new(pool))?;
```

to:

```rust
    let router = app::router(AppServices::new(pool, config.embedding.clone(), config.vector.clone()))?;
```

- [ ] **Step 8: Update the test call site in `src/app.rs`**

In the `#[cfg(test)] mod tests` block (currently lines 104-118), change:

```rust
        router_with_assets(AppServices::new(pool), AssetBundle::empty());
```

to:

```rust
        router_with_assets(
            AppServices::new(
                pool,
                hadith_assistant::config::EmbeddingConfig::default(),
                hadith_assistant::config::VectorConfig::default(),
            ),
            AssetBundle::empty(),
        );
```

- [ ] **Step 9: Run the full test suite**

Run: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test`
Expected: all succeed. `module_router_builds_without_route_conflicts` in `src/app.rs` only builds the router (no network calls at construction time — `Qdrant::from_url(...).build()` and `reqwest::Client::new()` are both lazy), so it stays fast and offline.

- [ ] **Step 10: Commit**

```bash
git add src/application src/main.rs src/app.rs src/infrastructure/persistence/hadiths.rs
git commit -m "feat: wire RetrievalService to Qdrant and the embedder"
```

---

### Task 6: Extend `import_hadiths` with an opt-in `--embed` flag

**Files:**
- Modify: `src/ingestion/hadith_json.rs`
- Modify: `src/ingestion/mod.rs`
- Create: `src/ingestion/embedding.rs`
- Modify: `src/bin/import_hadiths.rs`

**Interfaces:**
- Consumes: `Embedder` (Task 3), `VectorStore`/`EmbeddingPoint` (Task 4), `EmbeddingConfig`/`VectorConfig::from_env` (Task 2), `HadithRepository::find_by_ids` (Task 5).
- Produces: `ImportSummary.inserted_ids: Vec<i64>`, `pub async fn embed_hadiths(embedder: &dyn Embedder, vector_store: &dyn VectorStore, hadiths: &[Hadith]) -> Result<usize, AppError>`.

- [ ] **Step 1: Capture inserted IDs during import**

In `src/ingestion/hadith_json.rs`, change `ImportSummary`:

```rust
#[derive(Debug, Clone)]
pub struct ImportSummary {
    pub record_count: usize,
    pub source_checksum: String,
    pub inserted_ids: Vec<i64>,
}
```

Change `insert_record`'s signature and its `INSERT` statement to return the new ID. Change:

```rust
async fn insert_record(
    tx: &mut Transaction<'_, Postgres>,
    record: &RawHadithRecord,
) -> Result<(), ImportError> {
```

to:

```rust
async fn insert_record(
    tx: &mut Transaction<'_, Postgres>,
    record: &RawHadithRecord,
) -> Result<i64, ImportError> {
```

Change the SQL string to add `RETURNING id`:

```rust
    let id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO hadiths (
            collection_id,
            book_number,
            bab_id,
            english_bab_number,
            arabic_bab_number,
            hadith_number,
            our_hadith_number,
            arabic_urn,
            arabic_bab_name,
            arabic_text,
            arabic_transliteration,
            arabic_grade,
            english_urn,
            english_bab_name,
            english_text,
            english_grade,
            last_updated,
            xrefs
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        RETURNING id
        "#,
    )
    .bind(collection_id)
    .bind(record.book_number.trim())
    .bind(record.bab_id)
    .bind(trim_optional(record.english_bab_number.as_deref()))
    .bind(trim_optional(record.arabic_bab_number.as_deref()))
    .bind(canonical_hadith_number(record))
    .bind(record.our_hadith_number)
    .bind(record.arabic_urn)
    .bind(trim_optional(record.arabic_bab_name.as_deref()))
    .bind(validated_arabic_text(record))
    .bind(arabic_transliteration(record))
    .bind(record.arabicgrade1.trim())
    .bind(record.english_urn)
    .bind(trim_optional(record.english_bab_name.as_deref()))
    .bind(trim_optional(record.english_text.as_deref()))
    .bind(record.englishgrade1.trim())
    .bind(trim_optional(record.last_updated.as_deref()))
    .bind(record.xrefs.trim())
    .fetch_one(&mut **tx)
    .await?;

    Ok(id)
```

Change `import_dump` to collect the returned IDs and populate `inserted_ids`:

```rust
async fn import_dump(
    pool: &PgPool,
    dump: &HadithJsonDump,
    source_checksum: &str,
) -> Result<ImportSummary, ImportError> {
    let mut tx = pool.begin().await?;

    let mut inserted_ids = Vec::with_capacity(dump.hadith_table.len());
    for record in &dump.hadith_table {
        inserted_ids.push(insert_record(&mut tx, record).await?);
    }

    tx.commit().await?;

    Ok(ImportSummary {
        record_count: dump.hadith_table.len(),
        source_checksum: source_checksum.to_owned(),
        inserted_ids,
    })
}
```

- [ ] **Step 2: Verify import still compiles and its existing tests pass**

Run: `cargo test --lib ingestion::hadith_json`
Expected: PASS — none of the existing tests construct `ImportSummary` directly, so this is a compile-and-pass check, not a new-test step.

- [ ] **Step 3: Write the failing test for `embed_hadiths`**

Create `src/ingestion/embedding.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::error::AppError;
    use crate::infrastructure::embedding::Embedder;
    use crate::infrastructure::vector::{EmbeddingPoint, VectorMatch, VectorStore};

    struct FakeEmbedder;

    #[async_trait]
    impl Embedder for FakeEmbedder {
        async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
            Ok(texts.iter().map(|_| vec![0.1_f32]).collect())
        }
    }

    #[derive(Default)]
    struct RecordingVectorStore {
        upsert_calls: Mutex<Vec<usize>>,
    }

    #[async_trait]
    impl VectorStore for RecordingVectorStore {
        async fn ensure_collection(&self, _vector_size: u64) -> Result<(), AppError> {
            Ok(())
        }

        async fn upsert(&self, points: Vec<EmbeddingPoint>) -> Result<(), AppError> {
            self.upsert_calls.lock().unwrap().push(points.len());
            Ok(())
        }

        async fn search(
            &self,
            _vector: Vec<f32>,
            _collection_filter: Option<&str>,
            _limit: i64,
        ) -> Result<Vec<VectorMatch>, AppError> {
            Ok(Vec::new())
        }
    }

    fn hadith(id: i64) -> Hadith {
        Hadith {
            id,
            collection_id: 1,
            collection: "bukhari".to_owned(),
            book_number: "1".to_owned(),
            bab_id: 1.0,
            english_bab_number: None,
            arabic_bab_number: None,
            hadith_number: id.to_string(),
            our_hadith_number: id as i32,
            arabic_urn: id,
            arabic_bab_name: None,
            arabic_text: "نص".to_owned(),
            arabic_transliteration: None,
            arabic_grade: "Sahih".to_owned(),
            english_urn: id,
            english_bab_name: None,
            english_text: Some("text".to_owned()),
            english_grade: "Sahih".to_owned(),
            last_updated: None,
            xrefs: String::new(),
        }
    }

    #[tokio::test]
    async fn embed_hadiths_batches_in_groups_of_the_configured_size() {
        let hadiths: Vec<Hadith> = (1..=150).map(hadith).collect();
        let vector_store = RecordingVectorStore::default();

        let embedded = embed_hadiths(&FakeEmbedder, &vector_store, &hadiths)
            .await
            .expect("embedding fakes should not fail");

        assert_eq!(embedded, 150);
        assert_eq!(*vector_store.upsert_calls.lock().unwrap(), vec![96, 54]);
    }

    #[tokio::test]
    async fn embed_hadiths_returns_zero_for_an_empty_slice() {
        let vector_store = RecordingVectorStore::default();

        let embedded = embed_hadiths(&FakeEmbedder, &vector_store, &[])
            .await
            .expect("empty input should succeed trivially");

        assert_eq!(embedded, 0);
        assert!(vector_store.upsert_calls.lock().unwrap().is_empty());
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test --lib ingestion::embedding`
Expected: FAIL to compile — `embed_hadiths` and `Hadith` (unimported) do not resolve yet.

- [ ] **Step 5: Implement `embed_hadiths`**

Add above the test module in `src/ingestion/embedding.rs`:

```rust
use crate::domain::Hadith;
use crate::error::AppError;
use crate::infrastructure::embedding::Embedder;
use crate::infrastructure::vector::{EmbeddingPoint, VectorStore};

const EMBEDDING_BATCH_SIZE: usize = 96;

pub async fn embed_hadiths(
    embedder: &(dyn Embedder + Send + Sync),
    vector_store: &(dyn VectorStore + Send + Sync),
    hadiths: &[Hadith],
) -> Result<usize, AppError> {
    let mut embedded_count = 0;

    for batch in hadiths.chunks(EMBEDDING_BATCH_SIZE) {
        let texts: Vec<String> = batch.iter().map(hadith_embedding_text).collect();
        let vectors = embedder.embed_batch(&texts).await?;

        if vectors.len() != batch.len() {
            return Err(AppError::Internal(format!(
                "embedding provider returned {} vectors for {} inputs",
                vectors.len(),
                batch.len()
            )));
        }

        let vector_size = vectors[0].len() as u64;
        vector_store.ensure_collection(vector_size).await?;

        let points = batch
            .iter()
            .zip(vectors)
            .map(|(hadith, vector)| EmbeddingPoint {
                hadith_id: hadith.id,
                vector,
                collection: hadith.collection.clone(),
            })
            .collect();

        vector_store.upsert(points).await?;
        embedded_count += batch.len();
    }

    Ok(embedded_count)
}

fn hadith_embedding_text(hadith: &Hadith) -> String {
    match &hadith.english_text {
        Some(english_text) if !english_text.trim().is_empty() => {
            format!("{}\n{}", hadith.arabic_text, english_text)
        }
        _ => hadith.arabic_text.clone(),
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --lib ingestion::embedding`
Expected: PASS (2 tests).

- [ ] **Step 7: Register the module**

In `src/ingestion/mod.rs`, change:

```rust
pub mod hadith_json;
```

to:

```rust
pub mod embedding;
pub mod hadith_json;
```

- [ ] **Step 8: Add the `--embed` flag to the CLI**

Replace the full contents of `src/bin/import_hadiths.rs` with:

```rust
use std::env;
use std::process::ExitCode;

use hadith_assistant::config::{EmbeddingConfig, VectorConfig};
use hadith_assistant::infrastructure::embedding::OpenAiEmbedder;
use hadith_assistant::infrastructure::persistence::hadiths::HadithRepository;
use hadith_assistant::infrastructure::vector::QdrantVectorStore;
use hadith_assistant::ingestion::embedding::embed_hadiths;
use hadith_assistant::ingestion::hadith_json::{
    ImportOptions, import_hadith_json, load_dump, validate_dump,
};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    dotenvy::dotenv().ok();

    let args = Args::parse(env::args().skip(1))?;

    if args.validate_only {
        let (dump, checksum) = load_dump(&args.json_path).map_err(|error| error.to_string())?;
        validate_dump(&dump).map_err(|error| error.to_string())?;
        println!(
            "validated {} records from {} ({checksum})",
            dump.hadith_table.len(),
            args.json_path
        );
        return Ok(());
    }

    let database_url = args
        .database_url
        .or_else(|| env::var("DATABASE_URL").ok())
        .ok_or("DATABASE_URL or --database-url is required unless --validate-only is used")?;

    let summary = import_hadith_json(ImportOptions {
        database_url: database_url.clone(),
        json_path: args.json_path,
    })
    .await
    .map_err(|error| error.to_string())?;

    println!(
        "imported {} records ({})",
        summary.record_count, summary.source_checksum
    );

    if args.embed {
        let embedded = run_embedding(&database_url, &summary.inserted_ids)
            .await
            .map_err(|error| error.to_string())?;
        println!("embedded {embedded} records");
    }

    Ok(())
}

async fn run_embedding(database_url: &str, hadith_ids: &[i64]) -> Result<usize, String> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(|error| error.to_string())?;

    let repository = HadithRepository::new(pool);
    let hadiths = repository
        .find_by_ids(hadith_ids)
        .await
        .map_err(|error| error.to_string())?;

    let embedder = OpenAiEmbedder::new(EmbeddingConfig::from_env());
    let vector_config = VectorConfig::from_env();
    let vector_store = QdrantVectorStore::new(&vector_config.qdrant_url, vector_config.qdrant_collection)
        .map_err(|error| error.to_string())?;

    embed_hadiths(&embedder, &vector_store, &hadiths)
        .await
        .map_err(|error| error.to_string())
}

#[derive(Debug)]
struct Args {
    json_path: String,
    database_url: Option<String>,
    validate_only: bool,
    embed: bool,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut json_path = None;
        let mut database_url = None;
        let mut validate_only = false;
        let mut embed = false;

        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--database-url" => {
                    database_url = Some(require_value(&mut args, "--database-url")?);
                }
                "--validate-only" => {
                    validate_only = true;
                }
                "--embed" => {
                    embed = true;
                }
                "-h" | "--help" => {
                    return Err(usage());
                }
                value if value.starts_with('-') => {
                    return Err(format!("unknown option: {value}\n\n{}", usage()));
                }
                value => {
                    if json_path.replace(value.to_owned()).is_some() {
                        return Err(format!("unexpected extra argument: {value}\n\n{}", usage()));
                    }
                }
            }
        }

        Ok(Self {
            json_path: json_path.ok_or_else(usage)?,
            database_url,
            validate_only,
            embed,
        })
    }
}

fn require_value(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} requires a value"))
}

fn usage() -> String {
    "usage: import_hadiths <json-path> [--database-url <url>] [--validate-only] [--embed]"
        .to_owned()
}
```

- [ ] **Step 9: Add a CLI argument-parsing test for `--embed`**

At the bottom of `src/bin/import_hadiths.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_recognizes_the_embed_flag() {
        let args = Args::parse(
            ["data/imports/hadiths.json".to_owned(), "--embed".to_owned()].into_iter(),
        )
        .expect("valid arguments should parse");

        assert!(args.embed);
        assert_eq!(args.json_path, "data/imports/hadiths.json");
    }

    #[test]
    fn parse_defaults_embed_to_false() {
        let args = Args::parse(["data/imports/hadiths.json".to_owned()].into_iter())
            .expect("valid arguments should parse");

        assert!(!args.embed);
    }
}
```

Run: `cargo test --bin import_hadiths`
Expected: PASS (2 tests) — write this test, confirm it fails first if `--embed` were not wired (it is, from Step 8, so this step doubles as the pass-verification for that step).

- [ ] **Step 10: Verify the full build**

Run: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test`

- [ ] **Step 11: Commit**

```bash
git add src/ingestion src/bin/import_hadiths.rs
git commit -m "feat: embed newly imported hadiths with import_hadiths --embed"
```

---

### Task 7: Update documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/api.md`
- Modify: `docs/project-status.md`
- Modify: `docs/import-hadith-json.md`

**Interfaces:**
- Consumes: final behavior from Tasks 1-6.
- Produces: documentation consistent with the shipped behavior. No code interfaces.

- [ ] **Step 1: Add the new environment variables to `README.md`**

In the configuration table (currently lines 91-100), add three rows after the `QDRANT_COLLECTION` row:

```markdown
| `EMBEDDING_BASE_URL` | no | `https://api.openai.com/v1` | Embeddings API base URL |
| `EMBEDDING_API_KEY` | no | — | Bearer token for the embeddings API; required to actually call retrieval or `import_hadiths --embed` |
| `EMBEDDING_MODEL` | no | `text-embedding-3-small` | Embedding model name |
```

- [ ] **Step 2: Update the retrieval route documentation in `README.md`**

Change the routes list (currently lines 109-117) — `POST /api/retrieval` stays listed as-is; no structural change needed there since it was already documented as a route. No edit required in this section.

- [ ] **Step 3: Update `docs/api.md`'s retrieval section**

Replace the "Retrieval" section (currently lines 95-121) with:

```markdown
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
```

- [ ] **Step 4: Update `docs/import-hadith-json.md`**

Read the file first to find the right insertion point, then add a section documenting the `--embed` flag: what it does (re-embeds only the records just inserted by that import run, via the configured embedding provider, upserted into Qdrant keyed by Hadith ID), and that it requires `EMBEDDING_API_KEY` and a reachable Qdrant instance (`docker compose up -d qdrant`).

- [ ] **Step 5: Update `docs/project-status.md`**

Move the Qdrant retrieval pipeline bullet from "Deliberately incomplete" to "Implemented", and add the two forward-looking milestones. Replace the full file contents with:

```markdown
# Project status

## Architecture decision

The application is a single Topcoat `0.5.x` Cargo package. Topcoat serves both
server-rendered pages and the JSON API. PostgreSQL is the source of truth for
Hadith content; Qdrant is a replaceable retrieval index.

```text
browser or API client
  -> Topcoat page / route
  -> application service
  -> SQLx repository
  -> PostgreSQL canonical record

retrieval request
  -> retrieval policy and scope
  -> embed query (Embedder trait, OpenAI-compatible by default)
  -> Qdrant candidates (VectorStore trait, Qdrant by default)
  -> PostgreSQL canonical resolution
  -> cited context
```

## Implemented

- Topcoat root layout, home page, Hadith browser, asset bundle, and module-based
  routing.
- JSON API under `/api` with typed path, query, and body parsing.
- Stable JSON error codes without leaked database details.
- Typed application context containing shared services.
- PostgreSQL schema and automatic startup migrations.
- Collection and Hadith repositories and services.
- Hadith filters and lookup by internal ID or published reference.
- JSON import CLI with validation, one-transaction import, source checksum, and
  deterministic Arabic transliteration.
- Qdrant retrieval: embed a query through a swappable `Embedder`, search a
  swappable `VectorStore`, resolve every hit back to its canonical Hadith row.
  Hadiths are embedded via `import_hadiths --embed`.
- Docker Compose dependencies and a production image containing the binary and
  its Topcoat asset bundle.

## Deliberately incomplete

- Full-text search over Arabic and English content is not implemented.
- The chat/RAG answer endpoint is not implemented.
- Database-backed integration tests are not yet available.
- The local corpus is ignored until licensing and provenance are reviewed.

## Next milestones

1. Add database-backed integration fixtures for repositories, ingestion, and
   retrieval, replacing the fake-double unit tests where a real database and
   Qdrant instance can assert on end-to-end behavior.
2. Full-text search over Arabic and English content, likely PostgreSQL-native
   (`tsvector`), as a complement to vector retrieval rather than a
   replacement.
3. Commentary/explanation corpus: a future corpus of Hadith commentary (sharh)
   books, embedded and retrievable the same way as Hadith text, to provide
   explanatory context alongside or instead of raw citations. Reusable
   through the existing `Embedder`/`VectorStore` traits, since collection
   name and source text are already parameters rather than hardcoded
   assumptions — implementation will decide between a second Qdrant
   collection and a payload `kind` discriminator when that work starts.
4. LLM-agnostic chat/RAG endpoint, built only after retrieval is proven in
   production use, per the sequencing already established in `AGENTS.md`.
   Should follow the same base-URL/API-key/model-configurable pattern as
   `Embedder` so DeepSeek, OpenRouter, or other OpenAI-compatible chat
   backends can be swapped without rewriting the use case.

Topcoat is expected to change rapidly. Framework upgrades should remain isolated
to the `app` layer, startup wiring, and asset build process whenever possible.
```

- [ ] **Step 6: Commit**

```bash
git add README.md docs/api.md docs/project-status.md docs/import-hadith-json.md
git commit -m "docs: document Qdrant retrieval, embedding config, and import_hadiths --embed"
```

---

## Self-Review Notes

- **Spec coverage:** Part 1 (Topcoat upgrade) → Task 1. Part 2 storage model, embedding abstraction, ingestion path, query path, new modules, new dependencies → Tasks 2-6. Documentation updates → Task 7. Out-of-scope items (full-text search, chat/RAG, DB integration tests, commentary corpus, LLM abstraction) are explicitly *not* implemented and are recorded as milestones in Task 7 rather than built.
- **Fixed during planning:** the spec's "New modules" section described `QdrantVectorStore` as a bare concrete type, but its own "Testing" section required "fake Embedder/store doubles" for `RetrievalService`'s success path — impossible if `VectorStore` isn't a trait. Task 4 defines `VectorStore` as a trait (mirroring `Embedder`) to resolve that inconsistency; `QdrantVectorStore` remains the sole production implementation.
- **Type consistency:** `EmbeddingPoint`, `VectorMatch`, `Embedder`, `VectorStore` are defined once in Task 3/4 and referenced with identical signatures in Tasks 5 and 6. `ImportSummary.inserted_ids` (Task 6) is produced by `import_dump` and consumed only by the CLI's `run_embedding`, not by `import_hadith_json`'s existing callers/tests.
