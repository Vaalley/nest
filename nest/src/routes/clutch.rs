//! Clutch & Egg routes: the save-archive lifecycle (Phase 4) and the sync
//! coordination / conflict model (Phase 5).

use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use nest_shared::api::{
    ClutchSummary, CompareOutcome, CompareRequest, CompareResponse, Resolution, ResolveRequest,
    ResolveResponse,
};
use nest_shared::domain::{Egg, SyncStatus};

use crate::auth::AuthContext;
use crate::error::{AppError, AppResult};
use crate::repository::egg::NewEgg;
use crate::state::AppState;

/// A game-path parameter is rejected if it could escape the archive directory.
fn validate_game_id(game_id: &str) -> AppResult<()> {
    if game_id.is_empty() || game_id.len() > 128 {
        return Err(AppError::Validation(
            "game_id must be 1-128 characters".to_string(),
        ));
    }
    if game_id == "." || game_id == ".." {
        return Err(AppError::Validation("invalid game_id".to_string()));
    }
    if game_id.contains('/') || game_id.contains('\\') || game_id.contains('\0') {
        return Err(AppError::Validation(
            "game_id must not contain path separators".to_string(),
        ));
    }
    Ok(())
}

/// Produce a filesystem-safe directory name from a game id.
///
/// Collisions between two distinct game ids that sanitize to the same string are
/// acceptable for the MVP because each Egg file name already includes a UUID;
/// Phase 11 may revisit this with a more robust slug or percent-encoding.
fn sanitize_game_id(game_id: &str) -> String {
    let mut slug = String::with_capacity(game_id.len());
    let mut prev = '_';
    for c in game_id.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
            slug.push(c);
            prev = c;
        } else if prev != '_' {
            slug.push('_');
            prev = '_';
        }
    }
    let slug = slug.trim_matches('_').to_string();
    if slug.is_empty() {
        return "unknown".to_string();
    }
    if slug.len() > 64 {
        return slug[..64].trim_end_matches('_').to_string();
    }
    slug
}

/// The relative archive path for an Egg, stored in the database.
fn relative_egg_path(
    flock_id: Uuid,
    game_id: &str,
    egg_id: Uuid,
    created_at: OffsetDateTime,
) -> String {
    format!(
        "flocks/{}/{}/egg_{}_{}.zip",
        flock_id,
        sanitize_game_id(game_id),
        created_at.unix_timestamp(),
        egg_id
    )
}

fn absolute_path(state: &AppState, relative: &str) -> PathBuf {
    state.config().data_dir.join(relative)
}

