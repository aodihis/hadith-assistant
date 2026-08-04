use serde::Serialize;
use topcoat::{Result, router::route, router::content::Json};

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[route(GET)]
async fn health() -> Result<Json<HealthResponse>> {
    Ok(Json(HealthResponse { status: "ok" }))
}
