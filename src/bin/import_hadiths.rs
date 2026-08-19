use std::env;
use std::process::ExitCode;

use hadith_assistant::config::{EmbeddingConfig, VectorConfig};
use hadith_assistant::domain::HadithSearch;
use hadith_assistant::infrastructure::embedding::OpenAiEmbedder;
use hadith_assistant::infrastructure::persistence::hadiths::HadithRepository;
use hadith_assistant::infrastructure::vector::{QdrantVectorStore, VectorStore};
use hadith_assistant::ingestion::embedding::embed_hadiths;
use hadith_assistant::ingestion::hadith_json::{
    ImportOptions, import_hadith_json, load_dump, validate_dump,
};
use hadith_assistant::ingestion::narrator_backfill::backfill_narrators;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

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

    if let Some(collection) = args.embed_collection.clone() {
        let embedded =
            run_collection_embedding(&database_url, &collection, args.limit, args.re_embed)
                .await
                .map_err(|error| error.to_string())?;
        let verb = if args.re_embed {
            "re-embedded"
        } else {
            "embedded"
        };
        println!("{verb} {embedded} records from collection `{collection}`");
        return Ok(());
    }

    let json_path = args
        .json_path
        .clone()
        .expect("json_path required when not backfilling, enforced by Args::parse");

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
        "imported {} new records, skipped {} already present ({})",
        summary.inserted_ids.len(),
        summary.skipped_ids.len(),
        summary.source_checksum
    );

    if args.embed {
        // Both sets are offered to the embedder: a skipped record may still be
        // missing from the vector index, and run_embedding filters to whatever
        // Qdrant does not already hold.
        let candidates: Vec<i64> = summary
            .inserted_ids
            .iter()
            .chain(summary.skipped_ids.iter())
            .copied()
            .collect();

        let embedded = run_embedding(&database_url, &candidates)
            .await
            .map_err(|error| error.to_string())?;
        println!("embedded {embedded} records");
    }

    Ok(())
}

async fn run_backfill_narrators(database_url: &str) -> Result<(), String> {
    let pool = PgPoolOptions::new()
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

/// Embeds the given records, skipping any that already have a point in Qdrant.
///
/// Checking the index first means a re-run costs nothing for work already done,
/// and closes gaps left by an interrupted run — a record can be in Postgres yet
/// missing from the index.
async fn run_embedding(database_url: &str, hadith_ids: &[i64]) -> Result<usize, String> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(|error| error.to_string())?;

    let repository = HadithRepository::new(pool);

    let embedder = OpenAiEmbedder::new(EmbeddingConfig::from_env());
    let vector_config = VectorConfig::from_env().map_err(|error| error.to_string())?;
    let vector_store =
        QdrantVectorStore::new(&vector_config.qdrant_url, vector_config.qdrant_collection)
            .map_err(|error| error.to_string())?;

    let already_indexed = vector_store
        .existing_ids(hadith_ids)
        .await
        .map_err(|error| error.to_string())?;

    let missing: Vec<i64> = hadith_ids
        .iter()
        .copied()
        .filter(|id| !already_indexed.contains(id))
        .collect();

    if !already_indexed.is_empty() {
        println!(
            "{} already in the vector index, embedding the remaining {}",
            already_indexed.len(),
            missing.len()
        );
    }

    if missing.is_empty() {
        return Ok(0);
    }

    let hadiths = repository
        .find_by_ids(&missing)
        .await
        .map_err(|error| error.to_string())?;

    embed_hadiths(&embedder, &vector_store, &hadiths)
        .await
        .map_err(|error| error.to_string())
}

