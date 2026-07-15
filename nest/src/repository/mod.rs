//! Repository layer: all SQL lives here, one module per aggregate.
//!
//! Repositories map database rows to the transport-agnostic domain models in
//! `nest_shared`. Higher layers (handlers, services) depend only on these
//! repositories, never on raw SQL.

pub mod bird;
pub mod clutch;
pub mod egg;
pub mod flock;

use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

/// Parse a UUID stored as TEXT, treating malformed values as internal errors.
fn parse_uuid(s: &str) -> AppResult<Uuid> {
    Uuid::parse_str(s).map_err(|e| AppError::Internal(format!("invalid stored uuid {s}: {e}")))
}

/// Convert a stored Unix-epoch-seconds timestamp into an `OffsetDateTime`.
fn ts_to_datetime(secs: i64) -> AppResult<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(secs)
        .map_err(|e| AppError::Internal(format!("invalid stored timestamp {secs}: {e}")))
}

/// Convert an optional stored timestamp into an optional `OffsetDateTime`.
fn opt_ts_to_datetime(secs: Option<i64>) -> AppResult<Option<OffsetDateTime>> {
    secs.map(ts_to_datetime).transpose()
}

pub use bird::BirdRepository;
pub use clutch::ClutchRepository;
pub use egg::EggRepository;
pub use flock::FlockRepository;
