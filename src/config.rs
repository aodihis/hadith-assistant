use std::env;

use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub database_max_connections: u32,
    pub vector: VectorConfig,
}

#[derive(Debug, Clone)]
pub struct VectorConfig {
    pub provider: String,
    pub qdrant_url: String,
    pub qdrant_collection: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("invalid DATABASE_MAX_CONNECTIONS `{value}`: {source}")]
    InvalidDatabaseMaxConnections {
        value: String,
        source: std::num::ParseIntError,
    },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("DATABASE_URL").map_err(|_| ConfigError::MissingDatabaseUrl)?;
        let database_max_connections =
            env::var("DATABASE_MAX_CONNECTIONS").unwrap_or_else(|_| "10".to_owned());
        let database_max_connections =
            database_max_connections.parse::<u32>().map_err(|source| {
                ConfigError::InvalidDatabaseMaxConnections {
                    value: database_max_connections,
                    source,
                }
            })?;

        Ok(Self {
            database_url,
            database_max_connections,
            vector: VectorConfig::from_env(),
        })
    }
}

impl VectorConfig {
    fn from_env() -> Self {
        Self {
            provider: env::var("VECTOR_DB_PROVIDER").unwrap_or_else(|_| "qdrant".to_owned()),
            qdrant_url: env::var("QDRANT_URL")
                .unwrap_or_else(|_| "http://localhost:6333".to_owned()),
            qdrant_collection: env::var("QDRANT_COLLECTION")
                .unwrap_or_else(|_| "hadith_vectors".to_owned()),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            database_max_connections: 10,
            vector: VectorConfig {
                provider: "qdrant".to_owned(),
                qdrant_url: "http://localhost:6333".to_owned(),
                qdrant_collection: "hadith_vectors".to_owned(),
            },
        }
    }
}
