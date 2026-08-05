use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedNarrator {
    pub external_id: Option<i64>,
    pub role: String,
    pub name: String,
    pub position: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedText {
    pub clean_arabic_text: String,
    pub narrators: Vec<ParsedNarrator>,
}

static NARRATOR_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)\[narrator\s+([^\]]*)\].*?\[/narrator\]"#).expect("valid regex")
});
static ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(\w+)="([^"]*)""#).expect("valid regex"));
static MATN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)\[matn\](.*?)\[/matn\]"#).expect("valid regex"));
static ANY_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\[/?[A-Za-z]+(?:\s[^\]]*)?\]"#).expect("valid regex"));
static NARRATED_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*Narrated ([^:]{1,120}):"#).expect("valid regex"));

pub fn parse_isnad(raw_arabic_text: &str, english_text: Option<&str>) -> ParsedText {
    let mut narrators = extract_arabic_narrators(raw_arabic_text);
    if narrators.is_empty() {
        narrators = extract_english_fallback(english_text);
    }

    ParsedText {
        clean_arabic_text: extract_clean_text(raw_arabic_text),
        narrators,
    }
}

fn extract_arabic_narrators(raw: &str) -> Vec<ParsedNarrator> {
    let mut narrators = Vec::new();

    for capture in NARRATOR_TAG.captures_iter(raw) {
        let attrs_blob = &capture[1];

        let mut external_id = None;
        let mut role = None;
        let mut name = None;
        for attr in ATTR.captures_iter(attrs_blob) {
            match &attr[1] {
                "id" => external_id = attr[2].parse::<i64>().ok(),
                "role" => role = Some(attr[2].to_owned()),
                "tooltip" => name = Some(attr[2].trim().to_owned()),
                _ => {}
            }
        }

        let (Some(role), Some(name)) = (role, name) else {
            tracing::warn!(attrs = attrs_blob, "skipping malformed narrator tag");
            continue;
        };
        if name.is_empty() {
            continue;
        }

        narrators.push(ParsedNarrator {
            external_id,
            role,
            name,
            position: narrators.len() as i32,
        });
    }

    narrators
}

fn extract_english_fallback(english_text: Option<&str>) -> Vec<ParsedNarrator> {
    let Some(text) = english_text else {
        return Vec::new();
    };
    let Some(capture) = NARRATED_PREFIX.captures(text) else {
        return Vec::new();
    };

    let name = capture[1].trim().to_owned();
    if name.is_empty() {
        return Vec::new();
    }

    vec![ParsedNarrator {
        external_id: None,
        role: "english_fallback".to_owned(),
        name,
        position: 0,
    }]
}

