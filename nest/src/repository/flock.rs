//! Persistence for the `Flock` aggregate (user accounts).

use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use nest_shared::domain::Flock;

use super::{parse_uuid, ts_to_datetime};
use crate::error::AppResult;

/// Raw database row for the `flocks` table.
#[derive(Debug, FromRow)]
struct FlockRow {
    id: String,
    username: String,
    password_hash: String,
    created_at: i64,
}

impl FlockRow {
    fn into_domain(self) -> AppResult<Flock> {
        Ok(Flock {
            id: parse_uuid(&self.id)?,
            username: self.username,
            created_at: ts_to_datetime(self.created_at)?,
        })
    }
}

/// The credentials needed by the auth layer (Phase 2), kept out of the
/// public `Flock` model so password hashes never leak into API responses.
#[derive(Debug)]
pub struct FlockCredentials {
    pub flock: Flock,
    pub password_hash: String,
}

/// Repository over the `flocks` table.
#[derive(Debug, Clone)]
pub struct FlockRepository {
    pool: SqlitePool,
}

impl FlockRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Insert a new Flock and return the created domain model.
    pub async fn create(&self, username: &str, password_hash: &str) -> AppResult<Flock> {
        let id = Uuid::new_v4();
        let row = sqlx::query_as::<_, FlockRow>(
            "INSERT INTO flocks (id, username, password_hash) \
             VALUES (?1, ?2, ?3) \
             RETURNING id, username, password_hash, created_at",
        )
        .bind(id.to_string())
        .bind(username)
        .bind(password_hash)
        .fetch_one(&self.pool)
        .await?;

        row.into_domain()
    }

    /// Look up a Flock by id.
    pub async fn find_by_id(&self, id: Uuid) -> AppResult<Option<Flock>> {
        let row = sqlx::query_as::<_, FlockRow>(
            "SELECT id, username, password_hash, created_at FROM flocks WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.map(FlockRow::into_domain).transpose()
    }

    /// Look up a Flock (and its password hash) by username.
    pub async fn find_credentials_by_username(
        &self,
        username: &str,
    ) -> AppResult<Option<FlockCredentials>> {
        let row = sqlx::query_as::<_, FlockRow>(
            "SELECT id, username, password_hash, created_at FROM flocks WHERE username = ?1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let password_hash = row.password_hash.clone();
                Ok(Some(FlockCredentials {
                    flock: row.into_domain()?,
                    password_hash,
                }))
            }
            None => Ok(None),
        }
    }

    /// Whether a username is already taken.
    pub async fn username_exists(&self, username: &str) -> AppResult<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM flocks WHERE username = ?1")
            .bind(username)
            .fetch_one(&self.pool)
            .await?;
        Ok(count > 0)
    }
}
