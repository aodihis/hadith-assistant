# LLM Answer Generation (AnswerService) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reusable `AnswerService` that generates a grounded title + answer paragraph from a question and its retrieved hadiths via an OpenRouter-compatible chat-completion API, built and unit tested but not yet wired into any route.

**Architecture:** A low-level `OpenAiChatClient` (mirroring the existing `OpenAiEmbedder`) does the HTTP call. `AnswerService` sits above it, builds the prompt, calls the client, and absorbs every possible failure into `None` so callers never see an `Err` from it. Both a single `OPEN_ROUTER_API_KEY` config value now backs the embedding and chat clients.

**Tech Stack:** Rust, `reqwest` (already a dependency, no new crates needed), `async-trait`, `wiremock` for HTTP tests.

## Global Constraints

- `EMBEDDING_API_KEY` is removed; both `EmbeddingConfig` and the new `ChatConfig` read `OPEN_ROUTER_API_KEY` instead.
- Default `CHAT_MODEL` is `deepseek/deepseek-v4-flash`; default `CHAT_BASE_URL` is `https://openrouter.ai/api/v1`. Both overridable via env vars.
- `AnswerService::generate` never returns `Err` — every failure path (missing key, HTTP error, timeout, unparseable output) returns `None`.
- `OpenAiChatClient` uses a 20-second request timeout (unlike `OpenAiEmbedder`, which has none).
- `AnswerService` is constructed and exposed on `AppServices` in this plan but not called from any page or API route — that's a later plan.

---

### Task 1: Config — `ChatConfig` and the `OPEN_ROUTER_API_KEY` consolidation

**Files:**
- Modify: `src/config.rs`
- Modify: `.env`
- Modify: `.env.example`
- Modify: `README.md`

**Interfaces:**
- Produces: `pub struct ChatConfig { pub base_url: String, pub api_key: Option<String>, pub model: String }`, `ChatConfig::from_env() -> Self`, `impl Default for ChatConfig`, `Config.chat: ChatConfig` field — consumed by Task 2 (`OpenAiChatClient::new`) and Task 4 (`AppServices::new`).

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `src/config.rs`:

```rust
#[test]
fn chat_config_default_points_at_openrouter_with_deepseek_flash() {
    let config = ChatConfig::default();

    assert_eq!(config.base_url, "https://openrouter.ai/api/v1");
    assert_eq!(config.api_key, None);
    assert_eq!(config.model, "deepseek/deepseek-v4-flash");
}

#[test]
fn embedding_config_reads_open_router_api_key() {
    // SAFETY: test runs single-threaded within this process's env; no
    // other test reads OPEN_ROUTER_API_KEY concurrently.
    unsafe {
        std::env::set_var("OPEN_ROUTER_API_KEY", "test-shared-key");
    }
    let config = EmbeddingConfig::from_env();
    unsafe {
        std::env::remove_var("OPEN_ROUTER_API_KEY");
    }

    assert_eq!(config.api_key.as_deref(), Some("test-shared-key"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests`
Expected: compile error — `ChatConfig` doesn't exist yet, and `EmbeddingConfig::from_env` still reads `EMBEDDING_API_KEY`.

- [ ] **Step 3: Add `ChatConfig` and update `EmbeddingConfig`**

In `src/config.rs`, add after the `EmbeddingConfig` struct definition:

```rust
#[derive(Debug, Clone)]
pub struct ChatConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}
```

Change `EmbeddingConfig::from_env`'s `api_key` line from:

```rust
            api_key: env::var("EMBEDDING_API_KEY").ok(),
```

to:

```rust
            api_key: env::var("OPEN_ROUTER_API_KEY").ok(),
```

Add a `ChatConfig::from_env` impl after `EmbeddingConfig::from_env`:

```rust
impl ChatConfig {
    pub fn from_env() -> Self {
        Self {
            base_url: env::var("CHAT_BASE_URL")
                .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_owned()),
            api_key: env::var("OPEN_ROUTER_API_KEY").ok(),
            model: env::var("CHAT_MODEL")
                .unwrap_or_else(|_| "deepseek/deepseek-v4-flash".to_owned()),
        }
    }
}
```

Add a `Default` impl after `impl Default for EmbeddingConfig`:

```rust
impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            base_url: "https://openrouter.ai/api/v1".to_owned(),
            api_key: None,
            model: "deepseek/deepseek-v4-flash".to_owned(),
        }
    }
}
```

