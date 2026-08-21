use crate::domain::RetrievedHadith;
use crate::error::AppError;
use crate::infrastructure::completion::ChatMessage;
use crate::text::{grade_text, to_plain_text};

pub const CHAT_SYSTEM_PROMPT: &str = include_str!("../../prompts/chat_system.md");
pub const COMPACTION_SYSTEM_PROMPT: &str = include_str!("../../prompts/compaction_system.md");

/// One completed question-and-answer pair, as the client replays it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationTurn {
    pub question: String,
    pub answer: String,
    /// Refused turns are replayed too, so the model does not loop when a user
    /// rephrases the same out-of-scope question.
    pub refused: bool,
}

/// The conversation state the client holds and replays each turn.
///
/// This is *not* the visible transcript. The transcript only ever grows;
/// this shrinks as older turns are folded into `summary`, which is what keeps
/// the prompt bounded. Nothing here is stored server-side.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConversationHistory {
    pub summary: Option<String>,
    pub summarized_turns: usize,
    pub turns: Vec<ConversationTurn>,
}

/// Limits applied to client-supplied history before it reaches a paid provider.
#[derive(Debug, Clone, Copy)]
pub struct HistoryLimits {
    pub max_question_chars: usize,
    pub max_answer_chars: usize,
    pub max_summary_chars: usize,
    pub max_turns: usize,
    /// Compaction fires once the history exceeds this many turns.
    pub compact_after_turns: usize,
    /// How many turns survive compaction verbatim.
    pub keep_turns: usize,
    pub max_history_chars: usize,
}

impl ConversationHistory {
    /// Rejects history that is malformed or oversized.
    ///
    /// History arrives from the client and drives paid provider calls, so every
    /// field is bounded here rather than trusted.
    pub fn validate(&self, limits: &HistoryLimits) -> Result<(), AppError> {
        if self.turns.len() > limits.max_turns {
            return Err(AppError::Validation(format!(
                "conversation history may not exceed {} turns",
                limits.max_turns
            )));
        }

        if let Some(summary) = &self.summary
            && summary.chars().count() > limits.max_summary_chars
        {
            return Err(AppError::Validation(
                "conversation summary is too long".to_owned(),
            ));
        }

        for turn in &self.turns {
            if turn.question.chars().count() > limits.max_question_chars {
                return Err(AppError::Validation(
                    "a question in the conversation history is too long".to_owned(),
                ));
            }
            if turn.answer.chars().count() > limits.max_answer_chars {
                return Err(AppError::Validation(
                    "an answer in the conversation history is too long".to_owned(),
                ));
            }
        }

        Ok(())
    }

    fn chars(&self) -> usize {
        self.turns
            .iter()
            .map(|turn| turn.question.chars().count() + turn.answer.chars().count())
            .sum()
    }
}

/// Whether the history has outgrown its budget and should be compacted.
///
/// Evaluated after the new turn is appended, so the check reflects what the
/// *next* request would have to carry.
pub fn needs_compaction(history: &ConversationHistory, limits: &HistoryLimits) -> bool {
    history.turns.len() > limits.compact_after_turns || history.chars() > limits.max_history_chars
}

/// Splits history into the turns to fold into the summary and the turns to keep
/// verbatim. Keeping roughly half is what stops compaction firing every turn.
pub fn split_for_compaction(
    history: &ConversationHistory,
    limits: &HistoryLimits,
) -> (Vec<ConversationTurn>, Vec<ConversationTurn>) {
    let keep = limits.keep_turns.min(history.turns.len());
    let fold_count = history.turns.len() - keep;

    let fold = history.turns[..fold_count].to_vec();
    let kept = history.turns[fold_count..].to_vec();

    (fold, kept)
}

