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

        assert_eq!(
            content,
            "Title: Sincerity\nActions are judged by intentions."
        );
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