/// Embeds records that are already in Postgres, one page at a time.
///
/// `--embed` only covers ids from the import that just ran, so there was no way
/// to build the vector index for a collection imported earlier. This reads
/// canonical rows and writes only to Qdrant — it never inserts, updates, or
/// deletes canonical data.
///
/// `re_embed` rebuilds vectors that already exist rather than skipping them,
/// which is what a change to the text pipeline calls for: the canonical rows
/// are unchanged, but the plain text derived from them is not. Embedding upserts
/// by record id, so a rebuilt vector replaces its predecessor in place.
async fn run_collection_embedding(
    database_url: &str,
    collection: &str,
    limit: Option<usize>,
    re_embed: bool,
) -> Result<usize, String> {
    const PAGE_SIZE: i64 = 200;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(|error| error.to_string())?;

    let repository = HadithRepository::new(pool);
    let embedder = OpenAiEmbedder::new(EmbeddingConfig::from_env());
    let vector_config = VectorConfig::from_env().map_err(|error| error.to_string())?;
    let vector_store =
        QdrantVectorStore::new(&vector_config.qdrant_url, vector_config.qdrant_collection)
            .map_err(|error| error.to_string())?;

    let mut embedded = 0;
    let mut seen = 0;
    let mut offset = 0;

    loop {
        if limit.is_some_and(|limit| seen >= limit) {
            break;
        }

        let mut page = repository
            .list(&HadithSearch {
                collection: Some(collection.to_owned()),
                book_number: None,
                hadith_number: None,
                grade: None,
                limit: PAGE_SIZE,
                offset,
            })
            .await
            .map_err(|error| error.to_string())?;

        if page.is_empty() {
            break;
        }

        let count = page.len();

        // `--limit` bounds how many canonical records are considered, so a
        // partial run is reproducible: the same slug and limit always cover the
        // same records.
        if let Some(limit) = limit {
            page.truncate(limit.saturating_sub(seen));
        }
        seen += page.len();

        // Asking Qdrant which ids it holds is pointless when every one of them
        // is going to be rewritten anyway, so a rebuild skips the round trip.
        let already_indexed = if re_embed {
            Default::default()
        } else {
            let ids: Vec<i64> = page.iter().map(|hadith| hadith.id).collect();
            vector_store
                .existing_ids(&ids)
                .await
                .map_err(|error| error.to_string())?
        };

        let pending: Vec<_> = page
            .into_iter()
            .filter(|hadith| !already_indexed.contains(&hadith.id))
            .collect();

        if !pending.is_empty() {
            embedded += embed_hadiths(&embedder, &vector_store, &pending)
                .await
                .map_err(|error| error.to_string())?;
            println!("embedded {embedded} records so far…");
        }

        if (count as i64) < PAGE_SIZE {
            break;
        }
        offset += PAGE_SIZE;
    }

    if seen == 0 {
        return Err(format!(
            "no hadiths found in collection `{collection}` — check the slug"
        ));
    }

    Ok(embedded)
}

#[derive(Debug)]
struct Args {
    json_path: Option<String>,
    database_url: Option<String>,
    validate_only: bool,
    embed: bool,
    embed_collection: Option<String>,
    limit: Option<usize>,
    re_embed: bool,
    backfill_narrators: bool,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut json_path = None;
        let mut database_url = None;
        let mut validate_only = false;
        let mut embed = false;
        let mut embed_collection = None;
        let mut limit = None;
        let mut re_embed = false;
        let mut backfill_narrators = false;

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
                "--embed-collection" => {
                    embed_collection = Some(require_value(&mut args, "--embed-collection")?);
                }
                "--limit" => {
                    let raw = require_value(&mut args, "--limit")?;
                    let value = raw
                        .parse::<usize>()
                        .map_err(|error| format!("invalid --limit `{raw}`: {error}"))?;
                    if value == 0 {
                        return Err("--limit must be greater than zero".to_owned());
                    }
                    limit = Some(value);
                }
                "--re-embed" => {
                    re_embed = true;
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
        } else if re_embed && embed_collection.is_none() {
            // Silently ignoring it would look like a rebuild had happened.
            return Err(format!(
                "--re-embed only applies to --embed-collection

{}",
                usage()
            ));
        } else if embed_collection.is_some() {
            if json_path.is_some() {
                return Err(format!(
                    "--embed-collection takes no <json-path>\n\n{}",
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
            embed_collection,
            limit,
            re_embed,
            backfill_narrators,
        })
    }
}

fn require_value(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} requires a value"))
}

fn usage() -> String {
    "usage: import_hadiths <json-path> [--database-url <url>] [--validate-only] [--embed]\n       import_hadiths --embed-collection <slug> [--database-url <url>] [--limit <n>] [--re-embed]\n       import_hadiths --backfill-narrators [--database-url <url>]"
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_recognizes_the_embed_flag() {
        let args = Args::parse(["data/imports/hadiths.json".to_owned(), "--embed".to_owned()])
            .expect("valid arguments should parse");

        assert!(args.embed);
        assert_eq!(args.json_path.as_deref(), Some("data/imports/hadiths.json"));
    }

    #[test]
    fn parse_recognizes_the_re_embed_flag_alongside_a_collection() {
        let args = Args::parse([
            "--embed-collection".to_owned(),
            "bukhari".to_owned(),
            "--re-embed".to_owned(),
        ])
        .expect("valid arguments should parse");

        assert!(args.re_embed);
        assert_eq!(args.embed_collection.as_deref(), Some("bukhari"));
    }

    #[test]
    fn re_embed_without_a_collection_is_rejected_rather_than_ignored() {
        let error = Args::parse(["--re-embed".to_owned()])
            .expect_err("--re-embed alone should not be accepted");

        assert!(error.contains("--re-embed only applies to --embed-collection"));
    }

    #[test]
    fn parse_defaults_embed_to_false() {
        let args = Args::parse(["data/imports/hadiths.json".to_owned()])
            .expect("valid arguments should parse");

        assert!(!args.embed);
    }

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
}