/// Builds the standalone text used for retrieval.
///
/// A follow-up like "does that apply to travellers?" is unsearchable alone, so
/// the previous question is prepended to give the embedding something to anchor
/// on. This is deliberately deterministic rather than an LLM rewrite, which
/// would add a third paid call to every turn. It over-retrieves when the user
/// changes topic; the prompt shows the model what was retrieved so it can
/// ignore irrelevant hits. Isolated here so it can be swapped for a rewrite
/// later without touching the rest of the turn.
pub fn compose_retrieval_query(
    history: &ConversationHistory,
    message: &str,
    max_chars: usize,
) -> String {
    let Some(previous) = history
        .turns
        .iter()
        .rev()
        .find(|turn| !turn.refused)
        .map(|turn| turn.question.as_str())
    else {
        return message.trim().to_owned();
    };

    // "Summarize that" carries no topic of its own. Appending it to the
    // anchor moves the query away from the point the previous turn retrieved
    // at, so a different set of narrations comes back and the model — grounded
    // in whatever it is handed — writes a fresh answer to the same question
    // rather than going deeper on the one it already gave. Searching on the
    // anchor alone reproduces the previous turn's evidence exactly, which is
    // what "go further on this" actually asks for.
    let composed = if is_meta_request(message) {
        previous.trim().to_owned()
    } else {
        format!("{}\n{}", previous.trim(), message.trim())
    };

    if composed.chars().count() <= max_chars {
        return composed;
    }

    composed.chars().take(max_chars).collect()
}

/// Stems of the words that ask for a reply *about the previous reply*.
///
/// Matched as prefixes so ordinary misspellings ("sumarize") and inflections
/// ("summarised", "clarification") still land — a typo should not silently
/// change which narrations the turn is answered from.
const META_CUE_STEMS: &[&str] = &[
    "summar", "sumar", "explain", "explanat", "elaborat", "clarif", "expand", "recap", "simplif",
    "rephras", "restate", "detail", "brief", "concise", "condens", "overview", "gist", "tldr",
    "mean", "short",
];

/// Words that may sit alongside a cue without giving the message a topic:
/// pronouns and back-references, politeness and auxiliaries, and the generic
/// nouns this domain uses for "the thing we were just discussing".
#[rustfmt::skip]
const META_FILLER: &[&str] = &[
    "a", "about", "again", "all", "an", "and", "answer", "are", "as", "at", "based", "be", "bit",
    "both", "but", "by", "can", "could", "do", "does", "down", "earlier", "for", "from", "further",
    "give", "hadith", "hadiths", "hadeeth", "have", "how", "i", "in", "into", "is", "it", "its",
    "just", "last", "let", "little", "make", "me", "more", "my", "narration", "narrations", "of",
    "on", "one", "only", "or", "part", "please", "point", "points", "previous", "reply", "report",
    "reports", "said", "say", "some", "text", "texts", "than", "that", "the", "them", "these",
    "they", "this", "those", "to", "tell", "up", "us", "was", "we", "were", "what", "with",
    "would", "you", "your",
];

/// How long a message may be and still be read as a bare "go further" request.
const MAX_META_REQUEST_WORDS: usize = 20;

/// Whether a message asks for more on what was just answered rather than
/// raising something new.
///
/// Deliberately conservative: a message qualifies only when it carries a cue
/// *and* every other word in it is filler. "Explain the hadith about fasting"
/// names a topic and so is treated as a fresh question, even though it opens
/// with a cue — misreading a real question as a follow-up would answer it from
/// the wrong narrations, which is the more damaging mistake of the two.
pub fn is_meta_request(message: &str) -> bool {
    let words: Vec<String> = message
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| word.to_lowercase())
        .collect();

    if words.is_empty() || words.len() > MAX_META_REQUEST_WORDS {
        return false;
    }

    let is_cue = |word: &str| META_CUE_STEMS.iter().any(|stem| word.starts_with(stem));

    words.iter().any(|word| is_cue(word))
        && words
            .iter()
            .all(|word| is_cue(word) || META_FILLER.contains(&word.as_str()))
}

