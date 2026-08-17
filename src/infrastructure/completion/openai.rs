use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::ChatConfig;
use crate::error::AppError;

use super::{ChatCompleter, ChatMessage, CompletionOptions};

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
    messages: Vec<WireMessage<'a>>,
    temperature: f32,
    max_tokens: u32,
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
