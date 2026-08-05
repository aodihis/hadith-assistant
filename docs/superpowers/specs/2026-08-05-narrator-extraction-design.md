# Narrator extraction and arabic_text cleanup

## Context

This is spec 1 of 4 building the "Sanad" chat UI (see the imported
`Sanad.dc.html` design). The design's hadith cards show "Narrated by
{name}" — a field the `hadiths` table has never had (`src/domain/models.rs`
`Hadith` has no narrator column at all).

The narrator data exists, but only inside `arabicText` in
`data/imports/hadiths.json`, as inline isnad markup:

```
[prematn]حَدَّثَنَا [narrator id="4698" role="first" tooltip="الحميدي عبد الله بن الزبير"]الْحُمَيْدِيُّ...[/narrator]، قَالَ حَدَّثَنَا [narrator id="3443" role="chain" tooltip="..."]سُفْيَانُ[/narrator] ...[/prematn][matn]إِنَّمَا الأَعْمَالُ بِالنِّيَّاتِ...[/matn]
```

Nothing today strips this markup: `insert_record` in
`src/ingestion/hadith_json.rs` binds `record.arabic_text` straight into the
`arabic_text` column. The existing `/search` and `/hadiths` pages already
render these brackets verbatim to users — a pre-existing display bug this
spec also fixes, since it's the same parsing pass.

**Coverage is partial.** Checked against the full 44,896-record dataset:
- 7,020 records (~16%) have `[prematn]`/`[narrator ...]` isnad markup.
- 2,679 records (~6%, overlapping) have `englishText` starting with
  `"Narrated X:"`.
- The remaining ~84%+ have no narrator information in the source data at
  all. This is a data-availability limit, not a parsing gap — the UI must
  tolerate hadiths with no known narrator (spec 4 hides the narrator line
  in that case, per the approved design).

## Schema

New migration `migrations/0003_add_narrators.sql`:

```sql
CREATE TABLE narrators (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    hadith_id BIGINT NOT NULL REFERENCES hadiths(id) ON DELETE CASCADE,
    external_id BIGINT,
    role TEXT NOT NULL,
    name TEXT NOT NULL,
    position INT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT narrators_name_not_empty CHECK (length(btrim(name)) > 0),
    CONSTRAINT narrators_role_not_empty CHECK (length(btrim(role)) > 0)
);

CREATE INDEX narrators_hadith_id_idx ON narrators (hadith_id);
```

`role` is free text mirroring the source markup's `role=""` attribute
(`first`, `chain`, `sahabi`, ...), plus the synthetic value
`english_fallback` used when a narrator is recovered from `englishText`
instead of Arabic markup. `external_id` is the source markup's `id=""`
attribute (nullable — absent for `english_fallback` rows). `position` is
the narrator's order of appearance within the hadith's isnad, starting at
0, used to pick a primary narrator when no `role="sahabi"` tag exists.

## Parsing

New module `src/ingestion/narrator.rs`:

```rust
pub struct ParsedNarrator {
    pub external_id: Option<i64>,
    pub role: String,
    pub name: String,
    pub position: i32,
}

pub struct ParsedText {
    pub clean_arabic_text: String,
    pub narrators: Vec<ParsedNarrator>,
}

pub fn parse_isnad(raw_arabic_text: &str, english_text: Option<&str>) -> ParsedText
```

Behavior:

1. Look for `[prematn]...[/prematn]`, `[matn]...[/matn]`,
   `[postmatn]...[/postmatn]` segments (regex, since the markup is a fixed
   small vocabulary — not general-purpose HTML/XML). Any segment may be
   absent.
2. `clean_arabic_text` = the `[matn]` segment's inner text if a `[matn]`
   tag exists; otherwise the raw input with all `[...]...[/...]` /
   `[...]` tags stripped (a no-op for the ~84% of records with no tags at
   all, and a safe fallback for the 24 `[matn]`-only-without-wrapper edge
   cases already observed in the data).
3. Extract every `[narrator id="ID" role="ROLE" tooltip="NAME"]...[/narrator]`
   occurring in the `prematn`/`postmatn` segments, in document order, each
   becoming a `ParsedNarrator { external_id: Some(ID), role: ROLE, name:
   NAME, position: <index> }`.
4. If step 3 found zero narrators, try `^\s*Narrated ([^:]{1,120}):` against
   `english_text`. A match produces one `ParsedNarrator { external_id:
   None, role: "english_fallback", name: <captured group, trimmed>,
   position: 0 }`.
