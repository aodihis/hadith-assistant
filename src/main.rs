use hadith_assistant::app;
use hadith_assistant::application::AppServices;
use hadith_assistant::config::Config;
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = Config::from_env()?;
    tracing::info!(
        provider = %config.vector.provider,
        qdrant_url = %config.vector.qdrant_url,
        qdrant_collection = %config.vector.qdrant_collection,
        "vector backend configured"
    );

    let pool = PgPoolOptions::new()
        .max_connections(config.database_max_connections)
        .connect(&config.database_url)
        .await?;

    sqlx::migrate!().run(&pool).await?;
    tracing::info!("database migrations completed");

    let router = app::router(AppServices::new(
        pool,
        config.embedding.clone(),
        config.vector.clone(),
        config.chat.clone(),
    ))?;
    topcoat::start(router).await?;

    Ok(())
}
