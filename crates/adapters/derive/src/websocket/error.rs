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

//! Error types for the Derive WebSocket client.

use std::time::Duration;

use serde_json::Value;
use thiserror::Error;

use crate::signing::auth::AuthError;

/// Result alias for WebSocket operations.
pub type Result<T> = std::result::Result<T, DeriveWsError>;

/// Errors raised by the Derive WebSocket client.
#[derive(Debug, Error)]
pub enum DeriveWsError {
    /// Transport-level failure (handshake, send, broken pipe).
    #[error("transport error: {0}")]
    Transport(String),

    /// JSON (de)serialization failed for an outbound or inbound frame.
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    /// JSON-RPC error envelope returned by the venue.
    #[error("JSON-RPC error {code}: {message}")]
    JsonRpc {
        /// Venue-defined error code.
        code: i64,
        /// Human-readable error message.
        message: String,
        /// Optional structured diagnostic payload.
        data: Option<Value>,
    },

    /// Local pacing rejects a reservation before the request is transmitted.
    #[error("pacing reservation expired before dispatch, retry after {retry_after:?}")]
    RateLimited {
        /// Remaining wait until local capacity becomes available.
        retry_after: Duration,
    },

    /// Awaited request response was not delivered (handler dropped the sender).
    #[error("request `{method}` cancelled before response was received")]
    RequestCancelled {
        /// Method that was awaiting a response.
        method: String,
    },

    /// No response arrived within the configured request timeout. The request
    /// was sent, so the outcome of a state-changing write is unknown.
    #[error("request `{method}` timed out before response was received")]
    Timeout {
        /// Method that was awaiting a response.
        method: String,
    },

    /// Auth header construction failed (e.g. clock skew, signer error).
    #[error("auth error: {0}")]
    Auth(#[from] AuthError),

    /// The WebSocket session did not authenticate before a request could proceed.
    #[error("authentication failed for `{operation}`: {reason}")]
    Authentication {
        /// Operation attempting or awaiting authentication.
        operation: String,
        /// Authentication failure detail.
        reason: String,
    },

    /// One or more channels were rejected in a subscribe result.
    #[error("subscription failed: {details}")]
    Subscription {
        /// Per-channel failure detail returned by the venue.
        details: String,
    },

    /// Private operation invoked without credentials configured on the client.
    #[error("missing credentials for `{operation}`")]
    MissingCredentials {
        /// Operation that requires authentication.
        operation: String,
    },

    /// Client used before `connect()` completed.
    #[error("WebSocket client is not connected")]
    NotConnected,
}

impl DeriveWsError {
    /// Constructs a [`DeriveWsError::Transport`] error.
    #[must_use]
    pub fn transport(msg: impl Into<String>) -> Self {
        Self::Transport(msg.into())
    }

    pub(crate) fn redacted_message(&self) -> String {
        match self {
            Self::JsonRpc { code, .. } => self
                .session_key_expiry_reason()
                .unwrap_or_else(|| format!("JSON-RPC error {code}")),
            Self::RateLimited { retry_after } => {
                format!("rate limited, retry after {retry_after:?}")
            }
            Self::Transport(_) => "transport error".to_string(),
            Self::Serde(_) => "JSON error".to_string(),
            Self::RequestCancelled { .. } => "request cancelled".to_string(),
            Self::Timeout { .. } => "request timed out".to_string(),
            Self::Auth(_) | Self::Authentication { .. } => "authentication error".to_string(),
            Self::Subscription { .. } => "subscription failed".to_string(),
            Self::MissingCredentials { .. } => "missing credentials".to_string(),
            Self::NotConnected => "not connected".to_string(),
        }
    }

