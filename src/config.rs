use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub server_addr: SocketAddr,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("invalid SERVER_HOST `{value}`: {source}")]
    InvalidHost {
        value: String,
        source: std::net::AddrParseError,
    },
    #[error("invalid SERVER_PORT `{value}`: {source}")]
    InvalidPort {
        value: String,
        source: std::num::ParseIntError,
    },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("DATABASE_URL").map_err(|_| ConfigError::MissingDatabaseUrl)?;

        let host = env::var("SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
        let host = host
            .parse::<IpAddr>()
            .map_err(|source| ConfigError::InvalidHost {
                value: host,
                source,
            })?;

        let port = env::var("SERVER_PORT").unwrap_or_else(|_| "3000".to_owned());
        let port = port
            .parse::<u16>()
            .map_err(|source| ConfigError::InvalidPort {
                value: port,
                source,
            })?;

        Ok(Self {
            database_url,
            server_addr: SocketAddr::new(host, port),
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            server_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3000),
        }
    }
}
