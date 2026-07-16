//! Secure credential storage for the Bird client.
//!
//! Access tokens are stored in the OS credential manager when available
//! (Windows Credential Manager, macOS Keychain, Linux Secret Service).
//! A plaintext fallback file is used only when the OS keychain is unavailable,
//! which is acceptable for local development but flagged clearly in logs.

use std::path::PathBuf;

use crate::config::ConfigStore;
use crate::error::BirdResult;

const KEYRING_SERVICE: &str = "com.vaalley.nest-bird";
const FALLBACK_TOKEN_FILE: &str = "token.txt";

/// Store an authentication token securely.
pub fn set_token(username: &str, token: &str) -> BirdResult<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, username)?;
    match entry.set_password(token) {
        Ok(()) => Ok(()),
        Err(err) => {
            tracing::warn!(%err, "keyring unavailable; falling back to local token file");
            fallback_token_path().and_then(|path| {
                std::fs::write(&path, token)?;
                Ok(())
            })
        }
    }
}

/// Retrieve the stored authentication token, if any.
pub fn get_token(username: &str) -> BirdResult<Option<String>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, username)?;
    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => {
            tracing::warn!(%err, "keyring read failed; trying local fallback");
            match fallback_token_path() {
                Ok(path) if path.exists() => {
                    let token = std::fs::read_to_string(&path)?;
                    Ok(Some(token))
                }
                _ => Ok(None),
            }
        }
    }
}

/// Delete the stored authentication token.
pub fn delete_token(username: &str) -> BirdResult<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, username)?;
    if let Err(err) = entry.delete_credential() {
        tracing::warn!(%err, "keyring delete failed");
    }
    if let Ok(path) = fallback_token_path() {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn fallback_token_path() -> BirdResult<PathBuf> {
    Ok(ConfigStore::dir()?.join(FALLBACK_TOKEN_FILE))
}
