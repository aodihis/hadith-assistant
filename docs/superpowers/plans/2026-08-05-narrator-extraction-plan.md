# Narrator Extraction and arabic_text Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parse narrator/isnad data out of the raw `[prematn]/[narrator]/[matn]` markup in `arabicText`, store it in a new `narrators` table, and store clean matn-only text in `hadiths.arabic_text` — for both fresh JSON imports and a one-time backfill of the already-imported 44,896-row table.

**Architecture:** A pure parsing function (`parse_isnad`) turns raw markup into clean text + a list of narrators. It's called from two places: `insert_record` (fresh imports, inside the existing transaction) and a new `backfill_narrators` routine (re-processes existing rows in place, per-row transactions, idempotent). A shared `insert_narrators` helper writes `ParsedNarrator`s to the DB for both callers.

**Tech Stack:** Rust, sqlx (Postgres, runtime queries — no compile-time query macros in this codebase), `regex` crate (new dependency).

## Global Constraints

- No compile-time-checked sqlx queries (`sqlx::query!`) are used anywhere in this codebase — stick to `sqlx::query`/`sqlx::query_as`/`sqlx::query_scalar` with runtime binds, matching existing code.
- Migrations are embedded via `sqlx::migrate!()` in `src/main.rs:28` and run automatically on app startup — no manual migration-running step needed beyond adding the `.sql` file.
- Narrator coverage is inherently partial (~16% of records have isnad markup, ~6% have an English "Narrated X:" fallback, these overlap). Code must treat "no narrator found" as a normal, expected outcome, not an error.
- Follow existing file conventions: `src/ingestion/` holds import-time logic, `src/infrastructure/persistence/` holds repositories, `src/domain/models.rs` holds shared structs.

---

### Task 1: Migration, domain model, and dependency

**Files:**
- Create: `migrations/0003_add_narrators.sql`
- Modify: `Cargo.toml`
- Modify: `src/domain/models.rs`
- Modify: `src/domain/mod.rs`

**Interfaces:**
- Produces: `narrators` table (columns: `id`, `hadith_id`, `external_id`, `role`, `name`, `position`, `created_at`); `Narrator` struct (`domain::Narrator`) with `FromRow` for reading rows in later tasks.

- [ ] **Step 1: Add the `regex` dependency**

In `Cargo.toml`, add to `[dependencies]` (alphabetical, matching existing ordering):

```toml
regex = "1.10"
```

- [ ] **Step 2: Write the migration**

Create `migrations/0003_add_narrators.sql`:

```sql
CREATE TABLE narrators (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    hadith_id BIGINT NOT NULL REFERENCES hadiths(id) ON DELETE CASCADE,
    external_id BIGINT,
    role TEXT NOT NULL,
    name TEXT NOT NULL,
    "position" INT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT narrators_name_not_empty CHECK (length(btrim(name)) > 0),
    CONSTRAINT narrators_role_not_empty CHECK (length(btrim(role)) > 0)
);

CREATE INDEX narrators_hadith_id_idx ON narrators (hadith_id);
```

(`"position"` is quoted because `POSITION` is a reserved SQL function name.)

- [ ] **Step 3: Add the `Narrator` domain struct**

In `src/domain/models.rs`, append:

```rust
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Narrator {
    pub id: i64,
    pub hadith_id: i64,
    pub external_id: Option<i64>,
    pub role: String,
    pub name: String,
    pub position: i32,
}
```

- [ ] **Step 4: Export it**

In `src/domain/mod.rs`, change:

```rust
pub use models::{
    Collection, Hadith, HadithSearch, RetrievalQuery, RetrievalResult, RetrievedHadith,
};
```

to:

