//! Bird (registered device) routes.

use axum::extract::State;
use axum::Json;

use nest_shared::api::{RegisterBirdRequest, RegisterBirdResponse};
use nest_shared::domain::{Bird, Platform};

use crate::auth::{create_token, AuthContext};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// `POST /api/birds/register` — register a new device for the authenticated Flock.
pub async fn register(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(req): Json<RegisterBirdRequest>,
) -> AppResult<Json<RegisterBirdResponse>> {
    if req.name.trim().is_empty() || req.name.len() > 64 {
        return Err(AppError::Validation(
            "name must be 1-64 characters".to_string(),
        ));
    }

    let platform = parse_platform(&req.platform)?;
    let bird = state
        .birds()
        .create(auth.flock_id, req.name.trim(), platform)
        .await?;

    // Registration itself counts as device activity.
    state.birds().touch_last_seen(bird.id).await?;

    let token = create_token(
        auth.flock_id,
        Some(bird.id),
        &state.config().token_secret,
        state.config().token_expiry_seconds,
    )?;

    Ok(Json(RegisterBirdResponse { bird, token }))
}

/// `GET /api/birds` — list all Birds belonging to the authenticated Flock.
pub async fn list(State(state): State<AppState>, auth: AuthContext) -> AppResult<Json<Vec<Bird>>> {
    let birds = state.birds().list_by_flock(auth.flock_id).await?;
    Ok(Json(birds))
}

fn parse_platform(s: &str) -> AppResult<Platform> {
    let raw = s.trim().to_ascii_lowercase();
    match raw.as_str() {
        "windows" => Ok(Platform::Windows),
        "linux" | "steamos" => Ok(Platform::Linux),
        "macos" | "mac" | "osx" => Ok(Platform::MacOs),
        "other" => Ok(Platform::Other),
        _ => Err(AppError::Validation(
            "platform must be windows, linux, steamos, macos, or other".to_string(),
        )),
    }
}
