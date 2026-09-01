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
    /// Betfair JSON-RPC error with its optional API exception details.
    BetfairError {
        code: i64,
        message: String,
        api_error_code: Option<String>,
        api_error_details: Option<String>,
    },
    /// JSON serialization/deserialization error.
    JsonError(String),
    /// Malformed JSON-RPC response received after dispatch.
    ResponseError(String),
    /// Network-related error.
    NetworkError(String),
    /// Invalid client configuration.
    InvalidConfiguration(String),
    /// Request timeout.
    Timeout(String),
    /// Request canceled.
    Canceled(String),
    /// Unexpected HTTP status.
    UnexpectedStatus { status: u16, body: String },
    /// A later retry failed after an earlier attempt had an unknown outcome.
    OrderRequestAmbiguous(String),
}

impl Display for BetfairHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCredentials => write!(f, "Missing API credentials"),
            Self::LoginFailed { status } => write!(f, "Login failed: {status}"),
            Self::BetfairError {
                code,
                message,
                api_error_code,
                api_error_details,
            } => match (
                api_error_code.as_deref(),
                api_error_details
                    .as_deref()
                    .filter(|details| !details.is_empty()),
            ) {
                (Some(error_code), Some(details)) => {
                    write!(
                        f,
                        "Betfair error {code}: {message} ({error_code}: {details})"
                    )
                }
                (Some(error_code), None) => {
                    write!(f, "Betfair error {code}: {message} ({error_code})")
                }
                (None, _) => write!(f, "Betfair error {code}: {message}"),
            },
            Self::JsonError(msg) => write!(f, "JSON error: {msg}"),
            Self::ResponseError(msg) => write!(f, "Response error: {msg}"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::InvalidConfiguration(msg) => write!(f, "Invalid configuration: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
            Self::Canceled(msg) => write!(f, "Canceled: {msg}"),
            Self::UnexpectedStatus { status, body } => {
                write!(f, "Unexpected status {status}: {body}")
            }
            Self::OrderRequestAmbiguous(msg) => write!(f, "Ambiguous order request: {msg}"),
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
            Self::BetfairError {
                code,
                api_error_code,
                ..
            } => api_error_code.as_deref().map_or_else(
                || is_retryable_error_code(*code),
                is_retryable_api_error_code,
            ),
            _ => false,
        }
    }

    /// Returns whether an order request can be retried with the same `customerRef`.
    #[must_use]
    pub fn is_order_retryable(&self) -> bool {
        match self {
            Self::NetworkError(_) | Self::Timeout(_) | Self::ResponseError(_) => true,
            Self::UnexpectedStatus { status, .. } => *status >= 500 || *status == 429,
            Self::BetfairError { api_error_code, .. } => api_error_code
                .as_deref()
                .is_some_and(is_order_retryable_api_error_code),
            _ => false,
        }
    }

    /// Returns whether this is a login/auth rejection from the Identity API.
    ///
    /// `keep_alive` returns this when the session is expired or unrecognised.
    /// Transient errors (network, timeout) return different variants.
    #[must_use]
    pub fn is_login_failed(&self) -> bool {
        matches!(self, Self::LoginFailed { .. })
    }

    /// Returns whether this error is a session expiry that should trigger reconnection.
    ///
    /// Session errors (`NO_SESSION`, `INVALID_SESSION_INFORMATION`) occur every
    /// 12-24 hours and are resolved by re-authenticating.
    #[must_use]
    pub fn is_session_error(&self) -> bool {
        match self {
            Self::BetfairError { api_error_code, .. } => matches!(
                api_error_code.as_deref(),
                Some("NO_SESSION" | "INVALID_SESSION_INFORMATION")
            ),
            _ => false,
        }
    }

    /// Returns whether this error is a rate limit (`TOO_MANY_REQUESTS`) error.
    #[must_use]
    pub fn is_rate_limit_error(&self) -> bool {
        match self {
            Self::BetfairError { api_error_code, .. } => {
                api_error_code.as_deref() == Some("TOO_MANY_REQUESTS")
            }
            Self::UnexpectedStatus { status, .. } => *status == 429,
            _ => false,
        }
    }

    /// Returns whether this error leaves an order request in an ambiguous state.
    ///
    /// When true, the request may have been processed by Betfair despite the
    /// error. Callers must NOT emit `OrderRejected` for ambiguous errors
    /// because the order may be live on the exchange. The OCM stream will
    /// reconcile the order via its `customerOrderRef`.
    #[must_use]
    pub fn is_order_ambiguous(&self) -> bool {
        match self {
            Self::NetworkError(_)
            | Self::Timeout(_)
            | Self::Canceled(_)
            | Self::ResponseError(_)
            | Self::OrderRequestAmbiguous(_) => true,
            Self::UnexpectedStatus { status, .. } => *status >= 500,
            Self::BetfairError {
                code,
                api_error_code,
                ..
            } => match api_error_code.as_deref() {
                Some(api_error_code) => {
                    matches!(api_error_code, "TIMEOUT_ERROR" | "UNEXPECTED_ERROR")
                        || !is_known_api_error_code(api_error_code)
                }
                None => *code == -32603 || (-32099..=-32000).contains(code),
            },
            _ => false,
        }
    }

    /// Returns whether this error leaves order placement in an ambiguous state.
    #[must_use]
    pub fn is_order_placement_ambiguous(&self) -> bool {
        self.is_order_ambiguous()
    }
}