```rust
pub use models::{
    Collection, Hadith, HadithSearch, Narrator, RetrievalQuery, RetrievalResult, RetrievedHadith,
};
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check`
Expected: succeeds (migration files aren't validated by `cargo check`, only embedded at build time — that's fine, `sqlx::migrate!()` reads the directory at compile time via `include_str!`-style macro, so a syntactically valid new file is enough for this step; it gets exercised for real in Task 5's manual verification).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock migrations/0003_add_narrators.sql src/domain/models.rs src/domain/mod.rs
git commit -m "feat: add narrators table and domain model"
```

---

### Task 2: Isnad parser (`parse_isnad`)

**Files:**
- Create: `src/ingestion/narrator.rs`
- Modify: `src/ingestion/mod.rs`

**Interfaces:**
- Consumes: nothing from other tasks (pure function, no DB).
- Produces: `pub struct ParsedNarrator { external_id: Option<i64>, role: String, name: String, position: i32 }`, `pub struct ParsedText { clean_arabic_text: String, narrators: Vec<ParsedNarrator> }`, `pub fn parse_isnad(raw_arabic_text: &str, english_text: Option<&str>) -> ParsedText`, `pub async fn insert_narrators(conn: &mut sqlx::PgConnection, hadith_id: i64, narrators: &[ParsedNarrator]) -> Result<(), sqlx::Error>` — all consumed by Task 3 (fresh import) and Task 5 (backfill).

- [ ] **Step 1: Write the failing tests**

Create `src/ingestion/narrator.rs`:

```rust
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

pub fn parse_isnad(_raw_arabic_text: &str, _english_text: Option<&str>) -> ParsedText {
    unimplemented!()
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ingestion::narrator`
Expected: compile error or `unimplemented!()` panics — `parse_isnad` isn't implemented yet.

- [ ] **Step 3: Implement `parse_isnad`**

Replace the `unimplemented!()` body and add the regexes/helpers above it, so the top of the file (before `#[cfg(test)]`) becomes:

```rust
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

static NARRATOR_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)\[narrator\s+([^\]]*)\].*?\[/narrator\]"#).expect("valid regex"));
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
            position: 0,
        });
    }

    for (index, narrator) in narrators.iter_mut().enumerate() {
        narrator.position = index as i32;
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
```

Keep the existing `insert_narrators` function below this (from Step 1) unchanged.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ingestion::narrator`
Expected: all 8 tests pass.

- [ ] **Step 5: Register the module**

In `src/ingestion/mod.rs`, change:

```rust
pub mod embedding;
pub mod hadith_json;
```

to:

```rust
pub mod embedding;
pub mod hadith_json;
pub mod narrator;
```

- [ ] **Step 6: Run the full test suite**

Run: `cargo test --lib`
Expected: all tests pass (no regressions elsewhere).

- [ ] **Step 7: Commit**

```bash
git add src/ingestion/narrator.rs src/ingestion/mod.rs
git commit -m "feat: parse isnad markup into clean text and narrators"
```

---

### Task 3: Wire the parser into fresh JSON import

**Files:**
- Modify: `src/ingestion/hadith_json.rs`

**Interfaces:**
- Consumes: `crate::ingestion::narrator::{parse_isnad, insert_narrators}` from Task 2.
- Produces: `insert_record` now stores clean `arabic_text` and populates `narrators` for every freshly imported hadith.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/ingestion/hadith_json.rs`:

```rust
#[test]
fn arabic_transliteration_now_takes_clean_text_directly() {
    // arabic_transliteration must be called with already-cleaned text (no
    // markup), since insert_record now cleans before transliterating.
    assert_eq!(arabic_transliteration("إِنَّمَا"), "'innamaa");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ingestion::hadith_json::tests::arabic_transliteration_now_takes_clean_text_directly`
Expected: compile error — `arabic_transliteration` still takes `&RawHadithRecord`, not `&str`.

- [ ] **Step 3: Change `arabic_transliteration`'s signature and update `insert_record`**

In `src/ingestion/hadith_json.rs`, replace:

```rust
fn arabic_transliteration(record: &RawHadithRecord) -> String {
    transliterate(validated_arabic_text(record))
}
```

with:

```rust
fn arabic_transliteration(clean_arabic_text: &str) -> String {
    transliterate(clean_arabic_text)
}
```

Add the import at the top of the file:

```rust
use crate::ingestion::narrator::{insert_narrators, parse_isnad};
```

Replace the body of `insert_record` (keep the same function signature) from:

```rust
async fn insert_record(
    tx: &mut Transaction<'_, Postgres>,
    record: &RawHadithRecord,
) -> Result<i64, ImportError> {
    let collection_id = upsert_collection(tx, record.collection.trim()).await?;

    let id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO hadiths (
            collection_id,
            book_number,
            bab_id,
            english_bab_number,
            arabic_bab_number,
            hadith_number,
            our_hadith_number,
            arabic_urn,
            arabic_bab_name,
            arabic_text,
            arabic_transliteration,
            arabic_grade,
            english_urn,
            english_bab_name,
            english_text,
            english_grade,
            last_updated,
            xrefs
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        RETURNING id
        "#,
    )
    .bind(collection_id)
    .bind(record.book_number.trim())
    .bind(record.bab_id)
    .bind(trim_optional(record.english_bab_number.as_deref()))
    .bind(trim_optional(record.arabic_bab_number.as_deref()))
    .bind(canonical_hadith_number(record))
    .bind(record.our_hadith_number)
    .bind(record.arabic_urn)
    .bind(trim_optional(record.arabic_bab_name.as_deref()))
    .bind(validated_arabic_text(record))
    .bind(arabic_transliteration(record))
    .bind(record.arabicgrade1.trim())
    .bind(record.english_urn)
    .bind(trim_optional(record.english_bab_name.as_deref()))
    .bind(trim_optional(record.english_text.as_deref()))
    .bind(record.englishgrade1.trim())
    .bind(trim_optional(record.last_updated.as_deref()))
    .bind(record.xrefs.trim())
    .fetch_one(&mut **tx)
    .await?;

    Ok(id)
}
```

to:

```rust
async fn insert_record(
    tx: &mut Transaction<'_, Postgres>,
    record: &RawHadithRecord,
) -> Result<i64, ImportError> {
    let collection_id = upsert_collection(tx, record.collection.trim()).await?;
    let parsed = parse_isnad(validated_arabic_text(record), record.english_text.as_deref());

    let id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO hadiths (
            collection_id,
            book_number,
            bab_id,
            english_bab_number,
            arabic_bab_number,
            hadith_number,
            our_hadith_number,
            arabic_urn,
            arabic_bab_name,
            arabic_text,
            arabic_transliteration,
            arabic_grade,
            english_urn,
            english_bab_name,
            english_text,
            english_grade,
            last_updated,
            xrefs
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        RETURNING id
        "#,
    )
    .bind(collection_id)
    .bind(record.book_number.trim())
    .bind(record.bab_id)
    .bind(trim_optional(record.english_bab_number.as_deref()))
    .bind(trim_optional(record.arabic_bab_number.as_deref()))
    .bind(canonical_hadith_number(record))
    .bind(record.our_hadith_number)
    .bind(record.arabic_urn)
    .bind(trim_optional(record.arabic_bab_name.as_deref()))
    .bind(&parsed.clean_arabic_text)
    .bind(arabic_transliteration(&parsed.clean_arabic_text))
    .bind(record.arabicgrade1.trim())
    .bind(record.english_urn)
    .bind(trim_optional(record.english_bab_name.as_deref()))
    .bind(trim_optional(record.english_text.as_deref()))
    .bind(record.englishgrade1.trim())
    .bind(trim_optional(record.last_updated.as_deref()))
    .bind(record.xrefs.trim())
    .fetch_one(&mut **tx)
    .await?;

    insert_narrators(tx, id, &parsed.narrators).await?;

    Ok(id)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ingestion::hadith_json`
Expected: all tests pass, including the new one and the pre-existing `generates_transliteration_from_arabic_text_for_import` test (which calls `arabic_transliteration` — check that call site still compiles; if it still passes `&record`, update it to `arabic_transliteration(validated_arabic_text(&record))` to match the new signature).

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --lib`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/ingestion/hadith_json.rs
git commit -m "feat: store clean arabic_text and narrators on fresh import"
```

---

### Task 4: `NarratorRepository`

**Files:**
- Create: `src/infrastructure/persistence/narrators.rs`
- Modify: `src/infrastructure/persistence/mod.rs`

**Interfaces:**
- Consumes: `domain::Narrator` (Task 1).
- Produces: `pub struct NarratorRepository`, `pub fn new(pool: PgPool) -> Self`, `pub async fn find_by_hadith_id(&self, hadith_id: i64) -> Result<Vec<Narrator>, AppError>`, `pub async fn find_primary_by_hadith_ids(&self, hadith_ids: &[i64]) -> Result<HashMap<i64, Narrator>, AppError>` — consumed by spec 4 (Sanad UI), not by anything else in this plan.

- [ ] **Step 1: Write the repository**

Create `src/infrastructure/persistence/narrators.rs`:

```rust
use std::collections::HashMap;

use sqlx::PgPool;

use crate::domain::Narrator;
use crate::error::AppError;

#[derive(Clone)]
pub struct NarratorRepository {
    pool: PgPool,
}

impl NarratorRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_hadith_id(&self, hadith_id: i64) -> Result<Vec<Narrator>, AppError> {
        let narrators = sqlx::query_as::<_, Narrator>(
            r#"
            SELECT id, hadith_id, external_id, role, name, "position"
            FROM narrators
            WHERE hadith_id = $1
            ORDER BY "position"
            "#,
        )
        .bind(hadith_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(narrators)
    }

    /// Picks one narrator per hadith: the one with role "sahabi" if present,
    /// otherwise the one with the lowest position. Hadiths with no narrator
    /// rows are simply absent from the returned map.
    pub async fn find_primary_by_hadith_ids(
        &self,
        hadith_ids: &[i64],
    ) -> Result<HashMap<i64, Narrator>, AppError> {
        let narrators = sqlx::query_as::<_, Narrator>(
            r#"
            SELECT id, hadith_id, external_id, role, name, "position"
            FROM narrators
            WHERE hadith_id = ANY($1)
            ORDER BY hadith_id, "position"
            "#,
        )
        .bind(hadith_ids)
        .fetch_all(&self.pool)
        .await?;

        let mut primary: HashMap<i64, Narrator> = HashMap::new();
        for narrator in narrators {
            match primary.get(&narrator.hadith_id) {
                None => {
                    primary.insert(narrator.hadith_id, narrator);
                }
                Some(existing) if narrator.role == "sahabi" && existing.role != "sahabi" => {
                    primary.insert(narrator.hadith_id, narrator);
                }
                _ => {}
            }
        }

        Ok(primary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn narrator(hadith_id: i64, role: &str, position: i32, name: &str) -> Narrator {
        Narrator {
            id: position as i64 + 1,
            hadith_id,
            external_id: None,
            role: role.to_owned(),
            name: name.to_owned(),
            position,
        }
    }

    #[test]
    fn primary_selection_prefers_sahabi_over_lower_position() {
        let rows = vec![
            narrator(1, "first", 0, "Chain narrator"),
            narrator(1, "sahabi", 1, "The companion"),
        ];

        let mut primary: HashMap<i64, Narrator> = HashMap::new();
        for row in rows {
            match primary.get(&row.hadith_id) {
                None => {
                    primary.insert(row.hadith_id, row);
                }
                Some(existing) if row.role == "sahabi" && existing.role != "sahabi" => {
                    primary.insert(row.hadith_id, row);
                }
                _ => {}
            }
        }

        assert_eq!(primary[&1].name, "The companion");
    }

    #[test]
    fn primary_selection_falls_back_to_lowest_position_without_sahabi() {
        let rows = vec![
            narrator(1, "first", 0, "Earliest mentioned"),
            narrator(1, "chain", 1, "Later mentioned"),
        ];

        let mut primary: HashMap<i64, Narrator> = HashMap::new();
        for row in rows {
            match primary.get(&row.hadith_id) {
                None => {
                    primary.insert(row.hadith_id, row);
                }
                Some(existing) if row.role == "sahabi" && existing.role != "sahabi" => {
                    primary.insert(row.hadith_id, row);
                }
                _ => {}
            }
        }

        assert_eq!(primary[&1].name, "Earliest mentioned");
    }
}
```

(These unit tests duplicate the selection logic inline rather than calling `find_primary_by_hadith_ids` directly, since that method requires a live Postgres connection — matching this codebase's existing pattern where `HadithRepository` has no unit tests of its own and is instead exercised by the ignored `tests/retrieval_integration.rs`.)

- [ ] **Step 2: Register the module**

In `src/infrastructure/persistence/mod.rs`, change:

```rust
pub mod collections;
pub mod hadiths;
```

to:

```rust
pub mod collections;
pub mod hadiths;
pub mod narrators;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib infrastructure::persistence::narrators`
Expected: both tests pass.

- [ ] **Step 4: Run the full test suite**

Run: `cargo test --lib`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/infrastructure/persistence/narrators.rs src/infrastructure/persistence/mod.rs
git commit -m "feat: add NarratorRepository for reading narrators by hadith"
```

---

### Task 5: Backfill for already-imported rows

**Files:**
- Create: `src/ingestion/narrator_backfill.rs`
- Modify: `src/ingestion/mod.rs`
- Modify: `src/bin/import_hadiths.rs`

**Interfaces:**
- Consumes: `crate::ingestion::narrator::{parse_isnad, insert_narrators}` (Task 2).
- Produces: `pub struct BackfillSummary { rows_scanned, rows_updated, rows_skipped_already_processed, rows_failed }`, `pub async fn backfill_narrators(pool: &sqlx::PgPool) -> Result<BackfillSummary, sqlx::Error>`; `import_hadiths --backfill-narrators` CLI mode.

- [ ] **Step 1: Write the backfill module**

Create `src/ingestion/narrator_backfill.rs`:

```rust
use sqlx::PgPool;

use crate::ingestion::narrator::{insert_narrators, parse_isnad};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BackfillSummary {
    pub rows_scanned: usize,
    pub rows_updated: usize,
    pub rows_skipped_already_processed: usize,
    pub rows_failed: usize,
}

#[derive(Debug, sqlx::FromRow)]
struct BackfillRow {
    id: i64,
    arabic_text: String,
    english_text: Option<String>,
}

/// Re-parses every hadith's `arabic_text` for isnad markup, cleaning the
/// stored text and populating `narrators`. Safe to re-run: rows that
/// already have narrators are skipped, and each row commits independently
/// so one bad row doesn't block the rest of the table.
pub async fn backfill_narrators(pool: &PgPool) -> Result<BackfillSummary, sqlx::Error> {
    const BATCH_SIZE: i64 = 500;

    let mut summary = BackfillSummary::default();
    let mut last_id = 0i64;

    loop {
        let rows = sqlx::query_as::<_, BackfillRow>(
            "SELECT id, arabic_text, english_text FROM hadiths WHERE id > $1 ORDER BY id LIMIT $2",
        )
        .bind(last_id)
        .bind(BATCH_SIZE)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            break;
        }
        last_id = rows.last().expect("batch checked non-empty above").id;

        for row in rows {
            summary.rows_scanned += 1;
            match backfill_row(pool, &row).await {
                Ok(true) => summary.rows_updated += 1,
                Ok(false) => summary.rows_skipped_already_processed += 1,
                Err(error) => {
                    tracing::warn!(hadith_id = row.id, %error, "narrator backfill failed for row");
                    summary.rows_failed += 1;
                }
            }
        }
    }

    Ok(summary)
}

/// Returns Ok(true) if the row was processed, Ok(false) if it was already
/// processed (skipped).
async fn backfill_row(pool: &PgPool, row: &BackfillRow) -> Result<bool, sqlx::Error> {
    let already_processed: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM narrators WHERE hadith_id = $1)")
            .bind(row.id)
            .fetch_one(pool)
            .await?;
    if already_processed {
        return Ok(false);
    }

    let parsed = parse_isnad(&row.arabic_text, row.english_text.as_deref());

    let mut tx = pool.begin().await?;

    if parsed.clean_arabic_text != row.arabic_text {
        sqlx::query("UPDATE hadiths SET arabic_text = $1, updated_at = now() WHERE id = $2")
            .bind(&parsed.clean_arabic_text)
            .bind(row.id)
            .execute(&mut *tx)
            .await?;
    }

    insert_narrators(&mut tx, row.id, &parsed.narrators).await?;

    tx.commit().await?;

    Ok(true)
}
```

- [ ] **Step 2: Register the module**

In `src/ingestion/mod.rs`, change:

```rust
pub mod embedding;
pub mod hadith_json;
pub mod narrator;
```

to:

```rust
pub mod embedding;
pub mod hadith_json;
pub mod narrator;
pub mod narrator_backfill;
```

- [ ] **Step 3: Run tests to verify nothing broke**

Run: `cargo test --lib`
Expected: all tests pass (no new unit tests here — this module needs a live Postgres to test meaningfully, verified manually in Step 6 below, consistent with how `HadithRepository` and the JSON-import DB path are verified in this codebase).

- [ ] **Step 4: Wire the CLI flag**

In `src/bin/import_hadiths.rs`:

Add the import:

```rust
use hadith_assistant::ingestion::narrator_backfill::backfill_narrators;
```

Change the `Args` struct from:

```rust
#[derive(Debug)]
struct Args {
    json_path: String,
    database_url: Option<String>,
    validate_only: bool,
    embed: bool,
}
```

to:

```rust
#[derive(Debug)]
struct Args {
    json_path: Option<String>,
    database_url: Option<String>,
    validate_only: bool,
    embed: bool,
    backfill_narrators: bool,
}
```

Change the parse loop's match arms — add a case and change how `json_path` is captured. Replace:

```rust
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--database-url" => {
                    database_url = Some(require_value(&mut args, "--database-url")?);
                }
                "--validate-only" => {
                    validate_only = true;
                }
                "--embed" => {
                    embed = true;
                }
                "-h" | "--help" => {
                    return Err(usage());
                }
                value if value.starts_with('-') => {
                    return Err(format!("unknown option: {value}\n\n{}", usage()));
                }
                value => {
                    if json_path.replace(value.to_owned()).is_some() {
                        return Err(format!("unexpected extra argument: {value}\n\n{}", usage()));
                    }
                }
            }
        }

        Ok(Self {
            json_path: json_path.ok_or_else(usage)?,
            database_url,
            validate_only,
            embed,
        })
    }
