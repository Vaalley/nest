//! Persistence for the `Clutch` aggregate (per-game save collections).

use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use nest_shared::domain::Clutch;

use super::{parse_uuid, ts_to_datetime};
use crate::error::AppResult;

#[derive(Debug, FromRow)]
struct ClutchRow {
    id: String,
    flock_id: String,
    game_id: String,
    brood_limit: i64,
    created_at: i64,
}

impl ClutchRow {
    fn into_domain(self) -> AppResult<Clutch> {
        Ok(Clutch {
            id: parse_uuid(&self.id)?,
            flock_id: parse_uuid(&self.flock_id)?,
            game_id: self.game_id,
            brood_limit: self.brood_limit,
            created_at: ts_to_datetime(self.created_at)?,
        })
    }
}

/// Repository over the `clutches` table.
#[derive(Debug, Clone)]
pub struct ClutchRepository {
    pool: SqlitePool,
}

impl ClutchRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a Clutch for a `(flock, game)` pair.
    pub async fn create(
        &self,
        flock_id: Uuid,
        game_id: &str,
        brood_limit: i64,
    ) -> AppResult<Clutch> {
        let id = Uuid::new_v4();
        let row = sqlx::query_as::<_, ClutchRow>(
            "INSERT INTO clutches (id, flock_id, game_id, brood_limit) \
             VALUES (?1, ?2, ?3, ?4) \
             RETURNING id, flock_id, game_id, brood_limit, created_at",
        )
        .bind(id.to_string())
        .bind(flock_id.to_string())
        .bind(game_id)
        .bind(brood_limit)
        .fetch_one(&self.pool)
        .await?;

        row.into_domain()
    }

    /// Find a Clutch by its `(flock, game)` pair.
    pub async fn find_by_game(&self, flock_id: Uuid, game_id: &str) -> AppResult<Option<Clutch>> {
        let row = sqlx::query_as::<_, ClutchRow>(
            "SELECT id, flock_id, game_id, brood_limit, created_at \
             FROM clutches WHERE flock_id = ?1 AND game_id = ?2",
        )
        .bind(flock_id.to_string())
        .bind(game_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(ClutchRow::into_domain).transpose()
    }

    /// Return an existing Clutch for a `(flock, game)` pair, creating it if necessary.
    pub async fn find_or_create(
        &self,
        flock_id: Uuid,
        game_id: &str,
        brood_limit: i64,
    ) -> AppResult<Clutch> {
        if let Some(clutch) = self.find_by_game(flock_id, game_id).await? {
            return Ok(clutch);
        }
        self.create(flock_id, game_id, brood_limit).await
    }

    /// List all Clutches for a Flock, newest first.
    pub async fn list_by_flock(&self, flock_id: Uuid) -> AppResult<Vec<Clutch>> {
        let rows = sqlx::query_as::<_, ClutchRow>(
            "SELECT id, flock_id, game_id, brood_limit, created_at \
             FROM clutches WHERE flock_id = ?1 ORDER BY created_at DESC",
        )
        .bind(flock_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(ClutchRow::into_domain).collect()
    }
}
