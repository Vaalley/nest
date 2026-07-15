//! Local application configuration for the Bird client.
//!
//! Non-sensitive values (Nest URL, username, Bird id/name) are stored in a JSON
//! file under the OS config directory. Tokens are stored in the OS credential
//! manager via [`crate::storage`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{BirdError, BirdResult};

const APP_NAME: &str = "nest-bird";
const CONFIG_FILE: &str = "config.json";

/// User-facing application configuration persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Base URL of the Nest server, e.g. `http://127.0.0.1:8140`.
    #[serde(default = "default_nest_url")]
    pub nest_url: String,
    /// Human-readable name for this Bird device.
    #[serde(default = "default_bird_name")]
    pub bird_name: String,
    /// Platform string reported during device registration.
    #[serde(default = "default_platform")]
    pub platform: String,
    /// Username of the authenticated Flock.
    pub flock_username: Option<String>,
    /// Id of the Bird device once it has been registered with the Nest.
    pub bird_id: Option<Uuid>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            nest_url: default_nest_url(),
            bird_name: default_bird_name(),
            platform: default_platform(),
            flock_username: None,
            bird_id: None,
        }
    }
}

fn default_nest_url() -> String {
    "http://127.0.0.1:8140".to_string()
}

fn default_bird_name() -> String {
    let from_env = || {
        if cfg!(target_os = "windows") {
            std::env::var("COMPUTERNAME").ok()
        } else {
            std::env::var("HOSTNAME").ok()
        }
    };
    from_env()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown Bird".to_string())
}

fn default_platform() -> String {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "other"
    }
    .to_string()
}

/// Manages loading and saving [`AppConfig`].
#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    /// Open the default config store, creating the config directory if needed.
    pub fn new() -> BirdResult<Self> {
        let config_dir = Self::ensure_config_dir()?;
        Ok(Self {
            path: config_dir.join(CONFIG_FILE),
        })
    }

    /// Config directory used by this store.
    pub fn dir() -> BirdResult<PathBuf> {
        Self::ensure_config_dir()
    }

    fn ensure_config_dir() -> BirdResult<PathBuf> {
        let dir = dirs::config_dir()
            .ok_or_else(|| BirdError::Config("could not determine config directory".to_string()))?
            .join(APP_NAME);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Load the config file, falling back to defaults on first run.
    pub fn load(&self) -> BirdResult<AppConfig> {
        if !self.path.exists() {
            return Ok(AppConfig::default());
        }
        let contents = std::fs::read_to_string(&self.path)?;
        let config: AppConfig = serde_json::from_str(&contents)?;
        Ok(config)
    }

    /// Persist the given config to disk.
    pub fn save(&self, config: &AppConfig) -> BirdResult<()> {
        let contents = serde_json::to_string_pretty(config)?;
        std::fs::write(&self.path, contents)?;
        Ok(())
    }

    /// Path to the on-disk config file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}
