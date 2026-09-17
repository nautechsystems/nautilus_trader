// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Error type for the Kalshi HTTP client.

use nautilus_network::http::HttpClientError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The result type for Kalshi HTTP operations.
///
/// The error type is a defaulted parameter, so a caller can substitute a more precise error.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The `{code, message, details}` body the Kalshi API returns on a failed request.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiApiErrorBody {
    /// The machine-readable error code, when the exchange supplies one.
    pub code: Option<String>,
    /// The human-readable error message.
    pub message: Option<String>,
    /// Additional details about the error, when available.
    pub details: Option<String>,
}

/// Errors returned by the Kalshi HTTP client.
#[derive(Clone, Debug, Error)]
pub enum Error {
    /// No credential was available for an authenticated request.
    #[error("Kalshi request requires credentials: {0}")]
    MissingCredential(String),
    /// A request could not be signed.
    #[error("Kalshi request signature failed: {0}")]
    Signature(String),
    /// The request failed at the transport layer.
    #[error("HTTP request failed: {0}")]
    HttpClient(String),
    /// The exchange rejected the request with a non-success status.
    #[error("Kalshi API error {status}: {message}")]
    Http {
        /// The HTTP status code.
        status: u16,
        /// The error message, taken from the response body when it decodes.
        message: String,
    },
    /// The exchange rate-limited the request.
    #[error("Kalshi rate limited the request: {message}")]
    RateLimited {
        /// The error message, taken from the response body when it decodes.
        message: String,
    },
    /// A response body did not decode.
    #[error("JSON decode error: {0}")]
    Serde(String),
    /// A URL could not be parsed.
    #[error("Invalid URL: {0}")]
    UrlParse(String),
    /// A paginated request did not terminate within its budget.
    #[error("Kalshi pagination did not terminate: {0}")]
    Pagination(String),
}

impl Error {
    /// Returns whether retrying the same request can succeed.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimited { .. } | Self::HttpClient(_) => true,
            Self::Http { status, .. } => *status == 429 || *status >= 500,
            Self::MissingCredential(_)
            | Self::Signature(_)
            | Self::Serde(_)
            | Self::UrlParse(_)
            | Self::Pagination(_) => false,
        }
    }

    /// Builds an error from a non-success status and the raw response body.
    #[must_use]
    pub fn from_status_code(status: u16, body: &str) -> Self {
        let message = match serde_json::from_str::<KalshiApiErrorBody>(body) {
            Ok(decoded) => {
                let code = decoded.code.unwrap_or_default();
                let message = decoded.message.unwrap_or_else(|| body.to_string());
                let details = decoded.details.unwrap_or_default();

                match (code.is_empty(), details.is_empty()) {
                    (true, true) => message,
                    (false, true) => format!("{code}: {message}"),
                    (true, false) => format!("{message}: {details}"),
                    (false, false) => format!("{code}: {message}: {details}"),
                }
            }
            Err(_) => body.trim().to_string(),
        };

        if status == 429 {
            return Self::RateLimited { message };
        }

        Self::Http { status, message }
    }
}

impl From<HttpClientError> for Error {
    fn from(error: HttpClientError) -> Self {
        Self::HttpClient(error.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Serde(error.to_string())
    }
}

/// Returns whether an HTTP status carries no response body that a caller should decode.
#[must_use]
pub const fn is_success(status: u16) -> bool {
    status >= 200 && status < 300
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_api_error_body_is_unpacked_into_the_message() {
        let error = Error::from_status_code(
            401,
            r#"{"code":"auth_error","message":"invalid signature","details":"check the key"}"#,
        );

        assert_eq!(
            error.to_string(),
            "Kalshi API error 401: auth_error: invalid signature: check the key"
        );
        assert!(!error.is_retryable());
    }

    #[rstest]
    fn test_status_429_is_reported_as_rate_limited_and_retryable() {
        let error = Error::from_status_code(429, r#"{"message":"too many requests"}"#);

        assert!(matches!(error, Error::RateLimited { .. }));
        assert!(error.is_retryable());
        assert!(error.to_string().contains("too many requests"));
    }

    #[rstest]
    fn test_non_json_body_is_preserved_verbatim() {
        let error = Error::from_status_code(503, "  upstream unavailable  ");

        assert_eq!(
            error.to_string(),
            "Kalshi API error 503: upstream unavailable"
        );
        assert!(error.is_retryable());
    }

    #[rstest]
    fn test_server_errors_are_retryable_but_client_errors_are_not() {
        assert!(Error::from_status_code(500, "").is_retryable());
        assert!(!Error::from_status_code(400, "").is_retryable());
        assert!(!Error::Serde("bad".to_string()).is_retryable());
        assert!(Error::HttpClient("reset".to_string()).is_retryable());
    }

    #[rstest]
    fn test_is_success_covers_the_2xx_range() {
        assert!(is_success(200));
        assert!(is_success(204));
        assert!(!is_success(301));
        assert!(!is_success(404));
    }
}
