use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use hadith_assistant::application::RetrievalService;
use hadith_assistant::config::EmbeddingConfig;
use hadith_assistant::domain::RetrievalQuery;
use hadith_assistant::infrastructure::embedding::OpenAiEmbedder;
use hadith_assistant::infrastructure::persistence::hadiths::HadithRepository;
use hadith_assistant::infrastructure::vector::QdrantVectorStore;
use hadith_assistant::ingestion::embedding::embed_hadiths;
use qdrant_client::Qdrant;
use sqlx::PgPool;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const QDRANT_URL: &str = "http://localhost:6334";

/// End-to-end proof that RetrievalService's three real components (Postgres,
/// Qdrant, and an OpenAI-compatible embedder) actually work together, not
/// just individually against fakes. Requires `docker compose up -d postgres
/// qdrant` and a `DATABASE_URL` pointing at that Postgres instance — run
/// with `cargo test --test retrieval_integration -- --ignored`.
#[ignore = "requires `docker compose up -d postgres qdrant`"]
#[sqlx::test]
async fn retrieve_resolves_a_seeded_hadith_through_real_postgres_and_qdrant(pool: PgPool) {
    let collection_name = format!("test-retrieval-{}", unique_suffix());
    let outcome = run(pool, collection_name.clone()).await;

    let client = Qdrant::from_url(QDRANT_URL)
        .build()
        .expect("qdrant client should build");
    let _ = client.delete_collection(collection_name).await;

    outcome.expect("integration test should pass");
}

async fn run(pool: PgPool, collection_name: String) -> Result<(), Box<dyn std::error::Error>> {
    let hadith_id = seed_hadith(&pool).await?;

    let repository = HadithRepository::new(pool);
    let seeded_hadith = repository.find_by_id(hadith_id).await?;

    let embedding_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{ "embedding": [1.0, 0.0, 0.0], "index": 0 }]
        })))
        .mount(&embedding_server)
        .await;

    let embedder = OpenAiEmbedder::new(EmbeddingConfig {
        base_url: embedding_server.uri(),
        api_key: None,
        model: "test-embedding-model".to_owned(),
    });
    let vector_store = QdrantVectorStore::new(QDRANT_URL, collection_name)?;

    embed_hadiths(
        &embedder,
        &vector_store,
        std::slice::from_ref(&seeded_hadith),
    )
    .await?;

    let service = RetrievalService::new(Arc::new(embedder), Arc::new(vector_store), repository);

    let result = service
        .retrieve(RetrievalQuery {
            query: "the reward of intentions".to_owned(),
            collection: None,
            limit: 5,
        })
        .await?;

    assert_eq!(
        result.results.len(),
        1,
        "expected exactly the seeded hadith back"
    );
    let resolved = &result.results[0];
    assert_eq!(resolved.hadith_id, hadith_id);
    assert_eq!(resolved.arabic_text, seeded_hadith.arabic_text);
    assert_eq!(resolved.collection, "test-collection");

    Ok(())
}

async fn seed_hadith(pool: &PgPool) -> Result<i64, sqlx::Error> {
    let collection_id: i64 =
        sqlx::query_scalar("INSERT INTO collections (slug, name) VALUES ($1, $1) RETURNING id")
            .bind("test-collection")
            .fetch_one(pool)
            .await?;

    let hadith_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO hadiths (
            collection_id, book_number, bab_id, hadith_number, our_hadith_number,
            arabic_urn, arabic_text, arabic_grade, english_urn, english_text, english_grade
        )
        VALUES ($1, '1', 1.0, '1', 1, 100001, $2, 'Sahih', 200001, $3, 'Sahih')
        RETURNING id
        "#,
    )
    .bind(collection_id)
    .bind("إنما الأعمال بالنيات")
    .bind("Actions are judged by intentions.")
    .fetch_one(pool)
    .await?;

    Ok(hadith_id)
}

fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after the unix epoch")
        .as_nanos();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);

    format!("{nanos}-{count}")
}
