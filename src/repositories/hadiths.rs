use sqlx::{PgPool, QueryBuilder};

use crate::domain::{Hadith, HadithInput, HadithSearch};
use crate::error::AppError;

#[derive(Clone)]
pub struct HadithRepository {
    pool: PgPool,
}

impl HadithRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list(&self, search: &HadithSearch) -> Result<Vec<Hadith>, AppError> {
        let mut query = QueryBuilder::new(HADITH_SELECT);
        query.push(" WHERE 1 = 1");

        if let Some(collection) = &search.collection {
            query.push(" AND c.slug = ").push_bind(collection);
        }

        if let Some(book_number) = &search.book_number {
            query.push(" AND h.book_number = ").push_bind(book_number);
        }

        if let Some(hadith_number) = &search.hadith_number {
            query
                .push(" AND h.hadith_number = ")
                .push_bind(hadith_number);
        }

        if let Some(grade) = &search.grade {
            query
                .push(" AND (h.arabic_grade = ")
                .push_bind(grade)
                .push(" OR h.english_grade = ")
                .push_bind(grade)
                .push(")");
        }

        query
            .push(" ORDER BY c.slug, h.book_number, h.id LIMIT ")
            .push_bind(search.limit)
            .push(" OFFSET ")
            .push_bind(search.offset);

        let hadiths = query
            .build_query_as::<Hadith>()
            .fetch_all(&self.pool)
            .await?;

        Ok(hadiths)
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Hadith, AppError> {
        let hadith = sqlx::query_as::<_, Hadith>(&format!("{HADITH_SELECT} WHERE h.id = $1"))
            .bind(id)
            .fetch_one(&self.pool)
            .await?;

        Ok(hadith)
    }

    pub async fn find_by_reference(
        &self,
        collection: &str,
        book_number: &str,
        hadith_number: &str,
    ) -> Result<Hadith, AppError> {
        let hadith = sqlx::query_as::<_, Hadith>(&format!(
            "{HADITH_SELECT} WHERE c.slug = $1 AND h.book_number = $2 AND h.hadith_number = $3"
        ))
        .bind(collection)
        .bind(book_number)
        .bind(hadith_number)
        .fetch_one(&self.pool)
        .await?;

        Ok(hadith)
    }

    pub async fn create(&self, input: &HadithInput) -> Result<Hadith, AppError> {
        let collection_id = self.collection_id(&input.collection_slug).await?;
        let id = sqlx::query_scalar::<_, i64>(HADITH_INSERT)
            .bind(collection_id)
            .bind(&input.book_number)
            .bind(input.bab_id)
            .bind(&input.english_bab_number)
            .bind(&input.arabic_bab_number)
            .bind(&input.hadith_number)
            .bind(input.our_hadith_number)
            .bind(input.arabic_urn)
            .bind(&input.arabic_bab_name)
            .bind(&input.arabic_text)
            .bind(&input.arabic_transliteration)
            .bind(&input.arabic_grade)
            .bind(input.english_urn)
            .bind(&input.english_bab_name)
            .bind(&input.english_text)
            .bind(&input.english_grade)
            .bind(&input.last_updated)
            .bind(&input.xrefs)
            .fetch_one(&self.pool)
            .await?;

        self.find_by_id(id).await
    }

    pub async fn update(&self, id: i64, input: &HadithInput) -> Result<Hadith, AppError> {
        let collection_id = self.collection_id(&input.collection_slug).await?;
        let result = sqlx::query(HADITH_UPDATE)
            .bind(collection_id)
            .bind(&input.book_number)
            .bind(input.bab_id)
            .bind(&input.english_bab_number)
            .bind(&input.arabic_bab_number)
            .bind(&input.hadith_number)
            .bind(input.our_hadith_number)
            .bind(input.arabic_urn)
            .bind(&input.arabic_bab_name)
            .bind(&input.arabic_text)
            .bind(&input.arabic_transliteration)
            .bind(&input.arabic_grade)
            .bind(input.english_urn)
            .bind(&input.english_bab_name)
            .bind(&input.english_text)
            .bind(&input.english_grade)
            .bind(&input.last_updated)
            .bind(&input.xrefs)
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("hadith not found".to_owned()));
        }

        self.find_by_id(id).await
    }

    pub async fn delete(&self, id: i64) -> Result<(), AppError> {
        let result = sqlx::query(
            r#"
            DELETE FROM hadiths
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("hadith not found".to_owned()));
        }

        Ok(())
    }

    async fn collection_id(&self, slug: &str) -> Result<i64, AppError> {
        let id = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT id
            FROM collections
            WHERE slug = $1
            "#,
        )
        .bind(slug)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => AppError::NotFound("collection not found".to_owned()),
            other => AppError::Database(other),
        })?;

        Ok(id)
    }
}

const HADITH_SELECT: &str = r#"
    SELECT
        h.id,
        h.collection_id,
        c.slug AS collection,
        h.book_number,
        h.bab_id,
        h.english_bab_number,
        h.arabic_bab_number,
        h.hadith_number,
        h.our_hadith_number,
        h.arabic_urn,
        h.arabic_bab_name,
        h.arabic_text,
        h.arabic_transliteration,
        h.arabic_grade,
        h.english_urn,
        h.english_bab_name,
        h.english_text,
        h.english_grade,
        h.last_updated,
        h.xrefs
    FROM hadiths h
    JOIN collections c ON c.id = h.collection_id
"#;

const HADITH_INSERT: &str = r#"
    INSERT INTO hadiths (
        collection_id,
        book_number,
        bab_id,
        english_bab_number,
        arabic_bab_number,
        hadith_number,
        our_hadith_number,
        arabic_urn,
        arabic_bab_name,
        arabic_text,
        arabic_transliteration,
        arabic_grade,
        english_urn,
        english_bab_name,
        english_text,
        english_grade,
        last_updated,
        xrefs
    )
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
    RETURNING id
"#;

const HADITH_UPDATE: &str = r#"
    UPDATE hadiths
    SET collection_id = $1,
        book_number = $2,
        bab_id = $3,
        english_bab_number = $4,
        arabic_bab_number = $5,
        hadith_number = $6,
        our_hadith_number = $7,
        arabic_urn = $8,
        arabic_bab_name = $9,
        arabic_text = $10,
        arabic_transliteration = $11,
        arabic_grade = $12,
        english_urn = $13,
        english_bab_name = $14,
        english_text = $15,
        english_grade = $16,
        last_updated = $17,
        xrefs = $18,
        updated_at = now()
    WHERE id = $19
"#;
