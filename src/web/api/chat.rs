pub mod session;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::content::Json,
    router::content::sse::{Event, KeepAlive, Sse},
    router::headers,
    router::route,
};

use crate::application::{
    AppServices, ConversationHistory, ConversationTurn, ReplyAssembler, StreamEvent,
};
use crate::domain::RetrievedHadith;
use crate::error::AppError;

type ChatEventStream = std::pin::Pin<
    Box<
        dyn futures_util::Stream<Item = std::result::Result<Event, std::convert::Infallible>>
            + Send,
    >,
>;

/// Header carrying the chat session token issued by `/api/chat/session`.
pub(crate) const SESSION_HEADER: &str = "x-sanad-session";

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    history: Option<HistoryDto>,
}

#[derive(Deserialize, Default)]
struct HistoryDto {
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    summarized_turns: usize,
    #[serde(default)]
    turns: Vec<TurnDto>,
}

#[derive(Deserialize, Serialize, Clone)]
struct TurnDto {
    question: String,
    answer: String,
    #[serde(default)]
    refused: bool,
}

#[derive(Serialize)]
struct HistoryOut {
    summary: Option<String>,
    summarized_turns: usize,
    turns: Vec<TurnDto>,
}

#[derive(Serialize)]
struct MemoryEvent {
    history: HistoryOut,
    compacted: bool,
}

#[derive(Serialize)]
struct ErrorEvent {
    code: &'static str,
    message: String,
}

impl From<HistoryDto> for ConversationHistory {
    fn from(dto: HistoryDto) -> Self {
        Self {
            summary: dto.summary,
            summarized_turns: dto.summarized_turns,
            turns: dto
                .turns
                .into_iter()
                .map(|turn| ConversationTurn {
                    question: turn.question,
                    answer: turn.answer,
                    refused: turn.refused,
                })
                .collect(),
        }
    }
}

impl From<ConversationHistory> for HistoryOut {
    fn from(history: ConversationHistory) -> Self {
        Self {
            summary: history.summary,
            summarized_turns: history.summarized_turns,
            turns: history
                .turns
                .into_iter()
                .map(|turn| TurnDto {
                    question: turn.question,
                    answer: turn.answer,
                    refused: turn.refused,
                })
                .collect(),
        }
    }
}

/// Streams one chat turn.
///
/// Event order is deliberate and is the contract clients depend on:
///
/// - `citations` — released only once the first line proves this is an answer,
///   so a refusal never flashes narration cards and retracts them.
/// - `title`, then repeated `delta` — the prose, with the protocol scaffold
///   already stripped.
/// - `refusal` — instead of the three above, when the model declines. Carries
///   no citations, ever.
/// - `memory` — the authoritative next-turn history. The client replaces its
///   copy wholesale from this and must commit the turn only when it arrives.
/// - `error` — generation failed. A closed socket and a finished turn must
///   never look alike.
/// - `done` — terminal.
#[route(POST)]
async fn chat(cx: &Cx, Json(request): Json<ChatRequest>) -> Result<Sse<ChatEventStream>> {
    let services = app_context::<AppServices>(cx).clone();

    let token = headers(cx)
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();

    let history: ConversationHistory = request.history.unwrap_or_default().into();
    let message = request.message;
    let collection = request.collection;

    let stream = async_stream::stream! {
        // Session and budget are checked before anything is retrieved or
        // generated, so a rejected caller costs nothing.
        let session = services.sessions.check(&token);
        if let Err(error) = session {
            yield Ok(error_event(&error));
            yield Ok(done_event());
            return;
        }

        let prepared = match services.conversations.prepare(&message, collection, &history).await {
            Ok(prepared) => prepared,
            Err(error) => {
                yield Ok(error_event(&error));
                yield Ok(done_event());
                return;
            }
        };

        let mut deltas = match services.conversations.stream(&prepared).await {
            Ok(deltas) => deltas,
            Err(error) => {
                yield Ok(error_event(&error));
                yield Ok(done_event());
                return;
            }
        };

        let mut assembler = ReplyAssembler::new();
        let mut citations_sent = false;
        let mut failed = None;

        while let Some(chunk) = deltas.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    failed = Some(error);
                    break;
                }
            };

            for event in assembler.push(&chunk) {
                for out in render(event, &prepared.citations, &mut citations_sent) {
                    yield Ok(out);
                }
            }
        }

        // The assembler classified every byte on the way past, so the reply it
        // returns is exactly what was streamed. Re-parsing the raw text here
        // instead is what used to fail turns the reader had already read in
        // full — and, because no `memory` event followed, silently desynchronise
        // the client's history.
        let (trailing, reply) = assembler.finish();
        for event in trailing {
            for out in render(event, &prepared.citations, &mut citations_sent) {
                yield Ok(out);
            }
        }

        let Some(reply) = reply else {
            // Phrased for the reader rather than for a log. The turn is lost
            // either way, but "internal error" tells them nothing and hides
            // the one useful fact: asking again usually works.
            let error = failed.unwrap_or_else(|| {
                AppError::Internal(
                    "the answer stopped before it was finished — please ask again".to_owned(),
                )
            });
            tracing::warn!(%error, "chat turn produced no usable reply");
            yield Ok(error_event(&error));
            yield Ok(done_event());
            return;
        };

        if let Some(error) = failed {
            tracing::warn!(%error, "chat stream ended early but a usable reply had already arrived");
        }

        let (history, compacted) = services
            .conversations
            .finish(history, prepared.question, &reply)
            .await;

        yield Ok(sse_event("memory", MemoryEvent { history: history.into(), compacted }));

        yield Ok(done_event());
    };

    // Boxed so the stream's type is concrete and 'static; an `impl Stream`
    // return would capture the request context's lifetime.
    let stream: ChatEventStream = Box::pin(stream);

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Builds one named SSE frame.
///
/// A payload that will not serialize degrades to `{}` under the same event
/// name rather than dropping the frame: the client's state machine is driven by
/// event order, so a missing frame would strand it waiting.
fn sse_event(name: &str, payload: impl serde::Serialize) -> Event {
    Event::new()
        .event(name)
        .json_data(&payload)
        .unwrap_or_else(|_| Event::new().event(name).data("{}"))
}

/// Maps an assembler event onto the wire, releasing citations at the moment the
/// turn is known to be an answer.
fn render(
    event: StreamEvent,
    citations: &[RetrievedHadith],
    citations_sent: &mut bool,
) -> Vec<Event> {
    match event {
        // The turn is an answer, which is the moment its citations become
        // safe to send. Nothing else goes on the wire for it: the frame the
        // client needs is the citations frame, and an empty marker beside it
        // would only be a frame to ignore.
        StreamEvent::Answered => {
            if *citations_sent {
                return Vec::new();
            }
            *citations_sent = true;

            vec![sse_event(
                "citations",
                serde_json::json!({ "citations": citations }),
            )]
        }
        StreamEvent::Delta(text) => {
            vec![sse_event("delta", serde_json::json!({ "text": text }))]
        }
        StreamEvent::Refused { reason, message } => vec![sse_event(
            "refusal",
            serde_json::json!({
                "reason": reason.as_str(),
                "message": message,
            }),
        )],
    }
}

fn error_event(error: &AppError) -> Event {
    if !matches!(
        error,
        AppError::Validation(_) | AppError::SessionExpired(_) | AppError::TooManyRequests(_)
    ) {
        tracing::error!(error = ?error, "chat turn failed");
    }

    sse_event(
        "error",
        ErrorEvent {
            code: error.code(),
            message: error.public_message(),
        },
    )
}

fn done_event() -> Event {
    Event::new().event("done").data("{}")
}
