use crate::application::Answer;
use crate::application::answer::parse_answer;

/// Why a turn produced no grounded answer.
///
/// Kept distinct from a failure: declining is a correct outcome, not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    /// The question is not about hadith or Islamic teachings.
    OffTopic,
    /// The question is in scope, but the retrieved narrations do not address it.
    NotCovered,
}

impl RefusalReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OffTopic => "off_topic",
            Self::NotCovered => "not_covered",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off_topic" => Some(Self::OffTopic),
            "not_covered" => Some(Self::NotCovered),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatReply {
    Answered(Answer),
    Refused {
        reason: RefusalReason,
        message: String,
    },
}

/// Parses a completed model response.
///
/// Delegates the answer shape to `parse_answer` so `/search` and `/api/answers`
/// keep their existing contract untouched; this only adds the refusal shape in
/// front of it.
pub fn parse_chat_reply(raw: &str) -> Option<ChatReply> {
    let (first_line, rest) = raw.split_once('\n').unwrap_or((raw, ""));

    if let Some(reason) = refusal_reason(first_line) {
        let message = rest.trim();
        if message.is_empty() {
            return None;
        }
        return Some(ChatReply::Refused {
            reason,
            message: message.to_owned(),
        });
    }

    parse_answer(raw).map(ChatReply::Answered)
}

fn refusal_reason(first_line: &str) -> Option<RefusalReason> {
    let rest = first_line
        .trim()
        .strip_prefix("Refusal:")
        .or_else(|| first_line.trim().strip_prefix("refusal:"))?;

    RefusalReason::parse(rest)
}

/// What the transport should emit next, decided purely from the text seen so
/// far. Ordering rules live here rather than in the SSE handler so they can be
/// tested without a socket.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// The turn is an answer. Citations are released alongside this, never
    /// before — see `ReplyAssembler`.
    Title(String),
    /// A chunk of answer prose, already stripped of the protocol scaffold.
    Delta(String),
    /// The turn is a refusal. Carries no citations, by design.
    Refused {
        reason: RefusalReason,
        message: String,
    },
}

/// Turns raw model deltas into ordered events.
///
/// The model's first line declares the turn: `Title: …` for an answer,
/// `Refusal: …` for a decline. Everything is buffered until that line is
/// complete, which does three jobs at once:
///
/// 1. Citations are withheld until the turn is known to be an answer, so a
///    refusal never flashes hadith cards on screen and then retracts them.
/// 2. The `Title:` / `Refusal:` scaffold never reaches the reader — forwarding
///    raw deltas would print it verbatim.
/// 3. A refusal message is delivered whole rather than typed out.
///
/// The first line is short, so this costs a fraction of a second, not the whole
/// generation.
#[derive(Debug, Default)]
pub struct ReplyAssembler {
    buffer: String,
    kind: Option<Kind>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Answer,
    Refusal(RefusalReason),
}

impl ReplyAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one chunk of model output, returning the events it completes.
    pub fn push(&mut self, chunk: &str) -> Vec<StreamEvent> {
        if let Some(Kind::Answer) = self.kind {
            // Past the header: prose flows straight through.
            return if chunk.is_empty() {
                Vec::new()
            } else {
                vec![StreamEvent::Delta(chunk.to_owned())]
            };
        }

        if matches!(self.kind, Some(Kind::Refusal(_))) {
            // A refusal body is held until finish() so it arrives as one piece.
            self.buffer.push_str(chunk);
            return Vec::new();
        }

        self.buffer.push_str(chunk);

        let Some(newline) = self.buffer.find('\n') else {
            return Vec::new();
        };

        let first_line = self.buffer[..newline].to_owned();
        let rest = self.buffer[newline + 1..].to_owned();

        if let Some(reason) = refusal_reason(&first_line) {
            self.kind = Some(Kind::Refusal(reason));
            self.buffer = rest;
            return Vec::new();
        }

        let Some(title) = parse_title(&first_line) else {
            // Not a shape we recognise. Treat everything as prose rather than
            // dropping the turn: the caller still validates the finished text.
            self.kind = Some(Kind::Answer);
            self.buffer = String::new();
            let mut events = vec![StreamEvent::Title(String::new())];
            if !rest.is_empty() {
                events.push(StreamEvent::Delta(rest));
            }
            return events;
        };

        self.kind = Some(Kind::Answer);
        self.buffer = String::new();

        let mut events = vec![StreamEvent::Title(title)];
        let trimmed = rest.trim_start_matches(['\n', '\r']);
        if !trimmed.is_empty() {
            events.push(StreamEvent::Delta(trimmed.to_owned()));
        }
        events
    }

    /// Ends the stream, flushing anything still held back.
    pub fn finish(mut self) -> Vec<StreamEvent> {
        match self.kind {
            Some(Kind::Refusal(reason)) => {
                let message = self.buffer.trim().to_owned();
                if message.is_empty() {
                    Vec::new()
                } else {
                    vec![StreamEvent::Refused { reason, message }]
                }
            }
            Some(Kind::Answer) => Vec::new(),
            // The stream ended before the first line completed — there was
            // never a usable turn.
            None => {
                self.buffer.clear();
                Vec::new()
            }
        }
    }
}

