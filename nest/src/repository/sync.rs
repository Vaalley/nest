//! Persistence for per-Bird, per-Clutch sync state.
//!
//! `bird_clutch_sync` stores the last-known-common state (the "baseline") between
//! a Bird and the Nest. This baseline is what lets the server detect divergence
//! and surface "Chilly Egg" conflicts.

use std::str::FromStr;

use sqlx::{FromRow, SqlitePool};
use time::OffsetDateTime;
use uuid::Uuid;

use nest_shared::domain::SyncStatus;

use super::{opt_ts_to_datetime, parse_uuid, ts_to_datetime};
use crate::error::{AppError, AppResult};

/// Raw database row for the `bird_clutch_sync` table.
#[derive(Debug, FromRow)]
struct SyncRow {
    bird_id: String,
    clutch_id: String,
    last_egg_id: Option<String>,
    last_synced_hash: Option<String>,
    last_synced_at: Option<i64>,
    status: String,
    created_at: i64,
    updated_at: i64,
}

impl SyncRow {
    fn into_domain(self) -> AppResult<SyncState> {
        Ok(SyncState {
            bird_id: parse_uuid(&self.bird_id)?,
            clutch_id: parse_uuid(&self.clutch_id)?,
            last_egg_id: self.last_egg_id.as_deref().map(parse_uuid).transpose()?,
            last_synced_hash: self.last_synced_hash,
            last_synced_at: opt_ts_to_datetime(self.last_synced_at)?,
            status: SyncStatus::from_str(&self.status)
                .map_err(|e| AppError::Internal(format!("invalid stored sync status: {e}")))?,
            created_at: ts_to_datetime(self.created_at)?,
            updated_at: ts_to_datetime(self.updated_at)?,
        })
    }
}

/// The persisted sync baseline for a single `(Bird, Clutch)` pair.
#[derive(Debug, Clone)]
pub struct SyncState {
    pub bird_id: Uuid,
    pub clutch_id: Uuid,
    /// The last Egg known to be identical on both sides, if any.
    pub last_egg_id: Option<Uuid>,
    /// Hash of the last known common state.
    pub last_synced_hash: Option<String>,
    /// Timestamp of the last known common state.
    pub last_synced_at: Option<OffsetDateTime>,
    /// Current sync status for this Bird/Clutch pair.
    pub status: SyncStatus,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// Repository over the `bird_clutch_sync` table.
#[derive(Debug, Clone)]
pub struct SyncRepository {
    pool: SqlitePool,
}

impl SyncRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Fetch the sync state for a specific Bird and Clutch.
    pub async fn find(&self, bird_id: Uuid, clutch_id: Uuid) -> AppResult<Option<SyncState>> {
        let row = sqlx::query_as::<_, SyncRow>(
            "SELECT bird_id, clutch_id, last_egg_id, last_synced_hash, last_synced_at, \
             status, created_at, updated_at \
             FROM bird_clutch_sync \
             WHERE bird_id = ?1 AND clutch_id = ?2",
        )
        .bind(bird_id.to_string())
        .bind(clutch_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.map(SyncRow::into_domain).transpose()
    }

    /// Upsert the full sync baseline for a Bird/Clutch pair.
    pub async fn set_baseline(
        &self,
        bird_id: Uuid,
        clutch_id: Uuid,
        last_egg_id: Option<Uuid>,
        last_synced_hash: Option<&str>,
        last_synced_at: Option<OffsetDateTime>,
        status: SyncStatus,
    ) -> AppResult<SyncState> {
        let row = sqlx::query_as::<_, SyncRow>(
            "INSERT INTO bird_clutch_sync \
             (bird_id, clutch_id, last_egg_id, last_synced_hash, last_synced_at, status, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch(), unixepoch()) \
             ON CONFLICT(bird_id, clutch_id) DO UPDATE SET \
               last_egg_id = excluded.last_egg_id, \
               last_synced_hash = excluded.last_synced_hash, \
               last_synced_at = excluded.last_synced_at, \
               status = excluded.status, \
               updated_at = unixepoch() \
             RETURNING bird_id, clutch_id, last_egg_id, last_synced_hash, last_synced_at, status, created_at, updated_at",
        )
        .bind(bird_id.to_string())
        .bind(clutch_id.to_string())
        .bind(last_egg_id.map(|id| id.to_string()))
        .bind(last_synced_hash)
        .bind(last_synced_at.map(|t| t.unix_timestamp()))
        .bind(status.as_str())
        .fetch_one(&self.pool)
        .await?;

        row.into_domain()
    }

    /// Upsert just the status column for a Bird/Clutch pair.
    pub async fn set_status(
        &self,
        bird_id: Uuid,
        clutch_id: Uuid,
        status: SyncStatus,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO bird_clutch_sync (bird_id, clutch_id, status, created_at, updated_at) \
             VALUES (?1, ?2, ?3, unixepoch(), unixepoch()) \
             ON CONFLICT(bird_id, clutch_id) DO UPDATE SET \
               status = excluded.status, \
               updated_at = unixepoch()",
        )
        .bind(bird_id.to_string())
        .bind(clutch_id.to_string())
        .bind(status.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Return the sync status for a specific Bird and Clutch, defaulting to
    /// `SafeInNest` when no row exists.
    pub async fn status_for_bird(&self, bird_id: Uuid, clutch_id: Uuid) -> AppResult<SyncStatus> {
        match self.find(bird_id, clutch_id).await? {
            Some(state) => Ok(state.status),
            None => Ok(SyncStatus::SafeInNest),
        }
    }

    /// Aggregate status across every Bird in a Flock for a given Clutch.
    /// The "worst" status wins: `chilly_egg` > `flying` > `safe_in_nest`.
    pub async fn aggregate_status_for_clutch(
        &self,
        flock_id: Uuid,
        clutch_id: Uuid,
    ) -> AppResult<SyncStatus> {
        let statuses: Vec<String> = sqlx::query_scalar(
            "SELECT bcs.status \
             FROM bird_clutch_sync bcs \
             JOIN birds b ON b.id = bcs.bird_id \
             WHERE bcs.clutch_id = ?1 AND b.flock_id = ?2",
        )
        .bind(clutch_id.to_string())
        .bind(flock_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        if statuses.iter().any(|s| s == SyncStatus::ChillyEgg.as_str()) {
            return Ok(SyncStatus::ChillyEgg);
        }
        if statuses.iter().any(|s| s == SyncStatus::Flying.as_str()) {
            return Ok(SyncStatus::Flying);
        }
        Ok(SyncStatus::SafeInNest)
    }

    /// Delete any sync baseline for a Bird/Clutch pair.
    pub async fn delete(&self, bird_id: Uuid, clutch_id: Uuid) -> AppResult<()> {
        sqlx::query("DELETE FROM bird_clutch_sync WHERE bird_id = ?1 AND clutch_id = ?2")
            .bind(bird_id.to_string())
            .bind(clutch_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
