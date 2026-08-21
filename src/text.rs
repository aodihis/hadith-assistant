//! Plain-text rendering of source markup.
//!
//! Roughly half the imported records carry HTML in their English text — `<p>`
//! paragraph markers and inline `<span>`s from the source dump. That markup is
//! part of the canonical record and is never rewritten in place; it is stripped
//! only where the text becomes *derived* data: embedding input, prompt input,
//! and display.
//!
//! Leaving it in would be actively harmful in two ways. Embedding `<p>` tags
//! spends tokens on markup and pollutes the vector with noise shared by every
//! record. Sending it to the model invites it to echo tags back into an answer.

/// Strips HTML tags and decodes the handful of entities the source uses,
/// collapsing the result into clean prose.
///
/// Block-level tags become paragraph breaks rather than vanishing, so sentences
/// that the markup separated do not run together. Newlines already present in
/// the source are *not* breaks — they are the dump's hard wrapping, and across
/// the corpus they fall mid-sentence far more often than at a sentence end — so
/// they collapse to spaces like any other whitespace.
pub fn to_plain_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            '<' => {
                let mut tag = String::new();
                for inner in chars.by_ref() {
                    if inner == '>' {
                        break;
                    }
                    tag.push(inner);
                }

                // A block element ended a line in the source; preserve that
                // break so paragraphs stay separated.
                let name = tag
                    .trim_start_matches('/')
                    .split(|c: char| c.is_whitespace())
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if matches!(
                    name.as_str(),
                    "p" | "br" | "div" | "li" | "tr" | "h1" | "h2" | "h3"
                ) {
                    out.push('\n');
                }
            }
            '&' => {
                let mut entity = String::new();
                while let Some(&next) = chars.peek() {
                    if next == ';' {
                        chars.next();
                        break;
                    }
                    if entity.len() > 8 || next.is_whitespace() {
                        break;
                    }
                    entity.push(next);
                    chars.next();
                }

                out.push_str(match entity.to_ascii_lowercase().as_str() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" | "#39" => "'",
                    "nbsp" => " ",
                    _ => "",
                });
            }
            // A newline in the source is a soft wrap, not a paragraph break:
            // the dump hard-wraps its prose, so most of these land
            // mid-sentence. Only the block tags above mark a real break.
            '\n' | '\r' => out.push(' '),
            other => out.push(other),
        }
    }

    collapse_whitespace(&out)
}

/// Collapses runs of spaces and blank lines without joining separate paragraphs.
///
/// By this point the only newlines left are the ones [`to_plain_text`] inserted
/// for block tags, so splitting on them recovers exactly the source paragraphs.
fn collapse_whitespace(text: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();

    for line in text.split('\n') {
        let cleaned = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !cleaned.is_empty() {
            paragraphs.push(cleaned);
        }
    }

    paragraphs.join("\n")
}

/// One scholar's grading, as the source stores it when a record carries more
/// than a bare label.
#[derive(serde::Deserialize)]
struct SourceGrade {
    grade: String,
    graded_by: Option<String>,
    /// The source's own ordering weight, highest first. Absent in some records,
    /// which then sort last rather than failing to parse.
    #[serde(default)]
    priority: i64,
}

