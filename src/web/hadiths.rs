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

use super::templates::hadiths::{HadithPage, hadith_list_view};

/// Page sizes offered in the UI. Clamping to this set keeps a hand-typed
/// `?limit=5000` from producing an error page instead of a usable one.
const PAGE_SIZES: [i64; 3] = [25, 50, 100];
const DEFAULT_PAGE_SIZE: i64 = 25;

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

    let limit = query
        .limit
        .filter(|limit| PAGE_SIZES.contains(limit))
        .unwrap_or(DEFAULT_PAGE_SIZE);
    let offset = query.offset.unwrap_or_default().max(0);

    let services = app_context::<AppServices>(cx);

    let filter = |value: &Option<String>| {
        value
            .clone()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };

    let search = HadithSearch {
        collection: filter(&query.collection),
        book_number: filter(&query.book_number),
        hadith_number: filter(&query.hadith_number),
        grade: filter(&query.grade),
        limit,
        offset,
    };

    let (hadiths, total) = services
        .hadiths
        .list_page(search)
        .await
        .map_err(page_error)?;

    let collections = services.collections.list().await.map_err(page_error)?;
    let (book_numbers, grades) = services
        .hadiths
        .filter_options()
        .await
        .map_err(page_error)?;

    view! {
        hadith_list_view(
            page: HadithPage {
                hadiths,
                total,
                limit,
                offset,
                page_sizes: PAGE_SIZES.to_vec(),
                collections,
                book_numbers,
                grades,
                selected_collection: query.collection.clone().unwrap_or_default(),
                selected_book_number: query.book_number.clone().unwrap_or_default(),
                selected_grade: query.grade.clone().unwrap_or_default(),
                hadith_number: query.hadith_number.clone().unwrap_or_default(),
            },
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
