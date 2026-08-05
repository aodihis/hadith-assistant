use topcoat::{
    Error, Result,
    context::{Cx, app_context},
    router::error::{bad_request, internal_server_error, not_found},
    router::{page, query_params},
    view::view,
};

use crate::application::AppServices;
use crate::domain::HadithSearch;
use crate::error::AppError;

use super::templates::hadiths::hadith_list_view;

#[topcoat::router::query_params(error = bad_request)]
struct BrowseQuery {
    collection: Option<String>,
    book_number: Option<String>,
    hadith_number: Option<String>,
    grade: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[page]
async fn browse(cx: &Cx) -> Result {
    let query = query_params::<BrowseQuery>(cx)?;
    let collection = query.collection.clone().unwrap_or_default();
    let book_number = query.book_number.clone().unwrap_or_default();
    let hadith_number = query.hadith_number.clone().unwrap_or_default();
    let grade = query.grade.clone().unwrap_or_default();
    let limit = query.limit.unwrap_or(25);
    let offset = query.offset.unwrap_or_default();

    let services = app_context::<AppServices>(cx);
    let hadiths = services
        .hadiths
        .list(HadithSearch {
            collection: query.collection.clone(),
            book_number: query.book_number.clone(),
            hadith_number: query.hadith_number.clone(),
            grade: query.grade.clone(),
            limit,
            offset,
        })
        .await
        .map_err(page_error)?;

    view! {
        hadith_list_view(
            hadiths: hadiths,
            collection: collection,
            book_number: book_number,
            hadith_number: hadith_number,
            grade: grade,
            limit: limit,
        )
    }
}

fn page_error(error: AppError) -> Error {
    match error {
        AppError::Validation(message) => bad_request(message).into(),
        AppError::NotFound(_) => not_found().into(),
        error => {
            tracing::error!(error = ?error, "page request failed");
            internal_server_error(error).into()
        }
    }
}