fn extract_clean_text(raw: &str) -> String {
    let base = MATN
        .captures(raw)
        .map(|capture| capture[1].to_owned())
        .unwrap_or_else(|| raw.to_owned());

    let stripped = ANY_TAG.replace_all(&base, "");
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub async fn insert_narrators(
    conn: &mut sqlx::PgConnection,
    hadith_id: i64,
    narrators: &[ParsedNarrator],
) -> Result<(), sqlx::Error> {
    for narrator in narrators {
        sqlx::query(
            r#"
            INSERT INTO narrators (hadith_id, external_id, role, name, "position")
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(hadith_id)
        .bind(narrator.external_id)
        .bind(&narrator.role)
        .bind(&narrator.name)
        .bind(narrator.position)
        .execute(&mut *conn)
        .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_with_no_markup_passes_through_unchanged() {
        let result = parse_isnad("إِنَّمَا الأَعْمَالُ بِالنِّيَّاتِ", None);

        assert_eq!(result.clean_arabic_text, "إِنَّمَا الأَعْمَالُ بِالنِّيَّاتِ");
        assert!(result.narrators.is_empty());
    }

    #[test]
    fn prematn_and_matn_extracts_narrators_and_clean_matn_text() {
        let raw = r#"[prematn]حَدَّثَنَا [narrator id="4698" role="first" tooltip="الحميدي عبد الله بن الزبير"]الْحُمَيْدِيُّ[/narrator]، قَالَ حَدَّثَنَا [narrator id="3443" role="sahabi" tooltip="سفيان"]سُفْيَانُ[/narrator][/prematn][matn]إِنَّمَا الأَعْمَالُ بِالنِّيَّاتِ[/matn]"#;

        let result = parse_isnad(raw, None);

        assert_eq!(result.clean_arabic_text, "إِنَّمَا الأَعْمَالُ بِالنِّيَّاتِ");
        assert_eq!(
            result.narrators,
            vec![
                ParsedNarrator {
                    external_id: Some(4698),
                    role: "first".to_owned(),
                    name: "الحميدي عبد الله بن الزبير".to_owned(),
                    position: 0,
                },
                ParsedNarrator {
                    external_id: Some(3443),
                    role: "sahabi".to_owned(),
                    name: "سفيان".to_owned(),
                    position: 1,
                },
            ]
        );
    }

    #[test]
    fn matn_only_without_prematn_wrapper_still_extracts_clean_text() {
        let raw = "[matn]نص الحديث[/matn]";

        let result = parse_isnad(raw, None);

        assert_eq!(result.clean_arabic_text, "نص الحديث");
        assert!(result.narrators.is_empty());
    }

    #[test]
    fn postmatn_narrators_are_captured_but_postmatn_text_is_dropped() {
        let raw = r#"[matn]نص الحديث[/matn][postmatn]قَالَ [narrator id="1" role="chain" tooltip="فلان"]فُلَانٌ[/narrator][/postmatn]"#;

        let result = parse_isnad(raw, None);

        assert_eq!(result.clean_arabic_text, "نص الحديث");
        assert_eq!(result.narrators.len(), 1);
        assert_eq!(result.narrators[0].name, "فلان");
    }

    #[test]
    fn falls_back_to_english_narrated_prefix_when_no_arabic_tags_present() {
        let result = parse_isnad(
            "إِنَّمَا الأَعْمَالُ بِالنِّيَّاتِ",
            Some("Narrated 'Umar bin Al-Khattab: The Prophet said..."),
        );

        assert_eq!(
            result.narrators,
            vec![ParsedNarrator {
                external_id: None,
                role: "english_fallback".to_owned(),
                name: "'Umar bin Al-Khattab".to_owned(),
                position: 0,
            }]
        );
    }

    #[test]
    fn english_fallback_is_not_used_when_arabic_narrators_exist() {
        let raw = r#"[prematn][narrator id="1" role="sahabi" tooltip="أبو هريرة"]أبو هريرة[/narrator][/prematn][matn]نص[/matn]"#;

        let result = parse_isnad(raw, Some("Narrated Umar: something else"));

        assert_eq!(result.narrators.len(), 1);
        assert_eq!(result.narrators[0].name, "أبو هريرة");
    }

    #[test]
    fn text_with_no_narrated_prefix_and_no_arabic_tags_has_no_narrators() {
        let result = parse_isnad("نص عادي", Some("Just some translation with no prefix."));

        assert!(result.narrators.is_empty());
    }

    #[test]
    fn malformed_narrator_tag_missing_tooltip_is_skipped_not_fatal() {
        let raw = r#"[prematn][narrator id="1" role="chain"]بلا تولتيب[/narrator][narrator id="2" role="sahabi" tooltip="اسم صحيح"]فلان[/narrator][/prematn][matn]نص[/matn]"#;

        let result = parse_isnad(raw, None);

        assert_eq!(result.clean_arabic_text, "نص");
        assert_eq!(result.narrators.len(), 1);
        assert_eq!(result.narrators[0].name, "اسم صحيح");
        assert_eq!(result.narrators[0].position, 0);
    }
}
