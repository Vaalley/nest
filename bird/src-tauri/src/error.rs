//! Central error type for the Bird client.
//!
//! Errors are surfaced to the frontend as JSON-style strings. Internals are
//! logged but never leaked to the UI unless explicitly user-facing.

use std::path::PathBuf;

use thiserror::Error;

/// Fallible result alias used throughout the Bird crate.
pub type BirdResult<T> = Result<T, BirdError>;

/// The single error type used by the Bird client.
#[derive(Debug, Error)]
pub enum BirdError {
    #[error("invalid request: {0}")]
    Validation(String),

    #[error("network request failed: {0}")]
    Network(#[from] reqwest::Error),

    #[error("invalid response from Nest: {0}")]
    Api(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("secure storage error: {0}")]
    SecureStorage(String),

    #[error("Nest returned error {status}: {message}")]
    Nest { status: u16, message: String },

    #[error("not authenticated")]
    NotAuthenticated,

    #[error("this device has not been registered as a Bird yet")]
    NotRegistered,

    #[error("game not found: {0}")]
    GameNotFound(String),

    #[error("save path does not exist: {0}")]
    SavePathNotFound(PathBuf),

    #[error("internal error: {0}")]
    Internal(String),
}

impl BirdError {
    /// A user-facing message that is safe to display in the UI.
    pub fn user_message(&self) -> String {
        match self {
            BirdError::Validation(msg) => msg.clone(),
            BirdError::Nest { message, .. } => message.clone(),
            BirdError::NotAuthenticated => "Please sign in first.".to_string(),
            BirdError::NotRegistered => "Please register this device first.".to_string(),
            BirdError::SavePathNotFound(_) => {
                "The save folder for this game was not found.".to_string()
            }
            BirdError::GameNotFound(id) => format!("Unknown game: {id}"),
            other => other.to_string(),
        }
    }

    /// Stable error code for the frontend to branch on.
    pub fn code(&self) -> &'static str {
        match self {
            BirdError::Validation(_) => "validation_error",
            BirdError::Network(_) => "network_error",
            BirdError::Api(_) => "api_error",
            BirdError::Serde(_) => "serde_error",
            BirdError::Io(_) => "io_error",
            BirdError::Config(_) => "config_error",
            BirdError::SecureStorage(_) => "secure_storage_error",
            BirdError::Nest { .. } => "nest_error",
            BirdError::NotAuthenticated => "not_authenticated",
            BirdError::NotRegistered => "not_registered",
            BirdError::GameNotFound(_) => "game_not_found",
            BirdError::SavePathNotFound(_) => "save_path_not_found",
            BirdError::Internal(_) => "internal_error",
        }
    }
}

impl From<keyring::Error> for BirdError {
    fn from(err: keyring::Error) -> Self {
        BirdError::SecureStorage(err.to_string())
    }
}

impl From<walkdir::Error> for BirdError {
    fn from(err: walkdir::Error) -> Self {
        let msg = err.to_string();
        let io = err
            .into_io_error()
            .unwrap_or_else(|| std::io::Error::other(msg));
        BirdError::Io(io)
    }
}
