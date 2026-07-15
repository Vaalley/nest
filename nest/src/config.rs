//! Runtime configuration for the Nest server.
//!
//! Configuration is loaded from environment variables (optionally sourced from
//! a `.env`-style file by the caller). All values have sensible defaults so the
//! server can boot with zero configuration for local development.

use std::net::SocketAddr;
use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use nest_shared::domain::DEFAULT_BROOD_LIMIT;

/// Environment variable names, centralised to avoid typos.
mod env_keys {
    pub const BIND_ADDR: &str = "NEST_BIND_ADDR";
    pub const DATA_DIR: &str = "NEST_DATA_DIR";
    pub const DB_PATH: &str = "NEST_DB_PATH";
    pub const BROOD_LIMIT: &str = "NEST_BROOD_LIMIT";
    pub const TOKEN_SECRET: &str = "NEST_TOKEN_SECRET";
    pub const TOKEN_EXPIRY_SECONDS: &str = "NEST_TOKEN_EXPIRY_SECONDS";
    pub const LOG_LEVEL: &str = "NEST_LOG";
}

/// Fully-resolved server configuration.
///
/// `Debug` is implemented manually so `token_secret` is never printed.
#[derive(Clone)]
pub struct Config {
    /// Address the HTTP server binds to.
    pub bind_addr: SocketAddr,
    /// Root directory for stored Eggs (`{data_dir}/flocks/...`).
    pub data_dir: PathBuf,
    /// Path to the SQLite database file.
    pub db_path: PathBuf,
    /// Default Brood Limit applied to newly created Clutches.
    pub default_brood_limit: i64,
    /// Secret used to sign/verify auth tokens (consumed in Phase 2).
    pub token_secret: String,
    /// Token lifetime in seconds (default 7 days).
    pub token_expiry_seconds: u64,
    /// Tracing filter directive (e.g. `info`, `nest=debug`).
    pub log_level: String,
}

impl Config {
    /// Load configuration from the process environment, applying defaults.
    pub fn from_env() -> AppResult<Self> {
        let bind_addr = env_or(env_keys::BIND_ADDR, "127.0.0.1:8140")
            .parse::<SocketAddr>()
            .map_err(|e| AppError::Config(format!("invalid {}: {e}", env_keys::BIND_ADDR)))?;

        let data_dir = PathBuf::from(env_or(env_keys::DATA_DIR, "data"));

        // Default the DB inside the data dir unless explicitly overridden.
        let db_path = match std::env::var(env_keys::DB_PATH) {
            Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
            _ => data_dir.join("nest.sqlite"),
        };

        let default_brood_limit = match std::env::var(env_keys::BROOD_LIMIT) {
            Ok(v) if !v.trim().is_empty() => v
                .trim()
                .parse::<i64>()
                .map_err(|e| AppError::Config(format!("invalid {}: {e}", env_keys::BROOD_LIMIT)))?,
            _ => DEFAULT_BROOD_LIMIT,
        };
        if default_brood_limit < 1 {
            return Err(AppError::Config(format!(
                "{} must be >= 1",
                env_keys::BROOD_LIMIT
            )));
        }

        let token_secret = env_or(env_keys::TOKEN_SECRET, "dev-insecure-secret-change-me");

        let token_expiry_seconds = match std::env::var(env_keys::TOKEN_EXPIRY_SECONDS) {
            Ok(v) if !v.trim().is_empty() => v.trim().parse::<u64>().map_err(|e| {
                AppError::Config(format!("invalid {}: {e}", env_keys::TOKEN_EXPIRY_SECONDS))
            })?,
            _ => 7 * 24 * 60 * 60,
        };

        let log_level = env_or(env_keys::LOG_LEVEL, "info");

        Ok(Self {
            bind_addr,
            data_dir,
            db_path,
            default_brood_limit,
            token_secret,
            token_expiry_seconds,
            log_level,
        })
    }

    /// SQLite connection URL for this configuration.
    ///
    /// Uses the `sqlite:` scheme with `mode=rwc` so the file is created if it
    /// does not yet exist.
    pub fn database_url(&self) -> String {
        // Normalise Windows backslashes for the URL form.
        let p = self.db_path.to_string_lossy().replace('\\', "/");
        format!("sqlite://{p}?mode=rwc")
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("bind_addr", &self.bind_addr)
            .field("data_dir", &self.data_dir)
            .field("db_path", &self.db_path)
            .field("default_brood_limit", &self.default_brood_limit)
            .field("token_secret", &"<redacted>")
            .field("token_expiry_seconds", &self.token_expiry_seconds)
            .field("log_level", &self.log_level)
            .finish()
    }
}

/// Read an env var, falling back to `default` when unset or empty.
fn env_or(key: &str, default: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}
