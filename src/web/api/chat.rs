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
    parse_chat_reply,
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
        let mut raw = String::new();
        let mut failed = None;

        while let Some(chunk) = deltas.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    failed = Some(error);
                    break;
                }
            };
            raw.push_str(&chunk);

            for event in assembler.push(&chunk) {
                for out in render(event, &prepared.citations, &mut citations_sent) {
                    yield Ok(out);
                }
            }
        }

        for event in assembler.finish() {
            for out in render(event, &prepared.citations, &mut citations_sent) {
                yield Ok(out);
            }
        }

        let Some(reply) = parse_chat_reply(&raw) else {
            tracing::warn!(
                raw_len = raw.len(),
                raw_prefix = %raw.chars().take(80).collect::<String>().escape_debug(),
                "chat reply did not match either expected shape"
            );
            let error = failed.unwrap_or_else(|| {
                AppError::Internal("the answer ended before it was complete".to_owned())
            });
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

        yield Ok(Event::new()
            .event("memory")
            .json_data(&MemoryEvent { history: history.into(), compacted })
            .unwrap_or_else(|_| Event::new().event("memory").data("{}")));

        yield Ok(done_event());
    };

    // Boxed so the stream's type is concrete and 'static; an `impl Stream`
    // return would capture the request context's lifetime.
    let stream: ChatEventStream = Box::pin(stream);

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Maps an assembler event onto the wire, releasing citations at the moment the
/// turn is known to be an answer.
fn render(
    event: StreamEvent,
    citations: &[RetrievedHadith],
    citations_sent: &mut bool,
) -> Vec<Event> {
    match event {
        StreamEvent::Title(title) => {
            let mut events = vec![
                Event::new()
                    .event("title")
                    .json_data(&serde_json::json!({ "title": title }))
                    .unwrap_or_else(|_| Event::new().event("title").data("{}")),
            ];

            if !*citations_sent {
                *citations_sent = true;
                events.push(
                    Event::new()
                        .event("citations")
                        .json_data(&serde_json::json!({ "citations": citations }))
                        .unwrap_or_else(|_| Event::new().event("citations").data("{}")),
                );
            }

            events
        }
        StreamEvent::Delta(text) => vec![
            Event::new()
                .event("delta")
                .json_data(&serde_json::json!({ "text": text }))
                .unwrap_or_else(|_| Event::new().event("delta").data("{}")),
        ],
        StreamEvent::Refused { reason, message } => vec![
            Event::new()
                .event("refusal")
                .json_data(&serde_json::json!({
                    "reason": reason.as_str(),
                    "message": message,
                }))
                .unwrap_or_else(|_| Event::new().event("refusal").data("{}")),
        ],
    }
}

fn error_event(error: &AppError) -> Event {
    if !matches!(
        error,
        AppError::Validation(_) | AppError::SessionExpired(_) | AppError::TooManyRequests(_)
    ) {
        tracing::error!(error = ?error, "chat turn failed");
    }

    Event::new()
        .event("error")
        .json_data(&ErrorEvent {
            code: error.code(),
            message: error.public_message(),
        })
        .unwrap_or_else(|_| Event::new().event("error").data("{}"))
}

fn done_event() -> Event {
    Event::new().event("done").data("{}")
}
