use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{path_param, route},
};

use crate::app::api::ApiResponse;
use crate::application::AppServices;
use crate::domain::Hadith;

use super::super::Collection;
use super::BookNumber;

#[topcoat::router::path_param]
struct HadithNumber(str);

#[route(GET)]
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
