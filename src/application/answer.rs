use std::sync::Arc;
use std::sync::LazyLock;

use regex::Regex;

use crate::domain::RetrievedHadith;
use crate::infrastructure::completion::{ChatCompleter, CompletionOptions};

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub title: String,
    pub answer: String,
}

pub struct AnswerService {
    completer: Arc<dyn ChatCompleter>,
    has_api_key: bool,
    options: CompletionOptions,
}

impl AnswerService {
    pub fn new(
        completer: Arc<dyn ChatCompleter>,
        has_api_key: bool,
        options: CompletionOptions,
    ) -> Self {
        Self {
            completer,
            has_api_key,
            options,
        }
    }

    pub async fn generate(&self, query: &str, hadiths: &[RetrievedHadith]) -> Option<Answer> {
        if !self.has_api_key || hadiths.is_empty() {
            return None;
        }

        let system_prompt = build_system_prompt();
        let user_prompt = build_user_prompt(query, hadiths);

        let raw = match self
            .completer
            .complete(&system_prompt, &user_prompt, self.options)
            .await
        {
            Ok(raw) => raw,
            Err(error) => {
                tracing::warn!(%error, "answer generation request failed");
                return None;
            }
        };

        match parse_answer(&raw) {
            Some(answer) => Some(answer),
            None => {
                tracing::warn!("answer generation returned unparseable output");
                None
            }
        }
    }
}

/// Prompts live in `prompts/*.md` so they can be read and edited as prose
/// rather than hunted for inside string literals. `include_str!` keeps them
/// compiled into the binary, so deployment stays a single artifact — editing a
/// prompt does require a rebuild.
const ANSWER_SYSTEM_PROMPT: &str = include_str!("../../prompts/answer_system.md");

fn build_system_prompt() -> String {
    ANSWER_SYSTEM_PROMPT.trim().to_owned()
}

fn build_user_prompt(query: &str, hadiths: &[RetrievedHadith]) -> String {
    let mut prompt = format!("Question: {query}\n\n");

    for (index, hadith) in hadiths.iter().enumerate() {
        prompt.push_str(&format!(
            "{}. ({}, book {}, hadith {})\n",
            index + 1,
            hadith.collection,
            hadith.book_number,
            hadith.hadith_number
        ));
        if let Some(english_text) = &hadith.english_text {
            prompt.push_str(&format!("English: {english_text}\n"));
        }
        prompt.push_str(&format!("Arabic: {}\n\n", hadith.arabic_text));
    }

    prompt
}

static TITLE_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*Title:\s*(.+)$").expect("valid regex"));

