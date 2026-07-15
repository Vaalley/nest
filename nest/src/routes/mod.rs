//! HTTP routing for the Nest server.

pub mod bird;
pub mod clutch;
pub mod flock;
pub mod health;

use axum::routing::{delete, get, post};
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
        .route("/api/clutches", get(clutch::list))
        .route("/api/clutches/:game_id/eggs", get(clutch::eggs))
        .route("/api/clutches/:game_id/lay", post(clutch::lay))
        .route("/api/clutches/:game_id/hatch/:egg_id", get(clutch::hatch))
        .route(
            "/api/clutches/:game_id/eggs/:egg_id",
            delete(clutch::delete),
        )
        .route("/api/clutches/:game_id/compare", post(clutch::compare))
        .route("/api/clutches/:game_id/resolve", post(clutch::resolve))
        .with_state(state)
}
