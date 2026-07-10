use hadith_assistant::config::Config;
use hadith_assistant::routes;
use hadith_assistant::state::AppState;
use sqlx::postgres::PgPoolOptions;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = Config::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;

    let app = routes::router(AppState::new(pool)).layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(config.server_addr).await?;

    tracing::info!("listening on {}", config.server_addr);
    axum::serve(listener, app).await?;

    Ok(())
}
