use sqlx::{PgPool, QueryBuilder};

use crate::domain::{Hadith, HadithSearch};
use crate::error::AppError;

/// Applies the shared filter predicates. Used by both `list` and `count` so
/// the two can never disagree about what matches.
fn push_filters<'a>(query: &mut QueryBuilder<'a, sqlx::Postgres>, search: &'a HadithSearch) {
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
}

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
        push_filters(&mut query, search);

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

    /// Total rows matching the same filters as `list`, ignoring paging.
    ///
    /// Shares `push_filters` with `list` on purpose: a count that drifts from
    /// the listing it labels is worse than no count at all.
    pub async fn count(&self, search: &HadithSearch) -> Result<i64, AppError> {
        let mut query = QueryBuilder::new(
            "SELECT COUNT(*) FROM hadiths h JOIN collections c ON c.id = h.collection_id",
        );
        push_filters(&mut query, search);

        let total = query
            .build_query_scalar::<i64>()
            .fetch_one(&self.pool)
            .await?;

        Ok(total)
    }

    /// Book numbers present in the data, for the browser's filter dropdown.
    pub async fn distinct_book_numbers(&self) -> Result<Vec<String>, AppError> {
        let books = sqlx::query_scalar::<_, String>(
            r#"
            -- GROUP BY rather than DISTINCT: ordering by a derived
            -- expression is only allowed when the column is grouped.
            -- Numeric-looking books sort numerically so 2 precedes 10.
            SELECT book_number
            FROM hadiths
            GROUP BY book_number
            ORDER BY NULLIF(regexp_replace(book_number, '\D', '', 'g'), '')::BIGINT NULLS LAST,
                     book_number
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(books)
    }

    /// The most common grades, most frequent first.
    ///
    /// The source data carries hundreds of grade spellings — casing and
    /// apostrophe variants of the same few grades — so offering every distinct
    /// value would be unusable. The common ones cover almost every record.
    pub async fn common_grades(&self, limit: i64) -> Result<Vec<String>, AppError> {
        let grades = sqlx::query_scalar::<_, String>(
            r#"
            SELECT english_grade
            FROM hadiths
            WHERE length(btrim(english_grade)) > 0
            GROUP BY english_grade
            ORDER BY COUNT(*) DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(grades)
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Hadith, AppError> {
        let hadith = sqlx::query_as::<_, Hadith>(&format!("{HADITH_SELECT} WHERE h.id = $1"))
            .bind(id)
            .fetch_one(&self.pool)
            .await?;

        Ok(hadith)
    }

    pub async fn find_by_ids(&self, ids: &[i64]) -> Result<Vec<Hadith>, AppError> {
        let hadiths = sqlx::query_as::<_, Hadith>(&format!(
            "{HADITH_SELECT} WHERE h.id = ANY($1) ORDER BY h.id"
        ))
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;

        Ok(hadiths)
    }

    pub async fn find_by_reference(
        &self,
        collection: &str,
        book_number: &str,
        hadith_number: &str,
    ) -> Result<Vec<Hadith>, AppError> {
        let hadiths = sqlx::query_as::<_, Hadith>(&format!(
            "{HADITH_SELECT} WHERE c.slug = $1 AND h.book_number = $2 AND h.hadith_number = $3 ORDER BY h.id"
        ))
        .bind(collection)
        .bind(book_number)
        .bind(hadith_number)
        .fetch_all(&self.pool)
        .await?;

        if hadiths.is_empty() {
            return Err(AppError::NotFound("Hadith reference not found".to_owned()));
        }

        Ok(hadiths)
    }
}

const HADITH_SELECT: &str = r#"
    SELECT
        h.id,
        h.collection_id,
        c.slug AS collection,
        c.name AS collection_name,
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
