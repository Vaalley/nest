//! API request/response DTOs shared between the Nest server and the Bird client.
//!
//! Keeping these types in `nest-shared` lets the Bird client use the same
//! strongly-typed contracts as the server, avoiding drift as the API evolves.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{Bird, Clutch, Egg, Flock, Platform, SyncStatus};

// ---------------------------------------------------------------------------
// Flock (authentication)
// ---------------------------------------------------------------------------

/// `POST /api/flock/register`
#[derive(Debug, Deserialize, Serialize)]
pub struct RegisterFlockRequest {
    pub username: String,
    pub password: String,
}

/// `POST /api/flock/login`
#[derive(Debug, Deserialize, Serialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Response returned by both register and login.
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub flock: Flock,
}

// Response for `GET /api/flock/me` (the Flock type is used directly).

// ---------------------------------------------------------------------------
// Bird (devices)
// ---------------------------------------------------------------------------

/// `POST /api/birds`
#[derive(Debug, Deserialize, Serialize)]
pub struct RegisterBirdRequest {
    pub name: String,
    pub platform: String,
}

/// Response returned when registering a Bird.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterBirdResponse {
    pub bird: Bird,
    pub token: String,
}

// ---------------------------------------------------------------------------
// Clutch & Egg summary
// ---------------------------------------------------------------------------

/// `GET /api/clutches` item.
#[derive(Debug, Serialize, Deserialize)]
pub struct ClutchSummary {
    #[serde(flatten)]
    pub clutch: Clutch,
    pub status: SyncStatus,
    pub egg_count: i64,
    pub latest_egg: Option<Egg>,
}

// ---------------------------------------------------------------------------
// Sync coordination
// ---------------------------------------------------------------------------

/// `POST /api/clutches/{game_id}/compare`
#[derive(Debug, Deserialize, Serialize)]
pub struct CompareRequest {
    pub local_hash: String,
    pub local_modified_at: i64,
    pub bird_id: Option<Uuid>,
}

/// Result of comparing the Bird's local save to the Nest's latest Egg.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareOutcome {
    /// Local save matches the latest Egg in the Nest.
    Identical,
    /// The Nest has a newer Egg; the Bird should pull it.
    Pull,
    /// The Bird's local save is newer; it should push after the play session.
    Push,
    /// Both sides have diverged since the last common ancestor.
    Conflict,
    /// This Clutch has no Eggs yet.
    NoEggs,
}

/// `POST /api/clutches/{game_id}/compare` response.
#[derive(Debug, Serialize, Deserialize)]
pub struct CompareResponse {
    pub outcome: CompareOutcome,
    pub status: SyncStatus,
    pub clutch_id: Uuid,
    pub latest_egg: Option<Egg>,
    pub last_synced_egg: Option<Egg>,
}

/// Resolution chosen for a Chilly Egg conflict.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// Keep the Nest's Egg.
    Nest,
    /// Keep the Bird's local save.
    Local,
}

/// `POST /api/clutches/{game_id}/resolve`
#[derive(Debug, Deserialize, Serialize)]
pub struct ResolveRequest {
    pub resolution: Resolution,
    /// Required when `resolution` is `local`.
    pub local_hash: Option<String>,
    /// Required when `resolution` is `local`.
    pub local_modified_at: Option<i64>,
    /// Optional specific Egg to keep when `resolution` is `nest` (defaults to latest).
    pub egg_id: Option<Uuid>,
    pub bird_id: Option<Uuid>,
}

/// `POST /api/clutches/{game_id}/resolve` response.
#[derive(Debug, Serialize, Deserialize)]
pub struct ResolveResponse {
    pub status: SyncStatus,
    pub baseline_egg: Option<Egg>,
}

// ---------------------------------------------------------------------------
// Foraging / Bird-local DTOs (not server endpoints, but shared vocabulary)
// ---------------------------------------------------------------------------

/// A game discovered by the Bird's foraging engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredGame {
    pub game_id: String,
    pub title: String,
    pub save_path: Option<std::path::PathBuf>,
    pub exists: bool,
    pub local_hash: Option<String>,
    /// Unix timestamp of the newest file in the save directory.
    pub local_modified_at: Option<i64>,
    pub status: SyncStatus,
}

/// Platform-agnostic description of a known/verified game entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEntry {
    pub game_id: String,
    pub title: String,
    /// Save paths keyed by platform. Each value is a list of possible
    /// relative path templates evaluated against the user's home/profile dirs.
    pub save_paths: std::collections::HashMap<Platform, Vec<String>>,
}