```

with:

```rust
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--database-url" => {
                    database_url = Some(require_value(&mut args, "--database-url")?);
                }
                "--validate-only" => {
                    validate_only = true;
                }
                "--embed" => {
                    embed = true;
                }
                "--backfill-narrators" => {
                    backfill_narrators = true;
                }
                "-h" | "--help" => {
                    return Err(usage());
                }
                value if value.starts_with('-') => {
                    return Err(format!("unknown option: {value}\n\n{}", usage()));
                }
                value => {
                    if json_path.replace(value.to_owned()).is_some() {
                        return Err(format!("unexpected extra argument: {value}\n\n{}", usage()));
                    }
                }
            }
        }

        if backfill_narrators {
            if json_path.is_some() {
                return Err(format!(
                    "--backfill-narrators takes no <json-path>\n\n{}",
                    usage()
                ));
            }
        } else if json_path.is_none() {
            return Err(usage());
        }

        Ok(Self {
            json_path,
            database_url,
            validate_only,
            embed,
            backfill_narrators,
        })
    }
```

And add `let mut backfill_narrators = false;` next to the other `let mut` declarations at the top of `parse`.

Update `usage()`:

```rust
fn usage() -> String {
    "usage: import_hadiths <json-path> [--database-url <url>] [--validate-only] [--embed]\n       import_hadiths --backfill-narrators [--database-url <url>]"
        .to_owned()
}
```

Update `run()` — replace:

```rust
async fn run() -> Result<(), String> {
    dotenvy::dotenv().ok();

    let args = Args::parse(env::args().skip(1))?;

    if args.validate_only {
        let (dump, checksum) = load_dump(&args.json_path).map_err(|error| error.to_string())?;
        validate_dump(&dump).map_err(|error| error.to_string())?;
        println!(
            "validated {} records from {} ({checksum})",
            dump.hadith_table.len(),
            args.json_path
        );
        return Ok(());
    }

    let database_url = args
        .database_url
        .or_else(|| env::var("DATABASE_URL").ok())
        .ok_or("DATABASE_URL or --database-url is required unless --validate-only is used")?;

    let summary = import_hadith_json(ImportOptions {
        database_url: database_url.clone(),
        json_path: args.json_path,
    })
    .await
    .map_err(|error| error.to_string())?;

    println!(
        "imported {} records ({})",
        summary.record_count, summary.source_checksum
    );

    if args.embed {
        let embedded = run_embedding(&database_url, &summary.inserted_ids)
            .await
            .map_err(|error| error.to_string())?;
        println!("embedded {embedded} records");
    }

    Ok(())
}
```

with:

```rust
async fn run() -> Result<(), String> {
    dotenvy::dotenv().ok();

    let args = Args::parse(env::args().skip(1))?;

    let database_url = args
        .database_url
        .clone()
        .or_else(|| env::var("DATABASE_URL").ok())
        .ok_or("DATABASE_URL or --database-url is required unless --validate-only is used")?;

    if args.backfill_narrators {
        return run_backfill_narrators(&database_url)
            .await
            .map_err(|error| error.to_string());
    }

    let json_path = args.json_path.clone().expect("json_path required when not backfilling, enforced by Args::parse");

    if args.validate_only {
        let (dump, checksum) = load_dump(&json_path).map_err(|error| error.to_string())?;
        validate_dump(&dump).map_err(|error| error.to_string())?;
        println!(
            "validated {} records from {} ({checksum})",
            dump.hadith_table.len(),
            json_path
        );
        return Ok(());
    }

    let summary = import_hadith_json(ImportOptions {
        database_url: database_url.clone(),
        json_path,
    })
    .await
    .map_err(|error| error.to_string())?;

    println!(
        "imported {} records ({})",
        summary.record_count, summary.source_checksum
    );

    if args.embed {
        let embedded = run_embedding(&database_url, &summary.inserted_ids)
            .await
            .map_err(|error| error.to_string())?;
        println!("embedded {embedded} records");
    }

    Ok(())
}

