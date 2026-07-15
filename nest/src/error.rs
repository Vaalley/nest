//! Central error type and the JSON error envelope used across the API.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// Convenience alias for fallible operations in the server.
pub type AppResult<T> = Result<T, AppError>;

/// The single error type surfaced by the application.
///
/// Each variant maps to a stable HTTP status code and a machine-readable
/// `code` in the JSON envelope so clients (the Bird) can branch on it.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("resource not found")]
    NotFound,

    #[error("invalid request: {0}")]
    Validation(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("rate limit exceeded")]
    RateLimited,

    #[error("conflict: {0}")]
    Conflict(String),

    #[error(transparent)]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// HTTP status code for this error.
    pub fn status(&self) -> StatusCode {
        match self {
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Validation(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::Config(_)
            | AppError::Database(_)
            | AppError::Migration(_)
            | AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable, machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Config(_) => "config_error",
            AppError::NotFound => "not_found",
            AppError::Validation(_) => "validation_error",
            AppError::Unauthorized => "unauthorized",
            AppError::Forbidden => "forbidden",
            AppError::RateLimited => "rate_limited",
            AppError::Conflict(_) => "conflict",
            AppError::Database(_) | AppError::Migration(_) => "database_error",
            AppError::Internal(_) => "internal_error",
        }
    }

    /// Client-safe message. Internal details are hidden behind a generic
    /// message and logged instead of leaked over the wire.
    fn client_message(&self) -> String {
        match self {
            AppError::Database(_) | AppError::Migration(_) | AppError::Internal(_) => {
                "an internal error occurred".to_string()
            }
            other => other.to_string(),
        }
    }
}

/// The JSON body returned for every error response.
#[derive(Debug, Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[derive(Debug, Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();

        // Log the full detail server-side; only expose a safe message.
        if status.is_server_error() {
            tracing::error!(error = %self, code = self.code(), "request failed");
        } else {
            tracing::debug!(error = %self, code = self.code(), "request rejected");
        }

        let body = ErrorEnvelope {
            error: ErrorBody {
                code: self.code(),
                message: self.client_message(),
            },
        };

        (status, Json(body)).into_response()
    }
}
