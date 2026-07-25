use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{path_param, query_params, route},
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

#[topcoat::router::path_param(error = bad_request)]
struct Id(i64);

#[route(GET "/api/hadiths/{id}")]
async fn get_hadith(cx: &Cx) -> Result<ApiResponse<Hadith>> {
    let services = app_context::<AppServices>(cx);
    let id = path_param::<Id>(cx)?;
    Ok(ApiResponse(services.hadiths.find_by_id(id.0).await))
}

#[topcoat::router::path_param]
struct Collection(str);

#[topcoat::router::path_param]
struct BookNumber(str);

#[topcoat::router::path_param]
struct HadithNumber(str);

#[route(
    GET "/api/hadiths/by-reference/{collection}/{book_number}/{hadith_number}"
)]
async fn get_hadith_by_reference(cx: &Cx) -> Result<ApiResponse<Vec<Hadith>>> {
    let services = app_context::<AppServices>(cx);
    let collection = path_param::<Collection>(cx);
    let book_number = path_param::<BookNumber>(cx);
    let hadith_number = path_param::<HadithNumber>(cx);
    Ok(ApiResponse(
        services
            .hadiths
            .find_by_reference(collection, book_number, hadith_number)
            .await,
    ))
}
