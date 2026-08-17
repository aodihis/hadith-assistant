use std::env;

use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub database_max_connections: u32,
    pub vector: VectorConfig,
    pub embedding: EmbeddingConfig,
    pub chat: ChatConfig,
}

#[derive(Debug, Clone)]
pub struct VectorConfig {
    pub provider: String,
    pub qdrant_url: String,
    pub qdrant_collection: String,
    /// Minimum cosine score a match must reach to be treated as relevant.
    ///
    /// Measured against the current index, natural-language questions score
    /// roughly 0.40-0.55 and a hadith quoted back verbatim peaks near 0.66, so
    /// a threshold at or above 0.7 discards everything. Tune with
    /// RETRIEVAL_MIN_SCORE.
    pub min_score: f64,
}

#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct ChatConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    /// Decoding profile for grounded answers.
    pub temperature: f32,
    pub max_tokens: u32,
    /// Decoding profile for history compaction. Deliberately colder than the
    /// answer profile — drift in a summary is a correctness risk, not a style
    /// one, because the next turn is conditioned on it.
    pub summary_temperature: f32,
    pub summary_max_tokens: u32,
    /// Compaction policy. Compaction triggers once history exceeds
    /// `history_max_turns`, folding everything but the newest
    /// `history_keep_turns` into the summary. Keeping roughly half means the
    /// extra summarizer call runs about once every `keep` turns rather than on
    /// every turn.
    pub history_max_turns: usize,
    pub history_keep_turns: usize,
    pub history_max_chars: usize,
    pub max_question_chars: usize,
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
    #[error("invalid {name} `{value}`: {source}")]
    InvalidChatFloat {
        name: &'static str,
        value: String,
        source: std::num::ParseFloatError,
    },
    #[error("invalid {name} `{value}`: {source}")]
    InvalidChatInteger {
        name: &'static str,
        value: String,
        source: std::num::ParseIntError,
    },
    #[error("{name} must be between {min} and {max}, got {value}")]
    ChatValueOutOfRange {
        name: &'static str,
        value: String,
        min: String,
        max: String,
    },
    #[error("invalid chat history policy: {message}")]
    InvalidChatHistoryPolicy { message: String },
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
            chat: ChatConfig::from_env()?,
        })
    }
}

impl VectorConfig {
    pub fn from_env() -> Self {
        Self {
            provider: env::var("VECTOR_DB_PROVIDER").unwrap_or_else(|_| "qdrant".to_owned()),
            qdrant_url: env::var("QDRANT_URL")
                .unwrap_or_else(|_| "http://localhost:6334".to_owned()),
            qdrant_collection: env::var("QDRANT_COLLECTION")
                .unwrap_or_else(|_| "hadith_vectors".to_owned()),
            min_score: env::var("RETRIEVAL_MIN_SCORE")
                .ok()
                .and_then(|raw| raw.trim().parse::<f64>().ok())
                .unwrap_or(0.7),
        }
    }
}

impl EmbeddingConfig {
    pub fn from_env() -> Self {
        Self {
            base_url: env::var("EMBEDDING_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_owned()),
            api_key: env::var("OPEN_ROUTER_API_KEY").ok(),
            model: env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| "text-embedding-3-small".to_owned()),
        }
    }
}

impl ChatConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let temperature = float_var("CHAT_TEMPERATURE", 0.3, 0.0, 0.7)?;
        let summary_temperature = float_var("CHAT_SUMMARY_TEMPERATURE", 0.1, 0.0, 0.7)?;
        let max_tokens = int_var::<u32>("CHAT_MAX_TOKENS", 700, 64, 1200)?;
        let summary_max_tokens = int_var::<u32>("CHAT_SUMMARY_MAX_TOKENS", 300, 64, 1200)?;
        let history_max_turns = int_var::<usize>("CHAT_HISTORY_MAX_TURNS", 8, 1, 50)?;
        let history_keep_turns = int_var::<usize>("CHAT_HISTORY_KEEP_TURNS", 4, 1, 50)?;
        let history_max_chars = int_var::<usize>("CHAT_HISTORY_MAX_CHARS", 6_000, 500, 100_000)?;
        let max_question_chars = int_var::<usize>("CHAT_MAX_QUESTION_CHARS", 1_000, 50, 10_000)?;

        validate_history_policy(history_keep_turns, history_max_turns)?;

        Ok(Self {
            base_url: env::var("CHAT_BASE_URL")
                .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_owned()),
            api_key: env::var("OPEN_ROUTER_API_KEY").ok(),
            model: env::var("CHAT_MODEL")
                .unwrap_or_else(|_| "deepseek/deepseek-v4-flash".to_owned()),
            temperature,
            max_tokens,
            summary_temperature,
            summary_max_tokens,
            history_max_turns,
            history_keep_turns,
            history_max_chars,
            max_question_chars,
        })
    }
}

