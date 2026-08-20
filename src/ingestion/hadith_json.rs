use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;

use crate::ingestion::narrator::{insert_narrators, parse_isnad};
use crate::transliteration::simple::transliterate;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("failed to read import file `{path}`: {source}")]
    ReadFile {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse import file `{path}` as Hadith JSON: {source}")]
    ParseJson {
        path: String,
        source: serde_json::Error,
    },
    #[error("invalid import record at index {index}: {message}")]
    InvalidRecord { index: usize, message: String },
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

#[derive(Debug, Deserialize)]
pub struct HadithJsonDump {
    #[serde(rename = "HadithTable")]
    pub hadith_table: Vec<RawHadithRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawHadithRecord {
    pub collection: String,
    pub book_number: String,
    #[serde(rename = "babID")]
    pub bab_id: f64,
    pub english_bab_number: Option<String>,
    pub arabic_bab_number: Option<String>,
    pub hadith_number: String,
    pub our_hadith_number: i32,
    #[serde(rename = "arabicURN")]
    pub arabic_urn: i64,
    pub arabic_bab_name: Option<String>,
    pub arabic_text: Option<String>,
    pub arabicgrade1: String,
    #[serde(rename = "englishURN")]
    pub english_urn: i64,
    pub english_bab_name: Option<String>,
    pub english_text: Option<String>,
    pub englishgrade1: String,
    pub last_updated: Option<String>,
    pub xrefs: String,
}

#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub database_url: String,
    pub json_path: String,
}

#[derive(Debug, Clone)]
pub struct ImportSummary {
    pub record_count: usize,
    pub source_checksum: String,
    pub inserted_ids: Vec<i64>,
    /// Records already present in Postgres. Carried separately from
    /// `inserted_ids` so an embed pass can close a vector-index gap for
    /// hadiths that were imported by an earlier, interrupted run.
    pub skipped_ids: Vec<i64>,
}

pub fn load_dump(path: impl AsRef<Path>) -> Result<(HadithJsonDump, String), ImportError> {
    let path = path.as_ref();
    let path_display = path.display().to_string();
    let file = File::open(path).map_err(|source| ImportError::ReadFile {
        path: path_display.clone(),
        source,
    })?;

    let mut reader = BufReader::new(file);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|source| ImportError::ReadFile {
            path: path_display.clone(),
            source,
        })?;

    let checksum = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
    let dump = serde_json::from_slice(&bytes).map_err(|source| ImportError::ParseJson {
        path: path_display,
        source,
    })?;

    Ok((dump, checksum))
}

pub fn validate_dump(dump: &HadithJsonDump) -> Result<(), ImportError> {
    for (index, record) in dump.hadith_table.iter().enumerate() {
        validate_record(index, record)?;
    }

    Ok(())
}

pub async fn import_hadith_json(options: ImportOptions) -> Result<ImportSummary, ImportError> {
    let (dump, source_checksum) = load_dump(&options.json_path)?;
    validate_dump(&dump)?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&options.database_url)
        .await?;

    import_dump(&pool, &dump, &source_checksum).await
}

async fn import_dump(
    pool: &PgPool,
    dump: &HadithJsonDump,
    source_checksum: &str,
) -> Result<ImportSummary, ImportError> {
    let mut tx = pool.begin().await?;

    let mut inserted_ids = Vec::with_capacity(dump.hadith_table.len());
    let mut skipped_ids = Vec::new();
    // A dump carries a handful of collections across many thousands of rows, so
    // resolving the collection per record would issue an upsert round trip that
    // almost always returns the id already known from the previous row.
    let mut collection_ids: HashMap<String, i64> = HashMap::new();

    for record in &dump.hadith_table {
        // (arabic_urn, english_urn) is the source dump's own stable identity,
        // so a re-import recognises records it already holds instead of
        // duplicating canonical text. The id is still collected, because the
        // record may exist in Postgres yet be missing from the vector index.
        match find_existing_id(&mut tx, record).await? {
            Some(existing_id) => skipped_ids.push(existing_id),
            None => {
                let slug = record.collection.trim();
                let collection_id = match collection_ids.get(slug) {
                    Some(id) => *id,
                    None => {
                        let id = upsert_collection(&mut tx, slug).await?;
                        collection_ids.insert(slug.to_owned(), id);
                        id
                    }
                };
                inserted_ids.push(insert_record(&mut tx, record, collection_id).await?);
            }
        }
    }

    tx.commit().await?;

    Ok(ImportSummary {
        record_count: dump.hadith_table.len(),
        source_checksum: source_checksum.to_owned(),
        inserted_ids,
        skipped_ids,
    })
}

