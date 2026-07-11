use sqlx::PgPool;

use crate::domain::Collection;
use crate::error::AppError;
use crate::repositories::collections::CollectionRepository;

#[derive(Clone)]
pub struct CollectionService {
    repository: CollectionRepository,
}

impl CollectionService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repository: CollectionRepository::new(pool),
        }
    }

    pub async fn list(&self) -> Result<Vec<Collection>, AppError> {
        self.repository.list().await
    }

    pub async fn find_by_slug(&self, slug: &str) -> Result<Collection, AppError> {
        validate_slug(slug)?;
        self.repository.find_by_slug(slug.trim()).await
    }
}

fn validate_slug(slug: &str) -> Result<(), AppError> {
    required("slug", slug).map(|_| ())
}

fn required(field: &str, value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::Validation(format!("{field} is required")));
    }

    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_slug_rejects_blank_slug() {
        let error = validate_slug(" ").expect_err("blank slug should fail");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "slug is required"
        ));
    }

    #[test]
    fn validate_slug_accepts_and_trims_non_blank_slug() {
        validate_slug(" bukhari ").expect("non-blank slug should pass");
    }
}
