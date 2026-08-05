use std::collections::HashMap;

use sqlx::PgPool;

use crate::domain::Narrator;
use crate::error::AppError;

#[derive(Clone)]
pub struct NarratorRepository {
    pool: PgPool,
}

impl NarratorRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_hadith_id(&self, hadith_id: i64) -> Result<Vec<Narrator>, AppError> {
        let narrators = sqlx::query_as::<_, Narrator>(
            r#"
            SELECT id, hadith_id, external_id, role, name, "position"
            FROM narrators
            WHERE hadith_id = $1
            ORDER BY "position"
            "#,
        )
        .bind(hadith_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(narrators)
    }

    /// Picks one narrator per hadith: the one with role "sahabi" if present,
    /// otherwise the one with the lowest position. Hadiths with no narrator
    /// rows are simply absent from the returned map. Selection happens in
    /// SQL (`DISTINCT ON`) rather than a client-side reduction so it scales
    /// with the batch instead of transferring every narrator row.
    pub async fn find_primary_by_hadith_ids(
        &self,
        hadith_ids: &[i64],
    ) -> Result<HashMap<i64, Narrator>, AppError> {
        let narrators = sqlx::query_as::<_, Narrator>(
            r#"
            SELECT DISTINCT ON (hadith_id) id, hadith_id, external_id, role, name, "position"
            FROM narrators
            WHERE hadith_id = ANY($1)
            ORDER BY hadith_id, (role = 'sahabi') DESC, "position"
            "#,
        )
        .bind(hadith_ids)
        .fetch_all(&self.pool)
        .await?;

        Ok(narrators
            .into_iter()
            .map(|narrator| (narrator.hadith_id, narrator))
            .collect())
    }
}
