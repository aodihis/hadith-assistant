use serde::Serialize;
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::route,
};

use crate::application::AppServices;

use super::super::ApiResponse;

#[derive(Serialize)]
struct SessionResponse {
    token: String,
    expires_in_seconds: u64,
}

/// Issues a chat session token.
///
/// The token carries no user identity — it exists to key rate limiting and to
/// make the chat endpoint awkward to drive from outside our own pages. It is
/// deliberately not authentication: anyone may request one.
#[route(POST)]
async fn create(cx: &Cx) -> Result<ApiResponse<SessionResponse>> {
    let services = app_context::<AppServices>(cx);

    let token = services.sessions.issue();
    let expires_in_seconds = services.sessions.ttl().as_secs();

    Ok(ApiResponse(Ok(SessionResponse {
        token,
        expires_in_seconds,
    })))
}
