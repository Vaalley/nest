//! HTTP routing for the Nest server.

pub mod bird;
pub mod clutch;
pub mod flock;
pub mod health;

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

/// Maximum request body size accepted for Egg uploads (50 MB).
///
/// `Multipart` respects `DefaultBodyLimit`, so applying this to the `lay`
/// route caps upload size while keeping the default 2 MB limit on JSON routes.
const MAX_UPLOAD_SIZE: usize = 50 * 1024 * 1024;

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
        .route(
            "/api/clutches/:game_id/lay",
            post(clutch::lay).layer(DefaultBodyLimit::max(MAX_UPLOAD_SIZE)),
        )
        .route("/api/clutches/:game_id/hatch/:egg_id", get(clutch::hatch))
        .route(
            "/api/clutches/:game_id/eggs/:egg_id",
            delete(clutch::delete),
        )
        .route("/api/clutches/:game_id/compare", post(clutch::compare))
        .route("/api/clutches/:game_id/resolve", post(clutch::resolve))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}
