use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::domain::{Hadith, HadithSearch};
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_hadiths))
        .route(
            "/by-reference/{collection}/{book_number}/{hadith_number}",
            get(get_hadith_by_reference),
        )
        .route("/{id}", get(get_hadith))
}

async fn list_hadiths(
    State(state): State<AppState>,
    Query(query): Query<HadithListQuery>,
) -> Result<Json<Vec<Hadith>>, AppError> {
    let hadiths = state.hadiths.list(query.into()).await?;
    Ok(Json(hadiths))
}

async fn get_hadith(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Hadith>, AppError> {
    let hadith = state.hadiths.find_by_id(id).await?;
    Ok(Json(hadith))
}

async fn get_hadith_by_reference(
    State(state): State<AppState>,
    Path((collection, book_number, hadith_number)): Path<(String, String, String)>,
) -> Result<Json<Hadith>, AppError> {
    let hadith = state
        .hadiths
        .find_by_reference(&collection, &book_number, &hadith_number)
        .await?;
    Ok(Json(hadith))
}

#[derive(Debug, Deserialize)]
struct HadithListQuery {
    collection: Option<String>,
    book_number: Option<String>,
    hadith_number: Option<String>,
    grade: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

impl From<HadithListQuery> for HadithSearch {
    fn from(query: HadithListQuery) -> Self {
        Self {
            collection: query.collection,
            book_number: query.book_number,
            hadith_number: query.hadith_number,
            grade: query.grade,
            limit: query.limit.unwrap_or_default(),
            offset: query.offset.unwrap_or_default(),
        }
    }
}
