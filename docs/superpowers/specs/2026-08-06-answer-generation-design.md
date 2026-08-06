# LLM answer generation (`AnswerService`)

## Context

This is spec 2 of 4 building the "Sanad" chat UI (see
`docs/superpowers/specs/2026-08-05-narrator-extraction-design.md` for
spec 1). The Sanad design shows a synthesized title + answer paragraph
above the retrieved hadith cards for each question — content this
project has no way to produce today, since `RetrievalService` is pure
vector search with no generation step.

This spec adds a reusable `AnswerService` that takes a question and its
retrieved hadiths and produces a grounded title + answer via an
OpenRouter-compatible chat-completion API. It is built and unit tested
here but **not wired into any route** — that's spec 4, same split as
spec 1's `NarratorRepository`.

Key decisions already made:
- Single API key: `OPEN_ROUTER_API_KEY` is read directly by both the
  embedding client and this chat client. `EMBEDDING_API_KEY` is removed.
- Default model: `deepseek/deepseek-v4-flash` (~$0.09/$0.18 per 1M
  input/output tokens on OpenRouter — not free, but negligible cost per
  question). Overridable via `CHAT_MODEL`.
- Failure mode: answer generation degrades gracefully. Any failure —
  missing API key, HTTP error, timeout, malformed model output — results
  in `AnswerService::generate` returning `None`, never an `Err`. A route
  calling it can always still render the retrieved hadith cards even if
  the answer paragraph is unavailable.

## Config

`src/config.rs` gains:

```rust
#[derive(Debug, Clone)]
pub struct ChatConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

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

`Config::from_env()` gains a `pub chat: ChatConfig` field, populated via
`ChatConfig::from_env()`.

`EmbeddingConfig::from_env()` changes its `api_key` line from
`env::var("EMBEDDING_API_KEY").ok()` to `env::var("OPEN_ROUTER_API_KEY").ok()`.
`.env`, `.env.example`, and the README's config table drop
`EMBEDDING_API_KEY` and gain `CHAT_BASE_URL` (documented default) and
`CHAT_MODEL` (documented default), with `OPEN_ROUTER_API_KEY` documented
as shared by both the embedding and chat clients.

## HTTP client

New module `src/infrastructure/completion/`, mirroring
`src/infrastructure/embedding/`'s shape:

```rust
// src/infrastructure/completion/mod.rs
mod openai;

pub use openai::OpenAiChatClient;

#[async_trait]
pub trait ChatCompleter: Send + Sync {
    async fn complete(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError>;
}
```

```rust
// src/infrastructure/completion/openai.rs
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
```

`complete()` POSTs `{base_url}/chat/completions` with:

```json
{
  "model": "<configured model>",
  "messages": [
    {"role": "system", "content": "<system_prompt>"},
    {"role": "user", "content": "<user_prompt>"}
  ],
  "temperature": 0.3,
  "max_tokens": 400
}
```

bearer-authenticated the same way `OpenAiEmbedder` is (`request.bearer_auth(api_key)`
when `api_key` is `Some`), and returns `response.choices[0].message.content`.
Non-2xx status and JSON-parse failures both map to `AppError::Internal`,
matching `OpenAiEmbedder`'s error handling exactly. The one deliberate
deviation from `OpenAiEmbedder`'s client: a 20-second request timeout
(`OpenAiEmbedder` uses `reqwest::Client::new()` with no timeout), because
`AnswerService` will eventually be called synchronously while rendering a
page in spec 4, and a hung upstream call must not hang page rendering
indefinitely.

## `AnswerService`

New `src/application/answer.rs`:

```rust
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
    pub fn new(completer: Arc<dyn ChatCompleter>, has_api_key: bool) -> Self { ... }

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
```

`has_api_key` is computed once at construction time (`ChatConfig.api_key.is_some()`)
so a deployment with no key configured never even attempts a network call —
not a correctness requirement (the HTTP call would just fail and be caught
below), but avoids a doomed round trip on every single question.

### Prompt construction

`build_system_prompt()` returns a fixed string (exact wording to be
finalized in a follow-up — implementation writes a reasonable first draft
covering: use only the hadiths provided below, no outside knowledge, no
new fiqh rulings, stay descriptive ("these narrations address...") not
prescriptive ("you must..."), and the required `Title:` output shape
described next).

`build_user_prompt(query, hadiths)` renders the question followed by each
hadith numbered, with its English text (when present) and Arabic text,
e.g.:

```
Question: What does Islam say about intentions?

1. (Sahih al-Bukhari, book 1, hadith 1)
English: Actions are but by intentions...
Arabic: إنما الأعمال بالنيات...

2. (Sahih Muslim, book 45, hadith 2564)
...
```

### Output parsing

The model is instructed to respond in exactly:
```
Title: <short title>
<answer paragraph(s)>
```

`parse_answer(raw: &str) -> Option<Answer>`:
1. Split `raw` on the first newline into `(first_line, rest)`.
2. Match `first_line` against `^\s*Title:\s*(.+)$` (case-insensitive). No
   match → `None`.
3. `title` = the captured group, trimmed. Empty after trim → `None`.
4. `answer` = `rest.trim()`. Empty → `None`.
5. Otherwise `Some(Answer { title, answer })`.

This plain-text shape is used instead of `response_format: json_object`
because free/cheap OpenRouter models don't reliably support JSON mode;
one regex plus a split is simple to parse and tolerant of minor
formatting drift (extra blank lines, trailing whitespace).

## Wiring into `AppServices`

`src/application/mod.rs`:
- `AppServices` gains `pub answers: Arc<AnswerService>`.
- `AppServices::new` gains a `chat: ChatConfig` parameter (after `vector`,
  matching the existing `pool, embedding, vector` ordering →
  `pool, embedding, vector, chat`).
- Constructs `let completer: Arc<dyn ChatCompleter> = Arc::new(OpenAiChatClient::new(chat.clone()));`
  and `let answers = Arc::new(AnswerService::new(completer, chat.api_key.is_some()));`.

Call sites needing the new argument: `src/main.rs` (`AppServices::new(pool, config.embedding.clone(), config.vector.clone(), config.chat.clone())`)
and `src/app.rs`'s `#[cfg(test)] module_router_builds_without_route_conflicts`
test (`crate::config::ChatConfig::default()`).

`AnswerService` is constructed and available on `AppServices` but not
called from any page or API route in this spec.

## Testing

- `OpenAiChatClient`: wiremock-based tests mirroring `OpenAiEmbedder`'s —
  successful response parses `choices[0].message.content`; non-2xx status
  returns `AppError::Internal` containing the status code; a response
  missing `choices` returns `AppError::Internal`.
- `parse_answer`: unit tests for well-formed input, missing `Title:`
  line, empty title, empty answer body, and case-insensitive `Title:`/`title:`.
- `AnswerService::generate` (using a fake `ChatCompleter`, mirroring
  `FakeEmbedder` in `retrieval.rs`): empty `hadiths` slice → `None`
  without the fake being invoked; `has_api_key: false` → `None` without
  the fake being invoked; fake returning `Err` → `None`; fake returning a
  well-formed `Title:` response → `Some(Answer)` with the expected
  title/answer split; fake returning unparseable text → `None`.

## Out of scope

- Calling `AnswerService` from any page or API route (spec 4).
- Streaming responses — `generate()` returns the complete answer or
  nothing, no partial/streaming output.
- Caching identical questions' answers.
- Multi-turn conversation context — each call is a single question plus
  its retrieved hadiths, no prior turns included.