    pub(crate) fn session_key_expiry_reason(&self) -> Option<String> {
        let Self::JsonRpc {
            code: -32602,
            data: Some(Value::String(detail)),
            ..
        } = self
        else {
            return None;
        };

        let detail = detail.lines().next()?;
        let expiries = detail.strip_prefix(
            "Error 14038: Action expiry exceeds session key expiry. Data: signature expires at ",
        )?;
        let (signature_expiry, key_expiry) =
            expiries.split_once("s but the signing session key expires at ")?;
        let signature_expiry = signature_expiry.parse::<u64>().ok()?;
        let key_expiry = key_expiry.strip_suffix('s')?.parse::<u64>().ok()?;

        if signature_expiry <= key_expiry {
            return None;
        }

        Some(format!(
            "14038: Signature expires at {signature_expiry}s, session key expires at {key_expiry}s; \
             use a key valid through the signature expiry",
        ))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_transport_constructor_carries_message() {
        let err = DeriveWsError::transport("broken pipe");
        assert!(err.to_string().contains("broken pipe"));
    }

    #[rstest]
    fn test_jsonrpc_error_renders_code_and_message() {
        let err = DeriveWsError::JsonRpc {
            code: -32601,
            message: "Method not found".to_string(),
            data: Some(json!({"method": "public/foo"})),
        };

        let rendered = err.to_string();
        assert!(rendered.contains("-32601"));
        assert!(rendered.contains("Method not found"));
    }

    #[rstest]
    fn test_session_key_expiry_reason_is_safe_for_logs(#[values(false, true)] backtrace: bool) {
        let mut response: Value = serde_json::from_str(include_str!(
            "../../test_data/ws_error_session_key_expiry.json"
        ))
        .unwrap();

        if !backtrace {
            let detail = response["error"]["data"].as_str().unwrap();
            response["error"]["data"] = json!(detail.lines().next().unwrap());
        }

        let error = DeriveWsError::JsonRpc {
            code: response["error"]["code"].as_i64().unwrap(),
            message: response["error"]["message"].as_str().unwrap().to_string(),
            data: Some(response["error"]["data"].clone()),
        };

        let expected = "14038: Signature expires at 1796263075s, session key expires at 1793728835s; use a key valid through the signature expiry";

        assert_eq!(error.session_key_expiry_reason().as_deref(), Some(expected));
        assert_eq!(error.redacted_message(), expected);
    }

    #[rstest]
    #[case::missing(None)]
    #[case::null(Some(Value::Null))]
    #[case::empty(Some(json!("")))]
    #[case::object(Some(json!({"detail": "opaque sensitive detail"})))]
    #[case::other_error(Some(json!("Error 14030: Session key expired")))]
    #[case::later_line(Some(json!("opaque sensitive detail\nError 14038: Action expiry exceeds session key expiry. Data: signature expires at 1796263075s but the signing session key expires at 1793728835s")))]
    #[case::signature_not_numeric(Some(json!("Error 14038: Action expiry exceeds session key expiry. Data: signature expires at secret-sig but the signing session key expires at 1793728835s")))]
    #[case::key_not_numeric(Some(json!("Error 14038: Action expiry exceeds session key expiry. Data: signature expires at 1796263075s but the signing session key expires at secret-key-s")))]
    #[case::overflow(Some(json!("Error 14038: Action expiry exceeds session key expiry. Data: signature expires at 18446744073709551616s but the signing session key expires at 1793728835s")))]
    #[case::key_overflow(Some(json!("Error 14038: Action expiry exceeds session key expiry. Data: signature expires at 18446744073709551615s but the signing session key expires at 18446744073709551616s")))]
    #[case::missing_suffix(Some(json!("Error 14038: Action expiry exceeds session key expiry. Data: signature expires at 1796263075s but the signing session key expires at 1793728835")))]
    #[case::trailing_text(Some(json!("Error 14038: Action expiry exceeds session key expiry. Data: signature expires at 1796263075s but the signing session key expires at 1793728835s secret")))]
    #[case::not_exceeded(Some(json!("Error 14038: Action expiry exceeds session key expiry. Data: signature expires at 1793728835s but the signing session key expires at 1796263075s")))]
    #[case::equal_expiries(Some(json!("Error 14038: Action expiry exceeds session key expiry. Data: signature expires at 1793728835s but the signing session key expires at 1793728835s")))]
    fn test_session_key_expiry_reason_keeps_unrecognized_data_redacted(
        #[case] data: Option<Value>,
    ) {
        let error = DeriveWsError::JsonRpc {
            code: -32602,
            message: "Invalid params".into(),
            data,
        };

        assert_eq!(error.session_key_expiry_reason(), None);
        assert_eq!(error.redacted_message(), "JSON-RPC error -32602");
    }

    #[rstest]
    fn test_session_key_expiry_reason_requires_invalid_params_code() {
        let response: Value = serde_json::from_str(include_str!(
            "../../test_data/ws_error_session_key_expiry.json"
        ))
        .unwrap();

        let error = DeriveWsError::JsonRpc {
            code: -32603,
            message: "Internal error".into(),
            data: Some(response["error"]["data"].clone()),
        };

        assert_eq!(error.session_key_expiry_reason(), None);
        assert_eq!(error.redacted_message(), "JSON-RPC error -32603");
    }

    #[rstest]
    fn test_session_key_expiry_reason_bounds_numeric_output() {
        let error = DeriveWsError::JsonRpc {
            code: -32602,
            message: "Invalid params".into(),
            data: Some(json!(format!(
                "Error 14038: Action expiry exceeds session key expiry. Data: signature expires at {}s but the signing session key expires at {}s",
                u64::MAX,
                u64::MAX - 1,
            ))),
        };

        let expected = "14038: Signature expires at 18446744073709551615s, session key expires at 18446744073709551614s; use a key valid through the signature expiry";

        assert_eq!(error.session_key_expiry_reason().as_deref(), Some(expected));
        assert!(expected.len() <= crate::common::parse::STRATEGY_REASON_MAX_CHARS);
    }

    #[rstest]
    fn test_missing_credentials_names_operation() {
        let err = DeriveWsError::MissingCredentials {
            operation: "public/login".to_string(),
        };

        assert!(err.to_string().contains("public/login"));
    }

    #[rstest]
    fn test_authentication_error_names_operation_and_reason() {
        let err = DeriveWsError::Authentication {
            operation: "private/order".to_string(),
            reason: "session recovery failed".to_string(),
        };

        assert!(err.to_string().contains("private/order"));
        assert!(err.to_string().contains("session recovery failed"));
    }
}