async fn run_backfill_narrators(database_url: &str) -> Result<(), String> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(|error| error.to_string())?;

    let summary = backfill_narrators(&pool)
        .await
        .map_err(|error| error.to_string())?;

    println!(
        "narrator backfill: scanned {}, updated {}, already processed {}, failed {}",
        summary.rows_scanned,
        summary.rows_updated,
        summary.rows_skipped_already_processed,
        summary.rows_failed
    );

    Ok(())
}
```

- [ ] **Step 5: Update the existing CLI arg-parsing tests**

The existing tests reference `args.json_path` as a plain `String`; since it's now `Option<String>`, update in the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn parse_recognizes_the_embed_flag() {
    let args = Args::parse(["data/imports/hadiths.json".to_owned(), "--embed".to_owned()])
        .expect("valid arguments should parse");

    assert!(args.embed);
    assert_eq!(args.json_path.as_deref(), Some("data/imports/hadiths.json"));
}

#[test]
fn parse_defaults_embed_to_false() {
    let args = Args::parse(["data/imports/hadiths.json".to_owned()])
        .expect("valid arguments should parse");

    assert!(!args.embed);
}
```

Add two new tests in the same block:

```rust
#[test]
fn parse_accepts_backfill_narrators_without_a_json_path() {
    let args = Args::parse(["--backfill-narrators".to_owned()])
        .expect("backfill mode should not require a json path");

    assert!(args.backfill_narrators);
    assert!(args.json_path.is_none());
}

#[test]
fn parse_rejects_backfill_narrators_combined_with_a_json_path() {
    let error = Args::parse([
        "data/imports/hadiths.json".to_owned(),
        "--backfill-narrators".to_owned(),
    ])
    .expect_err("backfill mode should reject a json path argument");

    assert!(error.contains("--backfill-narrators takes no <json-path>"));
}

#[test]
fn parse_rejects_missing_json_path_when_not_backfilling() {
    let error = Args::parse([]).expect_err("json path is required outside backfill mode");

    assert!(error.contains("usage:"));
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test --bin import_hadiths`
Expected: all tests pass.