/// Compaction must reduce the history, otherwise it would fire on every single
/// turn and pay for a summarizer call each time.
fn validate_history_policy(keep: usize, max: usize) -> Result<(), ConfigError> {
    if keep >= max {
        return Err(ConfigError::InvalidChatHistoryPolicy {
            message: format!(
                "CHAT_HISTORY_KEEP_TURNS ({keep}) must be less than \
                 CHAT_HISTORY_MAX_TURNS ({max}), otherwise compaction would run \
                 on every turn"
            ),
        });
    }

    Ok(())
}

fn float_var(name: &'static str, default: f32, min: f32, max: f32) -> Result<f32, ConfigError> {
    parse_float(name, env::var(name).ok().as_deref(), default, min, max)
}

fn parse_float(
    name: &'static str,
    raw: Option<&str>,
    default: f32,
    min: f32,
    max: f32,
) -> Result<f32, ConfigError> {
    let Some(raw) = raw else {
        return Ok(default);
    };

    let value = raw
        .trim()
        .parse::<f32>()
        .map_err(|source| ConfigError::InvalidChatFloat {
            name,
            value: raw.to_owned(),
            source,
        })?;

    if !(min..=max).contains(&value) {
        return Err(ConfigError::ChatValueOutOfRange {
            name,
            value: value.to_string(),
            min: min.to_string(),
            max: max.to_string(),
        });
    }

    Ok(value)
}