Add `pub chat: ChatConfig` to the `Config` struct, and `chat: ChatConfig::from_env(),` to `Config::from_env`'s return, and `chat: ChatConfig::default(),` to `impl Default for Config`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config::tests`
Expected: all pass, including the two new tests and the pre-existing `embedding_config_default_points_at_openai` (unaffected — it tests `Default`, not `from_env`).

- [ ] **Step 5: Update `.env.example`**

Replace:

```
# Embedding provider is OpenAI-compatible and swappable via config alone —
# these three values point it at OpenRouter's embeddings endpoint instead of
# the https://api.openai.com/v1 default.
EMBEDDING_BASE_URL=https://openrouter.ai/api/v1
EMBEDDING_API_KEY=
EMBEDDING_MODEL=openai/text-embedding-3-small
```

with:

```
# OPEN_ROUTER_API_KEY is shared by both the embedding and chat-completion
# clients below — one key, one provider.
OPEN_ROUTER_API_KEY=

# Embedding provider is OpenAI-compatible and swappable via config alone —
# these two values point it at OpenRouter's embeddings endpoint instead of
# the https://api.openai.com/v1 default.
EMBEDDING_BASE_URL=https://openrouter.ai/api/v1
EMBEDDING_MODEL=openai/text-embedding-3-small

# Chat completion (answer generation) is also OpenAI-compatible. The default
# model is a low-cost (not free) OpenRouter model — see README for current
# pricing before relying on it in production.
CHAT_BASE_URL=https://openrouter.ai/api/v1
CHAT_MODEL=deepseek/deepseek-v4-flash
```

- [ ] **Step 6: Tidy the local `.env`**

In `.env`, remove the now-unused `EMBEDDING_API_KEY=...` line (its value is
already duplicated by the existing `OPEN_ROUTER_API_KEY` line, which
`EmbeddingConfig` now reads directly). Leave `OPEN_ROUTER_API_KEY` as-is.
This file is gitignored — no need to add `CHAT_BASE_URL`/`CHAT_MODEL` here
since both have working defaults.

- [ ] **Step 7: Update the README config table**

In `README.md`, replace the three `EMBEDDING_*` table rows:

```
| `EMBEDDING_BASE_URL` | no | `https://api.openai.com/v1` | Embeddings API base URL |
| `EMBEDDING_API_KEY` | no | — | Bearer token for the embeddings API; required to actually call retrieval or `import_hadiths --embed` |
| `EMBEDDING_MODEL` | no | `text-embedding-3-small` | Embedding model name |
```

with:

```
| `OPEN_ROUTER_API_KEY` | no | — | Bearer token shared by the embedding and chat-completion clients; required to actually call retrieval, `import_hadiths --embed`, or answer generation |
| `EMBEDDING_BASE_URL` | no | `https://api.openai.com/v1` | Embeddings API base URL |
| `EMBEDDING_MODEL` | no | `text-embedding-3-small` | Embedding model name |
| `CHAT_BASE_URL` | no | `https://openrouter.ai/api/v1` | Chat-completion API base URL |
| `CHAT_MODEL` | no | `deepseek/deepseek-v4-flash` | Chat-completion model name used for answer generation |
```

- [ ] **Step 8: Run the full test suite**

Run: `cargo test --lib`
Expected: all tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/config.rs .env.example README.md
git commit -m "feat: add ChatConfig and consolidate on OPEN_ROUTER_API_KEY"
```

(`.env` is gitignored and won't be picked up by `git add` of tracked paths — no action needed there.)

---

### Task 2: `ChatCompleter` trait and `OpenAiChatClient`

**Files:**
- Create: `src/infrastructure/completion/mod.rs`
- Create: `src/infrastructure/completion/openai.rs`
- Modify: `src/infrastructure/mod.rs`

**Interfaces:**
- Consumes: `crate::config::ChatConfig` (Task 1).
- Produces: `pub trait ChatCompleter: Send + Sync { async fn complete(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError>; }`, `pub struct OpenAiChatClient`, `OpenAiChatClient::new(config: ChatConfig) -> Self` — consumed by Task 3 (`AnswerService`) and Task 4 (`AppServices`).

- [ ] **Step 1: Check the existing `infrastructure` module layout**

Read `src/infrastructure/mod.rs` and `src/infrastructure/embedding/mod.rs`
to confirm the `pub mod embedding;` declaration style and the `Embedder`
trait's exact shape, so `completion` mirrors it precisely. (No code
change in this step — just confirms the pattern before writing Step 2.)