5. Malformed markup (an unclosed `[narrator]` tag, an `id`/`role` missing
   from an otherwise-tagged narrator) is not a hard error: that one
   `[narrator ...]` occurrence is skipped (not counted, logged at `warn`
   with the hadith's `arabic_urn` for traceability), and parsing continues
   with whatever else was extractable. `clean_arabic_text` still strips
   whatever tags did match plus any leftover bracket fragments, so a
   malformed record never leaks bracket syntax into the UI even if its
   narrators are incomplete.

Primary-narrator selection (used by spec 4's card/drawer rendering, not
stored — computed at read time from the `narrators` rows for a hadith):
prefer `role == "sahabi"`, else the lowest `position`, else absent (no
narrator line shown).

## Import path integration

`insert_record` in `src/ingestion/hadith_json.rs` calls
`parse_isnad(&record.arabic_text_or_empty(), record.english_text.as_deref())`
before binding `arabic_text` (uses `clean_arabic_text` instead of the raw
field) and, after the `INSERT INTO hadiths ... RETURNING id`, inserts one
row per `ParsedNarrator` into `narrators` with that `hadith_id`. This
happens inside the same transaction as the hadith insert, so a fresh
import is clean (no markup, narrators populated) from the first row.

`validate_record` is unchanged — validation still checks the raw
`arabic_text` is non-empty before parsing runs.

## Backfill for already-imported data

The production `hadiths` table already has ~44,896 rows inserted with raw
markup still in `arabic_text`; re-running `import_hadith_json` would
duplicate them (no `ON CONFLICT` upsert exists). Backfill is therefore a
separate in-place pass, added as a new flag on the existing
`src/bin/import_hadiths.rs` (per your preference to keep this in the
existing import path rather than a one-off script):

```
import_hadiths --backfill-narrators --database-url <url>
```

This mode takes no `json_path`. It streams `id, arabic_text, english_text`
from `hadiths` in batches (e.g. 500 rows at a time, ordered by `id`, to
bound memory against a 44k-row table), and per row:

1. Skip rows that already have narrators (`SELECT 1 FROM narrators WHERE
   hadith_id = $1 LIMIT 1`) — makes the backfill idempotent and safely
   re-runnable.
2. Run `parse_isnad` on the row's current `arabic_text` / `english_text`.
3. If `clean_arabic_text != arabic_text`, `UPDATE hadiths SET arabic_text
   = $1, updated_at = now() WHERE id = $2`.
4. Insert any `ParsedNarrator`s into `narrators`.

Unlike the strict all-or-nothing transaction used for fresh JSON import,
each row here commits independently (autocommit, or a per-row
transaction) — a single row's parse failure or database error is logged
with the hadith `id` and skipped rather than aborting the other ~44,895
rows. The command prints a summary at the end: rows processed, rows
updated, rows with narrators found, rows skipped (already processed),
rows failed.

## Repository additions

`src/infrastructure/persistence/hadiths.rs` (or a new sibling
`narrators.rs` — implementation detail for the plan) gets:

```rust
pub async fn find_narrators_by_hadith_id(&self, hadith_id: i64) -> Result<Vec<Narrator>, AppError>;
pub async fn find_primary_narrators_by_hadith_ids(&self, hadith_ids: &[i64]) -> Result<HashMap<i64, Narrator>, AppError>;
```

`find_primary_narrators_by_hadith_ids` is batched (single query with
`WHERE hadith_id = ANY($1)`, primary selection applied in Rust after
fetch) since spec 4 needs primary narrators for a whole page of retrieval
results at once, not one hadith at a time.

## Testing

- `parse_isnad` unit tests: prematn+matn, matn-only (no prematn wrapper),
  prematn+matn+postmatn, no tags at all (plain text passthrough), isnad
  tags present but no `role="sahabi"` (position-based fallback still
  computed at read time, not here), English-only fallback (no Arabic
  tags, `englishText` starts with "Narrated X:"), malformed/unclosed
  `[narrator]` tag (parsing continues, bad occurrence skipped and
  logged).
- `insert_record` integration test (existing test infra, if any DB
  integration tests exist — otherwise a unit test against the query
  construction) confirming `arabic_text` stored is the clean text and
  `narrators` rows are inserted for a record with isnad markup.
- Backfill idempotency test: running the backfill twice against the same
  seeded rows produces no changes and no duplicate `narrators` rows on
  the second run.
- Primary-narrator selection unit tests for the three precedence cases
  (`sahabi` present, no `sahabi` but multiple narrators, single
  `english_fallback` narrator, zero narrators).

## Out of scope

- Wiring narrators into `RetrievedHadith` / the chat UI (spec 4).
- Any UI for browsing/searching by narrator.
- Normalizing narrator names (e.g. deduplicating the same person spelled
  differently across records) — each `narrators` row stores the tag's
  `tooltip` text as-is.