fn hash_to_hex(hash: &[u8]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn map_multipart_err(e: axum::extract::multipart::MultipartError) -> AppError {
    AppError::Validation(format!("multipart error: {e}"))
}

// ---------------------------------------------------------------------------
// DTOs live in `nest-shared::api` so the Bird client uses the same contracts.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Determine which Bird is performing the sync action.
fn resolve_bird_id(auth: &AuthContext, request_bird_id: Option<Uuid>) -> AppResult<Uuid> {
    if let Some(id) = request_bird_id {
        return Ok(id);
    }
    auth.bird_id.ok_or(AppError::Forbidden)
}

/// Validate that the requested Bird belongs to the authenticated Flock.
async fn validate_bird_owns(state: &AppState, flock_id: Uuid, bird_id: Uuid) -> AppResult<()> {
    if state
        .birds()
        .find_for_flock(flock_id, bird_id)
        .await?
        .is_none()
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

async fn fetch_latest_egg(state: &AppState, clutch_id: Uuid) -> AppResult<Option<Egg>> {
    state.eggs().find_latest_by_clutch(clutch_id).await
}

/// Update the sync baseline to a specific Egg and mark the pair as safe.
async fn sync_to_egg(state: &AppState, bird_id: Uuid, clutch_id: Uuid, egg: &Egg) -> AppResult<()> {
    state
        .sync()
        .set_baseline(
            bird_id,
            clutch_id,
            Some(egg.id),
            Some(&egg.file_hash),
            Some(egg.created_at),
            SyncStatus::SafeInNest,
        )
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// `GET /api/clutches` — list the Flock's tracked games and their status.
pub async fn list(
    State(state): State<AppState>,
    auth: AuthContext,
) -> AppResult<Json<Vec<ClutchSummary>>> {
    let clutches = state.clutches().list_by_flock(auth.flock_id).await?;
    let mut summaries = Vec::with_capacity(clutches.len());
    for clutch in clutches {
        let status = if let Some(bird_id) = auth.bird_id {
            state.sync().status_for_bird(bird_id, clutch.id).await?
        } else {
            state
                .sync()
                .aggregate_status_for_clutch(auth.flock_id, clutch.id)
                .await?
        };
        let egg_count = state.eggs().count_in_clutch(clutch.id).await?;
        let latest_egg = state.eggs().find_latest_by_clutch(clutch.id).await?;
        summaries.push(ClutchSummary {
            clutch,
            status,
            egg_count,
            latest_egg,
        });
    }
    Ok(Json(summaries))
}

/// `GET /api/clutches/{game_id}/eggs` — version history for a Clutch.
pub async fn eggs(
    State(state): State<AppState>,
    Path(game_id): Path<String>,
    auth: AuthContext,
) -> AppResult<Json<Vec<Egg>>> {
    validate_game_id(&game_id)?;
    let clutch = state
        .clutches()
        .find_by_game(auth.flock_id, &game_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let eggs = state.eggs().list_by_clutch(clutch.id).await?;
    Ok(Json(eggs))
}

/// `POST /api/clutches/{game_id}/lay` — upload a new Egg.
pub async fn lay(
    State(state): State<AppState>,
    Path(game_id): Path<String>,
    auth: AuthContext,
    mut multipart: Multipart,
) -> AppResult<Json<Egg>> {
    validate_game_id(&game_id)?;

    let archive_dir = state
        .config()
        .data_dir
        .join("flocks")
        .join(auth.flock_id.to_string())
        .join(sanitize_game_id(&game_id));
    fs::create_dir_all(&archive_dir).await?;

    let temp_name = format!(".tmp-{}", Uuid::new_v4());
    let temp_path = archive_dir.join(&temp_name);

    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut provided_hash: Option<String> = None;
    let mut source_bird_id: Option<Uuid> = auth.bird_id;

    while let Some(mut field) = multipart.next_field().await.map_err(map_multipart_err)? {
        let name = field.name().map(|n| n.to_string());
        match name.as_deref() {
            Some("file") => {
                let file = fs::File::create(&temp_path).await?;
                let mut writer = tokio::io::BufWriter::new(file);
                while let Some(chunk) = field.chunk().await.map_err(map_multipart_err)? {
                    hasher.update(&chunk);
                    written += chunk.len() as u64;
                    writer.write_all(&chunk).await?;
                }
                writer.flush().await?;
            }
            Some("file_hash") => {
                provided_hash = Some(field.text().await.map_err(map_multipart_err)?);
            }
            Some("source_bird_id") => {
                let text = field.text().await.map_err(map_multipart_err)?;
                source_bird_id = Some(
                    Uuid::parse_str(&text)
                        .map_err(|_| AppError::Validation("invalid source_bird_id".to_string()))?,
                );
            }
            _ => {
                let _ = field.bytes().await.map_err(map_multipart_err)?;
            }
        }
    }

    let source_bird_id = source_bird_id.ok_or(AppError::Forbidden)?;
    validate_bird_owns(&state, auth.flock_id, source_bird_id).await?;

    let expected_hash =
        provided_hash.ok_or_else(|| AppError::Validation("missing file_hash field".to_string()))?;

    let computed_hash = hash_to_hex(&hasher.finalize());
    if computed_hash != expected_hash.trim() {
        let _ = fs::remove_file(&temp_path).await;
        return Err(AppError::Validation(
            "file hash does not match payload".to_string(),
        ));
    }

    let clutch = state
        .clutches()
        .find_or_create(auth.flock_id, &game_id, state.config().default_brood_limit)
        .await?;

    let egg_id = Uuid::new_v4();
    let created_at = OffsetDateTime::now_utc();
    let relative_path = relative_egg_path(auth.flock_id, &game_id, egg_id, created_at);
    let final_path = absolute_path(&state, &relative_path);

    fs::rename(&temp_path, &final_path).await?;

    let egg = match state
        .eggs()
        .create(NewEgg {
            id: Some(egg_id),
            clutch_id: clutch.id,
            source_bird_id: Some(source_bird_id),
            file_hash: &computed_hash,
            size_bytes: written as i64,
            file_path: &relative_path,
            created_at: Some(created_at),
        })
        .await
    {
        Ok(egg) => egg,
        Err(e) => {
            let _ = fs::remove_file(&final_path).await;
            return Err(e);
        }
    };

    // Enforce the Brood Limit, deleting both the rows and the files.
    let pruned = state.eggs().prune(clutch.id, clutch.brood_limit).await?;
    for egg in &pruned {
        let path = absolute_path(&state, &egg.file_path);
        if let Err(err) = fs::remove_file(&path).await {
            tracing::warn!(%err, path = %path.display(), "failed to remove pruned egg file");
        }
    }

    // This Bird now knows it is in sync with the Egg it just laid.
    if let Err(err) = sync_to_egg(&state, source_bird_id, clutch.id, &egg).await {
        tracing::warn!(%err, "failed to update sync baseline after lay");
    }

    Ok(Json(egg))
}

/// `GET /api/clutches/{game_id}/hatch/{egg_id}` — download a specific Egg.
pub async fn hatch(
    State(state): State<AppState>,
    Path((game_id, egg_id)): Path<(String, Uuid)>,
    auth: AuthContext,
) -> AppResult<Response> {
    validate_game_id(&game_id)?;
    let clutch = state
        .clutches()
        .find_by_game(auth.flock_id, &game_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let egg = state
        .eggs()
        .find_in_clutch(clutch.id, egg_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let path = absolute_path(&state, &egg.file_path);
    if !path.exists() {
        return Err(AppError::NotFound);
    }

    let file = fs::File::open(&path).await?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    // A Bird that hatches an Egg is now in sync with it.
    if let Some(bird_id) = auth.bird_id {
        if let Err(err) = sync_to_egg(&state, bird_id, clutch.id, &egg).await {
            tracing::warn!(%err, "failed to update sync baseline after hatch");
        }
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("egg.zip");

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{file_name}\""),
            ),
        ],
        body,
    )
        .into_response())
}

/// `DELETE /api/clutches/{game_id}/eggs/{egg_id}` — discard an Egg.
pub async fn delete(
    State(state): State<AppState>,
    Path((game_id, egg_id)): Path<(String, Uuid)>,
    auth: AuthContext,
) -> AppResult<Json<Egg>> {
    validate_game_id(&game_id)?;
    let clutch = state
        .clutches()
        .find_by_game(auth.flock_id, &game_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let deleted = state
        .eggs()
        .delete_in_clutch(clutch.id, egg_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let path = absolute_path(&state, &deleted.file_path);
    if let Err(err) = fs::remove_file(&path).await {
        tracing::warn!(%err, path = %path.display(), "failed to remove deleted egg file");
    }

    Ok(Json(deleted))
}

/// `POST /api/clutches/{game_id}/compare` — pre-launch sync check.
pub async fn compare(
    State(state): State<AppState>,
    Path(game_id): Path<String>,
    auth: AuthContext,
    Json(req): Json<CompareRequest>,
) -> AppResult<Json<CompareResponse>> {
    validate_game_id(&game_id)?;
    let bird_id = resolve_bird_id(&auth, req.bird_id)?;
    validate_bird_owns(&state, auth.flock_id, bird_id).await?;

    let Some(clutch) = state
        .clutches()
        .find_by_game(auth.flock_id, &game_id)
        .await?
    else {
        return Ok(Json(CompareResponse {
            outcome: CompareOutcome::NoEggs,
            status: SyncStatus::SafeInNest,
            clutch_id: Uuid::nil(),
            latest_egg: None,
            last_synced_egg: None,
        }));
    };

    let latest = fetch_latest_egg(&state, clutch.id).await?;
    let latest = match latest {
        Some(egg) => egg,
        None => {
            return Ok(Json(CompareResponse {
                outcome: CompareOutcome::NoEggs,
                status: SyncStatus::SafeInNest,
                clutch_id: clutch.id,
                latest_egg: None,
                last_synced_egg: None,
            }));
        }
    };

    let sync = state.sync().find(bird_id, clutch.id).await?;
    let baseline_hash = sync.as_ref().and_then(|s| s.last_synced_hash.clone());

    let (outcome, new_status) = if req.local_hash == latest.file_hash {
        (CompareOutcome::Identical, SyncStatus::SafeInNest)
    } else if let Some(base_hash) = baseline_hash {
        let remote_changed = latest.file_hash != base_hash;
        let local_changed = req.local_hash != base_hash;
        match (remote_changed, local_changed) {
            (true, true) => (CompareOutcome::Conflict, SyncStatus::ChillyEgg),
            (true, false) => (CompareOutcome::Pull, SyncStatus::Flying),
            (false, true) => (CompareOutcome::Push, SyncStatus::Flying),
            (false, false) => (CompareOutcome::Identical, SyncStatus::SafeInNest),
        }
    } else {
        let latest_ts = latest.created_at.unix_timestamp();
        if req.local_modified_at > latest_ts {
            (CompareOutcome::Push, SyncStatus::Flying)
        } else if req.local_modified_at < latest_ts {
            (CompareOutcome::Pull, SyncStatus::Flying)
        } else {
            (CompareOutcome::Conflict, SyncStatus::ChillyEgg)
        }
    };

    // Persist the sync baseline / status for this Bird/Clutch pair.
    match outcome {
        CompareOutcome::Identical => {
            sync_to_egg(&state, bird_id, clutch.id, &latest).await?;
        }
        CompareOutcome::Pull | CompareOutcome::Push => {
            if sync.is_none() {
                // No common baseline yet; record the local state as the baseline.
                state
                    .sync()
                    .set_baseline(
                        bird_id,
                        clutch.id,
                        None,
                        Some(&req.local_hash),
                        Some(
                            OffsetDateTime::from_unix_timestamp(req.local_modified_at).map_err(
                                |e| AppError::Validation(format!("invalid local_modified_at: {e}")),
                            )?,
                        ),
                        new_status,
                    )
                    .await?;
            } else {
                state
                    .sync()
                    .set_status(bird_id, clutch.id, new_status)
                    .await?;
            }
        }
        CompareOutcome::Conflict => {
            if sync.is_none() {
                // No baseline means we cannot resolve automatically; the Bird must
                // use the resolve endpoint.
                state
                    .sync()
                    .set_status(bird_id, clutch.id, SyncStatus::ChillyEgg)
                    .await?;
            } else {
                state
                    .sync()
                    .set_status(bird_id, clutch.id, SyncStatus::ChillyEgg)
                    .await?;
            }
        }
        CompareOutcome::NoEggs => {}
    }

    let last_synced_egg = if let Some(ref sync_state) = sync {
        if let Some(egg_id) = sync_state.last_egg_id {
            state.eggs().find_in_clutch(clutch.id, egg_id).await?
        } else {
            None
        }
    } else {
        None
    };

    Ok(Json(CompareResponse {
        outcome,
        status: new_status,
        clutch_id: clutch.id,
        latest_egg: Some(latest),
        last_synced_egg,
    }))
}

/// `POST /api/clutches/{game_id}/resolve` — resolve a Chilly Egg conflict.
pub async fn resolve(
    State(state): State<AppState>,
    Path(game_id): Path<String>,
    auth: AuthContext,
    Json(req): Json<ResolveRequest>,
) -> AppResult<Json<ResolveResponse>> {
    validate_game_id(&game_id)?;
    let bird_id = resolve_bird_id(&auth, req.bird_id)?;
    validate_bird_owns(&state, auth.flock_id, bird_id).await?;

    let clutch = state
        .clutches()
        .find_by_game(auth.flock_id, &game_id)
        .await?
        .ok_or(AppError::NotFound)?;

    match req.resolution {
        Resolution::Nest => {
            let egg_id = req.egg_id;
            let egg = if let Some(egg_id) = egg_id {
                state
                    .eggs()
                    .find_in_clutch(clutch.id, egg_id)
                    .await?
                    .ok_or(AppError::NotFound)?
            } else {
                fetch_latest_egg(&state, clutch.id)
                    .await?
                    .ok_or(AppError::NotFound)?
            };
            sync_to_egg(&state, bird_id, clutch.id, &egg).await?;
            Ok(Json(ResolveResponse {
                status: SyncStatus::SafeInNest,
                baseline_egg: Some(egg),
            }))
        }
        Resolution::Local => {
            let local_hash = req.local_hash.ok_or_else(|| {
                AppError::Validation("local_hash is required for local resolution".to_string())
            })?;
            let local_modified_at = req.local_modified_at.ok_or_else(|| {
                AppError::Validation(
                    "local_modified_at is required for local resolution".to_string(),
                )
            })?;
            state
                .sync()
                .set_baseline(
                    bird_id,
                    clutch.id,
                    None,
                    Some(&local_hash),
                    Some(
                        OffsetDateTime::from_unix_timestamp(local_modified_at).map_err(|e| {
                            AppError::Validation(format!("invalid local_modified_at: {e}"))
                        })?,
                    ),
                    SyncStatus::SafeInNest,
                )
                .await?;
            Ok(Json(ResolveResponse {
                status: SyncStatus::SafeInNest,
                baseline_egg: None,
            }))
        }
    }
}
