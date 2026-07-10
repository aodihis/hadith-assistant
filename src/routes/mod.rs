pub mod collections;
pub mod hadiths;
pub mod health;
pub mod retrieval;

use axum::Router;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .nest("/collections", collections::router())
        .nest("/hadiths", hadiths::router())
        .nest("/retrieval", retrieval::router())
        .route("/health", axum::routing::get(health::health))
        .with_state(state)
}
