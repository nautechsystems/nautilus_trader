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

//! Betfair HTTP client error types.

use std::fmt::Display;

/// Represents HTTP client errors for the Betfair adapter.
#[derive(Debug, Clone)]
pub enum BetfairHttpError {
    /// Missing API credentials.
    MissingCredentials,
    /// Login failed with a non-success status.
    LoginFailed { status: String },
    /// Betfair JSON-RPC error with code and message.
    BetfairError { code: i64, message: String },
    /// JSON serialization/deserialization error.
    JsonError(String),
    /// Network-related error.
    NetworkError(String),
    /// Request timeout.
    Timeout(String),
    /// Request canceled.
    Canceled(String),
    /// Unexpected HTTP status.
    UnexpectedStatus { status: u16, body: String },
}

impl Display for BetfairHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCredentials => write!(f, "Missing API credentials"),
            Self::LoginFailed { status } => write!(f, "Login failed: {status}"),
            Self::BetfairError { code, message } => {
                write!(f, "Betfair error {code}: {message}")
            }
            Self::JsonError(msg) => write!(f, "JSON error: {msg}"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
            Self::Canceled(msg) => write!(f, "Canceled: {msg}"),
            Self::UnexpectedStatus { status, body } => {
                write!(f, "Unexpected status {status}: {body}")
            }
        }
    }
}

impl std::error::Error for BetfairHttpError {}

impl From<serde_json::Error> for BetfairHttpError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

impl From<anyhow::Error> for BetfairHttpError {
    fn from(error: anyhow::Error) -> Self {
        Self::NetworkError(error.to_string())
    }
}

impl BetfairHttpError {
    /// Returns whether this error is retryable.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::NetworkError(_) | Self::Timeout(_) => true,
            Self::UnexpectedStatus { status, .. } => *status >= 500 || *status == 429,
            Self::BetfairError { code, .. } => is_retryable_error_code(*code),
            _ => false,
        }
    }
}

/// Returns whether a Betfair JSON-RPC error code is retryable.
///
/// Retryable codes are transient server-side errors. Permanent errors
/// (invalid input, insufficient funds, etc.) should not be retried.
fn is_retryable_error_code(code: i64) -> bool {
    matches!(
        code,
        -32099 // Unexpected internal server error
        | -32700 // JSON parse error (may be transient corruption)
    )
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_display_missing_credentials() {
        let err = BetfairHttpError::MissingCredentials;
        assert_eq!(err.to_string(), "Missing API credentials");
    }

    #[rstest]
    fn test_display_login_failed() {
        let err = BetfairHttpError::LoginFailed {
            status: "CERT_AUTH_REQUIRED".to_string(),
        };
        assert_eq!(err.to_string(), "Login failed: CERT_AUTH_REQUIRED");
    }

    #[rstest]
    fn test_display_betfair_error() {
        let err = BetfairHttpError::BetfairError {
            code: -32600,
            message: "Invalid request".to_string(),
        };
        assert_eq!(err.to_string(), "Betfair error -32600: Invalid request");
    }

    #[rstest]
    fn test_display_unexpected_status() {
        let err = BetfairHttpError::UnexpectedStatus {
            status: 403,
            body: "Forbidden".to_string(),
        };
        assert_eq!(err.to_string(), "Unexpected status 403: Forbidden");
    }

    #[rstest]
    #[case(BetfairHttpError::NetworkError("timeout".to_string()), true)]
    #[case(BetfairHttpError::Timeout("read".to_string()), true)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 500, body: String::new() }, true)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 429, body: String::new() }, true)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 403, body: String::new() }, false)]
    #[case(BetfairHttpError::MissingCredentials, false)]
    #[case(BetfairHttpError::LoginFailed { status: "FAIL".to_string() }, false)]
    #[case(BetfairHttpError::JsonError("bad".to_string()), false)]
    fn test_is_retryable(#[case] error: BetfairHttpError, #[case] expected: bool) {
        assert_eq!(error.is_retryable(), expected);
    }

    #[rstest]
    fn test_from_serde_error() {
        let json_err = serde_json::from_str::<String>("not json").unwrap_err();
        let err: BetfairHttpError = json_err.into();
        assert!(matches!(err, BetfairHttpError::JsonError(_)));
    }

    #[rstest]
    fn test_from_anyhow_error() {
        let anyhow_err = anyhow::anyhow!("network failure");
        let err: BetfairHttpError = anyhow_err.into();
        assert!(matches!(err, BetfairHttpError::NetworkError(_)));
    }
}
