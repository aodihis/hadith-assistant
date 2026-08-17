use topcoat::{
    Error, Result,
    context::{Cx, app_context},
    router::error::{bad_request, internal_server_error, not_found},
    router::{page, query_params},
    view::view,
};

use crate::application::AppServices;
use crate::domain::RetrievalQuery;
use crate::error::AppError;

use super::templates::search::{SearchOutcome, search_view};

#[topcoat::router::query_params(error = bad_request)]
struct SearchQuery {
    q: Option<String>,
    collection: Option<String>,
}

#[page]
async fn search(cx: &Cx) -> Result {
    let query = query_params::<SearchQuery>(cx)?;
    let q = query.q.clone().unwrap_or_default();
    let selected_collection = query.collection.clone().unwrap_or_default();
    let submitted = query
        .q
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());

    let services = app_context::<AppServices>(cx);

    let collections = services.collections.list().await.map_err(page_error)?;

    let mut validation_error = None;
    let mut service_error = false;
    let mut results = Vec::new();

    if submitted {
        let retrieval_query = RetrievalQuery {
            query: q.clone(),
            collection: query
                .collection
                .clone()
                .filter(|value| !value.trim().is_empty()),
            limit: 0,
        };

        match services.retrieval.retrieve(retrieval_query).await {
            Ok(result) => results = result.results,
            Err(AppError::Validation(message)) => validation_error = Some(message),
            Err(error) => {
                tracing::error!(error = ?error, "search request failed");
                service_error = true;
            }
        }
    }

    let no_results =
        submitted && !service_error && validation_error.is_none() && results.is_empty();

    let outcome = SearchOutcome {
        submitted,
        validation_error,
        service_error,
        no_results,
        results,
    };

    view! {
        search_view(
            collections: collections,
            q: q,
            selected_collection: selected_collection,
            outcome: outcome,
        )
    }
}

fn page_error(error: AppError) -> Error {
    match error {
        AppError::Validation(message) => bad_request(message).into(),
        AppError::NotFound(_) => not_found().into(),
        error => {
            tracing::error!(error = ?error, "page request failed");
            internal_server_error(error).into()
        }
    }
}
