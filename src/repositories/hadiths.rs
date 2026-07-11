use sqlx::{PgPool, QueryBuilder};

use crate::domain::{Hadith, HadithSearch};
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
