//! Flight Home — the Bird-side sync engine.
//!
//! Phase 9 wires the Foraging Engine, Feather Agent, and Nest client into the
//! complete save-backup lifecycle:
//!
//! 1. Pre-launch compare ("Leaving the Branch").
//! 2. Pull / hatch when the Nest is newer.
//! 3. Surface Chilly Egg conflicts and wait for user resolution.
//! 4. Monitor the game while it runs.
//! 5. Post-exit zip + upload ("Laying a New Egg").
//! 6. Retry queue for uploads when the Nest is unreachable.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::sync::RwLock;
use uuid::Uuid;

use nest_shared::api::{CompareOutcome, Resolution};

use crate::agent::{AgentEvent, FeatherAgent};
use crate::api::NestClient;
use crate::config::ConfigStore;
use crate::egg;
use crate::error::{BirdError, BirdResult};
use crate::forage::ForagingEngine;

const QUEUE_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const UPLOAD_META_FILE: &str = "meta.json";
const UPLOAD_ZIP_FILE: &str = "egg.zip";

/// Current high-level state for a tracked game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightState {
    Idle,
    Flying,
    ChillyEgg,
    Error,
}

impl FlightState {
    pub fn as_str(&self) -> &'static str {
        match self {
            FlightState::Idle => "idle",
            FlightState::Flying => "flying",
            FlightState::ChillyEgg => "chilly_egg",
            FlightState::Error => "error",
        }
    }
}

/// Per-game sync status returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct GameSyncStatus {
    pub game_id: String,
    pub state: FlightState,
    pub message: String,
    pub local_hash: Option<String>,
    pub local_modified_at: Option<i64>,
}

impl Default for GameSyncStatus {
    fn default() -> Self {
        Self {
            game_id: String::new(),
            state: FlightState::Idle,
            message: String::new(),
            local_hash: None,
            local_modified_at: None,
        }
    }
}

impl GameSyncStatus {
    fn for_game(game_id: &str, state: FlightState, message: impl Into<String>) -> Self {
        Self {
            game_id: game_id.to_string(),
            state,
            message: message.into(),
            local_hash: None,
            local_modified_at: None,
        }
    }
}

/// Result of a manual sync attempt.
#[derive(Debug, Clone, Serialize)]
pub struct SyncResult {
    pub game_id: String,
    pub outcome: Option<String>,
    pub state: FlightState,
    pub message: String,
}

/// A game currently watched by the Feather Agent.
#[derive(Debug, Clone, Serialize)]
pub struct WatchedGame {
    pub game_id: String,
    pub process_names: Vec<String>,
    pub state: FlightState,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueuedUpload {
    game_id: String,
    source_bird_id: Uuid,
    file_hash: String,
}

/// Coordinates foraging, process monitoring, and Nest sync for one Bird.
#[derive(Clone)]
pub struct FlightHome {
    config_store: ConfigStore,
    forager: ForagingEngine,
    client: Arc<RwLock<Option<NestClient>>>,
    agent: FeatherAgent,
    statuses: Arc<RwLock<HashMap<String, GameSyncStatus>>>,
    queue_dir: PathBuf,
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
}

impl FlightHome {
    pub fn new(
        config_store: ConfigStore,
        forager: ForagingEngine,
        client: Arc<RwLock<Option<NestClient>>>,
    ) -> BirdResult<Self> {
        let queue_dir = ConfigStore::dir()?.join("upload-queue");
        Ok(Self {
            config_store,
            forager,
            client,
            agent: FeatherAgent::new(),
            statuses: Arc::new(RwLock::new(HashMap::new())),
            queue_dir,
            app_handle: Arc::new(RwLock::new(None)),
        })
    }

    /// Start the agent and the sync event/queue loops.
    pub async fn start(&self, app_handle: tauri::AppHandle) -> BirdResult<()> {
        *self.app_handle.write().await = Some(app_handle.clone());
        self.agent.start().await?;

        let flight = Arc::new(self.clone());
        let app = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            flight.process_events(app).await;
        });

