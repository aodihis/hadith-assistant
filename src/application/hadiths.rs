use sqlx::PgPool;

use crate::domain::{Hadith, HadithSearch};
use crate::error::AppError;
use crate::infrastructure::persistence::hadiths::HadithRepository;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

/// A page of results, with the filters that actually produced them.
///
/// `search` is the *effective* search, not the requested one: limits clamped,
/// blank filters dropped. A caller rendering the filter UI needs this rather
/// than the raw request, or the controls will claim a filter that was never
/// applied.
pub struct PagedHadiths {
    pub hadiths: Vec<Hadith>,
    pub total: i64,
    pub search: HadithSearch,
}

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

    /// A page of results together with the total matching the same filters.
    ///
    /// Validation runs once and both queries use the result, so the count can
    /// never describe a different filter set than the rows beside it.
    pub async fn list_page(&self, search: HadithSearch) -> Result<PagedHadiths, AppError> {
        let search = validate_search(search)?;
        let hadiths = self.repository.list(&search).await?;
        let total = self.repository.count(&search).await?;

        Ok(PagedHadiths {
            hadiths,
            total,
            search,
        })
    }

    /// Options for the browser's filter dropdowns.
    pub async fn filter_options(&self) -> Result<(Vec<String>, Vec<String>), AppError> {
        const COMMON_GRADES: i64 = 12;

        let books = self.repository.distinct_book_numbers().await?;
        let grades = self.repository.common_grades(COMMON_GRADES).await?;

        Ok((books, grades))
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
    ) -> Result<Vec<Hadith>, AppError> {
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

/// Trims a filter, treating blank as absent.
///
/// Takes the `String` by value so the common case — a filter that needs no
/// trimming, which is every value picked from a dropdown — reuses the caller's
/// allocation instead of copying it. That is the whole reason `validate_search`
/// consumes its argument rather than borrowing: borrowing would force a copy on
/// every field of every request.
fn trim_optional(value: Option<String>) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();

    if trimmed.is_empty() {
        return None;
    }

    if trimmed.len() == value.len() {
        return Some(value);
    }

    Some(trimmed.to_owned())
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
    fn an_already_trimmed_filter_keeps_its_allocation() {
        let original = "bukhari".to_owned();
        let address = original.as_ptr();

        let trimmed = trim_optional(Some(original)).expect("a non-blank filter survives");

        // Same buffer, not a copy: consuming the argument is only worthwhile if
        // the untouched case is free.
        assert_eq!(trimmed.as_ptr(), address);
        assert_eq!(trimmed, "bukhari");
    }

    #[test]
    fn a_padded_filter_is_trimmed() {
        assert_eq!(
            trim_optional(Some("  Sahih  ".to_owned())).as_deref(),
            Some("Sahih")
        );
        assert_eq!(trim_optional(Some("   ".to_owned())), None);
        assert_eq!(trim_optional(None), None);
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
