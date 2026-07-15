//! Binary entrypoint for the Nest server.

use nest_server::{init_tracing, run, Config};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("configuration error: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    init_tracing(&config.log_level);
    tracing::debug!(?config, "loaded configuration");

    match run(config).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "fatal error");
            std::process::ExitCode::FAILURE
        }
    }
}
