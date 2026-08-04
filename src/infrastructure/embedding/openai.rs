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