- [ ] **Step 2: Write the trait module**

Create `src/infrastructure/completion/mod.rs`:

```rust
pub mod openai;

pub use openai::OpenAiChatClient;

use async_trait::async_trait;

use crate::error::AppError;

#[async_trait]
pub trait ChatCompleter: Send + Sync {
    async fn complete(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError>;
}
```

- [ ] **Step 3: Write the failing tests for `OpenAiChatClient`**

Create `src/infrastructure/completion/openai.rs`:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::ChatConfig;
use crate::error::AppError;

use super::ChatCompleter;

#[derive(Clone)]
pub struct OpenAiChatClient {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
}

impl OpenAiChatClient {
    pub fn new(config: ChatConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .expect("reqwest client with a fixed timeout should always build");

        Self {
            client,
            base_url: config.base_url,
            api_key: config.api_key,
            model: config.model,
        }
    }
}

#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[async_trait]
impl ChatCompleter for OpenAiChatClient {
    async fn complete(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError> {
        let mut request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&ChatCompletionRequest {
                model: &self.model,
                messages: vec![
                    ChatMessage {
                        role: "system",
                        content: system_prompt,
                    },
                    ChatMessage {
                        role: "user",
                        content: user_prompt,
                    },
                ],
                temperature: 0.3,
                max_tokens: 400,
            });

        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = request.send().await.map_err(|error| {
            AppError::Internal(format!("chat completion request failed: {error}"))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "chat completion request failed with status {status}: {body}"
            )));
        }

        let mut body: ChatCompletionResponse = response.json().await.map_err(|error| {
            AppError::Internal(format!(
                "chat completion response was not valid JSON: {error}"
            ))
        })?;

        if body.choices.is_empty() {
            return Err(AppError::Internal(
                "chat completion response had no choices".to_owned(),
            ));
        }

        Ok(body.choices.remove(0).message.content)
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn complete_returns_the_first_choices_message_content() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [
                    { "message": { "content": "Title: Sincerity\nActions are judged by intentions." } }
                ]
            })))
            .mount(&server)
            .await;

        let client = OpenAiChatClient::new(ChatConfig {
            base_url: server.uri(),
            api_key: Some("test-key".to_owned()),
            model: "test-model".to_owned(),
        });

        let content = client
            .complete("system", "user")
            .await
            .expect("mocked chat completion request should succeed");

        assert_eq!(content, "Title: Sincerity\nActions are judged by intentions.");
    }

    #[tokio::test]
    async fn complete_returns_error_on_non_success_status() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = OpenAiChatClient::new(ChatConfig {
            base_url: server.uri(),
            api_key: None,
            model: "test-model".to_owned(),
        });

        let error = client
            .complete("system", "user")
            .await
            .expect_err("non-success status should fail");

        assert!(matches!(error, AppError::Internal(message) if message.contains("429")));
    }

    #[tokio::test]
    async fn complete_returns_error_when_choices_is_empty() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": []
            })))
            .mount(&server)
            .await;

        let client = OpenAiChatClient::new(ChatConfig {
            base_url: server.uri(),
            api_key: None,
            model: "test-model".to_owned(),
        });

        let error = client
            .complete("system", "user")
            .await
            .expect_err("empty choices should fail");

        assert!(matches!(error, AppError::Internal(message) if message.contains("no choices")));
    }
}
```

- [ ] **Step 4: Run tests to verify they fail, then pass**

Run: `cargo test --lib infrastructure::completion`
Expected: first compile-fails (module not registered — see Step 5), then
all 3 tests pass once Step 5 is done.

- [ ] **Step 5: Register the module**

In `src/infrastructure/mod.rs`, add `pub mod completion;` alongside the
existing `pub mod embedding;` (and any other `pub mod` lines already
there — insert alphabetically if that's the existing convention).

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --lib infrastructure::completion`
Expected: all 3 tests pass.

- [ ] **Step 7: Run the full test suite**

Run: `cargo test --lib`
Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/infrastructure/completion/ src/infrastructure/mod.rs
git commit -m "feat: add OpenAiChatClient for chat completions"
```

---

### Task 3: `AnswerService`

**Files:**
- Create: `src/application/answer.rs`
- Modify: `src/application/mod.rs` (module declaration + re-export only in this task; `AppServices` wiring is Task 4)

**Interfaces:**
- Consumes: `crate::infrastructure::completion::ChatCompleter` (Task 2), `crate::domain::RetrievedHadith` (existing).
- Produces: `pub struct Answer { pub title: String, pub answer: String }`, `pub struct AnswerService`, `AnswerService::new(completer: Arc<dyn ChatCompleter>, has_api_key: bool) -> Self`, `async fn generate(&self, query: &str, hadiths: &[RetrievedHadith]) -> Option<Answer>` — consumed by Task 4 (`AppServices`) and later, by a route (out of scope here).

- [ ] **Step 1: Write the failing tests**

Create `src/application/answer.rs`:

```rust
use std::sync::Arc;
use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;

