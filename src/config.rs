use std::env;

use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub database_max_connections: u32,
    pub vector: VectorConfig,
    pub embedding: EmbeddingConfig,
}

#[derive(Debug, Clone)]
pub struct VectorConfig {
    pub provider: String,
    pub qdrant_url: String,
    pub qdrant_collection: String,
}

#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
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
            embedding: EmbeddingConfig::from_env(),
        })
    }
}

impl VectorConfig {
    pub fn from_env() -> Self {
        Self {
            provider: env::var("VECTOR_DB_PROVIDER").unwrap_or_else(|_| "qdrant".to_owned()),
            qdrant_url: env::var("QDRANT_URL")
                .unwrap_or_else(|_| "http://localhost:6333".to_owned()),
            qdrant_collection: env::var("QDRANT_COLLECTION")
                .unwrap_or_else(|_| "hadith_vectors".to_owned()),
        }
    }
}

impl EmbeddingConfig {
    pub fn from_env() -> Self {
        Self {
            base_url: env::var("EMBEDDING_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_owned()),
            api_key: env::var("EMBEDDING_API_KEY").ok(),
            model: env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| "text-embedding-3-small".to_owned()),
        }
    }
}

impl Default for VectorConfig {
    fn default() -> Self {
        Self {
            provider: "qdrant".to_owned(),
            qdrant_url: "http://localhost:6333".to_owned(),
            qdrant_collection: "hadith_vectors".to_owned(),
        }
    }
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key: None,
            model: "text-embedding-3-small".to_owned(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            database_max_connections: 10,
            vector: VectorConfig::default(),
            embedding: EmbeddingConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_config_default_points_at_openai() {
        let config = EmbeddingConfig::default();

        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert_eq!(config.api_key, None);
        assert_eq!(config.model, "text-embedding-3-small");
    }

    #[test]
    fn vector_config_default_points_at_local_qdrant() {
        let config = VectorConfig::default();

        assert_eq!(config.provider, "qdrant");
        assert_eq!(config.qdrant_url, "http://localhost:6333");
        assert_eq!(config.qdrant_collection, "hadith_vectors");
    }
}
