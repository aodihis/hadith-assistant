use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;

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

    for record in &dump.hadith_table {
        insert_record(&mut tx, record).await?;
    }

    tx.commit().await?;

    Ok(ImportSummary {
        record_count: dump.hadith_table.len(),
        source_checksum: source_checksum.to_owned(),
    })
}

async fn insert_record(
    tx: &mut Transaction<'_, Postgres>,
    record: &RawHadithRecord,
) -> Result<(), ImportError> {
    let collection_id = upsert_collection(tx, record.collection.trim()).await?;

    sqlx::query(
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
            arabic_grade,
            english_urn,
            english_bab_name,
            english_text,
            english_grade,
            last_updated,
            xrefs
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
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
    .bind(
        record
            .arabic_text
            .as_deref()
            .expect("validated Arabic text is present"),
    )
    .bind(record.arabicgrade1.trim())
    .bind(record.english_urn)
    .bind(trim_optional(record.english_bab_name.as_deref()))
    .bind(trim_optional(record.english_text.as_deref()))
    .bind(record.englishgrade1.trim())
    .bind(trim_optional(record.last_updated.as_deref()))
    .bind(record.xrefs.trim())
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn upsert_collection(
    tx: &mut Transaction<'_, Postgres>,
    collection: &str,
) -> Result<i64, ImportError> {
    let id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO collections (slug, name)
        VALUES ($1, $1)
        ON CONFLICT (slug) DO UPDATE
        SET updated_at = now()
        RETURNING id
        "#,
    )
    .bind(collection)
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
