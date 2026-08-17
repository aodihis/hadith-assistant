use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::ChatConfig;
use crate::error::AppError;

use futures_util::StreamExt;

use super::{ChatCompleter, ChatMessage, CompletionOptions, CompletionStream};

#[derive(Clone)]
pub struct OpenAiChatClient {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
}

impl OpenAiChatClient {
    pub fn new(config: ChatConfig) -> Self {
        // Deliberately NOT a total-request timeout. A streamed completion's
        // body is the answer being written, so a whole-request deadline
        // truncates long answers mid-sentence — and a truncated reply is
        // indistinguishable from a broken one. Bound stalls instead: refuse to
        // wait forever to connect, or for the next byte, while letting a
        // healthy generation take as long as it needs.
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .read_timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client with stall timeouts should always build");

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
    messages: Vec<WireMessage<'a>>,
    temperature: f32,
    max_tokens: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

/// One `data:` frame of a streamed completion. Every field is optional because
/// providers differ in which they send on the final frame.
#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
}

#[derive(Deserialize, Default)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Serialize)]
struct WireMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
    usage: Option<Usage>,
}

/// Token accounting is logged, never returned — chat multiplies calls per user
/// action, so cost would otherwise be invisible.
#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
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
    async fn complete_messages(
        &self,
        messages: &[ChatMessage],
        options: CompletionOptions,
    ) -> Result<String, AppError> {
        let mut request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&ChatCompletionRequest {
                model: &self.model,
                messages: messages
                    .iter()
                    .map(|message| WireMessage {
                        role: message.role.as_str(),
                        content: &message.content,
                    })
                    .collect(),
                temperature: options.temperature(),
                max_tokens: options.max_tokens(),
                stream: false,
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

        if let Some(usage) = &body.usage {
            tracing::info!(
                model = %self.model,
                prompt_tokens = usage.prompt_tokens,
                completion_tokens = usage.completion_tokens,
                total_tokens = usage.total_tokens,
                "chat completion succeeded"
            );
        }

        Ok(body.choices.remove(0).message.content)
    }

    async fn stream_messages(
        &self,
        messages: &[ChatMessage],
        options: CompletionOptions,
    ) -> Result<CompletionStream, AppError> {
        let mut request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&ChatCompletionRequest {
                model: &self.model,
                messages: messages
                    .iter()
                    .map(|message| WireMessage {
                        role: message.role.as_str(),
                        content: &message.content,
                    })
                    .collect(),
                temperature: options.temperature(),
                max_tokens: options.max_tokens(),
                stream: true,
            });

        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = request.send().await.map_err(|error| {
            AppError::Internal(format!("chat completion stream request failed: {error}"))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "chat completion stream failed with status {status}: {body}"
            )));
        }

        let mut bytes = response.bytes_stream();

        // SSE frames are newline-delimited but arrive on arbitrary byte
        // boundaries, so a partial line must be carried across chunks rather
        // than parsed as-is.
        let stream = async_stream::stream! {
            let mut pending = String::new();

            while let Some(chunk) = bytes.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield Err(AppError::Internal(format!(
                            "chat completion stream ended early: {error}"
                        )));
                        return;
                    }
                };

                pending.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(newline) = pending.find('\n') {
                    let line: String = pending.drain(..=newline).collect();
                    match parse_stream_line(line.trim_end()) {
                        StreamLine::Content(text) => yield Ok(text),
                        StreamLine::Done => return,
                        StreamLine::Ignore => {}
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

enum StreamLine {
    Content(String),
    Done,
    Ignore,
}

/// Reads one SSE line. Comments, blank lines, and frames carrying no text are
/// ignored rather than treated as errors — providers interleave keep-alives and
/// role-only frames freely.
fn parse_stream_line(line: &str) -> StreamLine {
    let Some(payload) = line.strip_prefix("data:") else {
        return StreamLine::Ignore;
    };
    let payload = payload.trim();

    if payload == "[DONE]" {
        return StreamLine::Done;
    }

    let Ok(chunk) = serde_json::from_str::<StreamChunk>(payload) else {
        return StreamLine::Ignore;
    };

    match chunk
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.delta.content)
    {
        Some(text) if !text.is_empty() => StreamLine::Content(text),
        _ => StreamLine::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn test_config(server: &MockServer, api_key: Option<&str>) -> ChatConfig {
        ChatConfig {
            base_url: server.uri(),
            api_key: api_key.map(str::to_owned),
            model: "test-model".to_owned(),
            ..ChatConfig::default()
        }
    }

    fn options() -> CompletionOptions {
        CompletionOptions::new(0.3, 400)
    }

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

        let client = OpenAiChatClient::new(test_config(&server, Some("test-key")));

        let content = client
            .complete("system", "user", options())
            .await
            .expect("mocked chat completion request should succeed");

        assert_eq!(
            content,
            "Title: Sincerity\nActions are judged by intentions."
        );
    }

    #[tokio::test]
    async fn complete_messages_sends_every_turn_in_order_with_the_configured_profile() {
        let server = MockServer::start().await;

        // The mock only matches if the outgoing body carries all three turns in
        // order with the right roles, and the profile from CompletionOptions.
        // An unmatched request makes the call fail, so this asserts the wire
        // shape rather than just the happy path.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(serde_json::json!({
                "temperature": 0.25,
                "max_tokens": 512,
                "messages": [
                    { "role": "system",    "content": "rules" },
                    { "role": "user",      "content": "first question" },
                    { "role": "assistant", "content": "first answer" },
                    { "role": "user",      "content": "follow-up" }
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "content": "Title: Ok\nBody." } }]
            })))
            .mount(&server)
            .await;

        let client = OpenAiChatClient::new(test_config(&server, None));

        let content = client
            .complete_messages(
                &[
                    ChatMessage::system("rules"),
                    ChatMessage::user("first question"),
                    ChatMessage::assistant("first answer"),
                    ChatMessage::user("follow-up"),
                ],
                CompletionOptions::new(0.25, 512),
            )
            .await
            .expect("multi-turn request should match the mock and succeed");

        assert_eq!(content, "Title: Ok\nBody.");
    }

    #[tokio::test]
    async fn complete_returns_error_on_non_success_status() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = OpenAiChatClient::new(test_config(&server, None));

        let error = client
            .complete("system", "user", options())
            .await
            .expect_err("non-success status should fail");

        assert!(matches!(error, AppError::Internal(message) if message.contains("429")));
    }

    #[test]
    fn stream_lines_that_carry_no_text_are_ignored_rather_than_failing() {
        // Providers interleave keep-alive comments, blank lines, and role-only
        // frames. Treating any of them as an error would abort a healthy stream.
        for line in [
            "",
            ": keep-alive",
            "event: message",
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}",
            "data: {\"choices\":[]}",
            "data: not json at all",
        ] {
            assert!(
                matches!(parse_stream_line(line), StreamLine::Ignore),
                "line {line:?} should be ignored"
            );
        }
    }

    #[test]
    fn stream_lines_yield_content_and_recognise_the_terminator() {
        assert!(matches!(
            parse_stream_line("data: [DONE]"),
            StreamLine::Done
        ));

        let content = parse_stream_line("data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}");
        assert!(matches!(content, StreamLine::Content(text) if text == "Hi"));
    }

    #[tokio::test]
    async fn stream_messages_reassembles_deltas_split_across_byte_boundaries() {
        let server = MockServer::start().await;

        // The blank line between frames and the split are both realistic: SSE
        // frames do not arrive aligned to chunk boundaries.
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Title: Mercy\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"\\nBe \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"merciful.\"}}]}\n\n",
            "data: [DONE]\n\n",
        );

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(serde_json::json!({ "stream": true })))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let client = OpenAiChatClient::new(test_config(&server, None));

        // CompletionStream is not Debug, so unwrap via a match rather than expect.
        let Ok(mut stream) = client
            .stream_messages(&[ChatMessage::user("hi")], options())
            .await
        else {
            panic!("streaming request should start");
        };

        let mut collected = String::new();
        while let Some(chunk) = stream.next().await {
            collected.push_str(&chunk.expect("no chunk should error"));
        }

        assert_eq!(collected, "Title: Mercy\nBe merciful.");
    }

    #[tokio::test]
    async fn stream_messages_surfaces_a_failed_status_instead_of_an_empty_stream() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let client = OpenAiChatClient::new(test_config(&server, None));

        let Err(error) = client
            .stream_messages(&[ChatMessage::user("hi")], options())
            .await
        else {
            panic!("a failed status must not look like an empty answer");
        };

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

        let client = OpenAiChatClient::new(test_config(&server, None));

        let error = client
            .complete("system", "user", options())
            .await
            .expect_err("empty choices should fail");

        assert!(matches!(error, AppError::Internal(message) if message.contains("no choices")));
    }
}
