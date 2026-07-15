//! Core domain models (DDD aggregates) for the Nest.
//!
//! Glossary:
//! - **Flock** — a user account.
//! - **Bird** — a registered client device.
//! - **Egg** — a single zipped save-file snapshot.
//! - **Clutch** — the rolling collection of Eggs for one game.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Default number of Eggs retained per Clutch before the oldest are pruned
/// (the "Brood Limit").
pub const DEFAULT_BROOD_LIMIT: i64 = 10;

/// The operating system / device class a Bird runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    Linux,
    MacOs,
    Other,
}

impl Platform {
    /// Stable string representation used for persistence.
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Linux => "linux",
            Platform::MacOs => "macos",
            Platform::Other => "other",
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Platform {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "windows" => Ok(Platform::Windows),
            "linux" | "steamos" => Ok(Platform::Linux),
            "macos" | "mac" | "osx" => Ok(Platform::MacOs),
            _ => Ok(Platform::Other),
        }
    }
}

/// Sync status of a Clutch, mirroring the "Cozy UI" indicators from the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    /// Synced with the Nest.
    SafeInNest,
    /// A sync is in progress.
    Flying,
    /// A save conflict was detected.
    ChillyEgg,
}

impl SyncStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncStatus::SafeInNest => "safe_in_nest",
            SyncStatus::Flying => "flying",
            SyncStatus::ChillyEgg => "chilly_egg",
        }
    }
}

impl std::str::FromStr for SyncStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "safe_in_nest" => Ok(SyncStatus::SafeInNest),
            "flying" => Ok(SyncStatus::Flying),
            "chilly_egg" => Ok(SyncStatus::ChillyEgg),
            _ => Err(format!("unknown sync status: {s}")),
        }
    }
}

/// A user account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flock {
    pub id: Uuid,
    pub username: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A registered client device belonging to a Flock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bird {
    pub id: Uuid,
    pub flock_id: Uuid,
    pub name: String,
    pub platform: Platform,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_seen: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// The rolling collection of Eggs tracked for a single game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clutch {
    pub id: Uuid,
    pub flock_id: Uuid,
    pub game_id: String,
    pub brood_limit: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A single zipped save-file snapshot within a Clutch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Egg {
    pub id: Uuid,
    pub clutch_id: Uuid,
    pub source_bird_id: Option<Uuid>,
    pub file_hash: String,
    pub size_bytes: i64,
    pub file_path: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}
