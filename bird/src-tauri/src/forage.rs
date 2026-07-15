//! Foraging Engine — discovers installed games and resolves their save paths.
//!
//! Phase 7 focuses on a manually-curated subset of verified games plus the
//! infrastructure to fetch, cache, and refresh the open-source Ludusavi
//! manifest. Full placeholder resolution is intentionally limited to the
//! most common Windows paths for the MVP; Phase 12 expands coverage.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use nest_shared::api::DiscoveredGame;
use nest_shared::domain::{Platform, SyncStatus};

use crate::error::{BirdError, BirdResult};

const LUDUSAVI_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/mtkennerly/ludusavi/master/data/manifest.yaml";
const MANIFEST_FILE: &str = "ludusavi_manifest.yaml";
const CACHE_HOURS: i64 = 24;

/// A curated verified game entry with platform-specific save path templates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedGame {
    pub game_id: String,
    pub title: String,
    /// Save-path templates keyed by platform. Templates use common Ludusavi
    /// placeholders such as `<home>`, `<winAppData>`, `<winLocalAppData>`,
    /// `<winDocuments>`, `<winPublic>`, and `<root>` (game install root).
    pub save_paths: HashMap<Platform, Vec<String>>,
}

impl VerifiedGame {
    fn new(game_id: &str, title: &str) -> Self {
        Self {
            game_id: game_id.to_string(),
            title: title.to_string(),
            save_paths: HashMap::new(),
        }
    }

    fn with_path(mut self, platform: Platform, template: &str) -> Self {
        self.save_paths
            .entry(platform)
            .or_default()
            .push(template.to_string());
        self
    }
}

/// The foraging engine discovers local save locations and hashes them.
#[derive(Debug, Clone)]
pub struct ForagingEngine {
    cache_dir: PathBuf,
    verified: Vec<VerifiedGame>,
}

impl ForagingEngine {
    /// Create the engine with the built-in verified subset.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            verified: built_in_verified_games(),
        }
    }

    /// Path to the cached Ludusavi manifest, whether it has been downloaded or not.
    pub fn manifest_path(&self) -> PathBuf {
        self.cache_dir.join(MANIFEST_FILE)
    }

    /// Fetch and cache the Ludusavi manifest if it is missing or stale.
    pub async fn refresh_manifest(&self) -> BirdResult<()> {
        std::fs::create_dir_all(&self.cache_dir)?;
        let path = self.manifest_path();

        let should_download = if path.exists() {
            let meta = std::fs::metadata(&path)?;
            let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let elapsed = modified.elapsed().unwrap_or_default().as_secs() as i64 / 3600;
            elapsed >= CACHE_HOURS
        } else {
            true
        };

        if should_download {
            tracing::info!(url = LUDUSAVI_MANIFEST_URL, "downloading Ludusavi manifest");
            let response = reqwest::get(LUDUSAVI_MANIFEST_URL).await?;
            let bytes = response.bytes().await?;
            std::fs::write(&path, &bytes)?;
            tracing::info!(path = %path.display(), "cached Ludusavi manifest");
        }
        Ok(())
    }

    /// Return the last time the manifest was refreshed, if ever.
    pub fn manifest_refreshed_at(&self) -> BirdResult<Option<i64>> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(None);
        }
        let meta = std::fs::metadata(&path)?;
        let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let since_epoch = modified
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        Ok(Some(since_epoch.as_secs() as i64))
    }

    /// Discover all verified games, resolving their save paths and computing
    /// hashes/timestamps for those that exist locally.
    pub fn discover(&self) -> BirdResult<Vec<DiscoveredGame>> {
        let mut games = Vec::with_capacity(self.verified.len());
        for game in &self.verified {
            games.push(self.discover_game(game)?);
        }
        Ok(games)
    }

    /// Look up a single verified game by id.
    pub fn discover_one(&self, game_id: &str) -> BirdResult<DiscoveredGame> {
        let game = self
            .verified
            .iter()
            .find(|g| g.game_id == game_id)
            .ok_or_else(|| BirdError::GameNotFound(game_id.to_string()))?;
        self.discover_game(game)
    }

    fn discover_game(&self, game: &VerifiedGame) -> BirdResult<DiscoveredGame> {
        let platform = current_platform();
        let mut resolved: Option<PathBuf> = None;

        if let Some(templates) = game.save_paths.get(&platform) {
            for template in templates {
                let path = resolve_template(template)?;
                if path.exists() {
                    resolved = Some(path);
                    break;
                }
            }
        }

        let (exists, hash, modified_at) = match resolved {
            Some(ref path) if path.exists() => {
                let (hash, modified) = hash_and_mtime(path)?;
                (true, Some(hash), Some(modified))
            }
            Some(_) => (false, None, None),
            None => (false, None, None),
        };

        Ok(DiscoveredGame {
            game_id: game.game_id.clone(),
            title: game.title.clone(),
            save_path: resolved,
            exists,
            local_hash: hash,
            local_modified_at: modified_at,
            status: SyncStatus::SafeInNest,
        })
    }
}

fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Other
    }
}

