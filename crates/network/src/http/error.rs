//! HTTP client error types.

/// Errors returned by the HTTP client.
///
/// Includes generic transport errors, timeouts, and proxy configuration errors.
#[derive(thiserror::Error, Debug)]
pub enum HttpClientError {
    #[error("HTTP error occurred: {0}")]
    Error(String),

    #[error("HTTP request timed out: {0}")]
    TimeoutError(String),

    #[error("Invalid proxy URL: {0}")]
    InvalidProxy(String),

    #[error("Failed to build HTTP client: {0}")]
    ClientBuildError(String),
}

impl From<reqwest::Error> for HttpClientError {
    fn from(source: reqwest::Error) -> Self {
        if source.is_timeout() {
            Self::TimeoutError(source.to_string())
        } else {
            Self::Error(source.to_string())
        }
    }
}

impl From<String> for HttpClientError {
    fn from(value: String) -> Self {
        Self::Error(value)
    }
}