/// Renders a stored grade the way a reader can use it.
///
/// Most records hold a bare label — "Sahih". Around 4,700 instead hold a JSON
/// array of gradings, one per scholar, which reaches the page as a wall of
/// braces if it is passed through untouched.
///
/// Anything that does not parse as that array comes back as it was. Forty-odd
/// records hold prose that merely opens with a bracket, and prose is already
/// readable — guessing at it would lose more than it fixed.
pub fn grade_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if !trimmed.starts_with('[') {
        return trimmed.to_owned();
    }

    let Ok(mut gradings) = serde_json::from_str::<Vec<SourceGrade>>(trimmed) else {
        return trimmed.to_owned();
    };

    // Scholars disagree, and a record carrying two gradings is carrying both on
    // purpose. Showing only the first would present one scholar's reading as
    // the grade, so all of them are kept, in the source's own order of weight.
    gradings.sort_by_key(|grading| std::cmp::Reverse(grading.priority));

    gradings
        .iter()
        .filter_map(|grading| {
            let grade = grading.grade.trim();
            if grade.is_empty() {
                return None;
            }

            match grading
                .graded_by
                .as_deref()
                .map(str::trim)
                .filter(|by| !by.is_empty())
            {
                Some(by) => Some(format!("{grade} — {by}")),
                None => Some(grade.to_owned()),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passes_through_unchanged() {
        assert_eq!(
            to_plain_text("Actions are but by intentions."),
            "Actions are but by intentions."
        );
    }

    #[test]
    fn paragraph_tags_become_line_breaks_rather_than_running_sentences_together() {
        // This is the shape the dump actually uses: <p> as a separator with
        // heavy indentation around it.
        let raw = "<p>\n\n     Narrated Ibn 'Umar:\n<p>\n\n     Islam is based on five principles:";

        assert_eq!(
            to_plain_text(raw),
            "Narrated Ibn 'Umar:\nIslam is based on five principles:"
        );
    }

    #[test]
    fn hard_wrapped_lines_rejoin_instead_of_becoming_separate_paragraphs() {
        // The dump wraps prose at a fixed width and indents the continuation.
        // Splitting on those newlines cut sentences in half mid-clause.
        let raw = "Narrated Ibn `Abbas:\n<p>\n\n     One night I slept at the house of\n     Maimuna and the Prophet was there. He\n     performed ablution.";

        assert_eq!(
            to_plain_text(raw),
            "Narrated Ibn `Abbas:\nOne night I slept at the house of Maimuna and the Prophet was there. He performed ablution."
        );
    }

    #[test]
    fn a_blank_line_is_not_treated_as_a_paragraph_break_either() {
        // Blank lines split roughly evenly between sentence ends and
        // mid-sentence across the corpus, so they are no more trustworthy than
        // a single newline. Only markup decides where a paragraph ends.
        assert_eq!(to_plain_text("he said\n\n     to them"), "he said to them");
    }

    #[test]
    fn inline_tags_are_removed_without_leaving_a_gap() {
        assert_eq!(
            to_plain_text("the <span class=\"x\">Prophet</span> said"),
            "the Prophet said"
        );
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(
            to_plain_text("Allah&apos;s Apostle &amp; his companions"),
            "Allah's Apostle & his companions"
        );
    }

    #[test]
    fn an_unknown_entity_is_dropped_rather_than_shown_raw() {
        assert_eq!(to_plain_text("a &zzz; b"), "a b");
    }

    #[test]
    fn an_unterminated_tag_does_not_swallow_the_rest_forever() {
        assert_eq!(to_plain_text("text <span"), "text");
    }

    #[test]
    fn arabic_text_with_markup_is_cleaned_too() {
        assert_eq!(
            to_plain_text("<p>إِنَّمَا الأَعْمَالُ بِالنِّيَّاتِ</p>"),
            "إِنَّمَا الأَعْمَالُ بِالنِّيَّاتِ"
        );
    }

    #[test]
    fn a_bare_grade_label_is_left_alone() {
        assert_eq!(grade_text("  Sahih  "), "Sahih");
        assert_eq!(grade_text(""), "");
    }

    #[test]
    fn a_structured_grade_is_rendered_as_a_grade_and_its_scholar() {
        let raw = r#"[{"graded_by": "Al-Albani", "grade": "Da`if (Weak)", "priority": 40}]"#;

        assert_eq!(grade_text(raw), "Da`if (Weak) — Al-Albani");
    }

    #[test]
    fn every_scholars_grading_is_kept_in_the_sources_order_of_weight() {
        // Deliberately supplied lowest-priority first: scholars disagree, and
        // showing only the first would present one reading as the grade.
        let raw = r#"[
            {"graded_by": "Darussalam", "grade": "Hasan", "priority": 40},
            {"graded_by": "Al-Albani", "grade": "Sahih", "priority": 50}
        ]"#;

        assert_eq!(grade_text(raw), "Sahih — Al-Albani; Hasan — Darussalam");
    }

    #[test]
    fn prose_that_merely_opens_with_a_bracket_survives_untouched() {
        let raw = "[Hasan lighairihi; this isnad is da'eef}";

        assert_eq!(grade_text(raw), raw);
    }

    #[test]
    fn a_grading_with_no_scholar_named_is_rendered_as_the_grade_alone() {
        let raw = r#"[{"grade": "Sahih", "priority": 50}]"#;

        assert_eq!(grade_text(raw), "Sahih");
    }

    #[test]
    fn an_array_holding_no_usable_grade_renders_as_nothing() {
        // So the caller's "hide it when blank" check catches it, rather than
        // the page showing a label with an empty list after it.
        let raw = r#"[{"graded_by": "Al-Albani", "grade": "  ", "priority": 50}]"#;

        assert_eq!(grade_text(raw), "");
    }
}
