//! HTTP routing for the Nest server.

pub mod health;

use axum::routing::get;
use axum::Router;

use crate::state::AppState;

/// Build the full application router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .with_state(state)
}
