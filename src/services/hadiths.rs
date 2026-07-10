use sqlx::PgPool;

use crate::domain::{Hadith, HadithInput, HadithSearch};
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

    pub async fn create(&self, input: HadithInput) -> Result<Hadith, AppError> {
        self.repository.create(&validate_hadith(input)?).await
    }

    pub async fn update(&self, id: i64, input: HadithInput) -> Result<Hadith, AppError> {
        validate_id(id)?;
        self.repository.update(id, &validate_hadith(input)?).await
    }

    pub async fn delete(&self, id: i64) -> Result<(), AppError> {
        validate_id(id)?;
        self.repository.delete(id).await
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

fn validate_hadith(input: HadithInput) -> Result<HadithInput, AppError> {
    let collection_slug = required("collection_slug", &input.collection_slug)?;
    let book_number = required("book_number", &input.book_number)?;
    let hadith_number = required("hadith_number", &input.hadith_number)?;
    let arabic_text = required("arabic_text", &input.arabic_text)?;

    if input.arabic_urn <= 0 {
        return Err(AppError::Validation(
            "arabic_urn must be greater than 0".to_owned(),
        ));
    }

    if input.english_urn <= 0 {
        return Err(AppError::Validation(
            "english_urn must be greater than 0".to_owned(),
        ));
    }

    Ok(HadithInput {
        collection_slug,
        book_number,
        bab_id: input.bab_id,
        english_bab_number: trim_optional(input.english_bab_number),
        arabic_bab_number: trim_optional(input.arabic_bab_number),
        hadith_number,
        our_hadith_number: input.our_hadith_number,
        arabic_urn: input.arabic_urn,
        arabic_bab_name: trim_optional(input.arabic_bab_name),
        arabic_text,
        arabic_transliteration: trim_optional(input.arabic_transliteration),
        arabic_grade: input.arabic_grade.trim().to_owned(),
        english_urn: input.english_urn,
        english_bab_name: trim_optional(input.english_bab_name),
        english_text: trim_optional(input.english_text),
        english_grade: input.english_grade.trim().to_owned(),
        last_updated: trim_optional(input.last_updated),
        xrefs: input.xrefs.trim().to_owned(),
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
    fn validate_hadith_trims_required_and_optional_fields() {
        let hadith = validate_hadith(sample_hadith_input()).expect("valid Hadith should normalize");

        assert_eq!(hadith.collection_slug, "bukhari");
        assert_eq!(hadith.book_number, "1");
        assert_eq!(hadith.hadith_number, "1");
        assert_eq!(hadith.arabic_text, "arabic text");
        assert_eq!(hadith.english_bab_number.as_deref(), Some("1"));
        assert_eq!(hadith.arabic_bab_number, None);
        assert_eq!(hadith.arabic_bab_name.as_deref(), Some("باب"));
        assert_eq!(hadith.arabic_transliteration, None);
        assert_eq!(hadith.arabic_grade, "صحيح");
        assert_eq!(hadith.english_bab_name.as_deref(), Some("Chapter"));
        assert_eq!(hadith.english_text, None);
        assert_eq!(hadith.english_grade, "Sahih");
        assert_eq!(hadith.last_updated.as_deref(), Some("2021-03-04 23:36:31"));
        assert_eq!(hadith.xrefs, "");
    }

    #[test]
    fn validate_hadith_rejects_missing_required_fields() {
        for (field, mut input) in [
            ("collection_slug", sample_hadith_input()),
            ("book_number", sample_hadith_input()),
            ("hadith_number", sample_hadith_input()),
            ("arabic_text", sample_hadith_input()),
        ] {
            match field {
                "collection_slug" => input.collection_slug = " ".to_owned(),
                "book_number" => input.book_number = " ".to_owned(),
                "hadith_number" => input.hadith_number = " ".to_owned(),
                "arabic_text" => input.arabic_text = " ".to_owned(),
                _ => unreachable!("test only covers known required fields"),
            }

            let error = validate_hadith(input).expect_err("required field should fail");
            assert!(matches!(
                error,
                AppError::Validation(message) if message == format!("{field} is required")
            ));
        }
    }

    #[test]
    fn validate_hadith_rejects_non_positive_arabic_urn() {
        let mut input = sample_hadith_input();
        input.arabic_urn = 0;

        let error = validate_hadith(input).expect_err("invalid Arabic URN should fail");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "arabic_urn must be greater than 0"
        ));
    }

    #[test]
    fn validate_hadith_rejects_non_positive_english_urn() {
        let mut input = sample_hadith_input();
        input.english_urn = 0;

        let error = validate_hadith(input).expect_err("invalid English URN should fail");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "english_urn must be greater than 0"
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

    fn sample_hadith_input() -> HadithInput {
        HadithInput {
            collection_slug: " bukhari ".to_owned(),
            book_number: " 1 ".to_owned(),
            bab_id: 1.0,
            english_bab_number: Some(" 1 ".to_owned()),
            arabic_bab_number: Some(" ".to_owned()),
            hadith_number: " 1 ".to_owned(),
            our_hadith_number: 1,
            arabic_urn: 100010,
            arabic_bab_name: Some(" باب ".to_owned()),
            arabic_text: " arabic text ".to_owned(),
            arabic_transliteration: Some(" ".to_owned()),
            arabic_grade: " صحيح ".to_owned(),
            english_urn: 10,
            english_bab_name: Some(" Chapter ".to_owned()),
            english_text: Some(" ".to_owned()),
            english_grade: " Sahih ".to_owned(),
            last_updated: Some(" 2021-03-04 23:36:31 ".to_owned()),
            xrefs: " ".to_owned(),
        }
    }
}
