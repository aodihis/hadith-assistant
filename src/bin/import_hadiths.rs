use std::env;
use std::process::ExitCode;

use hadith_assistant::config::{EmbeddingConfig, VectorConfig};
use hadith_assistant::infrastructure::embedding::OpenAiEmbedder;
use hadith_assistant::infrastructure::persistence::hadiths::HadithRepository;
use hadith_assistant::infrastructure::vector::QdrantVectorStore;
use hadith_assistant::ingestion::embedding::embed_hadiths;
use hadith_assistant::ingestion::hadith_json::{
    ImportOptions, import_hadith_json, load_dump, validate_dump,
};
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

async fn run_embedding(database_url: &str, hadith_ids: &[i64]) -> Result<usize, String> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(|error| error.to_string())?;

    let repository = HadithRepository::new(pool);
    let hadiths = repository
        .find_by_ids(hadith_ids)
        .await
        .map_err(|error| error.to_string())?;

    let embedder = OpenAiEmbedder::new(EmbeddingConfig::from_env());
    let vector_config = VectorConfig::from_env();
    let vector_store =
        QdrantVectorStore::new(&vector_config.qdrant_url, vector_config.qdrant_collection)
            .map_err(|error| error.to_string())?;

    embed_hadiths(&embedder, &vector_store, &hadiths)
        .await
        .map_err(|error| error.to_string())
}

#[derive(Debug)]
struct Args {
    json_path: String,
    database_url: Option<String>,
    validate_only: bool,
    embed: bool,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut json_path = None;
        let mut database_url = None;
        let mut validate_only = false;
        let mut embed = false;

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
}

fn require_value(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} requires a value"))
}

fn usage() -> String {
    "usage: import_hadiths <json-path> [--database-url <url>] [--validate-only] [--embed]"
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
        assert_eq!(args.json_path, "data/imports/hadiths.json");
    }

    #[test]
    fn parse_defaults_embed_to_false() {
        let args = Args::parse(["data/imports/hadiths.json".to_owned()])
            .expect("valid arguments should parse");

        assert!(!args.embed);
    }
}