- [ ] **Step 7: Run the full test suite**

Run: `cargo test --lib --bins`
Expected: all tests pass.

- [ ] **Step 8: Manual verification against a real database**

This step needs a live Postgres and cannot be scripted as a unit test — run it yourself and report the output:

```bash
docker compose up -d postgres
cargo run --bin import_hadiths -- --backfill-narrators
```

Expected: prints a summary line (`narrator backfill: scanned N, updated M, ...`) with `rows_scanned` equal to the current row count of `hadiths`, and no crash. Then run it a second time immediately:

```bash
cargo run --bin import_hadiths -- --backfill-narrators
```

Expected: same `rows_scanned`, but `rows_updated: 0` and `rows_skipped_already_processed` equal to the previous run's `rows_updated` — proving idempotency.

Spot-check the result:

```bash
docker exec -it hadith-assistant-postgres psql -U postgres -d hadiths -c "SELECT count(*) FROM narrators;"
docker exec -it hadith-assistant-postgres psql -U postgres -d hadiths -c "SELECT arabic_text FROM hadiths WHERE arabic_text LIKE '%[matn]%' LIMIT 5;"
```

Expected: `narrators` has rows, and the second query returns zero rows (no leftover markup anywhere in `arabic_text`).

- [ ] **Step 9: Commit**