/// Looks up a record by the source dump's stable identifiers.
///
/// Returns the canonical row id when this hadith has already been imported, so
/// the caller can skip the insert without losing track of the record.
async fn find_existing_id(
    tx: &mut Transaction<'_, Postgres>,
    record: &RawHadithRecord,
) -> Result<Option<i64>, ImportError> {
    let id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM hadiths WHERE arabic_urn = $1 AND english_urn = $2",
    )
    .bind(record.arabic_urn)
    .bind(record.english_urn)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(id)
}

async fn insert_record(
    tx: &mut Transaction<'_, Postgres>,
    record: &RawHadithRecord,
    collection_id: i64,
) -> Result<i64, ImportError> {
    let parsed = parse_isnad(
        validated_arabic_text(record),
        record.english_text.as_deref(),
    );

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

/// The established title of each work, keyed by the source dump's collection
/// key.
///
/// These are the names the collections are published under, not anything
/// invented: a guess at the title of a religious work would be worse than
/// showing the raw key. A collection missing from this list keeps its key as
/// its name, which is honest rather than wrong.
///
/// Mirrored by migration 0005, which fixes databases whose collections were
/// created before this existed. The two lists must agree.
const COLLECTION_NAMES: &[(&str, &str)] = &[
    ("bukhari", "Sahih al-Bukhari"),
    ("muslim", "Sahih Muslim"),
    ("nasai", "Sunan an-Nasa'i"),
    ("abudawud", "Sunan Abi Dawud"),
    ("tirmidhi", "Jami` at-Tirmidhi"),
    ("ibnmajah", "Sunan Ibn Majah"),
    ("ahmad", "Musnad Ahmad"),
    ("adab", "Al-Adab Al-Mufrad"),
    ("shamail", "Ash-Shama'il Al-Muhammadiyah"),
    ("bulugh", "Bulugh al-Maram"),
    ("mishkat", "Mishkat al-Masabih"),
    ("riyadussalihin", "Riyad as-Salihin"),
    ("hisn", "Hisn al-Muslim"),
    ("forty", "40 Hadith an-Nawawi"),
    ("virtues", "Virtues of the Qur'an"),
];

/// The published title for `slug`, or `None` where the work is not one we
/// have a title for.
fn collection_name(slug: &str) -> Option<&'static str> {
    COLLECTION_NAMES
        .iter()
        .find(|(key, _)| *key == slug)
        .map(|(_, name)| *name)
}

/// Summary of a metadata-only run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataSummary {
    pub collections_seen: usize,
    pub collections_renamed: usize,
}

/// Brings collection titles in line with [`COLLECTION_NAMES`] without touching
/// any narration.
///
/// Titles are source metadata that changes far more often than the corpus, and
/// re-importing 44,896 records to correct a name is absurd. Only collections
/// already present are updated: a collection with no narrations in it would be
/// an empty shelf.
pub async fn sync_collection_metadata(database_url: &str) -> Result<MetadataSummary, ImportError> {
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(database_url)
        .await?;

    let mut summary = MetadataSummary::default();

    for (slug, name) in COLLECTION_NAMES {
        let updated = sqlx::query(
            r#"
            UPDATE collections
            SET name = $2, updated_at = now()
            WHERE slug = $1 AND name IS DISTINCT FROM $2
            "#,
        )
        .bind(slug)
        .bind(name)
        .execute(&pool)
        .await?
        .rows_affected();

        summary.collections_seen += 1;
        summary.collections_renamed += updated as usize;
    }

    Ok(summary)
}

async fn upsert_collection(
    tx: &mut Transaction<'_, Postgres>,
    collection: &str,
) -> Result<i64, ImportError> {
    // The name is set here rather than left to a migration: on a fresh
    // database the migrations run before any collection exists, so a migration
    // that fills in names has nothing to update and the import then creates
    // them holding the raw key.
    //
    // COLLECTION_NAMES is the source of truth, so a title we know is written on
    // every import. A collection we have no title for keeps whatever name it
    // has, since overwriting it with the raw key would be a downgrade.
    let known = collection_name(collection);
    let id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO collections (slug, name)
        VALUES ($1, $2)
        ON CONFLICT (slug) DO UPDATE
        SET name = CASE WHEN $3 THEN EXCLUDED.name ELSE collections.name END,
            updated_at = now()
        RETURNING id
        "#,
    )
    .bind(collection)
    .bind(known.unwrap_or(collection))
    .bind(known.is_some())
    .fetch_one(&mut **tx)
    .await?;

    Ok(id)
}

