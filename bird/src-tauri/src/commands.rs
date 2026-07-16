//! Tauri command handlers exposed to the Bird frontend.

use serde::Serialize;
use tauri::State;

use nest_shared::api::{
    AuthResponse, ClutchSummary, CompareResponse, DiscoveredGame, RegisterBirdResponse, Resolution,
    ResolveResponse,
};
use nest_shared::domain::Bird;

use crate::api::NestClient;
use crate::config::AppConfig;
use crate::error::{BirdError, BirdResult};
use crate::state::AppState;
use crate::sync::{GameSyncStatus, SyncResult, WatchedGame};

/// Serializable error returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

impl From<BirdError> for ErrorPayload {
    fn from(err: BirdError) -> Self {
        Self {
            code: err.code().to_string(),
            message: err.user_message(),
        }
    }
}

/// Convenience result type for commands.
pub type CommandResult<T> = Result<T, ErrorPayload>;

fn wrap<T>(res: BirdResult<T>) -> CommandResult<T> {
    res.map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Status & config
// ---------------------------------------------------------------------------

/// Application status shown on the onboarding screen.
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub nest_url: String,
    pub bird_name: String,
    pub platform: String,
    pub authenticated: bool,
    pub flock_username: Option<String>,
    pub bird_id: Option<String>,
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> CommandResult<StatusResponse> {
    let config = wrap(state.config())?;
    let client = state.client().await;
    Ok(StatusResponse {
        nest_url: config.nest_url,
        bird_name: config.bird_name,
        platform: config.platform,
        authenticated: client.is_some(),
        flock_username: config.flock_username,
        bird_id: config.bird_id.map(|id| id.to_string()),
    })
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> CommandResult<AppConfig> {
    wrap(state.config())
}

#[tauri::command]
pub async fn set_config(state: State<'_, AppState>, config: AppConfig) -> CommandResult<()> {
    wrap(state.set_config(config).await)
}

// ---------------------------------------------------------------------------
// Authentication & onboarding
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn register_flock(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> CommandResult<AuthResponse> {
    let config = wrap(state.config())?;
    let client = NestClient::new(&config.nest_url, None);
    let resp = wrap(client.register_flock(&username, &password).await)?;
    wrap(state.authenticate(username, resp.token.clone(), None).await)?;
    Ok(resp)
}

#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> CommandResult<AuthResponse> {
    let config = wrap(state.config())?;
    let client = NestClient::new(&config.nest_url, None);
    let resp = wrap(client.login(&username, &password).await)?;
    wrap(state.authenticate(username, resp.token.clone(), None).await)?;
    Ok(resp)
}

#[tauri::command]
pub async fn register_bird(
    state: State<'_, AppState>,
    name: Option<String>,
) -> CommandResult<RegisterBirdResponse> {
    let config = wrap(state.config())?;
    let name = name.unwrap_or(config.bird_name);
    let client = wrap(state.require_client().await)?;
    let resp = wrap(client.register_bird(&name, &config.platform).await)?;

    let mut config = wrap(state.config())?;
    config.bird_id = Some(resp.bird.id);
    config.bird_name = resp.bird.name.clone();
    wrap(state.set_config(config).await)?;

    // Rebuild the client with the device-scoped token.
    let new_client = NestClient::new(
        &state.config().map_err(ErrorPayload::from)?.nest_url,
        Some(resp.token.clone()),
    );
    state.set_client(Some(new_client)).await;

    Ok(resp)
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> CommandResult<()> {
    wrap(state.logout().await)
}

// ---------------------------------------------------------------------------
// Nest API proxies
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_birds(state: State<'_, AppState>) -> CommandResult<Vec<Bird>> {
    let client = wrap(state.require_client().await)?;
    wrap(client.list_birds().await)
}

#[tauri::command]
pub async fn list_clutches(state: State<'_, AppState>) -> CommandResult<Vec<ClutchSummary>> {
    let client = wrap(state.require_client().await)?;
    wrap(client.list_clutches().await)
}

#[tauri::command]
pub async fn list_eggs(
    state: State<'_, AppState>,
    game_id: String,
) -> CommandResult<Vec<nest_shared::domain::Egg>> {
    let client = wrap(state.require_client().await)?;
    wrap(client.list_eggs(&game_id).await)
}

#[tauri::command]
pub async fn delete_egg(
    state: State<'_, AppState>,
    game_id: String,
    egg_id: String,
) -> CommandResult<nest_shared::domain::Egg> {
    let egg_id = wrap(
        uuid::Uuid::parse_str(&egg_id)
            .map_err(|_| BirdError::Validation("invalid egg_id".to_string())),
    )?;
    let client = wrap(state.require_client().await)?;
    wrap(client.delete_egg(&game_id, egg_id).await)
}

#[tauri::command]
pub async fn restore_egg(
    state: State<'_, AppState>,
    game_id: String,
    egg_id: String,
) -> CommandResult<SyncResult> {
    let egg_id = wrap(
        uuid::Uuid::parse_str(&egg_id)
            .map_err(|_| BirdError::Validation("invalid egg_id".to_string())),
    )?;
    wrap(state.flight().restore_egg(&game_id, egg_id).await)
}

#[tauri::command]
pub async fn compare_game(
    state: State<'_, AppState>,
    game_id: String,
    local_hash: String,
    local_modified_at: i64,
) -> CommandResult<CompareResponse> {
    let client = wrap(state.require_client().await)?;
    wrap(
        client
            .compare(&game_id, &local_hash, local_modified_at)
            .await,
    )
}

#[tauri::command]
pub async fn resolve_game(
    state: State<'_, AppState>,
    game_id: String,
    resolution: String,
    local_hash: Option<String>,
    local_modified_at: Option<i64>,
) -> CommandResult<ResolveResponse> {
    let resolution = match resolution.as_str() {
        "nest" => Resolution::Nest,
        "local" => Resolution::Local,
        _ => {
            return Err(
                BirdError::Validation("resolution must be 'nest' or 'local'".to_string()).into(),
            )
        }
    };
    let client = wrap(state.require_client().await)?;
    wrap(
        client
            .resolve(
                &game_id,
                resolution,
                local_hash.as_deref(),
                local_modified_at,
            )
            .await,
    )
}

// ---------------------------------------------------------------------------
// Foraging
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn refresh_manifest(state: State<'_, AppState>) -> CommandResult<()> {
    wrap(state.forager().refresh_manifest().await)
}

#[tauri::command]
pub async fn discover_games(state: State<'_, AppState>) -> CommandResult<Vec<DiscoveredGame>> {
    wrap(state.forager().discover())
}

#[tauri::command]
pub async fn discover_game(
    state: State<'_, AppState>,
    game_id: String,
) -> CommandResult<DiscoveredGame> {
    wrap(state.forager().discover_one(&game_id))
}

// ---------------------------------------------------------------------------
// Flight Home (Phase 9)
// ---------------------------------------------------------------------------

/// Start watching a game for launch/exit events. `process_names` is optional;
/// when empty the forager's known names are used.
#[tauri::command]
pub async fn watch_game(
    state: State<'_, AppState>,
    game_id: String,
    process_names: Vec<String>,
) -> CommandResult<Vec<String>> {
    let names = if process_names.is_empty() {
        wrap(state.forager().process_names(&game_id))?
    } else {
        process_names
    };
    wrap(state.flight().watch_game(game_id, names).await)
}

#[tauri::command]
pub async fn unwatch_game(state: State<'_, AppState>, game_id: String) -> CommandResult<()> {
    state.flight().unwatch_game(&game_id).await;
    Ok(())
}

#[tauri::command]
pub async fn watched_games(state: State<'_, AppState>) -> CommandResult<Vec<WatchedGame>> {
    Ok(state.flight().watched_games().await)
}

#[tauri::command]
pub async fn sync_status(
    state: State<'_, AppState>,
    game_id: String,
) -> CommandResult<GameSyncStatus> {
    Ok(state.flight().status(&game_id).await)
}

#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>, game_id: String) -> CommandResult<SyncResult> {
    wrap(state.flight().sync_now(&game_id).await)
}

/// Resolve a Chilly Egg conflict and apply the chosen save.
#[tauri::command]
pub async fn resolve_and_sync(
    state: State<'_, AppState>,
    game_id: String,
    resolution: String,
) -> CommandResult<SyncResult> {
    let resolution = match resolution.as_str() {
        "nest" => Resolution::Nest,
        "local" => Resolution::Local,
        _ => {
            return Err(
                BirdError::Validation("resolution must be 'nest' or 'local'".to_string()).into(),
            )
        }
    };
    wrap(state.flight().resolve_and_sync(&game_id, resolution).await)
}
