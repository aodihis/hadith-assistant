pub mod related;

use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{path_param, route},
};

use crate::application::AppServices;
use crate::domain::Hadith;
use crate::web::api::ApiResponse;

#[topcoat::router::path_param(error = bad_request)]
pub(crate) struct Id(i64);

#[route(GET)]
async fn get_hadith(cx: &Cx) -> Result<ApiResponse<Hadith>> {
    let services = app_context::<AppServices>(cx);
    let id = path_param::<Id>(cx)?;
    Ok(ApiResponse(services.hadiths.find_by_id(*id).await))
}
