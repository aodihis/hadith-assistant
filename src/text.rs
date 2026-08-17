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
/// that the markup separated do not run together.
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
            other => out.push(other),
        }
    }

    collapse_whitespace(&out)
}

/// Collapses runs of spaces and blank lines without joining separate paragraphs.
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
}
