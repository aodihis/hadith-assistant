use serde::{Deserialize, Serialize};
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::content::Json,
    router::route,
};

use crate::application::{AnsweredQuestion, AppServices};
use crate::domain::{RetrievalQuery, RetrievedHadith};

use super::ApiResponse;

#[derive(Deserialize)]
struct AnswerRequest {
    query: String,
    collection: Option<String>,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct AnswerBody {
    title: String,
    answer: String,
}

/// `answer` is null when generation is unavailable or produced nothing
/// usable. `citations` is always present, so the response never carries
/// generated text without its canonical sources.
#[derive(Serialize)]
struct AnswerResponse {
    query: String,
    answer: Option<AnswerBody>,
    citations: Vec<RetrievedHadith>,
}

impl From<AnsweredQuestion> for AnswerResponse {
    fn from(answered: AnsweredQuestion) -> Self {
        Self {
            query: answered.query,
            answer: answered.answer.map(|generated| AnswerBody {
                title: generated.title,
                answer: generated.answer,
            }),
            citations: answered.citations,
        }
    }
}

#[route(POST)]
async fn answer(
    cx: &Cx,
    Json(request): Json<AnswerRequest>,
) -> Result<ApiResponse<AnswerResponse>> {
    let services = app_context::<AppServices>(cx);

    Ok(ApiResponse(
        services
            .questions
            .ask(RetrievalQuery {
                query: request.query,
                collection: request.collection,
                limit: request.limit.unwrap_or_default(),
            })
            .await
            .map(AnswerResponse::from),
    ))
}