fn validate_record(index: usize, record: &RawHadithRecord) -> Result<(), ImportError> {
    require_non_empty(index, "collection", &record.collection)?;
    require_non_empty(index, "bookNumber", &record.book_number)?;

    if record.hadith_number.trim().is_empty() && record.our_hadith_number <= 0 {
        return invalid_record(
            index,
            "hadithNumber is required when ourHadithNumber is not greater than 0",
        );
    }

    if record.arabic_urn <= 0 {
        return invalid_record(index, "arabicURN must be greater than 0");
    }

    if record.english_urn <= 0 {
        return invalid_record(index, "englishURN must be greater than 0");
    }

    if !non_empty(record.arabic_text.as_deref()) {
        return invalid_record(index, "arabicText is required");
    }

    Ok(())
}

fn require_non_empty(index: usize, field: &str, value: &str) -> Result<(), ImportError> {
    if value.trim().is_empty() {
        return invalid_record(index, format!("{field} is required"));
    }

    Ok(())
}

fn invalid_record(index: usize, message: impl Into<String>) -> Result<(), ImportError> {
    Err(ImportError::InvalidRecord {
        index,
        message: message.into(),
    })
}

fn non_empty(value: Option<&str>) -> bool {
    value.is_some_and(|text| !text.trim().is_empty())
}

fn trim_optional(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn canonical_hadith_number(record: &RawHadithRecord) -> String {
    let source_hadith_number = record.hadith_number.trim();
    if source_hadith_number.is_empty() {
        record.our_hadith_number.to_string()
    } else {
        source_hadith_number.to_owned()
    }
}

fn validated_arabic_text(record: &RawHadithRecord) -> &str {
    record
        .arabic_text
        .as_deref()
        .expect("validated Arabic text is present")
}

fn arabic_transliteration(clean_arabic_text: &str) -> String {
    transliterate(clean_arabic_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_required_arabic_text() {
        let dump = HadithJsonDump {
            hadith_table: vec![record_with_numbers("1", 1, Some(" "))],
        };

        let error = validate_dump(&dump).expect_err("empty Arabic text should fail validation");

        assert!(matches!(
            error,
            ImportError::InvalidRecord {
                index: 0,
                message
            } if message == "arabicText is required"
        ));
    }

    #[test]
    fn falls_back_to_our_hadith_number_when_source_hadith_number_is_blank() {
        let record = record_with_numbers(" ", 234, Some("text"));

        assert_eq!(canonical_hadith_number(&record), "234");
    }

    #[test]
    fn accepts_zero_our_hadith_number_when_source_hadith_number_is_present() {
        let dump = HadithJsonDump {
            hadith_table: vec![record_with_numbers("1a", 0, Some("text"))],
        };

        validate_dump(&dump).expect("source hadithNumber should be enough for canonical numbering");
    }

    #[test]
    fn generates_transliteration_from_arabic_text_for_import() {
        let record = record_with_numbers("1", 1, Some("إِنَّمَا"));

        assert_eq!(
            arabic_transliteration(validated_arabic_text(&record)),
            "'innamaa"
        );
    }

    #[test]
    fn arabic_transliteration_now_takes_clean_text_directly() {
        assert_eq!(arabic_transliteration("إِنَّمَا"), "'innamaa");
    }

    fn record_with_numbers(
        hadith_number: &str,
        our_hadith_number: i32,
        arabic_text: Option<&str>,
    ) -> RawHadithRecord {
        RawHadithRecord {
            collection: "bukhari".to_owned(),
            book_number: "1".to_owned(),
            bab_id: 1.0,
            english_bab_number: Some("1".to_owned()),
            arabic_bab_number: Some("1".to_owned()),
            hadith_number: hadith_number.to_owned(),
            our_hadith_number,
            arabic_urn: 100010,
            arabic_bab_name: Some("باب".to_owned()),
            arabic_text: arabic_text.map(str::to_owned),
            arabicgrade1: "صحيح".to_owned(),
            english_urn: 10,
            english_bab_name: Some("Chapter".to_owned()),
            english_text: Some("Translation".to_owned()),
            englishgrade1: "Sahih".to_owned(),
            last_updated: Some("2021-03-04 23:36:31".to_owned()),
            xrefs: String::new(),
        }
    }
}
