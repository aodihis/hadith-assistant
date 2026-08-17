use serde::Serialize;
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{path_param, query_params, route},
};

// `bad_request` is referenced by name inside the query_params macro, which the
// unused-import lint cannot see through.
#[allow(unused_imports)]
use topcoat::router::error::bad_request;

use crate::application::AppServices;
use crate::domain::RetrievedHadith;
use crate::web::api::ApiResponse;

use super::Id;

/// Related narrations are capped here as well as floored in the service.
/// `normalize_related_limit` only raises a non-positive limit, so without this
/// a hand-typed `?limit=100000` would reach Qdrant.
const MAX_RELATED: i64 = 10;

#[topcoat::router::query_params(error = bad_request)]
struct RelatedQuery {
    limit: Option<i64>,
}

#[derive(Serialize)]
struct RelatedResponse {
    hadith_id: i64,
    related: Vec<RetrievedHadith>,
}

#[route(GET)]
async fn related(cx: &Cx) -> Result<ApiResponse<RelatedResponse>> {
    let services = app_context::<AppServices>(cx);
    let id = path_param::<Id>(cx)?;
    let query = query_params::<RelatedQuery>(cx)?;

    let limit = query.limit.unwrap_or_default().min(MAX_RELATED);

    Ok(ApiResponse(
        services
            .retrieval
            .find_related(*id, limit)
            .await
            .map(|related| RelatedResponse {
                hadith_id: *id,
                related,
            }),
    ))
}
