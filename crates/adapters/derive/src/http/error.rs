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

//! Error types for the Derive HTTP client.

use nautilus_network::http::HttpClientError;
use serde_json::Value;
use thiserror::Error;

use crate::signing::auth::AuthError;

/// Result alias for HTTP operations.
pub type Result<T> = std::result::Result<T, DeriveHttpError>;

/// Errors raised by the Derive HTTP client.
#[derive(Debug, Error)]
pub enum DeriveHttpError {
    /// Network/transport failure with undefined venue outcome.
    #[error("transport error: {0}")]
    Transport(String),

    /// HTTP-level failure (non-2xx without a JSON-RPC body, or 2xx without
    /// the expected envelope).
    #[error("HTTP {status}: {message}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Truncated body text or status reason.
        message: String,
    },

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

    /// Successful envelope was missing the `result` field.
    #[error("missing `result` in JSON-RPC response for `{method}`")]
    MissingResult {
        /// Method that returned an empty envelope.
        method: String,
    },

    /// Response body could not be decoded as JSON-RPC.
    #[error("decode error: {0}")]
    Decode(String),

    /// JSON (de)serialization failed for a request or response payload.
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    /// Auth header construction failed (e.g. clock skew, signer error).
    #[error("auth error: {0}")]
    Auth(#[from] AuthError),

    /// Private endpoint invoked without credentials configured on the client.
    #[error("missing credentials for private endpoint `{method}`")]
    MissingCredentials {
        /// Method that requires authentication.
        method: String,
    },
}

impl DeriveHttpError {
    /// Constructs a [`DeriveHttpError::Transport`] error.
    #[must_use]
    pub fn transport(msg: impl Into<String>) -> Self {
        Self::Transport(msg.into())
    }

    /// Constructs a [`DeriveHttpError::Http`] error.
    #[must_use]
    pub fn http(status: u16, message: impl Into<String>) -> Self {
        Self::Http {
            status,
            message: message.into(),
        }
    }

    /// Constructs a [`DeriveHttpError::Decode`] error.
    #[must_use]
    pub fn decode(msg: impl Into<String>) -> Self {
        Self::Decode(msg.into())
    }

    /// Returns `true` for errors that did not reach the venue (transport,
    /// timeout). Callers reconciling order state should treat these as
    /// "unknown" rather than "rejected".
    #[must_use]
    pub fn is_transport_error(&self) -> bool {
        matches!(self, Self::Transport(_))
    }
}

impl From<HttpClientError> for DeriveHttpError {
    fn from(value: HttpClientError) -> Self {
        Self::Transport(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_transport_is_transport_error() {
        assert!(DeriveHttpError::transport("conn reset").is_transport_error());
    }

    #[rstest]
    fn test_http_is_not_transport_error() {
        assert!(!DeriveHttpError::http(503, "service unavailable").is_transport_error());
    }

    #[rstest]
    fn test_jsonrpc_error_carries_code_and_data() {
        let err = DeriveHttpError::JsonRpc {
            code: -32602,
            message: "Invalid params".to_string(),
            data: Some(json!({"field": "currency"})),
        };
        let text = err.to_string();
        assert!(text.contains("-32602"));
        assert!(text.contains("Invalid params"));
        assert!(!err.is_transport_error());
    }

    #[rstest]
    fn test_missing_credentials_names_method() {
        let err = DeriveHttpError::MissingCredentials {
            method: "private/order".to_string(),
        };
        assert!(err.to_string().contains("private/order"));
    }

    #[rstest]
    fn test_http_client_error_maps_to_transport() {
        let upstream = HttpClientError::Error("boom".to_string());
        let mapped: DeriveHttpError = upstream.into();
        assert!(mapped.is_transport_error());
    }
}
