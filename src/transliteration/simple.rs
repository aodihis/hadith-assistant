pub const SCHEME_NAME: &str = "simple-readable-v1";

pub fn transliterate(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut in_markup_tag = false;
    let mut last_segment = String::new();
    let mut trailing_vowel_len = None;

    for character in text.chars() {
        if in_markup_tag {
            output.push(character);
            if character == ']' {
                in_markup_tag = false;
            }
            continue;
        }

        if character == '[' {
            in_markup_tag = true;
            output.push(character);
            last_segment.clear();
            continue;
        }

        match transliterate_character(character) {
            CharacterOutput::Segment(segment) => {
                output.push_str(segment);
                last_segment.clear();
                last_segment.push_str(segment);
                trailing_vowel_len = None;
            }
            CharacterOutput::Vowel(vowel) => {
                output.push_str(vowel);
                trailing_vowel_len = Some(vowel.len());
            }
            CharacterOutput::RepeatPrevious => {
                if let Some(vowel_len) = trailing_vowel_len {
                    let insert_at = output.len() - vowel_len;
                    output.insert_str(insert_at, &last_segment);
                } else {
                    output.push_str(&last_segment);
                }
                trailing_vowel_len = None;
            }
            CharacterOutput::Skip => {}
            CharacterOutput::Original => {
                output.push(character);
                last_segment.clear();
                trailing_vowel_len = None;
            }
        }
    }

    output
}

enum CharacterOutput {
    Segment(&'static str),
    Vowel(&'static str),
    RepeatPrevious,
    Skip,
    Original,
}

fn transliterate_character(character: char) -> CharacterOutput {
    match character {
        'ء' | 'أ' | 'إ' | 'ؤ' | 'ئ' => CharacterOutput::Segment("'"),
        'آ' => CharacterOutput::Segment("'aa"),
        'ا' | 'ٱ' => CharacterOutput::Segment("a"),
        'ب' => CharacterOutput::Segment("b"),
        'ت' => CharacterOutput::Segment("t"),
        'ث' => CharacterOutput::Segment("th"),
        'ج' => CharacterOutput::Segment("j"),
        'ح' => CharacterOutput::Segment("h"),
        'خ' => CharacterOutput::Segment("kh"),
        'د' => CharacterOutput::Segment("d"),
        'ذ' => CharacterOutput::Segment("dh"),
        'ر' => CharacterOutput::Segment("r"),
        'ز' => CharacterOutput::Segment("z"),
        'س' => CharacterOutput::Segment("s"),
        'ش' => CharacterOutput::Segment("sh"),
        'ص' => CharacterOutput::Segment("s"),
        'ض' => CharacterOutput::Segment("d"),
        'ط' => CharacterOutput::Segment("t"),
        'ظ' => CharacterOutput::Segment("z"),
        'ع' => CharacterOutput::Segment("'"),
        'غ' => CharacterOutput::Segment("gh"),
        'ف' => CharacterOutput::Segment("f"),
        'ق' => CharacterOutput::Segment("q"),
        'ك' => CharacterOutput::Segment("k"),
        'ل' => CharacterOutput::Segment("l"),
        'م' => CharacterOutput::Segment("m"),
        'ن' => CharacterOutput::Segment("n"),
        'ه' => CharacterOutput::Segment("h"),
        'ة' => CharacterOutput::Segment("h"),
        'و' => CharacterOutput::Segment("w"),
        'ي' | 'ى' => CharacterOutput::Segment("y"),
        'ﻻ' => CharacterOutput::Segment("la"),
        'َ' => CharacterOutput::Vowel("a"),
        'ُ' => CharacterOutput::Vowel("u"),
        'ِ' => CharacterOutput::Vowel("i"),
        'ً' => CharacterOutput::Vowel("an"),
        'ٌ' => CharacterOutput::Vowel("un"),
        'ٍ' => CharacterOutput::Vowel("in"),
        'ّ' => CharacterOutput::RepeatPrevious,
        'ْ' | 'ـ' | '\u{0670}' => CharacterOutput::Skip,
        _ => CharacterOutput::Original,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transliterates_basic_arabic_letters() {
        assert_eq!(transliterate("بسم الله"), "bsm allh");
    }

    #[test]
    fn transliterates_diacritics_deterministically() {
        assert_eq!(transliterate("إِنَّمَا"), "'innamaa");
    }

    #[test]
    fn preserves_markup_tags_unchanged() {
        let text = "[narrator tooltip=\"عمر\"]عُمَرُ[/narrator]";

        assert_eq!(
            transliterate(text),
            "[narrator tooltip=\"عمر\"]'umaru[/narrator]"
        );
    }

    #[test]
    fn keeps_non_arabic_text_as_is() {
        assert_eq!(transliterate("Hadith 1: قال"), "Hadith 1: qal");
    }

    #[test]
    fn exposes_scheme_name() {
        assert_eq!(SCHEME_NAME, "simple-readable-v1");
    }
}
