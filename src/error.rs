use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("not implemented: {0}")]
    NotImplemented(String),
    #[error("database error")]
    Database(sqlx::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    code: &'static str,
    message: String,
}

impl AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
            Self::Database(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "validation_error",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::NotImplemented(_) => "not_implemented",
            Self::Database(_) => "database_error",
            Self::Internal(_) => "internal_error",
        }
    }

    fn public_message(&self) -> String {
        match self {
            Self::Database(_) => "database error".to_owned(),
            _ => self.to_string(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = Json(ErrorResponse {
            code: self.code(),
            message: self.public_message(),
        });

        (status, body).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::RowNotFound => Self::NotFound("record not found".to_owned()),
            sqlx::Error::Database(db_error) if db_error.is_unique_violation() => {
                Self::Conflict("record already exists".to_owned())
            }
            _ => Self::Database(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_error_variants_to_expected_status_and_code() {
        let cases = [
            (
                AppError::Validation("bad input".to_owned()),
                StatusCode::BAD_REQUEST,
                "validation_error",
                "validation failed: bad input",
            ),
            (
                AppError::NotFound("missing".to_owned()),
                StatusCode::NOT_FOUND,
                "not_found",
                "not found: missing",
            ),
            (
                AppError::Conflict("duplicate".to_owned()),
                StatusCode::CONFLICT,
                "conflict",
                "conflict: duplicate",
            ),
            (
                AppError::NotImplemented("later".to_owned()),
                StatusCode::NOT_IMPLEMENTED,
                "not_implemented",
                "not implemented: later",
            ),
            (
                AppError::Internal("oops".to_owned()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "internal error: oops",
            ),
        ];

        for (error, status, code, message) in cases {
            assert_eq!(error.status_code(), status);
            assert_eq!(error.code(), code);
            assert_eq!(error.public_message(), message);
        }
    }

    #[test]
    fn database_errors_use_safe_public_message() {
        let error = AppError::Database(sqlx::Error::RowNotFound);

        assert_eq!(error.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.code(), "database_error");
        assert_eq!(error.public_message(), "database error");
    }

    #[test]
    fn row_not_found_sqlx_error_maps_to_not_found() {
        let error = AppError::from(sqlx::Error::RowNotFound);

        assert!(matches!(
            error,
            AppError::NotFound(message) if message == "record not found"
        ));
    }
}
