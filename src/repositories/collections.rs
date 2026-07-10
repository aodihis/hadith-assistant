use sqlx::PgPool;

use crate::domain::{Collection, NewCollection};
use crate::error::AppError;

#[derive(Clone)]
pub struct CollectionRepository {
    pool: PgPool,
}

impl CollectionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list(&self) -> Result<Vec<Collection>, AppError> {
        let collections = sqlx::query_as::<_, Collection>(
            r#"
            SELECT id, slug, name
            FROM collections
            ORDER BY slug
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(collections)
    }

    pub async fn find_by_slug(&self, slug: &str) -> Result<Collection, AppError> {
        let collection = sqlx::query_as::<_, Collection>(
            r#"
            SELECT id, slug, name
            FROM collections
            WHERE slug = $1
            "#,
        )
        .bind(slug)
        .fetch_one(&self.pool)
        .await?;

        Ok(collection)
    }

    pub async fn create(&self, input: &NewCollection) -> Result<Collection, AppError> {
        let collection = sqlx::query_as::<_, Collection>(
            r#"
            INSERT INTO collections (slug, name)
            VALUES ($1, $2)
            RETURNING id, slug, name
            "#,
        )
        .bind(&input.slug)
        .bind(&input.name)
        .fetch_one(&self.pool)
        .await?;

        Ok(collection)
    }

    pub async fn update(&self, slug: &str, input: &NewCollection) -> Result<Collection, AppError> {
        let collection = sqlx::query_as::<_, Collection>(
            r#"
            UPDATE collections
            SET slug = $2,
                name = $3,
                updated_at = now()
            WHERE slug = $1
            RETURNING id, slug, name
            "#,
        )
        .bind(slug)
        .bind(&input.slug)
        .bind(&input.name)
        .fetch_one(&self.pool)
        .await?;

        Ok(collection)
    }

    pub async fn delete(&self, slug: &str) -> Result<(), AppError> {
        let result = sqlx::query(
            r#"
            DELETE FROM collections
            WHERE slug = $1
            "#,
        )
        .bind(slug)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("collection not found".to_owned()));
        }

        Ok(())
    }
}
