use crate::domain::{RetrievalQuery, RetrievalResult};
use crate::error::AppError;

const DEFAULT_LIMIT: i64 = 10;
const MAX_LIMIT: i64 = 20;

#[derive(Clone, Default)]
pub struct RetrievalService;

impl RetrievalService {
    pub fn new() -> Self {
        Self
    }

    pub async fn retrieve(&self, query: RetrievalQuery) -> Result<RetrievalResult, AppError> {
        let query = validate_query(query)?;

        // TODO: Implement retrieval by calling the selected vector database,
        // applying collection filters, and resolving matches back to hadiths
        // by the unique reference (collection_id, book_number, hadith_number).
        Err(AppError::NotImplemented(format!(
            "retrieval is not implemented yet for query `{}`",
            query.query
        )))
    }
}

fn validate_query(query: RetrievalQuery) -> Result<RetrievalQuery, AppError> {
    let text = query.query.trim();
    if text.is_empty() {
        return Err(AppError::Validation("query is required".to_owned()));
    }

    let limit = if query.limit == 0 {
        DEFAULT_LIMIT
    } else {
        query.limit
    };

    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(AppError::Validation(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }

    Ok(RetrievalQuery {
        query: text.to_owned(),
        collection: query
            .collection
            .map(|collection| collection.trim().to_owned())
            .filter(|collection| !collection.is_empty()),
        limit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retrieve_returns_not_implemented_after_validation() {
        let service = RetrievalService::new();

        let error = service
            .retrieve(RetrievalQuery {
                query: " intentions ".to_owned(),
                collection: Some(" bukhari ".to_owned()),
                limit: 0,
            })
            .await
            .expect_err("retrieval should remain explicitly unimplemented");

        assert!(matches!(
            error,
            AppError::NotImplemented(message)
                if message == "retrieval is not implemented yet for query `intentions`"
        ));
    }

    #[test]
    fn validate_query_trims_query_and_collection_and_defaults_limit() {
        let query = validate_query(RetrievalQuery {
            query: " intentions ".to_owned(),
            collection: Some(" bukhari ".to_owned()),
            limit: 0,
        })
        .expect("valid query should normalize");

        assert_eq!(query.query, "intentions");
        assert_eq!(query.collection.as_deref(), Some("bukhari"));
        assert_eq!(query.limit, DEFAULT_LIMIT);
    }

    #[test]
    fn validate_query_drops_empty_collection() {
        let query = validate_query(RetrievalQuery {
            query: "intentions".to_owned(),
            collection: Some(" ".to_owned()),
            limit: 3,
        })
        .expect("empty optional collection should be ignored");

        assert_eq!(query.collection, None);
        assert_eq!(query.limit, 3);
    }

    #[test]
    fn validate_query_rejects_empty_query() {
        let error = validate_query(RetrievalQuery {
            query: " ".to_owned(),
            collection: None,
            limit: 1,
        })
        .expect_err("empty query should be invalid");

        assert!(matches!(
            error,
            AppError::Validation(message) if message == "query is required"
        ));
    }

    #[test]
    fn validate_query_rejects_out_of_range_limit() {
        let error = validate_query(RetrievalQuery {
            query: "intentions".to_owned(),
            collection: None,
            limit: MAX_LIMIT + 1,
        })
        .expect_err("limit above max should be invalid");

        assert!(matches!(
            error,
            AppError::Validation(message)
                if message == format!("limit must be between 1 and {MAX_LIMIT}")
        ));
    }
}