```bash
git add src/ingestion/narrator_backfill.rs src/ingestion/mod.rs src/bin/import_hadiths.rs
git commit -m "feat: add narrator backfill for already-imported hadiths"
```

---

## Self-Review Notes

- **Spec coverage:** schema (Task 1), parsing incl. malformed-tag handling (Task 2), fresh-import wiring (Task 3), repository reads incl. primary-narrator precedence (Task 4), backfill incl. idempotency and per-row isolation (Task 5). All spec sections have a corresponding task.
- **Out of scope reminder:** wiring narrators into the retrieval/chat UI is spec 4, not this plan — `NarratorRepository` is built but not yet called from any route.

## Execution Notes (found while running Task 5's manual verification)

- The original `already_processed` check used `SELECT 1 ... LIMIT 1` decoded
  into `Option<i64>`. Postgres integer literals are `int4`, not `int8`, so
  this decoded fine when the query returned zero rows (first backfill run,
  narrators table empty) but errored on any row that actually had a match
  (second run) — every row with narrators failed instead of being skipped.
  Fixed by switching to `SELECT EXISTS(...)` decoded as `bool`. The code
  above already reflects the fix.
- Verified against the live database (44,896 rows): first run —
  `updated 44896, already processed 0, failed 0`. After the fix, second
  run — `updated 35136, already processed 9760, failed 0`, with
  `narrators` row count unchanged (44,034) and zero remaining
  `[matn]`/`[prematn]`/`[narrator` occurrences in `arabic_text`. Note the
  "updated" count never reaches exactly 0 on repeat runs: hadiths with no
  narrators found at all have no `narrators` row to short-circuit on, so
  they're harmlessly reprocessed (no-op) every run rather than counted as
  "skipped" — this is expected, not a bug, and confirmed by the flat
  `narrators` row count across runs.
