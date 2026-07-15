//! SQLite connection pool creation and migration handling.

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::config::Config;
use crate::error::AppResult;

/// Embedded migrations, compiled from the `migrations/` directory.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Create a connection pool, ensuring the data directory exists first.
pub async fn connect(config: &Config) -> AppResult<SqlitePool> {
    // Ensure the parent directory for the SQLite file exists.
    if let Some(parent) = config.db_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                crate::error::AppError::Internal(format!(
                    "failed to create data dir {}: {e}",
                    parent.display()
                ))
            })?;
        }
    }

    let options = SqliteConnectOptions::from_str(&config.database_url())?
        // WAL keeps reads/writes concurrent and is friendly to our low footprint.
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    Ok(pool)
}

/// Apply all pending migrations to the database.
pub async fn run_migrations(pool: &SqlitePool) -> AppResult<()> {
    MIGRATOR.run(pool).await?;
    Ok(())
}
