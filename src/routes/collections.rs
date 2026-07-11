use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};

use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_collections))
        .route("/{slug}", get(get_collection))
}

async fn list_collections(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::domain::Collection>>, AppError> {
    let collections = state.collections.list().await?;
    Ok(Json(collections))
}

async fn get_collection(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<crate::domain::Collection>, AppError> {
    let collection = state.collections.find_by_slug(&slug).await?;
    Ok(Json(collection))
}
