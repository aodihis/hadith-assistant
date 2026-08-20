//! Recognising an explicit hadith reference in a question.
//!
//! "Explain Sahih al-Bukhari 1" names a specific record. Answering it by
//! similarity search would be a guess at something the reader already told us
//! exactly, and similarity is quite capable of returning a different narration
//! that merely reads alike. A reference is resolved by lookup instead.

/// A hadith named directly in a question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HadithReference {
    /// The collection slug, as stored.
    pub collection: String,
    /// The hadith's number within that collection.
    pub hadith_number: String,
}

/// Spellings a reader might reasonably type for each collection.
///
/// Written out rather than derived from the stored titles because readers type
/// what they say — "abu dawud", not "Sunan Abi Dawud" — and transliteration
/// varies. Longer spellings are matched ahead of shorter ones, which matters
/// for the pairs that contain each other: "hisn al-muslim" must not be read as
/// "muslim", and "sunan ibn majah" must not be read as "majah" alone.
const ALIASES: &[(&str, &[&str])] = &[
    (
        "bukhari",
        &[
            "sahih al-bukhari",
            "sahih al bukhari",
            "sahih bukhari",
            "al-bukhari",
            "bukhari",
            "bukhaari",
            "bukhary",
        ],
    ),
    ("muslim", &["sahih muslim", "muslim"]),
    (
        "nasai",
        &[
            "sunan an-nasa'i",
            "an-nasa'i",
            "an-nasai",
            "nasa'i",
            "nasai",
        ],
    ),
    (
        "abudawud",
        &[
            "sunan abi dawud",
            "abu dawood",
            "abu dawud",
            "abi dawud",
            "abudawud",
        ],
    ),
    (
        "tirmidhi",
        &[
            "jami` at-tirmidhi",
            "jami at-tirmidhi",
            "at-tirmidhi",
            "tirmidhi",
            "tirmidzi",
        ],
    ),
    (
        "ibnmajah",
        &["sunan ibn majah", "ibn majah", "ibnmajah", "ibn maajah"],
    ),
    ("ahmad", &["musnad ahmad", "musnad", "ahmad"]),
    (
        "riyadussalihin",
        &[
            "riyad as-salihin",
            "riyadh as-salihin",
            "riyadus salihin",
            "riyadussalihin",
        ],
    ),
    (
        "adab",
        &["al-adab al-mufrad", "adab al-mufrad", "al-adab", "adab"],
    ),
    (
        "shamail",
        &[
            "ash-shama'il al-muhammadiyah",
            "ash-shama'il",
            "shama'il",
            "shamail",
        ],
    ),
    ("bulugh", &["bulugh al-maram", "bulugh"]),
    ("mishkat", &["mishkat al-masabih", "mishkat"]),
    ("hisn", &["hisn al-muslim", "hisn"]),
    (
        "forty",
        &[
            "40 hadith an-nawawi",
            "40 hadith nawawi",
            "forty hadith",
            "an-nawawi",
            "nawawi",
            "arbain",
        ],
    ),
    ("virtues", &["virtues of the qur'an", "virtues"]),
];

/// Words that may sit between a collection and its number.
///
/// "of", "in" and "from" are here for the reversed form — "hadith 3 of
/// Bukhari" — where the preposition is what joins the number to the work.
const FILLER: &[&str] = &[
    "no", "no.", "number", "hadith", "#", ":", "-", "nomor", "n°", "of", "in", "from",
];

/// Reads an explicit reference out of `question`, if there is one.
///
/// Returns `None` when no collection is named, or when a collection is named
/// without a number — "what does Bukhari say about charity" is a topic
/// question that belongs in similarity search, not a lookup.
pub fn parse_reference(question: &str) -> Option<HadithReference> {
    let haystack = question.to_lowercase();
    let (slug, start, end) = find_collection(&haystack)?;
    let hadith_number = find_number(&haystack, start, end)?;

    Some(HadithReference {
        collection: slug.to_owned(),
        hadith_number,
    })
}

/// Reads a reference from a retrieval query, preferring the most recent line.
///
/// The query carries the previous question ahead of the current one so topical
/// follow-ups still retrieve well, which makes a whole-text scan wrong twice
/// over: it would resolve "explain Bukhari 3" against a Muslim reference left
/// on the earlier line, and it would pick up a reference from a turn already
/// answered.
///
/// Scanning backwards fixes both, and keeps the behaviour that makes a bare
/// "is it sahih?" resolve against the hadith just asked about.
pub fn parse_reference_in_query(text: &str) -> Option<HadithReference> {
    text.rsplit('\n').find_map(parse_reference)
}

