use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::domain::NewCollection;
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_collections).post(create_collection))
        .route(
            "/{slug}",
            get(get_collection)
                .put(update_collection)
                .delete(delete_collection),
        )
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

async fn create_collection(
    State(state): State<AppState>,
    Json(request): Json<CollectionRequest>,
) -> Result<(StatusCode, Json<crate::domain::Collection>), AppError> {
    let collection = state.collections.create(request.into()).await?;
    Ok((StatusCode::CREATED, Json(collection)))
}

async fn update_collection(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(request): Json<CollectionRequest>,
) -> Result<Json<crate::domain::Collection>, AppError> {
    let collection = state.collections.update(&slug, request.into()).await?;
    Ok(Json(collection))
}

async fn delete_collection(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<StatusCode, AppError> {
    state.collections.delete(&slug).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct CollectionRequest {
    slug: String,
    name: String,
}

impl From<CollectionRequest> for NewCollection {
    fn from(request: CollectionRequest) -> Self {
        Self {
            slug: request.slug,
            name: request.name,
        }
    }
}
