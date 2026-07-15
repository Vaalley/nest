//! Flock (user account) routes: registration, login, and profile.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use nest_shared::domain::Flock;

use crate::auth::{create_token, hash_password, verify_password, AuthContext};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    token: String,
    flock: Flock,
}

/// `POST /api/flock/register` — create a new Flock account.
pub async fn register(
    State(state): State<AppState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Json(req): Json<RegisterRequest>,
) -> AppResult<Json<AuthResponse>> {
    if let Some(ConnectInfo(addr)) = connect_info {
        state.rate_limiter().check(addr.ip())?;
    }

    validate_username(&req.username)?;
    validate_password(&req.password)?;

    if state.flocks().username_exists(&req.username).await? {
        return Err(AppError::Conflict("username already taken".to_string()));
    }

    let password_hash = hash_password(&req.password)?;
    let flock = state.flocks().create(&req.username, &password_hash).await?;
    let token = create_token(
        flock.id,
        None,
        &state.config().token_secret,
        state.config().token_expiry_seconds,
    )?;

    Ok(Json(AuthResponse { token, flock }))
}

/// `POST /api/flock/login` — authenticate and receive a signed token.
pub async fn login(
    State(state): State<AppState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<AuthResponse>> {
    if let Some(ConnectInfo(addr)) = connect_info {
        state.rate_limiter().check(addr.ip())?;
    }

    let creds = state
        .flocks()
        .find_credentials_by_username(&req.username)
        .await?;

    let creds = match creds {
        Some(creds) => creds,
        None => return Err(AppError::Unauthorized),
    };

    if !verify_password(&req.password, &creds.password_hash)? {
        return Err(AppError::Unauthorized);
    }

    let token = create_token(
        creds.flock.id,
        None,
        &state.config().token_secret,
        state.config().token_expiry_seconds,
    )?;

    Ok(Json(AuthResponse {
        token,
        flock: creds.flock,
    }))
}

/// `GET /api/flock/me` — return the currently authenticated Flock.
pub async fn me(State(state): State<AppState>, auth: AuthContext) -> AppResult<Json<Flock>> {
    let flock = state
        .flocks()
        .find_by_id(auth.flock_id)
        .await?
        .ok_or(AppError::Unauthorized)?;
    Ok(Json(flock))
}

fn validate_username(username: &str) -> AppResult<()> {
    if username.is_empty()
        || username.len() > 32
        || username
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
    {
        return Err(AppError::Validation(
            "username must be 1-32 ASCII alphanumeric, underscore, or hyphen characters"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_password(password: &str) -> AppResult<()> {
    if password.len() < 8 || password.len() > 128 {
        return Err(AppError::Validation(
            "password must be 8-128 characters".to_string(),
        ));
    }
    Ok(())
}