fn parse_title(first_line: &str) -> Option<String> {
    let rest = first_line
        .trim_start()
        .strip_prefix("Title:")
        .or_else(|| first_line.trim_start().strip_prefix("title:"))?;

    let title = rest.trim();
    if title.is_empty() {
        return None;
    }

    Some(title.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(chunks: &[&str]) -> Vec<StreamEvent> {
        let mut assembler = ReplyAssembler::new();
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(assembler.push(chunk));
        }
        events.extend(assembler.finish());
        events
    }

    #[test]
    fn parse_chat_reply_reads_the_answer_shape() {
        let reply = parse_chat_reply("Title: Kindness\nBe gentle with others.")
            .expect("a well-formed answer should parse");

        assert_eq!(
            reply,
            ChatReply::Answered(Answer {
                title: "Kindness".to_owned(),
                answer: "Be gentle with others.".to_owned(),
            })
        );
    }

    #[test]
    fn parse_chat_reply_reads_both_refusal_reasons() {
        let off_topic = parse_chat_reply("Refusal: off_topic\nI can only help with hadith.")
            .expect("off_topic should parse");
        assert_eq!(
            off_topic,
            ChatReply::Refused {
                reason: RefusalReason::OffTopic,
                message: "I can only help with hadith.".to_owned(),
            }
        );

        let not_covered =
            parse_chat_reply("Refusal: not_covered\nThese narrations do not address that.")
                .expect("not_covered should parse");
        assert!(matches!(
            not_covered,
            ChatReply::Refused {
                reason: RefusalReason::NotCovered,
                ..
            }
        ));
    }

    #[test]
    fn parse_chat_reply_rejects_an_unknown_refusal_reason_rather_than_guessing() {
        // Falls through to the answer parser, which also rejects it — an
        // invented reason must never be silently treated as off_topic.
        assert_eq!(parse_chat_reply("Refusal: because\nSome text."), None);
    }

    #[test]
    fn parse_chat_reply_rejects_an_empty_refusal_message() {
        assert_eq!(parse_chat_reply("Refusal: off_topic\n   \n"), None);
    }

    #[test]
    fn streaming_emits_the_title_before_any_prose() {
        let events = feed(&["Title: Sincerity\nActions are ", "judged by intentions."]);

        assert_eq!(
            events,
            vec![
                StreamEvent::Title("Sincerity".to_owned()),
                StreamEvent::Delta("Actions are ".to_owned()),
                StreamEvent::Delta("judged by intentions.".to_owned()),
            ]
        );
    }

    #[test]
    fn streaming_never_leaks_the_protocol_scaffold_into_the_prose() {
        // The header arrives split across chunks, which is the normal case.
        let events = feed(&["Tit", "le: Mer", "cy\nBe merci", "ful."]);

        assert_eq!(
            events,
            vec![
                StreamEvent::Title("Mercy".to_owned()),
                StreamEvent::Delta("Be merci".to_owned()),
                StreamEvent::Delta("ful.".to_owned()),
            ]
        );
        assert!(
            !events.iter().any(|event| matches!(
                event,
                StreamEvent::Delta(text) if text.contains("Title:")
            )),
            "the reader must never see the Title: marker"
        );
    }

    #[test]
    fn a_refusal_emits_one_event_and_no_prose_deltas() {
        let events = feed(&[
            "Refusal: off_topic\nI can only help ",
            "with questions about hadith.",
        ]);

        assert_eq!(
            events,
            vec![StreamEvent::Refused {
                reason: RefusalReason::OffTopic,
                message: "I can only help with questions about hadith.".to_owned(),
            }],
            "a refusal must not stream as deltas, so no citations are ever released for it"
        );
    }

    #[test]
    fn a_stream_that_dies_before_the_first_line_yields_nothing() {
        // Nothing is emitted, so the caller cannot mistake a truncated turn for
        // an answer and must surface it as a failure.
        assert_eq!(feed(&["Title: Sinceri"]), Vec::new());
    }

    #[test]
    fn unrecognised_output_still_streams_as_prose_rather_than_vanishing() {
        let events = feed(&["some unexpected shape\nwith a body"]);

        assert_eq!(
            events,
            vec![
                StreamEvent::Title(String::new()),
                StreamEvent::Delta("with a body".to_owned()),
            ]
        );
    }
}