/// Parses the single-shot `/api/answers` reply shape.
///
/// The streaming chat path deliberately does NOT share this. It classifies the
/// header as the bytes arrive, because it has to decide whether to release
/// citations before the reply is complete — see `chat::ReplyAssembler`. Two
/// parsers over one format is a hazard the chat path was already bitten by, so
/// if `/api/answers` ever grows the refusal shape, route it through the
/// assembler rather than teaching this a second contract.
pub(crate) fn parse_answer(raw: &str) -> Option<Answer> {
    let (first_line, rest) = raw.split_once('\n').unwrap_or((raw, ""));

    let title = TITLE_LINE
        .captures(first_line)?
        .get(1)?
        .as_str()
        .trim()
        .to_owned();
    if title.is_empty() {
        return None;
    }

    let answer = rest.trim().to_owned();
    if answer.is_empty() {
        return None;
    }

    Some(Answer { title, answer })
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::error::AppError;
    use crate::infrastructure::completion::ChatMessage;

    fn test_options() -> CompletionOptions {
        CompletionOptions::new(0.3, 400)
    }

    #[test]
    fn parse_answer_splits_title_and_body() {
        let raw =
            "Title: Sincerity & Intention\nActions are judged by intentions.\nSecond paragraph.";

        let answer = parse_answer(raw).expect("well-formed output should parse");

        assert_eq!(answer.title, "Sincerity & Intention");
        assert_eq!(
            answer.answer,
            "Actions are judged by intentions.\nSecond paragraph."
        );
    }

    #[test]
    fn parse_answer_is_case_insensitive_on_the_title_label() {
        let raw = "title: Kindness\nBe kind to others.";

        let answer = parse_answer(raw).expect("lowercase title label should still parse");

        assert_eq!(answer.title, "Kindness");
    }

    #[test]
    fn parse_answer_rejects_missing_title_line() {
        assert_eq!(parse_answer("Just some text with no title line."), None);
    }

    #[test]
    fn parse_answer_rejects_empty_body() {
        assert_eq!(parse_answer("Title: Something\n   \n"), None);
    }

    struct FakeCompleter {
        response: Result<&'static str, ()>,
    }

    #[async_trait]
    impl ChatCompleter for FakeCompleter {
        async fn complete_messages(
            &self,
            _messages: &[ChatMessage],
            _options: CompletionOptions,
        ) -> Result<String, AppError> {
            match self.response {
                Ok(text) => Ok(text.to_owned()),
                Err(()) => Err(AppError::Internal("simulated failure".to_owned())),
            }
        }
    }

    struct PanicsIfCalledCompleter;

    #[async_trait]
    impl ChatCompleter for PanicsIfCalledCompleter {
        async fn complete_messages(
            &self,
            _messages: &[ChatMessage],
            _options: CompletionOptions,
        ) -> Result<String, AppError> {
            panic!("completer should not be called");
        }
    }

    fn sample_hadith() -> RetrievedHadith {
        RetrievedHadith {
            hadith_id: 1,
            collection: "bukhari".to_owned(),
            book_number: "1".to_owned(),
            hadith_number: "1".to_owned(),
            arabic_text: "إنما الأعمال بالنيات".to_owned(),
            english_text: Some("Actions are but by intentions.".to_owned()),
            arabic_grade: "صحيح".to_owned(),
            english_grade: "Sahih".to_owned(),
            narrator: Some(crate::domain::NarratorRef {
                name: "Umar ibn al-Khattab".to_owned(),
                role: "sahabi".to_owned(),
            }),
            score: Some(0.9),
        }
    }

    #[tokio::test]
    async fn generate_returns_none_for_empty_hadiths_without_calling_the_completer() {
        let service = AnswerService::new(Arc::new(PanicsIfCalledCompleter), true, test_options());

        let result = service.generate("What is intention?", &[]).await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn generate_returns_none_without_an_api_key_without_calling_the_completer() {
        let service = AnswerService::new(Arc::new(PanicsIfCalledCompleter), false, test_options());

        let result = service
            .generate("What is intention?", &[sample_hadith()])
            .await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn generate_returns_none_when_the_completer_errors() {
        let service = AnswerService::new(
            Arc::new(FakeCompleter { response: Err(()) }),
            true,
            test_options(),
        );

        let result = service
            .generate("What is intention?", &[sample_hadith()])
            .await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn generate_returns_none_when_the_completer_output_is_unparseable() {
        let service = AnswerService::new(
            Arc::new(FakeCompleter {
                response: Ok("not the expected shape"),
            }),
            true,
            test_options(),
        );

        let result = service
            .generate("What is intention?", &[sample_hadith()])
            .await;

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn generate_returns_the_parsed_answer_on_success() {
        let service = AnswerService::new(
            Arc::new(FakeCompleter {
                response: Ok("Title: Sincerity\nActions are judged by intentions."),
            }),
            true,
            test_options(),
        );

        let result = service
            .generate("What is intention?", &[sample_hadith()])
            .await;

        assert_eq!(
            result,
            Some(Answer {
                title: "Sincerity".to_owned(),
                answer: "Actions are judged by intentions.".to_owned(),
            })
        );
    }
}
