mod answers;
mod chat;
mod collections;
mod hadiths;
mod health;
mod retrieval;

use serde::Serialize;
use topcoat::{
    Result,
    context::{Cx, CxBuilder},
    router::content::Json,
    router::{Body, IntoResponse, Next, Response, StatusCode, layer},
};

use crate::error::AppError;

#[layer]
async fn trace_request(cx: &mut CxBuilder, body: Body, next: Next<'_>) -> Result<Response> {
    let started = std::time::Instant::now();
    let response = next.run(cx, body).await?;
    tracing::info!(
        status = response.status().as_u16(),
        elapsed_ms = started.elapsed().as_millis(),
        "API request completed"
    );
    Ok(response)
}

pub(super) struct ApiResponse<T>(pub Result<T, AppError>);

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

impl<T> IntoResponse for ApiResponse<T>
where
    T: Serialize,
{
    fn into_response(self, cx: &Cx) -> Result<Response> {
        match self.0 {
            Ok(value) => Json(value).into_response(cx),
            Err(error) => {
                let status = status_for(&error);
                if status.is_server_error() {
                    tracing::error!(error = ?error, "API request failed");
                }
                (
                    status,
                    Json(ErrorBody {
                        code: error.code(),
                        message: error.public_message(),
                    }),
                )
                    .into_response(cx)
            }
        }
    }
}

fn status_for(error: &AppError) -> StatusCode {
    match error {
        AppError::Validation(_) => StatusCode::BAD_REQUEST,
        AppError::NotFound(_) => StatusCode::NOT_FOUND,
        AppError::Conflict(_) => StatusCode::CONFLICT,
        AppError::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
        // 440 is not a registered status; 401 is the closest standard fit for
        // "your session is no longer valid, get a new one". The stable
        // `session_expired` code in the body is what clients should branch on.
        AppError::SessionExpired(_) => StatusCode::UNAUTHORIZED,
        AppError::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
        AppError::Database(_) | AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
