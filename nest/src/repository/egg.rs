//! Persistence for the `Egg` aggregate (individual save snapshots).

use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use nest_shared::domain::Egg;

use super::{parse_uuid, ts_to_datetime};
use crate::error::AppResult;

#[derive(Debug, FromRow)]
struct EggRow {
    id: String,
    clutch_id: String,
    source_bird_id: Option<String>,
    file_hash: String,
    size_bytes: i64,
    file_path: String,
    created_at: i64,
}

impl EggRow {
    fn into_domain(self) -> AppResult<Egg> {
        let source_bird_id = match self.source_bird_id {
            Some(ref s) => Some(parse_uuid(s)?),
            None => None,
        };
        Ok(Egg {
            id: parse_uuid(&self.id)?,
            clutch_id: parse_uuid(&self.clutch_id)?,
            source_bird_id,
            file_hash: self.file_hash,
            size_bytes: self.size_bytes,
            file_path: self.file_path,
            created_at: ts_to_datetime(self.created_at)?,
        })
    }
}

/// Parameters for inserting a new Egg.
#[derive(Debug, Clone)]
pub struct NewEgg<'a> {
    pub clutch_id: Uuid,
    pub source_bird_id: Option<Uuid>,
    pub file_hash: &'a str,
    pub size_bytes: i64,
    pub file_path: &'a str,
}

/// Repository over the `eggs` table.
#[derive(Debug, Clone)]
pub struct EggRepository {
    pool: SqlitePool,
}

impl EggRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Insert a new Egg into a Clutch.
    pub async fn create(&self, new_egg: NewEgg<'_>) -> AppResult<Egg> {
        let id = Uuid::new_v4();
        let row = sqlx::query_as::<_, EggRow>(
            "INSERT INTO eggs (id, clutch_id, source_bird_id, file_hash, size_bytes, file_path) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             RETURNING id, clutch_id, source_bird_id, file_hash, size_bytes, file_path, created_at",
        )
        .bind(id.to_string())
        .bind(new_egg.clutch_id.to_string())
        .bind(new_egg.source_bird_id.map(|b| b.to_string()))
        .bind(new_egg.file_hash)
        .bind(new_egg.size_bytes)
        .bind(new_egg.file_path)
        .fetch_one(&self.pool)
        .await?;

        row.into_domain()
    }

    /// List all Eggs in a Clutch, newest first.
    pub async fn list_by_clutch(&self, clutch_id: Uuid) -> AppResult<Vec<Egg>> {
        let rows = sqlx::query_as::<_, EggRow>(
            "SELECT id, clutch_id, source_bird_id, file_hash, size_bytes, file_path, created_at \
             FROM eggs WHERE clutch_id = ?1 ORDER BY created_at DESC, id DESC",
        )
        .bind(clutch_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(EggRow::into_domain).collect()
    }

    /// Fetch a single Egg by id within a Clutch.
    pub async fn find_in_clutch(&self, clutch_id: Uuid, egg_id: Uuid) -> AppResult<Option<Egg>> {
        let row = sqlx::query_as::<_, EggRow>(
            "SELECT id, clutch_id, source_bird_id, file_hash, size_bytes, file_path, created_at \
             FROM eggs WHERE id = ?1 AND clutch_id = ?2",
        )
        .bind(egg_id.to_string())
        .bind(clutch_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.map(EggRow::into_domain).transpose()
    }

    /// Count the Eggs currently stored in a Clutch.
    pub async fn count_in_clutch(&self, clutch_id: Uuid) -> AppResult<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM eggs WHERE clutch_id = ?1")
            .bind(clutch_id.to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    /// Delete an Egg row, returning it if it existed (so callers can clean up
    /// the on-disk file). Scoped to the owning Clutch.
    pub async fn delete_in_clutch(&self, clutch_id: Uuid, egg_id: Uuid) -> AppResult<Option<Egg>> {
        let row = sqlx::query_as::<_, EggRow>(
            "DELETE FROM eggs WHERE id = ?1 AND clutch_id = ?2 \
             RETURNING id, clutch_id, source_bird_id, file_hash, size_bytes, file_path, created_at",
        )
        .bind(egg_id.to_string())
        .bind(clutch_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.map(EggRow::into_domain).transpose()
    }
}
