use sanad::application::AppServices;
use sanad::config::Config;
use sanad::web;
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

    let router = web::router(AppServices::new(
        pool,
        config.embedding.clone(),
        config.vector.clone(),
        config.chat.clone(),
        session_config(),
    ))?;
    topcoat::start(router).await?;

    Ok(())
}

/// Builds the chat session policy.
///
/// `SESSION_SECRET` is optional on purpose: it holds no user data, so the only
/// consequence of leaving it unset is that tokens issued before a restart stop
/// being honoured. That is a visible, self-healing annoyance ("start a new
/// chat") rather than a security hole, so it warns instead of refusing to boot.
fn session_config() -> sanad::application::SessionConfig {
    use sanad::application::SessionConfig;

    let secret = match std::env::var("SESSION_SECRET") {
        Ok(secret) if !secret.trim().is_empty() => secret.into_bytes(),
        _ => {
            tracing::warn!("SESSION_SECRET is not set; chat sessions will not survive a restart");
            format!("ephemeral-{:?}", std::time::SystemTime::now()).into_bytes()
        }
    };

    SessionConfig {
        secret,
        ..SessionConfig::default()
    }
}