use crate::domain::RetrievedHadith;
use crate::error::AppError;
use crate::infrastructure::completion::ChatCompleter;

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub title: String,
    pub answer: String,
}

pub struct AnswerService {
    completer: Arc<dyn ChatCompleter>,
    has_api_key: bool,
}

impl AnswerService {
    pub fn new(completer: Arc<dyn ChatCompleter>, has_api_key: bool) -> Self {
        Self {
            completer,
            has_api_key,
        }
    }

    pub async fn generate(&self, query: &str, hadiths: &[RetrievedHadith]) -> Option<Answer> {
        if !self.has_api_key || hadiths.is_empty() {
            return None;
        }

        let system_prompt = build_system_prompt();
        let user_prompt = build_user_prompt(query, hadiths);

        let raw = match self.completer.complete(&system_prompt, &user_prompt).await {
            Ok(raw) => raw,
            Err(error) => {
                tracing::warn!(%error, "answer generation request failed");
                return None;
            }
        };

        match parse_answer(&raw) {
            Some(answer) => Some(answer),
            None => {
                tracing::warn!("answer generation returned unparseable output");
                None
            }
        }
    }
}

fn build_system_prompt() -> String {
    "You are a study companion summarizing hadith narrations for a Muslim \
     asking a sincere question. You will be given a question and a numbered \
     list of hadiths retrieved for it. Follow these rules strictly:\n\
     \n\
     1. Use ONLY the hadiths provided below. Never introduce hadiths, \
        narrations, or facts from outside this list.\n\
     2. Never issue a fiqh ruling (halal/haram, obligatory/forbidden) or \
        claim certainty on matters of Islamic law. Stay descriptive \
        (\"these narrations address...\", \"the Prophet is reported to have \
        said...\") rather than prescriptive (\"you must...\", \"it is \
        obligatory to...\").\n\
     3. If the provided hadiths do not clearly address the question, say so \
        plainly rather than stretching them to fit.\n\
     4. Respond in English, in exactly this shape, with nothing before the \
        first line and nothing after the final paragraph:\n\
     \n\
     Title: <a short, neutral title for this topic, under 8 words>\n\
     <one or two short paragraphs summarizing what the provided hadiths say, \
     in plain prose>"
        .to_owned()
}

fn build_user_prompt(query: &str, hadiths: &[RetrievedHadith]) -> String {
    let mut prompt = format!("Question: {query}\n\n");

    for (index, hadith) in hadiths.iter().enumerate() {
        prompt.push_str(&format!(
            "{}. ({}, book {}, hadith {})\n",
            index + 1,
            hadith.collection,
            hadith.book_number,
            hadith.hadith_number
        ));
        if let Some(english_text) = &hadith.english_text {
            prompt.push_str(&format!("English: {english_text}\n"));
        }
        prompt.push_str(&format!("Arabic: {}\n\n", hadith.arabic_text));
    }

    prompt
}

static TITLE_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*Title:\s*(.+)$").expect("valid regex"));

