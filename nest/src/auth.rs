//! Authentication primitives: Argon2 password hashing, HMAC-SHA256 signed
//! tokens, and the `AuthContext` request extractor.
//!
//! Tokens follow a compact JWT-like layout (`base64url(header).base64url(payload).
//! base64url(signature)`) but are implemented with pure-Rust crates to avoid
//! pulling in platform-specific C/assembly crypto backends.

use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::{
    Error as PasswordError, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

const JWT_HEADER: &[u8] = br#"{"alg":"HS256","typ":"JWT"}"#;

/// Claims carried by a signed token.
#[derive(Debug, Serialize, Deserialize)]
struct TokenClaims {
    /// Flock (user) id, carried in the `sub` claim.
    sub: String,
    /// Optional Bird (device) id.
    #[serde(skip_serializing_if = "Option::is_none")]
    bid: Option<String>,
    /// Expiration timestamp (seconds since Unix epoch).
    exp: usize,
    /// Issued-at timestamp.
    iat: usize,
}

/// Parsed and validated token contents.
#[derive(Debug, Clone)]
pub struct AuthClaims {
    pub flock_id: Uuid,
    pub bird_id: Option<Uuid>,
}

/// Authenticated request context, extracted from the `Authorization` header.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub flock_id: Uuid,
    pub bird_id: Option<Uuid>,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthContext {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let state = state.clone();

        let auth_header = parts
            .headers
            .get("authorization")
            .ok_or(AppError::Unauthorized)?
            .to_str()
            .map_err(|_| AppError::Unauthorized)?
            .to_string();

        let token = auth_header
            .strip_prefix("Bearer ")
            .or_else(|| auth_header.strip_prefix("bearer "))
            .ok_or(AppError::Unauthorized)?
            .trim()
            .to_string();

        let claims = decode_token(&token, &state.config().token_secret)?;

        if let Some(bird_id) = claims.bird_id {
            let _ = state.birds().touch_last_seen(bird_id).await;
        }

        Ok(AuthContext {
            flock_id: claims.flock_id,
            bird_id: claims.bird_id,
        })
    }
}

/// Hash a plaintext password with Argon2id.
pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AppError::Internal(format!("failed to hash password: {e}")))
}

/// Verify a plaintext password against an Argon2 PHC string.
pub fn verify_password(password: &str, password_hash: &str) -> AppResult<bool> {
    let parsed = PasswordHash::new(password_hash)
        .map_err(|e| AppError::Internal(format!("invalid stored password hash: {e}")))?;

    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(PasswordError::Password) => Ok(false),
        Err(e) => Err(AppError::Internal(format!(
            "failed to verify password: {e}"
        ))),
    }
}

/// Create a signed token for a Flock, optionally scoped to a Bird.
pub fn create_token(
    flock_id: Uuid,
    bird_id: Option<Uuid>,
    secret: &str,
    ttl_secs: u64,
) -> AppResult<String> {
    let now = unix_now()?;
    let exp = now
        .checked_add(ttl_secs as usize)
        .ok_or_else(|| AppError::Internal("token expiry overflow".to_string()))?;

    let claims = TokenClaims {
        sub: flock_id.to_string(),
        bid: bird_id.map(|id| id.to_string()),
        exp,
        iat: now,
    };

    let header_b64 = b64_encode(JWT_HEADER);
    let payload = serde_json::to_vec(&claims)
        .map_err(|e| AppError::Internal(format!("failed to serialize token claims: {e}")))?;
    let payload_b64 = b64_encode(&payload);

    let signing_input = format!("{header_b64}.{payload_b64}");
    let signature = hmac_sha256(secret, signing_input.as_bytes())?;
    let signature_b64 = b64_encode(&signature);

    Ok(format!("{signing_input}.{signature_b64}"))
}

/// Decode and verify a signed token, checking its HMAC and expiration.
pub fn decode_token(token: &str, secret: &str) -> AppResult<AuthClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AppError::Unauthorized);
    }

    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let signature = b64_decode(parts[2])?;
    verify_hmac(secret, signing_input.as_bytes(), &signature)?;

    let payload = b64_decode(parts[1])?;
    let claims: TokenClaims =
        serde_json::from_slice(&payload).map_err(|_| AppError::Unauthorized)?;

    let now = unix_now()?;
    if claims.exp < now {
        return Err(AppError::Unauthorized);
    }

    let flock_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized)?;
    let bird_id = claims
        .bid
        .map(|s| Uuid::parse_str(&s).map_err(|_| AppError::Unauthorized))
        .transpose()?;

    Ok(AuthClaims { flock_id, bird_id })
}

fn hmac_sha256(secret: &str, data: &[u8]) -> AppResult<Vec<u8>> {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| AppError::Internal(format!("invalid token secret: {e}")))?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn verify_hmac(secret: &str, data: &[u8], signature: &[u8]) -> AppResult<()> {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| AppError::Internal(format!("invalid token secret: {e}")))?;
    mac.update(data);
    mac.verify_slice(signature)
        .map_err(|_| AppError::Unauthorized)
}

fn b64_encode(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

fn b64_decode(s: &str) -> AppResult<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|_| AppError::Unauthorized)
}

fn unix_now() -> AppResult<usize> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as usize)
        .map_err(|e| AppError::Internal(format!("system clock error: {e}")))
}
