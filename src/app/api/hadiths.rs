mod by_reference;
mod id;

use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{query_params, route},
};

use crate::application::AppServices;
use crate::domain::{Hadith, HadithSearch};

use super::ApiResponse;

#[topcoat::router::query_params(error = bad_request)]
struct HadithListQuery {
    collection: Option<String>,
    book_number: Option<String>,
    hadith_number: Option<String>,
    grade: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[route(GET)]
async fn list_hadiths(cx: &Cx) -> Result<ApiResponse<Vec<Hadith>>> {
    let services = app_context::<AppServices>(cx);
    let query = query_params::<HadithListQuery>(cx)?;
    Ok(ApiResponse(
        services
            .hadiths
            .list(HadithSearch {
                collection: query.collection.clone(),
                book_number: query.book_number.clone(),
                hadith_number: query.hadith_number.clone(),
                grade: query.grade.clone(),
                limit: query.limit.unwrap_or_default(),
                offset: query.offset.unwrap_or_default(),
            })
            .await,
    ))
}
