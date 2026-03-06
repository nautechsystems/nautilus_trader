use serde::Deserialize;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize)]
pub(crate) struct TardisErrorResponse {
    pub code: u64,
    pub message: String,
}

/// HTTP errors for the Tardis HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Tardis API error [{code}]: {message}")]
    ApiError {
        status: u16,
        code: u64,
        message: String,
    },

    #[error("Failed to parse response body as JSON: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Failed to parse response as Tardis type: {0}")]
    ResponseParse(String),
}
