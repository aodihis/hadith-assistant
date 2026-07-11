use sqlx::PgPool;

use crate::domain::{Hadith, HadithSearch};
use crate::error::AppError;
use crate::repositories::hadiths::HadithRepository;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

#[derive(Clone)]
pub struct HadithService {
    repository: HadithRepository,
}

impl HadithService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repository: HadithRepository::new(pool),
        }
    }

    pub async fn list(&self, search: HadithSearch) -> Result<Vec<Hadith>, AppError> {
        self.repository.list(&validate_search(search)?).await
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Hadith, AppError> {
        validate_id(id)?;
        self.repository.find_by_id(id).await
    }

    pub async fn find_by_reference(
        &self,
        collection: &str,
        book_number: &str,
        hadith_number: &str,
    ) -> Result<Hadith, AppError> {
        let collection = required("collection", collection)?;
        let book_number = required("book_number", book_number)?;
        let hadith_number = required("hadith_number", hadith_number)?;

        self.repository
            .find_by_reference(&collection, &book_number, &hadith_number)
            .await
    }
}

fn validate_search(search: HadithSearch) -> Result<HadithSearch, AppError> {
    let limit = if search.limit == 0 {
        DEFAULT_LIMIT
    } else {
        search.limit
    };

    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(AppError::Validation(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }

    if search.offset < 0 {
        return Err(AppError::Validation(
            "offset must be greater than or equal to 0".to_owned(),
        ));
    }

    Ok(HadithSearch {
        collection: trim_optional(search.collection),
        book_number: trim_optional(search.book_number),
        hadith_number: trim_optional(search.hadith_number),
        grade: trim_optional(search.grade),
        limit,
        offset: search.offset,
    })
}

fn validate_id(id: i64) -> Result<(), AppError> {
    if id <= 0 {
        return Err(AppError::Validation("id must be greater than 0".to_owned()));
    }

    Ok(())
}

fn required(field: &str, value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::Validation(format!("{field} is required")));
    }

    Ok(value.to_owned())
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_search_defaults_limit_and_trims_filters() {
        let search = validate_search(HadithSearch {
            collection: Some(" bukhari ".to_owned()),
            book_number: Some(" 1 ".to_owned()),
            hadith_number: Some(" 1 ".to_owned()),
            grade: Some(" Sahih ".to_owned()),
            limit: 0,
            offset: 5,
        })
        .expect("valid search should normalize");

        assert_eq!(search.collection.as_deref(), Some("bukhari"));
        assert_eq!(search.book_number.as_deref(), Some("1"));
        assert_eq!(search.hadith_number.as_deref(), Some("1"));
        assert_eq!(search.grade.as_deref(), Some("Sahih"));
        assert_eq!(search.limit, DEFAULT_LIMIT);
        assert_eq!(search.offset, 5);
    }

    #[test]
    fn validate_search_drops_empty_optional_filters() {
        let search = validate_search(HadithSearch {
            collection: Some(" ".to_owned()),
            book_number: Some(" ".to_owned()),
            hadith_number: Some(" ".to_owned()),
            grade: Some(" ".to_owned()),
            limit: 10,
            offset: 0,
        })
        .expect("empty optional filters should be ignored");

        assert_eq!(search.collection, None);
        assert_eq!(search.book_number, None);
        assert_eq!(search.hadith_number, None);
        assert_eq!(search.grade, None);
    }

    #[test]
    fn validate_search_rejects_invalid_limit() {
        let error = validate_search(HadithSearch {
            limit: MAX_LIMIT + 1,
            ..HadithSearch::default()
        })
        .expect_err("limit above max should fail");

        assert!(matches!(
            error,
            AppError::Validation(message)
                if message == format!("limit must be between 1 and {MAX_LIMIT}")
        ));
    }

    #[test]
    fn validate_search_rejects_negative_offset() {
        let error = validate_search(HadithSearch {
            limit: 10,
            offset: -1,
            ..HadithSearch::default()
        })
        .expect_err("negative offset should fail");

        assert!(matches!(
            error,
            AppError::Validation(message)
                if message == "offset must be greater than or equal to 0"
        ));
    }

    #[test]
    fn validate_id_rejects_non_positive_id() {
        let error = validate_id(0).expect_err("non-positive id should fail");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "id must be greater than 0"
        ));
    }

    #[test]
    fn validate_id_accepts_positive_id() {
        validate_id(1).expect("positive id should be valid");
    }
}