/// Renders retrieved narrations for the prompt.
///
/// Grades are included verbatim and labelled, because the system prompt forbids
/// the model from inventing or re-characterizing them — it can only do that if
/// it is told what they are.
pub fn render_narrations(hadiths: &[RetrievedHadith]) -> String {
    let mut block = String::new();

    for (index, hadith) in hadiths.iter().enumerate() {
        // A record with no score was looked up because the question named it,
        // rather than matched because it reads similarly. That distinction
        // decides whether the narration is the subject of the answer or
        // support for it, so the model is told which it is holding.
        let named = if hadith.score.is_none() {
            "  [the narration the question names]"
        } else {
            ""
        };

        // The leading number is what the model cites with, so it is the one
        // identifier it needs. The reference is given for grounding, but the
        // model is forbidden from writing it: the interface renders the
        // citation from the record, so it cannot be misquoted.
        block.push_str(&format!(
            "{}. {} {} (book {}){}\n",
            index + 1,
            hadith.collection_name,
            hadith.hadith_number,
            hadith.book_number,
            named
        ));

        // Normalised for the same reason the text is: a grade stored as a JSON
        // array of gradings would otherwise reach the model as braces, and the
        // prompt requires it to report the grade exactly as given.
        let grade = grade_text(&hadith.english_grade);
        if !grade.is_empty() {
            block.push_str(&format!("Grade: {grade}\n"));
        }
        if let Some(narrator) = &hadith.narrator {
            block.push_str(&format!("Narrated by: {}\n", narrator.name));
        }
        // Stripped before the model sees it: source markup wastes prompt tokens
        // and invites the model to echo tags back into an answer.
        if let Some(english_text) = &hadith.english_text {
            block.push_str(&format!("English: {}\n", to_plain_text(english_text)));
        }
        block.push_str(&format!(
            "Arabic: {}\n\n",
            to_plain_text(&hadith.arabic_text)
        ));
    }

    block
}

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

    /// Used only when the model declared a refusal but wrote no message.
    /// Delivering a bare decline beats failing a turn the model completed.
    fn default_message(self) -> &'static str {
        match self {
            Self::OffTopic => {
                "I can only help with questions about hadith and Islamic teachings. \
                 Please ask me one of those."
            }
            Self::NotCovered => {
                "I could not find a narration in the indexed collections that speaks to \
                 that question. Try rephrasing or narrowing it."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatReply {
    /// The answer prose, as the reader saw it.
    Answered(String),
    Refused {
        reason: RefusalReason,
        message: String,
    },
}

/// A recognised `Refusal:` header line.
struct RefusalHeader {
    reason: RefusalReason,
    /// Text that followed the label but was not a reason token, so it belongs
    /// to the message rather than being discarded.
    trailing: String,
}

fn refusal_header(first_line: &str) -> Option<RefusalHeader> {
    let rest = strip_label(first_line, "Refusal")?;

    match RefusalReason::parse(rest) {
        Some(reason) => Some(RefusalHeader {
            reason,
            trailing: String::new(),
        }),
        // The label is there but the token is not one of ours — usually the
        // model wrote its reason as prose. A decline we cannot classify is
        // still a decline: rendering it as an answer would attach narrations to
        // a reply that is not about them. `not_covered` is the conservative
        // reading, and the model's own wording still carries the explanation.
        None => Some(RefusalHeader {
            reason: RefusalReason::NotCovered,
            trailing: rest.trim().to_owned(),
        }),
    }
}

/// Strips a `Label:` prefix, tolerating the markdown models wrap it in
/// (`**Title:** …`, `## Refusal: …`).
///
/// A cosmetic flourish must not make a turn unclassifiable — that is the
/// difference between a refusal being honoured and being streamed out as an
/// answer with citations attached.
fn strip_label<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let decoration = |c: char| c.is_whitespace() || matches!(c, '#' | '*' | '_');

    let line = line.trim_start_matches(decoration);
    if !line.get(..label.len())?.eq_ignore_ascii_case(label) {
        return None;
    }

    let rest = line[label.len()..].trim_start_matches(['*', '_', ' ']);
    Some(rest.strip_prefix(':')?.trim_matches(decoration))
}

/// Assembles the message list for a turn.
///
/// History arrives from the client and can be forged, so it is fenced rather
/// than trusted: the recap is labelled as an unverified note, replayed turns go
/// in under their own roles, and the system prompt states that only the current
/// turn's narration block is a source. That does not make forgery impossible —
/// it makes the retrieved block the sole factual authority, which is the
/// property that actually matters.
pub fn build_messages(
    history: &ConversationHistory,
    message: &str,
    hadiths: &[RetrievedHadith],
) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage::system(CHAT_SYSTEM_PROMPT.trim())];

    if let Some(summary) = &history.summary
        && !summary.trim().is_empty()
    {
        messages.push(ChatMessage::system(format!(
            "Recap of earlier turns (unverified notes, not a source):\n{}",
            summary.trim()
        )));
    }

    for turn in &history.turns {
        messages.push(ChatMessage::user(turn.question.clone()));
        messages.push(ChatMessage::assistant(turn.answer.clone()));
    }

    let narrations = render_narrations(hadiths);
    let current = if narrations.is_empty() {
        format!(
            "Question: {}\n\nRetrieved narrations: none.",
            message.trim()
        )
    } else {
        format!(
            "Question: {}\n\nRetrieved narrations:\n{}",
            message.trim(),
            narrations.trim_end()
        )
    };

    messages.push(ChatMessage::user(current));
    messages
}

