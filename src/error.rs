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

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "validation_error",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::NotImplemented(_) => "not_implemented",
            Self::Database(_) => "database_error",
            Self::Internal(_) => "internal_error",
        }
    }

    pub fn public_message(&self) -> String {
        match self {
            Self::Database(_) => "database error".to_owned(),
            _ => self.to_string(),
        }
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
    fn exposes_stable_codes_and_safe_messages() {
        let cases = [
            (
                AppError::Validation("bad input".to_owned()),
                "validation_error",
                "validation failed: bad input",
            ),
            (
                AppError::NotFound("missing".to_owned()),
                "not_found",
                "not found: missing",
            ),
            (
                AppError::Conflict("duplicate".to_owned()),
                "conflict",
                "conflict: duplicate",
            ),
            (
                AppError::NotImplemented("later".to_owned()),
                "not_implemented",
                "not implemented: later",
            ),
            (
                AppError::Internal("oops".to_owned()),
                "internal_error",
                "internal error: oops",
            ),
        ];

        for (error, code, message) in cases {
            assert_eq!(error.code(), code);
            assert_eq!(error.public_message(), message);
        }
    }

    #[test]
    fn database_errors_use_safe_public_message() {
        let error = AppError::Database(sqlx::Error::RowNotFound);

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
