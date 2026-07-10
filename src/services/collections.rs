use sqlx::PgPool;

use crate::domain::{Collection, NewCollection};
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

    pub async fn create(&self, input: NewCollection) -> Result<Collection, AppError> {
        let input = validate_collection(input)?;
        self.repository.create(&input).await
    }

    pub async fn update(
        &self,
        current_slug: &str,
        input: NewCollection,
    ) -> Result<Collection, AppError> {
        validate_slug(current_slug)?;
        let input = validate_collection(input)?;
        self.repository.update(current_slug.trim(), &input).await
    }

    pub async fn delete(&self, slug: &str) -> Result<(), AppError> {
        validate_slug(slug)?;
        self.repository.delete(slug.trim()).await
    }
}

fn validate_collection(input: NewCollection) -> Result<NewCollection, AppError> {
    let slug = required("slug", &input.slug)?;
    let name = required("name", &input.name)?;

    Ok(NewCollection { slug, name })
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
    fn validate_collection_trims_slug_and_name() {
        let collection = validate_collection(NewCollection {
            slug: " bukhari ".to_owned(),
            name: " Sahih al-Bukhari ".to_owned(),
        })
        .expect("valid collection should normalize");

        assert_eq!(collection.slug, "bukhari");
        assert_eq!(collection.name, "Sahih al-Bukhari");
    }

    #[test]
    fn validate_collection_rejects_blank_slug() {
        let error = validate_collection(NewCollection {
            slug: " ".to_owned(),
            name: "Sahih al-Bukhari".to_owned(),
        })
        .expect_err("blank slug should fail");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "slug is required"
        ));
    }

    #[test]
    fn validate_collection_rejects_blank_name() {
        let error = validate_collection(NewCollection {
            slug: "bukhari".to_owned(),
            name: " ".to_owned(),
        })
        .expect_err("blank name should fail");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "name is required"
        ));
    }

    #[test]
    fn validate_slug_rejects_blank_slug() {
        let error = validate_slug(" ").expect_err("blank slug should fail");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "slug is required"
        ));
    }
}