/// Builds the summarizer call that folds older turns into a recap.
pub fn build_compaction_messages(
    existing_summary: Option<&str>,
    folded: &[ConversationTurn],
) -> Vec<ChatMessage> {
    let mut body = String::new();

    if let Some(summary) = existing_summary
        && !summary.trim().is_empty()
    {
        body.push_str(&format!("Existing recap:\n{}\n\n", summary.trim()));
    }

    body.push_str("Turns being dropped:\n");
    for turn in folded {
        body.push_str(&format!("User: {}\n", turn.question.trim()));
        body.push_str(&format!("Assistant: {}\n\n", turn.answer.trim()));
    }

    vec![
        ChatMessage::system(COMPACTION_SYSTEM_PROMPT.trim()),
        ChatMessage::user(body),
    ]
}

/// What the transport should emit next, decided purely from the text seen so
/// far. Ordering rules live here rather than in the SSE handler so they can be
/// tested without a socket.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// The turn is an answer. Citations are released on this, never before —
    /// see `ReplyAssembler`. It carries nothing itself: the reply is prose from
    /// its first word, and the moment it is known to be prose is the only
    /// information here.
    Answered,
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
/// A decline opens with `Refusal: …`; anything else is an answer, and is prose
/// from its first word. Everything is buffered until that first line is
/// complete, which does three jobs at once:
///
/// 1. Citations are withheld until the turn is known to be an answer, so a
///    refusal never flashes hadith cards on screen and then retracts them.
/// 2. The `Refusal:` scaffold never reaches the reader — forwarding raw deltas
///    would print it verbatim.
/// 3. A refusal message is delivered whole rather than typed out.
///
/// The first line is short, so this costs a fraction of a second, not the whole
/// generation.
///
/// `finish` also returns the turn as it will be remembered. That is deliberate:
/// the reply is derived from the same pass that produced the events, so what
/// the reader saw and what the history records cannot disagree. Classifying the
/// raw text a second time is what previously let a turn stream out in full and
/// then fail — the lenient streaming path accepted output that the strict
/// re-parse rejected.
#[derive(Debug, Default)]
pub struct ReplyAssembler {
    buffer: String,
    kind: Option<Kind>,
    body: String,
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
        match self.kind {
            // Past the header: prose flows straight through, and is retained so
            // the finished answer is the text the reader actually saw.
            Some(Kind::Answer) => {
                if chunk.is_empty() {
                    return Vec::new();
                }
                self.body.push_str(chunk);
                vec![StreamEvent::Delta(chunk.to_owned())]
            }
            // A refusal body is held until finish() so it arrives as one piece.
            Some(Kind::Refusal(_)) => {
                self.buffer.push_str(chunk);
                Vec::new()
            }
            None => {
                self.buffer.push_str(chunk);
                match self.buffer.find('\n') {
                    Some(newline) => {
                        let first_line = self.buffer[..newline].to_owned();
                        let rest = self.buffer[newline + 1..].to_owned();
                        self.classify(&first_line, &rest)
                    }
                    None => Vec::new(),
                }
            }
        }
    }

    /// Resolves the header line into a turn kind and emits whatever that makes
    /// deliverable.
    fn classify(&mut self, first_line: &str, rest: &str) -> Vec<StreamEvent> {
        if let Some(header) = refusal_header(first_line) {
            self.kind = Some(Kind::Refusal(header.reason));
            self.buffer = if header.trailing.is_empty() {
                rest.to_owned()
            } else {
                format!("{}\n{rest}", header.trailing)
            };
            return Vec::new();
        }

        self.kind = Some(Kind::Answer);
        self.buffer = String::new();

        let prose = if is_stray_title(first_line) {
            // Answers no longer carry a title, but a model that has seen a
            // million of them will occasionally write one anyway. Dropping the
            // line is better than printing "Title: …" at the top of the reply;
            // nothing is lost, because whatever it says the prose repeats.
            rest.trim_start_matches(['\n', '\r']).to_owned()
        } else if rest.is_empty() {
            // The ordinary case: the reply is prose from its first word, so the
            // first line is answer text and keeping it is what stops the answer
            // being silently truncated.
            first_line.to_owned()
        } else {
            format!("{first_line}\n{rest}")
        };

        let mut events = vec![StreamEvent::Answered];
        if !prose.is_empty() {
            self.body.push_str(&prose);
            events.push(StreamEvent::Delta(prose));
        }
        events
    }

    /// Ends the stream, flushing anything still held back and returning the
    /// turn as it will be remembered.
    ///
    /// `None` means no usable turn arrived, and is the only case the caller
    /// should surface as an error.
    pub fn finish(mut self) -> (Vec<StreamEvent>, Option<ChatReply>) {
        let mut events = Vec::new();

        if self.kind.is_none() {
            // The stream ended before any newline arrived. A single-line reply
            // is complete rather than truncated, so classify it as if one had.
            let buffered = std::mem::take(&mut self.buffer);
            if buffered.trim().is_empty() {
                tracing::warn!("the model returned an empty reply");
                return (events, None);
            }
            events = self.classify(&buffered, "");
        }

        match self.kind {
            Some(Kind::Refusal(reason)) => {
                let trimmed = self.buffer.trim();
                let message = if trimmed.is_empty() {
                    reason.default_message().to_owned()
                } else {
                    trimmed.to_owned()
                };
                events.push(StreamEvent::Refused {
                    reason,
                    message: message.clone(),
                });
                (events, Some(ChatReply::Refused { reason, message }))
            }
            // A header with no prose behind it is a truncated turn, not an
            // answer. Any events classify() just produced are dropped with it,
            // so citations are never released for a turn that then fails.
            Some(Kind::Answer) if self.body.trim().is_empty() => {
                // Logged with what did arrive: the failure is otherwise
                // indistinguishable from the provider returning nothing at
                // all, and the two want different fixes.
                tracing::warn!(
                    buffered = self.buffer.trim().len(),
                    "the model produced a header with no prose behind it"
                );
                (Vec::new(), None)
            }
            Some(Kind::Answer) => {
                let answer = self.body.trim().to_owned();
                (events, Some(ChatReply::Answered(answer)))
            }
            None => (events, None),
        }
    }
}