- 89 hadiths still contain literal `[`/`]` after cleanup — confirmed
  these are unrelated real content (e.g. `[رضى الله عنها]` honorifics,
  `[رَوَاهُ مُسْلِمٌ]` citation markers, `<a href="...">` links from a
  different source format), not leftover isnad tags. Correctly left
  untouched since `ANY_TAG` only matches `[A-Za-z]+`-named tags.

## Post-Implementation Simplification Pass (`/simplify`)

After Task 5's manual verification, a 4-angle cleanup review (reuse,
simplification, efficiency, altitude) found three worth applying, all
re-verified against the live 44,896-row database afterward:

- `extract_arabic_narrators` (`src/ingestion/narrator.rs`) collapsed from
  a two-pass push-then-renumber into a single pass (`position:
  narrators.len() as i32` at push time), since skipped/malformed tags
  never get pushed so the pre-push length already equals the correct
  index.
- `NarratorRepository::find_primary_by_hadith_ids`
  (`src/infrastructure/persistence/narrators.rs`) moved primary-narrator
  selection from a client-side `HashMap` reduction to `SELECT DISTINCT ON
  (hadith_id) ... ORDER BY hadith_id, (role = 'sahabi') DESC, "position"`
  in SQL. This also removed two unit tests that had silently drifted into
  testing a hand-pasted copy of the selection logic instead of the real
  method — the method now has no unit tests, consistent with this
  codebase's convention that DB-backed repository logic isn't unit
  tested (see `HadithRepository`).
- `backfill_narrators` (`src/ingestion/narrator_backfill.rs`) no longer
  issues a `SELECT EXISTS` round trip per row (44,896 extra round trips
  across a full run). The "already processed" check is now a single
  `NOT EXISTS` clause folded into each batch's `SELECT`, plus one
  `SELECT count(*)` up front for the `rows_scanned` total.

Not applied (judged out of scope for a cleanup pass, noted for awareness
rather than acted on): bulk multi-row `INSERT` for narrators instead of
one `INSERT` per narrator (real but minor waste, shared with the
low-volume live import path — the reviewing agent itself didn't insist),
and splitting `import_hadiths` into real subcommands instead of a
`--backfill-narrators` flag bolted onto the existing flat `Args` struct
(a legitimate observation but an interface redesign, not a mechanical
cleanup).
