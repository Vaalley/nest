//! HTTP routing for the Nest server.

pub mod bird;
pub mod flock;
pub mod health;

use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

/// Build the full application router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/api/flock/register", post(flock::register))
        .route("/api/flock/login", post(flock::login))
        .route("/api/flock/me", get(flock::me))
        .route("/api/birds", post(bird::register).get(bird::list))
        .with_state(state)
}