/// Whether a first line is a leftover `Title:` header rather than answer prose.
fn is_stray_title(first_line: &str) -> bool {
    strip_label(first_line, "Title").is_some_and(|title| !title.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `score` is what separates a similarity match from a record looked up
    /// because the question named it, so it is the interesting parameter here.
    fn retrieved(
        collection_name: &str,
        hadith_number: &str,
        score: Option<f64>,
    ) -> RetrievedHadith {
        RetrievedHadith {
            hadith_id: 1,
            collection: "bukhari".to_owned(),
            collection_name: collection_name.to_owned(),
            book_number: "1".to_owned(),
            hadith_number: hadith_number.to_owned(),
            arabic_text: "نص".to_owned(),
            english_text: Some("Actions are but by intentions.".to_owned()),
            arabic_grade: "صحيح".to_owned(),
            english_grade: "Sahih".to_owned(),
            narrator: None,
            score,
        }
    }

    /// The leading number is what the model cites with, and the prompt forbids
    /// it from writing the reference itself, so both have to be present and the
    /// numbering has to start at one.
    #[test]
    fn narrations_are_numbered_from_one_and_carry_the_published_title() {
        let block = render_narrations(&[
            retrieved("Sahih al-Bukhari", "1", Some(0.4)),
            retrieved("Sahih Muslim", "1907", Some(0.3)),
        ]);

        assert!(block.contains("1. Sahih al-Bukhari 1 (book 1)"), "{block}");
        assert!(block.contains("2. Sahih Muslim 1907 (book 1)"), "{block}");
        // Both carry a score here, so neither is the named subject.
        assert!(
            !block.contains("the narration the question names"),
            "{block}"
        );
        // The slug is an internal identifier and would only invite the model to
        // echo it back into an answer.
        assert!(!block.contains("bukhari book"), "{block}");
    }

    /// A record with no score was looked up because the question named it. The
    /// prompt turns on that marker to decide whether the narration is the
    /// subject of the answer or support for it, so it has to be rendered.
    #[test]
    fn a_looked_up_narration_is_marked_as_the_one_the_question_names() {
        let block = render_narrations(&[
            retrieved("Sahih al-Bukhari", "3", None),
            retrieved("Sahih Muslim", "1907", Some(0.31)),
        ]);

        assert!(
            block.contains("1. Sahih al-Bukhari 3 (book 1)  [the narration the question names]"),
            "{block}"
        );
        assert_eq!(
            block.matches("the narration the question names").count(),
            1,
            "only the looked-up record is the subject:\n{block}"
        );
    }

    fn limits() -> HistoryLimits {
        HistoryLimits {
            max_question_chars: 1_000,
            max_answer_chars: 4_000,
            max_summary_chars: 4_000,
            max_turns: 12,
            compact_after_turns: 8,
            keep_turns: 4,
            max_history_chars: 6_000,
        }
    }

    fn turn(question: &str) -> ConversationTurn {
        ConversationTurn {
            question: question.to_owned(),
            answer: "Title: T\nBody.".to_owned(),
            refused: false,
        }
    }

    fn history_of(count: usize) -> ConversationHistory {
        ConversationHistory {
            summary: None,
            summarized_turns: 0,
            turns: (0..count).map(|i| turn(&format!("q{i}"))).collect(),
        }
    }

    #[test]
    fn history_over_the_turn_cap_is_rejected() {
        let history = history_of(13);

        assert!(matches!(
            history.validate(&limits()),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn an_oversized_question_or_answer_in_history_is_rejected() {
        let mut history = history_of(1);
        history.turns[0].question = "x".repeat(1_001);
        assert!(matches!(
            history.validate(&limits()),
            Err(AppError::Validation(_))
        ));

        let mut history = history_of(1);
        history.turns[0].answer = "x".repeat(4_001);
        assert!(matches!(
            history.validate(&limits()),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn compaction_does_not_fire_on_consecutive_turns() {
        let limits = limits();

        // Below the threshold nothing happens, so no extra provider call.
        assert!(!needs_compaction(&history_of(8), &limits));

        // At the threshold it fires and drops back to keep_turns, which is what
        // buys several quiet turns before the next one.
        let over = history_of(9);
        assert!(needs_compaction(&over, &limits));

        let (folded, kept) = split_for_compaction(&over, &limits);
        assert_eq!(folded.len(), 5);
        assert_eq!(kept.len(), 4);

        let after = ConversationHistory {
            summary: Some("recap".to_owned()),
            summarized_turns: folded.len(),
            turns: kept,
        };
        assert!(
            !needs_compaction(&after, &limits),
            "compaction must leave the history under the threshold, or it fires every turn"
        );
    }

    #[test]
    fn a_long_history_triggers_compaction_on_size_even_under_the_turn_cap() {
        let mut history = history_of(3);
        history.turns[0].answer = "x".repeat(7_000);

        assert!(needs_compaction(&history, &limits()));
    }

    #[test]
    fn the_first_question_is_used_for_retrieval_unchanged() {
        let query =
            compose_retrieval_query(&ConversationHistory::default(), "  What is niyyah? ", 400);

        assert_eq!(query, "What is niyyah?");
    }

    #[test]
    fn a_follow_up_is_anchored_on_the_previous_question() {
        // "Does that apply to travellers?" retrieves nothing useful alone.
        let history = history_of(1);
        let query = compose_retrieval_query(&history, "Does that apply to travellers?", 400);

        assert_eq!(query, "q0\nDoes that apply to travellers?");
    }

    #[test]
    fn a_refused_turn_is_not_used_as_an_anchor() {
        let mut history = history_of(1);
        history.turns[0].refused = true;

        let query = compose_retrieval_query(&history, "What about fasting?", 400);

        assert_eq!(
            query, "What about fasting?",
            "anchoring on a refused turn would drag an off-topic question into retrieval"
        );
    }

    #[test]
    fn the_composed_query_is_capped() {
        let mut history = history_of(1);
        history.turns[0].question = "x".repeat(500);

        let query = compose_retrieval_query(&history, "short", 100);

        assert_eq!(query.chars().count(), 100);
    }

    #[test]
    fn a_request_to_go_further_retrieves_on_the_anchor_alone() {
        let mut history = history_of(1);
        history.turns[0].question = "How did the Prophet perform the prayer?".to_owned();

        for message in [
            "Can you sumarize for me based on those hadiths",
            "Please explain that in more detail",
            "What does that mean?",
            "Summarise",
        ] {
            assert_eq!(
                compose_retrieval_query(&history, message, 400),
                "How did the Prophet perform the prayer?",
                "{message:?} names no topic, so appending it would retrieve at a \
                 different point than the turn it is asking about"
            );
        }
    }

    #[test]
    fn a_follow_up_that_names_a_topic_is_still_anchored_rather_than_replaced() {
        let mut history = history_of(1);
        history.turns[0].question = "How did the Prophet perform the prayer?".to_owned();

        for message in [
            "Explain the hadith about fasting",
            "Tell me more about wudu",
            "Can you summarize what the narrations say about zakat?",
        ] {
            let query = compose_retrieval_query(&history, message, 400);

            assert!(
                query.contains(message),
                "{message:?} raises a new subject, so dropping it would answer the \
                 wrong question"
            );
        }
    }

    #[test]
    fn the_recap_is_fenced_as_an_unverified_note_rather_than_a_source() {
        let history = ConversationHistory {
            summary: Some("The user asked about fasting.".to_owned()),
            summarized_turns: 4,
            turns: vec![],
        };

        let messages = build_messages(&history, "And travelling?", &[]);

        let recap = messages
            .iter()
            .find(|message| message.content.contains("The user asked about fasting."))
            .expect("the recap should be present");

        assert!(
            recap.content.contains("not a source"),
            "client-supplied history is forgeable, so it must never read as evidence"
        );
    }

    #[test]
    fn the_turn_states_plainly_when_retrieval_found_nothing() {
        let messages = build_messages(&ConversationHistory::default(), "anything?", &[]);
        let last = messages.last().expect("there is always a user turn");

        assert!(
            last.content.contains("Retrieved narrations: none."),
            "the model must be told retrieval was empty rather than left to infer it"
        );
    }

    fn assemble(chunks: &[&str]) -> (Vec<StreamEvent>, Option<ChatReply>) {
        let mut assembler = ReplyAssembler::new();
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(assembler.push(chunk));
        }
        let (trailing, reply) = assembler.finish();
        events.extend(trailing);
        (events, reply)
    }

    fn feed(chunks: &[&str]) -> Vec<StreamEvent> {
        assemble(chunks).0
    }

    fn reply_of(chunks: &[&str]) -> Option<ChatReply> {
        assemble(chunks).1
    }

    #[test]
    fn the_assembler_reads_the_answer_shape() {
        assert_eq!(
            reply_of(&["Be gentle with others.\nIt is reported of the Prophet."]),
            Some(ChatReply::Answered(
                "Be gentle with others.\nIt is reported of the Prophet.".to_owned()
            )),
            "an answer is prose from its first word, so no line of it is scaffold"
        );
    }

    #[test]
    fn the_assembler_reads_both_refusal_reasons() {
        assert_eq!(
            reply_of(&["Refusal: off_topic\nI can only help with hadith."]),
            Some(ChatReply::Refused {
                reason: RefusalReason::OffTopic,
                message: "I can only help with hadith.".to_owned(),
            })
        );

        assert!(matches!(
            reply_of(&["Refusal: not_covered\nThese narrations do not address that."]),
            Some(ChatReply::Refused {
                reason: RefusalReason::NotCovered,
                ..
            })
        ));
    }

    #[test]
    fn every_streamed_turn_also_yields_the_reply_that_was_streamed() {
        // The bug this guards: the streaming path was lenient and the final
        // classification was strict, so output the reader saw in full could
        // still fail the turn — and never reach the client's history.
        for raw in [
            "Title: Sincerity\nActions are judged by intentions.",
            "Refusal: off_topic\nAsk me about hadith instead.",
            "no header at all, just prose\nspanning two lines",
            "**Title:** Mercy\nBe merciful.",
            "Refusal: the question is about the weather\nI only cover hadith.",
        ] {
            let (events, reply) = assemble(&[raw]);
            assert!(
                reply.is_some(),
                "streamed {} event(s) but produced no reply for {raw:?}",
                events.len()
            );
        }
    }

    #[test]
    fn an_unclassifiable_refusal_reason_stays_a_refusal() {
        // It must not fall through to the answer path: that would attach
        // narrations to a reply which is not about them.
        let (events, reply) = assemble(&["Refusal: because I cannot\nSome text."]);

        assert!(matches!(
            reply,
            Some(ChatReply::Refused {
                reason: RefusalReason::NotCovered,
                ..
            })
        ));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, StreamEvent::Answered)),
            "a refusal must never release citations"
        );
    }

    #[test]
    fn a_refusal_with_no_message_still_delivers_one() {
        // The model declared the decline; failing the turn over a missing body
        // would show an error instead of an answer it had already given.
        assert_eq!(
            reply_of(&["Refusal: off_topic"]),
            Some(ChatReply::Refused {
                reason: RefusalReason::OffTopic,
                message: RefusalReason::OffTopic.default_message().to_owned(),
            })
        );
    }

    #[test]
    fn a_title_the_model_wrote_out_of_habit_is_dropped_rather_than_printed() {
        for raw in [
            "Title: Mercy\nBe merciful to others.",
            "**Title:** Mercy\nBe merciful to others.",
            "## Title: Mercy\nBe merciful to others.",
        ] {
            assert_eq!(
                reply_of(&[raw]),
                Some(ChatReply::Answered("Be merciful to others.".to_owned())),
                "answers carry no title, so a stray one is scaffold rather than prose"
            );
        }
    }

    #[test]
    fn streaming_announces_the_answer_before_any_prose() {
        let events = feed(&["Actions are judged\nby ", "intentions."]);

        assert_eq!(
            events,
            vec![
                StreamEvent::Answered,
                StreamEvent::Delta("Actions are judged\nby ".to_owned()),
                StreamEvent::Delta("intentions.".to_owned()),
            ],
            "citations ride on the first event, so it must precede every delta"
        );
    }

    #[test]
    fn streaming_never_leaks_the_protocol_scaffold_into_the_prose() {
        // A stray header arrives split across chunks, which is the normal case.
        let events = feed(&["Tit", "le: Mer", "cy\nBe merci", "ful."]);

        assert_eq!(
            events,
            vec![
                StreamEvent::Answered,
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
    fn unrecognised_output_streams_as_prose_without_losing_its_first_line() {
        let events = feed(&["some unexpected shape\nwith a body"]);

        assert_eq!(
            events,
            vec![
                StreamEvent::Answered,
                StreamEvent::Delta("some unexpected shape\nwith a body".to_owned()),
            ]
        );
    }

    #[test]
    fn a_single_line_answer_that_never_sent_a_newline_is_still_delivered() {
        assert_eq!(
            reply_of(&["Actions are judged by intentions."]),
            Some(ChatReply::Answered(
                "Actions are judged by intentions.".to_owned()
            ))
        );
    }
}
