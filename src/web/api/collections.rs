use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{path_param, route},
};

use crate::application::AppServices;
use crate::domain::Collection;

use super::ApiResponse;

#[route(GET)]
async fn list_collections(cx: &Cx) -> Result<ApiResponse<Vec<Collection>>> {
    let services = app_context::<AppServices>(cx);
    Ok(ApiResponse(services.collections.list().await))
}

#[topcoat::router::path_param]
struct Slug(str);

#[route(GET "/api/collections/{slug}")]
async fn get_collection(cx: &Cx) -> Result<ApiResponse<Collection>> {
    let services = app_context::<AppServices>(cx);
    let slug = path_param::<Slug>(cx);
    Ok(ApiResponse(services.collections.find_by_slug(slug).await))
}
