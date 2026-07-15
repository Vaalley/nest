//! Health / readiness endpoint.

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
    database: &'static str,
}

/// `GET /health` — liveness + database readiness probe.
pub async fn health(State(state): State<AppState>) -> AppResult<Json<HealthResponse>> {
    // Verify the database is reachable so this doubles as a readiness check.
    sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(state.pool())
        .await?;

    Ok(Json(HealthResponse {
        status: "ok",
        service: "nest",
        version: env!("CARGO_PKG_VERSION"),
        database: "ok",
    }))
}