/// Finds the earliest collection mentioned, preferring the longest spelling at
/// that position.
fn find_collection(haystack: &str) -> Option<(&'static str, usize, usize)> {
    let mut best: Option<(&'static str, usize, usize)> = None;

    for (slug, aliases) in ALIASES {
        for alias in *aliases {
            let Some(at) = find_word(haystack, alias) else {
                continue;
            };
            let candidate = (*slug, at, at + alias.len());
            best = match best {
                // Earlier wins; at the same position the longer spelling wins,
                // so "hisn al-muslim" is not read as "muslim".
                Some((_, best_at, best_end))
                    if best_at < at || (best_at == at && best_end >= candidate.2) =>
                {
                    best
                }
                _ => Some(candidate),
            };
        }
    }

    best
}

/// Finds `needle` in `haystack` only where it stands as a whole word.
///
/// Without this, "ahmad" matches inside a longer name and "adab" inside
/// "adabun", turning an unrelated word into a collection.
fn find_word(haystack: &str, needle: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(offset) = haystack[from..].find(needle) {
        let at = from + offset;
        let before_ok = at == 0
            || !haystack[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric());
        let after = at + needle.len();
        let after_ok = after >= haystack.len()
            || !haystack[after..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric());

        if before_ok && after_ok {
            return Some(at);
        }
        // `needle` is ASCII here, so stepping one byte stays on a boundary.
        from = at + 1;
    }
    None
}

/// Finds the hadith number belonging to a collection named at `start..end`.
///
/// Looks after the collection first, since that is how references are written,
/// and falls back to a number before it for "hadith 3 of Bukhari".
fn find_number(haystack: &str, start: usize, end: usize) -> Option<String> {
    number_in(&haystack[end..], true).or_else(|| number_in(&haystack[..start], false))
}

/// Reads the first number in `text`, provided only filler separates it.
///
/// The filler rule is what stops "Bukhari on charity, and also 3 others" from
/// being read as a reference: prose between the collection and the number means
/// the number is about something else.
fn number_in(text: &str, forward: bool) -> Option<String> {
    let mut tokens: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|token| !token.is_empty())
        .collect();

    if !forward {
        tokens.reverse();
    }

    for token in tokens {
        let cleaned = token.trim_matches(|c: char| !c.is_alphanumeric());
        if cleaned.is_empty() {
            // Pure punctuation, such as the "#" or ":" in "Bukhari #3".
            continue;
        }
        if cleaned.chars().all(|c| c.is_ascii_digit()) {
            // Numbers run to five digits in the largest collections; anything
            // longer is a year or an identifier, not a hadith number.
            return (cleaned.len() <= 5).then(|| cleaned.to_owned());
        }
        if !FILLER.contains(&cleaned) {
            return None;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(question: &str) -> Option<(String, String)> {
        parse_reference(question).map(|r| (r.collection, r.hadith_number))
    }

    #[test]
    fn reads_a_bare_collection_and_number() {
        assert_eq!(
            parsed("explain bukhari 3"),
            Some(("bukhari".to_owned(), "3".to_owned()))
        );
    }

    #[test]
    fn reads_the_published_title() {
        assert_eq!(
            parsed("What does Sahih al-Bukhari 1 mean?"),
            Some(("bukhari".to_owned(), "1".to_owned()))
        );
    }

    #[test]
    fn reads_the_filler_readers_actually_type() {
        for question in [
            "bukhari no. 3",
            "bukhari no 3",
            "bukhari #3",
            "bukhari hadith 3",
            "bukhari: 3",
            "Bukhari number 3",
        ] {
            assert_eq!(
                parsed(question),
                Some(("bukhari".to_owned(), "3".to_owned())),
                "failed on {question:?}"
            );
        }
    }

    #[test]
    fn reads_a_number_written_before_the_collection() {
        assert_eq!(
            parsed("what is hadith 3 of muslim about"),
            Some(("muslim".to_owned(), "3".to_owned()))
        );
    }

    /// The collection whose name contains another collection's name. Read
    /// shortest-first this would resolve to Sahih Muslim, a different work.
    #[test]
    fn prefers_the_longer_title_when_one_contains_another() {
        assert_eq!(
            parsed("hisn al-muslim 5"),
            Some(("hisn".to_owned(), "5".to_owned()))
        );
    }

    #[test]
    fn reads_multi_word_collections() {
        assert_eq!(
            parsed("is ibn majah 4 authentic"),
            Some(("ibnmajah".to_owned(), "4".to_owned()))
        );
        assert_eq!(
            parsed("abu dawud 100"),
            Some(("abudawud".to_owned(), "100".to_owned()))
        );
    }

    #[test]
    fn a_collection_without_a_number_is_a_topic_question() {
        assert_eq!(parsed("what does bukhari say about charity"), None);
    }

    #[test]
    fn no_collection_named_is_not_a_reference() {
        assert_eq!(parsed("what do the narrations say about mercy"), None);
    }

    /// Prose between the collection and the number means the number is about
    /// something else entirely.
    #[test]
    fn a_distant_number_is_not_read_as_a_reference() {
        assert_eq!(
            parsed("does bukhari mention charity given to 3 people"),
            None
        );
    }

    #[test]
    fn a_collection_name_inside_a_longer_word_is_not_a_reference() {
        assert_eq!(parsed("adabun hasanun 3"), None);
    }

    #[test]
    fn an_implausibly_long_number_is_rejected() {
        assert_eq!(parsed("bukhari 1234567"), None);
    }

    /// The retrieval query is the previous question followed by the current
    /// one. A whole-text scan finds the earliest collection, which here is the
    /// wrong work entirely.
    #[test]
    fn the_current_line_wins_over_a_reference_in_the_previous_question() {
        let query = "what does muslim say about fasting\nexplain bukhari 3";

        assert_eq!(
            parse_reference_in_query(query).map(|r| (r.collection, r.hadith_number)),
            Some(("bukhari".to_owned(), "3".to_owned()))
        );
    }

    /// A follow-up carries no reference of its own, so the one being discussed
    /// still applies.
    #[test]
    fn a_follow_up_resolves_against_the_previous_reference() {
        let query = "explain bukhari 3\nis it sahih?";

        assert_eq!(
            parse_reference_in_query(query).map(|r| (r.collection, r.hadith_number)),
            Some(("bukhari".to_owned(), "3".to_owned()))
        );
    }

    #[test]
    fn the_grading_question_is_still_a_reference() {
        assert_eq!(
            parsed("is Sahih Muslim 1907 sahih?"),
            Some(("muslim".to_owned(), "1907".to_owned()))
        );
    }
}