fn parse_answer(raw: &str) -> Option<Answer> {
    let (first_line, rest) = raw.split_once('\n').unwrap_or((raw, ""));

    let title = TITLE_LINE
        .captures(first_line)?
        .get(1)?
        .as_str()
        .trim()
        .to_owned();
    if title.is_empty() {
        return None;
    }

    let answer = rest.trim().to_owned();
    if answer.is_empty() {
        return None;
    }

    Some(Answer { title, answer })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_answer_splits_title_and_body() {
        let raw = "Title: Sincerity & Intention\nActions are judged by intentions.\nSecond paragraph.";

        let answer = parse_answer(raw).expect("well-formed output should parse");

        assert_eq!(answer.title, "Sincerity & Intention");
        assert_eq!(
            answer.answer,
            "Actions are judged by intentions.\nSecond paragraph."
        );
    }

    #[test]
    fn parse_answer_is_case_insensitive_on_the_title_label() {
        let raw = "title: Kindness\nBe kind to others.";

        let answer = parse_answer(raw).expect("lowercase title label should still parse");

        assert_eq!(answer.title, "Kindness");
    }

    #[test]
    fn parse_answer_rejects_missing_title_line() {
        assert_eq!(parse_answer("Just some text with no title line."), None);
    }

    #[test]
    fn parse_answer_rejects_empty_body() {
        assert_eq!(parse_answer("Title: Something\n   \n"), None);
    }

    struct FakeCompleter {
        response: Result<&'static str, ()>,
    }

    #[async_trait]
    impl ChatCompleter for FakeCompleter {
        async fn complete(&self, _system_prompt: &str, _user_prompt: &str) -> Result<String, AppError> {
            match self.response {
                Ok(text) => Ok(text.to_owned()),
                Err(()) => Err(AppError::Internal("simulated failure".to_owned())),
            }
        }
    }

    struct PanicsIfCalledCompleter;

    #[async_trait]
    impl ChatCompleter for PanicsIfCalledCompleter {
        async fn complete(&self, _system_prompt: &str, _user_prompt: &str) -> Result<String, AppError> {
            panic!("completer should not be called");
        }
    }

    fn sample_hadith() -> RetrievedHadith {
        RetrievedHadith {
            hadith_id: 1,
            collection: "bukhari".to_owned(),
            book_number: "1".to_owned(),
            hadith_number: "1".to_owned(),
            arabic_text: "إنما الأعمال بالنيات".to_owned(),
            english_text: Some("Actions are but by intentions.".to_owned()),
            score: Some(0.9),
        }
    }

    #[tokio::test]
    async fn generate_returns_none_for_empty_hadiths_without_calling_the_completer() {
        let service = AnswerService::new(Arc::new(PanicsIfCalledCompleter), true);

        let result = service.generate("What is intention?", &[]).await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn generate_returns_none_without_an_api_key_without_calling_the_completer() {
        let service = AnswerService::new(Arc::new(PanicsIfCalledCompleter), false);

        let result = service
            .generate("What is intention?", &[sample_hadith()])
            .await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn generate_returns_none_when_the_completer_errors() {
        let service = AnswerService::new(Arc::new(FakeCompleter { response: Err(()) }), true);

        let result = service
            .generate("What is intention?", &[sample_hadith()])
            .await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn generate_returns_none_when_the_completer_output_is_unparseable() {
        let service = AnswerService::new(
            Arc::new(FakeCompleter {
                response: Ok("not the expected shape"),
            }),
            true,
        );

        let result = service
            .generate("What is intention?", &[sample_hadith()])
            .await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn generate_returns_the_parsed_answer_on_success() {
        let service = AnswerService::new(
            Arc::new(FakeCompleter {
                response: Ok("Title: Sincerity\nActions are judged by intentions."),
            }),
            true,
        );

        let result = service
            .generate("What is intention?", &[sample_hadith()])
            .await;

        assert_eq!(
            result,
            Some(Answer {
                title: "Sincerity".to_owned(),
                answer: "Actions are judged by intentions.".to_owned(),
            })
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib application::answer`
Expected: compile error — module not registered yet (Step 3).

- [ ] **Step 3: Register the module**

In `src/application/mod.rs`, add `mod answer;` alongside the existing
`mod collections; mod hadiths; mod retrieval;` lines, and add
`pub use answer::{Answer, AnswerService};` alongside the existing
`pub use` lines.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib application::answer`
Expected: all 9 tests pass (4 `parse_answer` unit tests + 5 `generate` tests).

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --lib`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/application/answer.rs src/application/mod.rs
git commit -m "feat: add AnswerService with grounded prompt and graceful degradation"
```

---

### Task 4: Wire `AnswerService` into `AppServices`

**Files:**
- Modify: `src/application/mod.rs`
- Modify: `src/main.rs`
- Modify: `src/app.rs`

**Interfaces:**
- Consumes: `AnswerService::new` (Task 3), `OpenAiChatClient::new` (Task 2), `ChatConfig` (Task 1).
- Produces: `AppServices.answers: Arc<AnswerService>`; `AppServices::new(pool, embedding, vector, chat)` (new 4th parameter) — consumed by any future route (out of scope here) and by this task's own call-site updates.

- [ ] **Step 1: Update `AppServices`**

In `src/application/mod.rs`, add these imports alongside the existing
`use crate::infrastructure::embedding::{Embedder, OpenAiEmbedder};` line:

```rust
use crate::config::ChatConfig;
use crate::infrastructure::completion::{ChatCompleter, OpenAiChatClient};
```

Add `pub answers: Arc<AnswerService>,` to the `AppServices` struct.

Change `AppServices::new`'s signature from:

```rust
    pub fn new(pool: PgPool, embedding: EmbeddingConfig, vector: VectorConfig) -> Self {
```

to:

```rust
    pub fn new(
        pool: PgPool,
        embedding: EmbeddingConfig,
        vector: VectorConfig,
        chat: ChatConfig,
    ) -> Self {
```

Inside the function body, after the existing `vector_store` construction
and before the `Self { ... }` return, add:

```rust
        let has_chat_api_key = chat.api_key.is_some();
        let completer: Arc<dyn ChatCompleter> = Arc::new(OpenAiChatClient::new(chat));
        let answers = Arc::new(AnswerService::new(completer, has_chat_api_key));
```

Add `answers,` to the `Self { ... }` struct literal (alongside
`collections`, `hadiths`, `retrieval`).

- [ ] **Step 2: Update `src/main.rs`**

Change:

```rust
    let router = app::router(AppServices::new(
        pool,
        config.embedding.clone(),
        config.vector.clone(),
    ))?;
```

to:

```rust
    let router = app::router(AppServices::new(
        pool,
        config.embedding.clone(),
        config.vector.clone(),
        config.chat.clone(),
    ))?;
```

- [ ] **Step 3: Update `src/app.rs`'s test call site**

Change:

```rust
        router_without_assets(AppServices::new(
            pool,
            crate::config::EmbeddingConfig::default(),
            crate::config::VectorConfig::default(),
        ));
```

to:

```rust
        router_without_assets(AppServices::new(
            pool,
            crate::config::EmbeddingConfig::default(),
            crate::config::VectorConfig::default(),
            crate::config::ChatConfig::default(),
        ));
```

- [ ] **Step 4: Compile and run the full test suite**

Run: `cargo check`
Expected: succeeds (this task only rewires existing constructors — no new
tests, since `AppServices::new`'s wiring is exercised by
`app::tests::module_router_builds_without_route_conflicts`, already
updated in Step 3).

Run: `cargo test --lib`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/application/mod.rs src/main.rs src/app.rs
git commit -m "feat: wire AnswerService into AppServices"
```

---

### Task 5: Final verification

**Files:** none (verification only).

- [ ] **Step 1: Format and lint**

Run: `cargo fmt --check`
Expected: no diff. If there is one, run `cargo fmt` and re-check.

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: no warnings. Fix any that appear (follow clippy's suggestion,
matching the fix style used in spec 1's `/simplify` pass — e.g. prefer
auto-deref over explicit `&mut *x` where clippy flags it).

- [ ] **Step 2: Full test suite**

Run: `cargo test --lib --bins`
Expected: all tests pass, including every test added in Tasks 1-4.

- [ ] **Step 3: Manual smoke test against a real OpenRouter key**

This needs a live `OPEN_ROUTER_API_KEY` and cannot be scripted as a unit
test — run it yourself and report the output. Add a temporary throwaway
`#[tokio::test]` (or use `cargo run` with a small ad hoc `main` snippet,
whichever is faster) that constructs `OpenAiChatClient::new(ChatConfig::from_env())`
and calls `.complete("You are a helpful assistant.", "Say hello in exactly \
three words.")`, printing the result. Confirm it returns a real
completion from `deepseek/deepseek-v4-flash` (not an error), then delete
the throwaway test/snippet — it was for manual verification only, not
part of the permanent test suite.

## Self-Review Notes

- **Spec coverage:** config + key consolidation (Task 1), HTTP client
  incl. 20s timeout and error mapping (Task 2), `AnswerService` incl.
  prompt construction, output parsing, and every graceful-degradation
  path from the spec — empty hadiths, missing key, completer error,
  unparseable output (Task 3), `AppServices` wiring (Task 4). All spec
  sections have a corresponding task.
- **Out of scope reminder:** calling `AnswerService` from a route is
  explicitly not part of this plan (spec 4's job) — `generate()` exists
  and is tested but nothing in the app calls it yet, matching how spec
  1's `NarratorRepository` shipped unwired.
- **Type consistency check:** `Answer`, `AnswerService::new(completer, has_api_key)`,
  and `generate(query, hadiths) -> Option<Answer>` are defined once in
  Task 3 and referenced identically in Task 4 — no drift.
