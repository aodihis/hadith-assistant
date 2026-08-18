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

    // Filters are passed through raw: `HadithService` normalizes them, and it
    // is the only place that should, so the page and the JSON route cannot
    // disagree about what an empty filter means.
    let search = HadithSearch {
        collection: query.collection.clone(),
        book_number: query.book_number.clone(),
        hadith_number: query.hadith_number.clone(),
        grade: query.grade.clone(),
        limit,
        offset,
    };

    // The page needs all three before it can render and none depends on
    // another, so they go out together rather than as three serial round trips.
    let (page, collections, (book_numbers, grades)) = tokio::try_join!(
        services.hadiths.list_page(search),
        services.collections.list(),
        services.hadiths.filter_options()
    )
    .map_err(page_error)?;

    // Rendered from the effective search rather than the raw query, so the
    // controls always show the filters that actually ran. The strings are moved
    // out of it, not copied again.
    let applied = page.search;

    view! {
        hadith_list_view(
            page: HadithPage {
                hadiths: page.hadiths,
                total: page.total,
                limit: applied.limit,
                offset: applied.offset,
                page_sizes: PAGE_SIZES.to_vec(),
                collections,
                book_numbers,
                grades,
                selected_collection: applied.collection.unwrap_or_default(),
                selected_book_number: applied.book_number.unwrap_or_default(),
                selected_grade: applied.grade.unwrap_or_default(),
                hadith_number: applied.hadith_number.unwrap_or_default(),
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