/// Resolve a Ludusavi-style path template to an absolute path.
///
/// Supported placeholders:
/// - `<home>` — the user's home directory (`%USERPROFILE%` on Windows, `$HOME` elsewhere).
/// - `<winAppData>` — `%APPDATA%` (RoamingAppData on Windows).
/// - `<winLocalAppData>` — `%LOCALAPPDATA%`.
/// - `<winLocalLow>` — `%USERPROFILE%\AppData\LocalLow`.
/// - `<winDocuments>` — the user's Documents folder.
/// - `<winPublic>` — the Public user folder.
/// - `<xdgData>` / `<xdgConfig>` — XDG base directories on Linux/macOS.
/// - `<root>` — game install root (not implemented for the MVP; returns an error).
fn resolve_template(template: &str) -> BirdResult<PathBuf> {
    let mut resolved = template.to_string();

    if let Some(home) = dirs::home_dir() {
        resolved = resolved.replace("<home>", &home.to_string_lossy());
    }

    if cfg!(target_os = "windows") {
        if let Some(roaming) = dirs::data_dir() {
            resolved = resolved.replace("<winAppData>", &roaming.to_string_lossy());
        }
        if let Some(local) = dirs::data_local_dir() {
            resolved = resolved.replace("<winLocalAppData>", &local.to_string_lossy());
            // LocalLow is a sibling of Local under AppData.
            if let Some(parent) = local.parent() {
                resolved =
                    resolved.replace("<winLocalLow>", &parent.join("LocalLow").to_string_lossy());
            }
        }
        if let Some(docs) = dirs::document_dir() {
            resolved = resolved.replace("<winDocuments>", &docs.to_string_lossy());
        }
        if let Some(public) = dirs::public_dir() {
            resolved = resolved.replace("<winPublic>", &public.to_string_lossy());
        }
    } else {
        if let Some(data) = dirs::data_dir() {
            resolved = resolved.replace("<xdgData>", &data.to_string_lossy());
        }
        if let Some(config) = dirs::config_dir() {
            resolved = resolved.replace("<xdgConfig>", &config.to_string_lossy());
        }
    }

    if resolved.contains("<root>") {
        return Err(BirdError::Internal(
            "<root> placeholder requires game install detection (Phase 12)".to_string(),
        ));
    }

    // Collapse mixed separators on Windows.
    if cfg!(target_os = "windows") {
        resolved = resolved.replace('/', "\\");
    }

    Ok(PathBuf::from(resolved))
}

/// Compute a stable SHA-256 hash of all files under `path` plus the newest
/// file modification time (Unix seconds). Files are walked in sorted order so
/// the hash is deterministic.
fn hash_and_mtime(path: &Path) -> BirdResult<(String, i64)> {
    let mut hasher = Sha256::new();
    let mut newest: i64 = 0;

    for entry in WalkDir::new(path)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let meta = entry.metadata()?;
            if let Ok(modified) = meta.modified() {
                let since_epoch = modified
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .unwrap_or_default();
                let ts = since_epoch.as_secs() as i64;
                if ts > newest {
                    newest = ts;
                }
            }

            // Hash the file path (relative to the root) and contents.
            let rel = entry.path().strip_prefix(path).unwrap_or(entry.path());
            hasher.update(rel.to_string_lossy().as_bytes());
            hasher.update([0]);

            let mut file = std::fs::File::open(entry.path())?;
            let mut buf = [0u8; 8192];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            hasher.update([0]);
        }
    }

    let hash = format!("{:x}", hasher.finalize());
    Ok((hash, newest))
}

/// Manually-verified test games used for the Phase 7 MVP.
fn built_in_verified_games() -> Vec<VerifiedGame> {
    vec![
        VerifiedGame::new("stardew-valley", "Stardew Valley")
            .with_path(Platform::Windows, "<winAppData>/StardewValley/Saves")
            .with_path(Platform::Linux, "<home>/.config/StardewValley/Saves")
            .with_path(Platform::MacOs, "<home>/.config/StardewValley/Saves"),
        VerifiedGame::new("hollow-knight", "Hollow Knight")
            .with_path(
                Platform::Windows,
                "<winLocalAppData>Low/Team Cherry/Hollow Knight",
            )
            .with_path(
                Platform::Linux,
                "<home>/.config/unity3d/Team Cherry/Hollow Knight",
            ),
        VerifiedGame::new("celeste", "Celeste")
            .with_path(Platform::Windows, "<winAppData>/Celeste")
            .with_path(Platform::Linux, "<home>/.local/share/Celeste")
            .with_path(
                Platform::MacOs,
                "<home>/Library/Application Support/Celeste",
            ),
        VerifiedGame::new("hades", "Hades")
            .with_path(Platform::Windows, "<winDocuments>/Saved Games/Hades")
            .with_path(
                Platform::Linux,
                "<home>/.local/share/Supergiant Games/Hades",
            ),
        VerifiedGame::new("terraria", "Terraria")
            .with_path(Platform::Windows, "<winDocuments>/My Games/Terraria")
            .with_path(Platform::Linux, "<home>/.local/share/Terraria"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_subset_populated() {
        let games = built_in_verified_games();
        assert!(!games.is_empty());
        assert!(games.iter().any(|g| g.game_id == "stardew-valley"));
    }

    #[test]
    fn current_platform_is_windows_on_windows() {
        if cfg!(target_os = "windows") {
            assert_eq!(current_platform(), Platform::Windows);
        }
    }

    #[test]
    fn hash_and_mtime_computes_stable_hash() {
        let tmp = std::env::temp_dir().join(format!("nest-bird-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();

        let file_a = tmp.join("a.txt");
        let file_b = tmp.join("b.txt");
        std::fs::write(&file_a, b"hello").unwrap();
        std::fs::write(&file_b, b"world").unwrap();

        let (hash, mtime) = hash_and_mtime(&tmp).unwrap();
        assert!(!hash.is_empty());
        assert!(mtime > 0);

        // Recomputing on unchanged directory yields the same hash.
        let (hash2, _) = hash_and_mtime(&tmp).unwrap();
        assert_eq!(hash, hash2);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn discover_returns_verified_games() {
        let engine = ForagingEngine::new(std::env::temp_dir());
        let games = engine.discover().unwrap();
        assert_eq!(games.len(), built_in_verified_games().len());
    }
}