fn is_retryable_api_error_code(code: &str) -> bool {
    is_order_retryable_api_error_code(code) || code == "TIMEOUT_ERROR"
}

fn is_order_retryable_api_error_code(code: &str) -> bool {
    matches!(
        code,
        "TOO_MANY_REQUESTS" | "SERVICE_BUSY" | "UNEXPECTED_ERROR"
    )
}

fn is_known_api_error_code(code: &str) -> bool {
    matches!(
        code,
        "TOO_MUCH_DATA"
            | "INVALID_INPUT_DATA"
            | "INVALID_SESSION_INFORMATION"
            | "NO_APP_KEY"
            | "NO_SESSION"
            | "UNEXPECTED_ERROR"
            | "INVALID_APP_KEY"
            | "TOO_MANY_REQUESTS"
            | "SERVICE_BUSY"
            | "TIMEOUT_ERROR"
            | "REQUEST_SIZE_EXCEEDS_LIMIT"
            | "ACCESS_DENIED"
    )
}

/// Returns whether a Betfair JSON-RPC error code is retryable.
///
/// Retryable codes are transient server-side errors. Permanent errors
/// (invalid input, insufficient funds, etc.) should not be retried.
fn is_retryable_error_code(code: i64) -> bool {
    // -32099 is an unexpected internal server error,
    // and -32700 is a potentially transient JSON parse error.
    matches!(code, -32099 | -32700)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn betfair_error(code: i64, message: &str, api_error_code: Option<&str>) -> BetfairHttpError {
        BetfairHttpError::BetfairError {
            code,
            message: message.to_string(),
            api_error_code: api_error_code.map(str::to_string),
            api_error_details: None,
        }
    }

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
            api_error_code: None,
            api_error_details: None,
        };
        assert_eq!(err.to_string(), "Betfair error -32600: Invalid request");
    }

    #[rstest]
    fn test_display_betfair_api_error() {
        let err = BetfairHttpError::BetfairError {
            code: -32099,
            message: "ANGX-0001".to_string(),
            api_error_code: Some("TOO_MUCH_DATA".to_string()),
            api_error_details: Some("MaxResults must be less than or equal to 1000".to_string()),
        };
        assert_eq!(
            err.to_string(),
            "Betfair error -32099: ANGX-0001 (TOO_MUCH_DATA: MaxResults must be less than or equal to 1000)",
        );
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
    fn test_display_invalid_configuration() {
        let err = BetfairHttpError::InvalidConfiguration("bad rate".to_string());
        assert_eq!(err.to_string(), "Invalid configuration: bad rate");
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
    #[case(Some("TOO_MANY_REQUESTS"), true, true, false)]
    #[case(Some("SERVICE_BUSY"), true, true, false)]
    #[case(Some("UNEXPECTED_ERROR"), true, true, true)]
    #[case(Some("TIMEOUT_ERROR"), true, false, true)]
    #[case(Some("INVALID_INPUT_DATA"), false, false, false)]
    #[case(Some("FUTURE_ERROR"), false, false, true)]
    #[case(None, true, false, true)]
    fn test_api_error_retry_matrix(
        #[case] api_error_code: Option<&str>,
        #[case] retryable: bool,
        #[case] order_retryable: bool,
        #[case] order_ambiguous: bool,
    ) {
        let error = betfair_error(-32099, "ANGX-0001", api_error_code);

        assert_eq!(error.is_retryable(), retryable);
        assert_eq!(error.is_order_retryable(), order_retryable);
        assert_eq!(error.is_order_ambiguous(), order_ambiguous);
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

    #[rstest]
    #[case(BetfairHttpError::NetworkError("connection reset".to_string()), true)]
    #[case(BetfairHttpError::Timeout("read".to_string()), true)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 502, body: "error code: 502".to_string() }, true)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 500, body: String::new() }, true)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 429, body: String::new() }, false)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 403, body: String::new() }, false)]
    #[case(betfair_error(-32600, "Invalid", None), false)]
    #[case(betfair_error(32603, "Internal error", None), false)]
    #[case(betfair_error(32000, "Invalid positive code", None), false)]
    #[case(betfair_error(-32099, "ANGX-UNKNOWN", None), true)]
    #[case(betfair_error(-32099, "ANGX-UNKNOWN", Some("FUTURE_ERROR")), true)]
    #[case(betfair_error(-32099, "ANGX-0001", Some("INVALID_INPUT_DATA")), false)]
    #[case(BetfairHttpError::JsonError("bad".to_string()), false)]
    #[case(BetfairHttpError::MissingCredentials, false)]
    #[case(BetfairHttpError::ResponseError("truncated".to_string()), true)]
    #[case(BetfairHttpError::Canceled("shutdown".to_string()), true)]
    #[case(betfair_error(-32099, "ANGX-0004", Some("TIMEOUT_ERROR")), true)]
    #[case(betfair_error(-32099, "ANGX-0003", Some("NO_SESSION")), false)]
    fn test_is_order_ambiguous(#[case] error: BetfairHttpError, #[case] expected: bool) {
        assert_eq!(error.is_order_ambiguous(), expected);
        assert_eq!(error.is_order_placement_ambiguous(), expected);
    }

    #[rstest]
    #[case(BetfairHttpError::NetworkError("connection reset".to_string()), true)]
    #[case(BetfairHttpError::ResponseError("truncated".to_string()), true)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 502, body: String::new() }, true)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 429, body: String::new() }, true)]
    #[case(betfair_error(-32099, "ANGX-0002", Some("SERVICE_BUSY")), true)]
    #[case(betfair_error(-32099, "ANGX-0003", Some("NO_SESSION")), false)]
    #[case(betfair_error(-32099, "ANGX-0004", Some("TIMEOUT_ERROR")), false)]
    #[case(BetfairHttpError::Canceled("shutdown".to_string()), false)]
    fn test_is_order_retryable(#[case] error: BetfairHttpError, #[case] expected: bool) {
        assert_eq!(error.is_order_retryable(), expected);
    }

    #[rstest]
    #[case(betfair_error(-32099, "server error", None), false)]
    #[case(betfair_error(-32099, "ANGX-0003", Some("NO_SESSION")), true)]
    #[case(betfair_error(-32099, "ANGX-0003", Some("INVALID_SESSION_INFORMATION")), true)]
    #[case(betfair_error(-32600, "Invalid request", None), false)]
    #[case(BetfairHttpError::NetworkError("timeout".to_string()), false)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 429, body: String::new() }, false)]
    fn test_is_session_error(#[case] error: BetfairHttpError, #[case] expected: bool) {
        assert_eq!(error.is_session_error(), expected);
    }

    #[rstest]
    #[case(BetfairHttpError::LoginFailed { status: "NO_SESSION".to_string() }, true)]
    #[case(BetfairHttpError::LoginFailed { status: "CERT_AUTH_REQUIRED".to_string() }, true)]
    #[case(BetfairHttpError::NetworkError("timeout".to_string()), false)]
    #[case(BetfairHttpError::Timeout("read".to_string()), false)]
    #[case(betfair_error(-32099, "server error", None), false)]
    #[case(BetfairHttpError::JsonError("bad".to_string()), false)]
    #[case(BetfairHttpError::MissingCredentials, false)]
    fn test_is_login_failed(#[case] error: BetfairHttpError, #[case] expected: bool) {
        assert_eq!(error.is_login_failed(), expected);
    }

    #[rstest]
    #[case(betfair_error(-32099, "ANGX-0002", Some("TOO_MANY_REQUESTS")), true)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 429, body: String::new() }, true)]
    #[case(betfair_error(-32099, "ANGX-0003", Some("NO_SESSION")), false)]
    #[case(BetfairHttpError::UnexpectedStatus { status: 500, body: String::new() }, false)]
    #[case(BetfairHttpError::NetworkError("timeout".to_string()), false)]
    fn test_is_rate_limit_error(#[case] error: BetfairHttpError, #[case] expected: bool) {
        assert_eq!(error.is_rate_limit_error(), expected);
    }
}
