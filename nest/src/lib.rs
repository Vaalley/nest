//! The Nest server library.
//!
//! Exposes the building blocks (config, state, router, migrations) so both the
//! binary and integration tests can construct a fully-wired application.

pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod rate_limit;
pub mod repository;
pub mod routes;
pub mod state;

use std::net::SocketAddr;

use tokio::net::TcpListener;

pub use config::Config;
pub use error::{AppError, AppResult};
pub use state::AppState;

/// Initialise process-wide tracing from the configured log level.
///
/// Safe to call once at startup; uses `try_init` so repeated calls (e.g. in
/// tests) do not panic.
pub fn init_tracing(log_level: &str) {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}

/// Connect to the database, run migrations, and build the wired application
/// state. Shared by the binary and integration tests.
pub async fn build_state(config: Config) -> AppResult<AppState> {
    let pool = db::connect(&config).await?;
    db::run_migrations(&pool).await?;
    Ok(AppState::new(config, pool))
}

/// Run the server until a shutdown signal is received.
pub async fn run(config: Config) -> AppResult<()> {
    let bind_addr = config.bind_addr;
    let state = build_state(config).await?;
    let app = routes::router(state).into_make_service_with_connect_info::<SocketAddr>();

    let listener = TcpListener::bind(bind_addr)
        .await
        .map_err(|e| AppError::Internal(format!("failed to bind {bind_addr}: {e}")))?;

    tracing::info!(%bind_addr, "Nest is listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| AppError::Internal(format!("server error: {e}")))?;

    tracing::info!("Nest has shut down gracefully");
    Ok(())
}

/// Resolve when the process receives Ctrl-C or (on Unix) SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sig) = signal(SignalKind::terminate()) {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received");
}
