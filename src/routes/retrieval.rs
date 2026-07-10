use axum::routing::post;
use axum::{Json, Router, extract::State};
use serde::Deserialize;

use crate::domain::{RetrievalQuery, RetrievalResult};
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", post(retrieve))
}

async fn retrieve(
    State(state): State<AppState>,
    Json(request): Json<RetrievalRequest>,
) -> Result<Json<RetrievalResult>, AppError> {
    let result = state.retrieval.retrieve(request.into()).await?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
struct RetrievalRequest {
    query: String,
    collection: Option<String>,
    limit: Option<i64>,
}

impl From<RetrievalRequest> for RetrievalQuery {
    fn from(request: RetrievalRequest) -> Self {
        Self {
            query: request.query,
            collection: request.collection,
            limit: request.limit.unwrap_or_default(),
        }
    }
}
