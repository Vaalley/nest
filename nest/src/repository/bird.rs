//! Persistence for the `Bird` aggregate (registered devices).

use std::str::FromStr;

use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use nest_shared::domain::{Bird, Platform};

use super::{opt_ts_to_datetime, parse_uuid, ts_to_datetime};
use crate::error::AppResult;

#[derive(Debug, FromRow)]
struct BirdRow {
    id: String,
    flock_id: String,
    name: String,
    platform: String,
    last_seen: Option<i64>,
    created_at: i64,
}

impl BirdRow {
    fn into_domain(self) -> AppResult<Bird> {
        Ok(Bird {
            id: parse_uuid(&self.id)?,
            flock_id: parse_uuid(&self.flock_id)?,
            name: self.name,
            platform: Platform::from_str(&self.platform).unwrap_or(Platform::Other),
            last_seen: opt_ts_to_datetime(self.last_seen)?,
            created_at: ts_to_datetime(self.created_at)?,
        })
    }
}

/// Repository over the `birds` table.
#[derive(Debug, Clone)]
pub struct BirdRepository {
    pool: SqlitePool,
}

impl BirdRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Register a new Bird for a Flock.
    pub async fn create(&self, flock_id: Uuid, name: &str, platform: Platform) -> AppResult<Bird> {
        let id = Uuid::new_v4();
        let row = sqlx::query_as::<_, BirdRow>(
            "INSERT INTO birds (id, flock_id, name, platform) \
             VALUES (?1, ?2, ?3, ?4) \
             RETURNING id, flock_id, name, platform, last_seen, created_at",
        )
        .bind(id.to_string())
        .bind(flock_id.to_string())
        .bind(name)
        .bind(platform.as_str())
        .fetch_one(&self.pool)
        .await?;

        row.into_domain()
    }

    /// List all Birds belonging to a Flock, newest first.
    pub async fn list_by_flock(&self, flock_id: Uuid) -> AppResult<Vec<Bird>> {
        let rows = sqlx::query_as::<_, BirdRow>(
            "SELECT id, flock_id, name, platform, last_seen, created_at \
             FROM birds WHERE flock_id = ?1 ORDER BY created_at DESC",
        )
        .bind(flock_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(BirdRow::into_domain).collect()
    }

    /// Fetch a single Bird scoped to its owning Flock.
    pub async fn find_for_flock(&self, flock_id: Uuid, id: Uuid) -> AppResult<Option<Bird>> {
        let row = sqlx::query_as::<_, BirdRow>(
            "SELECT id, flock_id, name, platform, last_seen, created_at \
             FROM birds WHERE id = ?1 AND flock_id = ?2",
        )
        .bind(id.to_string())
        .bind(flock_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.map(BirdRow::into_domain).transpose()
    }

    /// Update a Bird's `last_seen` timestamp to now.
    pub async fn touch_last_seen(&self, id: Uuid) -> AppResult<()> {
        sqlx::query("UPDATE birds SET last_seen = unixepoch() WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
