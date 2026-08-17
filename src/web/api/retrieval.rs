use serde::Deserialize;
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::content::Json,
    router::route,
};

use crate::application::AppServices;
use crate::domain::{RetrievalQuery, RetrievalResult};

use super::ApiResponse;

#[derive(Deserialize)]
struct RetrievalRequest {
    query: String,
    collection: Option<String>,
    limit: Option<i64>,
}

#[route(POST)]
async fn retrieve(
    cx: &Cx,
    Json(request): Json<RetrievalRequest>,
) -> Result<ApiResponse<RetrievalResult>> {
    let services = app_context::<AppServices>(cx);
    Ok(ApiResponse(
        services
            .retrieval
            .retrieve(RetrievalQuery {
                query: request.query,
                collection: request.collection,
                limit: request.limit.unwrap_or_default(),
            })
            .await,
    ))
}
