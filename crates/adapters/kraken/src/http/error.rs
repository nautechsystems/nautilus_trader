//! Error types for Kraken HTTP client operations.

use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum KrakenHttpError {
    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("API error: {}", format_api_errors(.0))]
    ApiError(Vec<String>),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Missing credentials")]
    MissingCredentials,
}

/// Formats API error messages, handling empty error arrays.
fn format_api_errors(errors: &[String]) -> String {
    if errors.is_empty() {
        "unknown error (empty error list)".to_string()
    } else {
        errors.join(", ")
    }
}

impl From<anyhow::Error> for KrakenHttpError {
    fn from(err: anyhow::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}