fn int_var<T>(name: &'static str, default: T, min: T, max: T) -> Result<T, ConfigError>
where
    T: std::str::FromStr<Err = std::num::ParseIntError> + PartialOrd + std::fmt::Display + Copy,
{
    parse_int(name, env::var(name).ok().as_deref(), default, min, max)
}

fn parse_int<T>(
    name: &'static str,
    raw: Option<&str>,
    default: T,
    min: T,
    max: T,
) -> Result<T, ConfigError>
where
    T: std::str::FromStr<Err = std::num::ParseIntError> + PartialOrd + std::fmt::Display + Copy,
{
    let Some(raw) = raw else {
        return Ok(default);
    };

    let value = raw
        .trim()
        .parse::<T>()
        .map_err(|source| ConfigError::InvalidChatInteger {
            name,
            value: raw.to_owned(),
            source,
        })?;

    if value < min || value > max {
        return Err(ConfigError::ChatValueOutOfRange {
            name,
            value: value.to_string(),
            min: min.to_string(),
            max: max.to_string(),
        });
    }

    Ok(value)
}

impl Default for VectorConfig {
    fn default() -> Self {
        Self {
            provider: "qdrant".to_owned(),
            qdrant_url: "http://localhost:6334".to_owned(),
            qdrant_collection: "hadith_vectors".to_owned(),
            min_score: 0.7,
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

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            base_url: "https://openrouter.ai/api/v1".to_owned(),
            api_key: None,
            model: "deepseek/deepseek-v4-flash".to_owned(),
            temperature: 0.3,
            max_tokens: 700,
            summary_temperature: 0.1,
            summary_max_tokens: 300,
            history_max_turns: 8,
            history_keep_turns: 4,
            history_max_chars: 6_000,
            max_question_chars: 1_000,
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
            chat: ChatConfig::default(),
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
        assert_eq!(config.qdrant_url, "http://localhost:6334");
        assert_eq!(config.qdrant_collection, "hadith_vectors");
    }

    #[test]
    fn chat_config_default_points_at_openrouter_with_deepseek_flash() {
        let config = ChatConfig::default();

        assert_eq!(config.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(config.api_key, None);
        assert_eq!(config.model, "deepseek/deepseek-v4-flash");
    }

    #[test]
    fn embedding_config_reads_open_router_api_key() {
        // SAFETY: test runs single-threaded within this process's env; no
        // other test reads OPEN_ROUTER_API_KEY concurrently.
        unsafe {
            std::env::set_var("OPEN_ROUTER_API_KEY", "test-shared-key");
        }
        let config = EmbeddingConfig::from_env();
        unsafe {
            std::env::remove_var("OPEN_ROUTER_API_KEY");
        }

        assert_eq!(config.api_key.as_deref(), Some("test-shared-key"));
    }

    #[test]
    fn chat_config_default_uses_a_restrained_generation_profile() {
        let config = ChatConfig::default();

        assert_eq!(config.temperature, 0.3);
        assert_eq!(config.max_tokens, 700);
        // The summarizer runs colder than the answer profile: a drifting
        // summary silently corrupts every later turn.
        assert!(config.summary_temperature < config.temperature);
        assert!(config.history_keep_turns < config.history_max_turns);
    }

    #[test]
    fn unset_chat_values_fall_back_to_their_defaults() {
        assert_eq!(
            parse_float("CHAT_TEMPERATURE", None, 0.3, 0.0, 0.7).expect("unset is valid"),
            0.3
        );
        assert_eq!(
            parse_int("CHAT_MAX_TOKENS", None, 700, 64, 1200).expect("unset is valid"),
            700
        );
    }

    #[test]
    fn unparseable_chat_values_are_rejected_rather_than_silently_defaulted() {
        let error = parse_float("CHAT_TEMPERATURE", Some("hot"), 0.3, 0.0, 0.7)
            .expect_err("a non-numeric temperature must fail startup");
        assert!(
            matches!(error, ConfigError::InvalidChatFloat { name, .. } if name == "CHAT_TEMPERATURE")
        );

        let error = parse_int("CHAT_MAX_TOKENS", Some("lots"), 700, 64, 1200)
            .expect_err("a non-numeric token budget must fail startup");
        assert!(
            matches!(error, ConfigError::InvalidChatInteger { name, .. } if name == "CHAT_MAX_TOKENS")
        );
    }

    #[test]
    fn out_of_range_chat_values_are_rejected() {
        // An operator asking for 1.5 believes they configured something we
        // would silently ignore — refuse instead.
        assert!(matches!(
            parse_float("CHAT_TEMPERATURE", Some("1.5"), 0.3, 0.0, 0.7),
            Err(ConfigError::ChatValueOutOfRange { .. })
        ));
        assert!(matches!(
            parse_int("CHAT_MAX_TOKENS", Some("50000"), 700, 64, 1200),
            Err(ConfigError::ChatValueOutOfRange { .. })
        ));
        assert!(matches!(
            parse_int("CHAT_MAX_TOKENS", Some("1"), 700, 64, 1200),
            Err(ConfigError::ChatValueOutOfRange { .. })
        ));
    }

    #[test]
    fn in_range_chat_values_are_accepted() {
        assert_eq!(
            parse_float("CHAT_TEMPERATURE", Some("0.15"), 0.3, 0.0, 0.7).expect("0.15 is in range"),
            0.15
        );
        assert_eq!(
            parse_int("CHAT_MAX_TOKENS", Some(" 512 "), 700, 64, 1200)
                .expect("512 is in range, surrounding whitespace trimmed"),
            512
        );
    }

    #[test]
    fn history_policy_rejects_a_keep_window_that_never_shrinks_the_history() {
        assert!(validate_history_policy(4, 8).is_ok());
        assert!(matches!(
            validate_history_policy(8, 8),
            Err(ConfigError::InvalidChatHistoryPolicy { .. })
        ));
        assert!(matches!(
            validate_history_policy(9, 8),
            Err(ConfigError::InvalidChatHistoryPolicy { .. })
        ));
    }
}
