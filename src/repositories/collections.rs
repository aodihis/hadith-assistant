use sqlx::PgPool;

use crate::domain::Collection;
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
}