        let flight = Arc::new(self.clone());
        tauri::async_runtime::spawn(async move {
            flight.process_queue().await;
        });

        Ok(())
    }

    /// Start watching `game_id`. If `process_names` is empty, the forager's
    /// defaults are used.
    pub async fn watch_game(
        &self,
        game_id: String,
        process_names: Vec<String>,
    ) -> BirdResult<Vec<String>> {
        let names = if process_names.is_empty() {
            self.forager.process_names(&game_id)?
        } else {
            process_names
        };

        if names.is_empty() {
            return Err(BirdError::Validation(format!(
                "no process names known for {game_id}"
            )));
        }

        self.agent.track(game_id.clone(), names.clone()).await;
        self.set_status(&game_id, FlightState::Idle, "Watching for game launch")
            .await;
        Ok(names)
    }

    pub async fn unwatch_game(&self, game_id: &str) {
        self.agent.untrack(game_id).await;
        let mut statuses = self.statuses.write().await;
        statuses.remove(game_id);
    }

    pub async fn watched_games(&self) -> Vec<WatchedGame> {
        let watched = self.agent.watched().await;
        let statuses = self.statuses.read().await;
        watched
            .into_iter()
            .map(|(game_id, process_names)| {
                let status = statuses.get(&game_id).cloned().unwrap_or_default();
                WatchedGame {
                    game_id,
                    process_names,
                    state: status.state,
                    message: status.message,
                }
            })
            .collect()
    }

    pub async fn status(&self, game_id: &str) -> GameSyncStatus {
        self.statuses
            .read()
            .await
            .get(game_id)
            .cloned()
            .unwrap_or_else(|| GameSyncStatus::for_game(game_id, FlightState::Idle, "Not watched"))
    }

    /// Perform a one-shot sync for `game_id` (pre-launch + post-exit lay if needed).
    pub async fn sync_now(&self, game_id: &str) -> BirdResult<SyncResult> {
        self.pre_launch_sync(game_id).await?;
        let status = self.status(game_id).await;
        if status.state == FlightState::ChillyEgg {
            return Ok(SyncResult {
                game_id: game_id.to_string(),
                outcome: Some("conflict".to_string()),
                state: status.state,
                message: status.message,
            });
        }
        self.post_exit_sync(game_id).await
    }

    /// Resolve a Chilly Egg conflict and apply the chosen version.
    pub async fn resolve_and_sync(
        &self,
        game_id: &str,
        resolution: Resolution,
    ) -> BirdResult<SyncResult> {
        let client = self.require_client().await?;
        let game = self.forager.discover_one(game_id)?;
        let save_path = game.save_path.ok_or_else(|| {
            BirdError::SavePathNotFound(PathBuf::from(format!("save path for {game_id}")))
        })?;

        let (local_hash, local_modified_at) = (game.local_hash, game.local_modified_at);

        let resp = client
            .resolve(
                game_id,
                resolution.clone(),
                local_hash.as_deref(),
                local_modified_at,
            )
            .await?;

        match resolution {
            Resolution::Nest => {
                if let Some(egg) = resp.baseline_egg {
                    let zip = client.hatch(game_id, egg.id).await?;
                    egg::replace_with_egg(&zip, &save_path)?;
                    self.set_status(
                        game_id,
                        FlightState::Idle,
                        "Pulled the Nest's Egg; ready to play",
                    )
                    .await;
                } else {
                    return Err(BirdError::Sync(
                        "Nest resolution returned no baseline Egg".to_string(),
                    ));
                }
            }
            Resolution::Local => {
                self.set_status(game_id, FlightState::Idle, "Kept the local save")
                    .await;
            }
        }

        Ok(SyncResult {
            game_id: game_id.to_string(),
            outcome: Some("resolved".to_string()),
            state: FlightState::Idle,
            message: format!("Conflict resolved: {resolution:?}"),
        })
    }

    /// Download a specific Egg and replace the local save folder with it.
    pub async fn restore_egg(&self, game_id: &str, egg_id: Uuid) -> BirdResult<SyncResult> {
        let client = self.require_client().await?;
        let game = self.forager.discover_one(game_id)?;
        let save_path = game.save_path.ok_or_else(|| {
            BirdError::SavePathNotFound(PathBuf::from(format!("save path for {game_id}")))
        })?;

        let zip = client.hatch(game_id, egg_id).await?;
        egg::replace_with_egg(&zip, &save_path)?;

        self.set_status(game_id, FlightState::Idle, "Restored the selected Egg")
            .await;

        Ok(SyncResult {
            game_id: game_id.to_string(),
            outcome: Some("restore".to_string()),
            state: FlightState::Idle,
            message: "Restored the selected Egg".to_string(),
        })
    }

    // -----------------------------------------------------------------------
    // Internal lifecycle
    // -----------------------------------------------------------------------

    async fn pre_launch_sync(&self, game_id: &str) -> BirdResult<CompareOutcome> {
        self.set_status(
            game_id,
            FlightState::Flying,
            "Comparing local save with the Nest...",
        )
        .await;

        let client = self.require_client().await?;
        let game = self.forager.discover_one(game_id)?;
        let save_path = match game.save_path {
            Some(p) if p.exists() => p,
            Some(p) => {
                self.set_status(
                    game_id,
                    FlightState::Error,
                    format!("Save path not found: {}", p.display()),
                )
                .await;
                return Err(BirdError::SavePathNotFound(p));
            }
            None => {
                self.set_status(game_id, FlightState::Error, "No save path for this game")
                    .await;
                return Err(BirdError::GameNotFound(game_id.to_string()));
            }
        };

        let local_hash = game.local_hash.unwrap_or_default();
        let local_modified_at = game.local_modified_at.unwrap_or(0);

        let compare = client
            .compare(game_id, &local_hash, local_modified_at)
            .await?;

        match compare.outcome {
            CompareOutcome::Identical => {
                self.set_status(
                    game_id,
                    FlightState::Idle,
                    "Save is already safe in the Nest",
                )
                .await;
            }
            CompareOutcome::Pull => {
                if let Some(egg) = compare.latest_egg {
                    let zip = client.hatch(game_id, egg.id).await?;
                    egg::replace_with_egg(&zip, &save_path)?;
                    self.set_status(
                        game_id,
                        FlightState::Idle,
                        "Hatched the latest Egg before launch",
                    )
                    .await;
                } else {
                    self.set_status(
                        game_id,
                        FlightState::Error,
                        "Pull requested but no Egg found",
                    )
                    .await;
                    return Err(BirdError::Sync("no Egg to hatch".to_string()));
                }
            }
            CompareOutcome::Push | CompareOutcome::NoEggs => {
                self.set_status(
                    game_id,
                    FlightState::Flying,
                    "Local save is newer; will lay an Egg after exit",
                )
                .await;
            }
            CompareOutcome::Conflict => {
                self.set_status(
                    game_id,
                    FlightState::ChillyEgg,
                    "Chilly Egg: both the local save and the Nest have changed",
                )
                .await;
                if let Some(app) = self.app_handle.read().await.as_ref() {
                    let _ = app.emit(
                        "sync-conflict",
                        GameSyncStatus::for_game(game_id, FlightState::ChillyEgg, "Chilly Egg"),
                    );
                }
            }
        }

        Ok(compare.outcome)
    }

    async fn post_exit_sync(&self, game_id: &str) -> BirdResult<SyncResult> {
        self.set_status(
            game_id,
            FlightState::Flying,
            "Game exited; preparing to lay an Egg...",
        )
        .await;

        let client = self.require_client().await?;
        let game = self.forager.discover_one(game_id)?;
        let save_path = game.save_path.ok_or_else(|| {
            BirdError::SavePathNotFound(PathBuf::from(format!("save path for {game_id}")))
        })?;
        if !save_path.exists() {
            self.set_status(game_id, FlightState::Error, "Save path missing after exit")
                .await;
            return Err(BirdError::SavePathNotFound(save_path));
        }

        let local_hash = game.local_hash.unwrap_or_default();
        let local_modified_at = game.local_modified_at.unwrap_or(0);

        let compare = client
            .compare(game_id, &local_hash, local_modified_at)
            .await?;

        match compare.outcome {
            CompareOutcome::Identical => {
                self.set_status(game_id, FlightState::Idle, "No changes to upload")
                    .await;
                return Ok(SyncResult {
                    game_id: game_id.to_string(),
                    outcome: Some("identical".to_string()),
                    state: FlightState::Idle,
                    message: "No changes to upload".to_string(),
                });
            }
            CompareOutcome::Pull => {
                if let Some(egg) = compare.latest_egg {
                    let zip = client.hatch(game_id, egg.id).await?;
                    egg::replace_with_egg(&zip, &save_path)?;
                    self.set_status(game_id, FlightState::Idle, "Pulled newer Egg from the Nest")
                        .await;
                    return Ok(SyncResult {
                        game_id: game_id.to_string(),
                        outcome: Some("pull".to_string()),
                        state: FlightState::Idle,
                        message: "Pulled newer Egg from the Nest".to_string(),
                    });
                }
                self.set_status(
                    game_id,
                    FlightState::Error,
                    "Pull requested but no Egg found",
                )
                .await;
                return Err(BirdError::Sync("no Egg to hatch".to_string()));
            }
            CompareOutcome::Conflict => {
                self.set_status(
                    game_id,
                    FlightState::ChillyEgg,
                    "Chilly Egg: conflict detected after exit",
                )
                .await;
                if let Some(app) = self.app_handle.read().await.as_ref() {
                    let _ = app.emit(
                        "sync-conflict",
                        GameSyncStatus::for_game(game_id, FlightState::ChillyEgg, "Chilly Egg"),
                    );
                }
                return Ok(SyncResult {
                    game_id: game_id.to_string(),
                    outcome: Some("conflict".to_string()),
                    state: FlightState::ChillyEgg,
                    message: "Chilly Egg: conflict detected after exit".to_string(),
                });
            }
            _ => {}
        }

        // Push / NoEggs: package and lay.
        let (zip_bytes, hash) = egg::package_directory(&save_path)?;
        let bird_id = self
            .config_store
            .load()?
            .bird_id
            .ok_or(BirdError::NotRegistered)?;

        match client.lay(game_id, bird_id, zip_bytes.clone(), &hash).await {
            Ok(_) => {
                self.set_status(game_id, FlightState::Idle, "Laid a fresh Egg in the Nest")
                    .await;
                if let Some(app) = self.app_handle.read().await.as_ref() {
                    let _ = app.emit(
                        "sync-complete",
                        GameSyncStatus::for_game(
                            game_id,
                            FlightState::Idle,
                            "Laid a fresh Egg in the Nest",
                        ),
                    );
                }
                Ok(SyncResult {
                    game_id: game_id.to_string(),
                    outcome: Some("lay".to_string()),
                    state: FlightState::Idle,
                    message: "Laid a fresh Egg in the Nest".to_string(),
                })
            }
            Err(err) => {
                self.queue_upload(game_id, bird_id, zip_bytes, &hash)
                    .await?;
                self.set_status(
                    game_id,
                    FlightState::Error,
                    format!("Upload failed; queued for retry: {err}"),
                )
                .await;
                Ok(SyncResult {
                    game_id: game_id.to_string(),
                    outcome: Some("queued".to_string()),
                    state: FlightState::Error,
                    message: format!("Upload failed; queued for retry: {err}"),
                })
            }
        }
    }

    async fn process_events(&self, app_handle: tauri::AppHandle) {
        let mut rx = self.agent.subscribe();
        while let Ok(event) = rx.recv().await {
            match event {
                AgentEvent::Launched { game_id, pid } => {
                    tracing::info!(%game_id, %pid, "game launched");
                    let _ = app_handle.emit(
                        "game-launched",
                        GameSyncStatus::for_game(&game_id, FlightState::Flying, "Game launched"),
                    );
                    if let Err(err) = self.pre_launch_sync(&game_id).await {
                        tracing::error!(%err, %game_id, "pre-launch sync failed");
                        self.set_status(
                            &game_id,
                            FlightState::Error,
                            format!("Pre-launch sync failed: {err}"),
                        )
                        .await;
                    }
                }
                AgentEvent::Exited { game_id, pid } => {
                    tracing::info!(%game_id, %pid, "game exited");
                    let _ = app_handle.emit(
                        "game-exited",
                        GameSyncStatus::for_game(&game_id, FlightState::Flying, "Game exited"),
                    );
                    if let Err(err) = self.post_exit_sync(&game_id).await {
                        tracing::error!(%err, %game_id, "post-exit sync failed");
                        self.set_status(
                            &game_id,
                            FlightState::Error,
                            format!("Post-exit sync failed: {err}"),
                        )
                        .await;
                    }
                }
                AgentEvent::Running { game_id, pid } => {
                    tracing::debug!(%game_id, %pid, "game still running");
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Upload queue
    // -----------------------------------------------------------------------

    async fn queue_upload(
        &self,
        game_id: &str,
        source_bird_id: Uuid,
        zip_bytes: Vec<u8>,
        file_hash: &str,
    ) -> BirdResult<()> {
        std::fs::create_dir_all(&self.queue_dir)?;
        let id = Uuid::new_v4();
        let dir = self.queue_dir.join(format!("{game_id}-{id}"));
        std::fs::create_dir_all(&dir)?;

        let meta = QueuedUpload {
            game_id: game_id.to_string(),
            source_bird_id,
            file_hash: file_hash.to_string(),
        };
        std::fs::write(dir.join(UPLOAD_META_FILE), serde_json::to_vec(&meta)?)?;
        std::fs::write(dir.join(UPLOAD_ZIP_FILE), zip_bytes)?;
        Ok(())
    }

    async fn process_queue(&self) {
        let mut interval = tokio::time::interval(QUEUE_RETRY_INTERVAL);
        loop {
            interval.tick().await;

            let client = match self.require_client().await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let entries = match std::fs::read_dir(&self.queue_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let meta_path = path.join(UPLOAD_META_FILE);
                let zip_path = path.join(UPLOAD_ZIP_FILE);
                if !meta_path.exists() || !zip_path.exists() {
                    continue;
                }

                let meta: QueuedUpload = match std::fs::read_to_string(&meta_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                {
                    Some(m) => m,
                    None => continue,
                };

                let zip_bytes = match std::fs::read(&zip_path) {
                    Ok(b) => b,
                    Err(_) => continue,
                };

                match client
                    .lay(
                        &meta.game_id,
                        meta.source_bird_id,
                        zip_bytes,
                        &meta.file_hash,
                    )
                    .await
                {
                    Ok(_) => {
                        if let Err(err) = std::fs::remove_dir_all(&path) {
                            tracing::warn!(%err, "failed to remove queued upload directory");
                        }
                    }
                    Err(err) => {
                        tracing::warn!(%err, game_id = %meta.game_id, "queued upload failed, will retry");
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    async fn set_status(&self, game_id: &str, state: FlightState, message: impl Into<String>) {
        let mut statuses = self.statuses.write().await;
        let message = message.into();
        let entry = statuses.entry(game_id.to_string()).or_default();
        entry.game_id = game_id.to_string();
        entry.state = state;
        entry.message = message.clone();
        drop(statuses);

        if let Some(app) = self.app_handle.read().await.as_ref() {
            let _ = app.emit(
                "sync-status",
                GameSyncStatus::for_game(game_id, state, message),
            );
        }
    }

    async fn require_client(&self) -> BirdResult<NestClient> {
        self.client
            .read()
            .await
            .clone()
            .ok_or(BirdError::NotAuthenticated)
    }
}
