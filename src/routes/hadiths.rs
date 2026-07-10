use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::domain::{Hadith, HadithInput, HadithSearch};
use crate::error::AppError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_hadiths).post(create_hadith))
        .route(
            "/by-reference/{collection}/{book_number}/{hadith_number}",
            get(get_hadith_by_reference),
        )
        .route(
            "/{id}",
            get(get_hadith).put(update_hadith).delete(delete_hadith),
        )
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

async fn create_hadith(
    State(state): State<AppState>,
    Json(request): Json<HadithRequest>,
) -> Result<(StatusCode, Json<Hadith>), AppError> {
    let hadith = state.hadiths.create(request.into()).await?;
    Ok((StatusCode::CREATED, Json(hadith)))
}

async fn update_hadith(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(request): Json<HadithRequest>,
) -> Result<Json<Hadith>, AppError> {
    let hadith = state.hadiths.update(id, request.into()).await?;
    Ok(Json(hadith))
}

async fn delete_hadith(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    state.hadiths.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
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

#[derive(Debug, Deserialize)]
struct HadithRequest {
    collection_slug: String,
    book_number: String,
    bab_id: f64,
    english_bab_number: Option<String>,
    arabic_bab_number: Option<String>,
    hadith_number: String,
    our_hadith_number: i32,
    arabic_urn: i64,
    arabic_bab_name: Option<String>,
    arabic_text: String,
    arabic_transliteration: Option<String>,
    arabic_grade: Option<String>,
    english_urn: i64,
    english_bab_name: Option<String>,
    english_text: Option<String>,
    english_grade: Option<String>,
    last_updated: Option<String>,
    xrefs: Option<String>,
}

impl From<HadithRequest> for HadithInput {
    fn from(request: HadithRequest) -> Self {
        Self {
            collection_slug: request.collection_slug,
            book_number: request.book_number,
            bab_id: request.bab_id,
            english_bab_number: request.english_bab_number,
            arabic_bab_number: request.arabic_bab_number,
            hadith_number: request.hadith_number,
            our_hadith_number: request.our_hadith_number,
            arabic_urn: request.arabic_urn,
            arabic_bab_name: request.arabic_bab_name,
            arabic_text: request.arabic_text,
            arabic_transliteration: request.arabic_transliteration,
            arabic_grade: request.arabic_grade.unwrap_or_default(),
            english_urn: request.english_urn,
            english_bab_name: request.english_bab_name,
            english_text: request.english_text,
            english_grade: request.english_grade.unwrap_or_default(),
            last_updated: request.last_updated,
            xrefs: request.xrefs.unwrap_or_default(),
        }
    }
}
