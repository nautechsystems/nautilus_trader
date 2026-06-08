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

//! Adapter-level error aggregation for the Derive integration.
//!
//! Component clients raise their own error taxonomies ([`DeriveHttpError`],
//! [`DeriveWsError`], [`AuthError`]); [`DeriveError`] unifies them at the
//! adapter boundary so callers can match on a single type without losing the
//! per-component detail.

use thiserror::Error;

use crate::{
    common::retry::{
        is_fatal_http_error, is_fatal_ws_error, should_retry_http_error, should_retry_ws_error,
    },
    http::DeriveHttpError,
    signing::auth::AuthError,
    websocket::DeriveWsError,
};

/// Result alias for adapter-level operations.
pub type Result<T> = std::result::Result<T, DeriveError>;

/// Unified error type aggregating the Derive adapter's component errors.
#[derive(Debug, Error)]
pub enum DeriveError {
    /// HTTP transport, JSON-RPC, or credential errors raised by [`DeriveHttpError`].
    #[error("HTTP error: {0}")]
    Http(#[from] DeriveHttpError),

    /// WebSocket transport, framing, or login errors raised by [`DeriveWsError`].
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] DeriveWsError),

    /// Signing or session authentication errors.
    #[error("auth error: {0}")]
    Auth(#[from] AuthError),

    /// Configuration error surfaced during client construction (placeholder
    /// constants, invalid hex, missing credentials).
    #[error("configuration error: {0}")]
    Config(String),
}

impl DeriveError {
    /// Constructs a [`DeriveError::Config`] error.
    #[must_use]
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Returns `true` for errors that did not reach the venue and can safely
    /// be retried (transport, timeout, gateway 5xx).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http(e) => should_retry_http_error(e),
            Self::WebSocket(e) => should_retry_ws_error(e),
            Self::Auth(_) | Self::Config(_) => false,
        }
    }

    /// Returns `true` for errors that indicate a fatal session state
    /// (deregistered session key, subaccount withdrawn, compliance halt).
    /// Fatal errors require operator intervention and must not be retried.
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        match self {
            Self::Http(e) => is_fatal_http_error(e),
            Self::WebSocket(e) => is_fatal_ws_error(e),
            Self::Auth(_) => true,
            Self::Config(_) => true,
        }
    }
}

impl From<serde_json::Error> for DeriveError {
    fn from(value: serde_json::Error) -> Self {
        Self::Http(DeriveHttpError::Serde(value))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_http_transport_is_retryable() {
        let err: DeriveError = DeriveHttpError::transport("conn reset").into();
        assert!(err.is_retryable());
        assert!(!err.is_fatal());
    }

    #[rstest]
    fn test_http_jsonrpc_invalid_params_is_not_retryable() {
        let err: DeriveError = DeriveHttpError::JsonRpc {
            code: -32602,
            message: "Invalid params".to_string(),
            data: None,
        }
        .into();
        assert!(!err.is_retryable());
    }

    #[rstest]
    fn test_config_error_is_fatal() {
        let err = DeriveError::config("missing constants");
        assert!(!err.is_retryable());
        assert!(err.is_fatal());
    }

    #[rstest]
    fn test_auth_error_is_fatal() {
        let err: DeriveError = AuthError::ClockBeforeEpoch.into();
        assert!(!err.is_retryable());
        assert!(err.is_fatal());
    }

    #[rstest]
    fn test_ws_transport_is_retryable() {
        let err: DeriveError = DeriveWsError::transport("broken pipe").into();
        assert!(err.is_retryable());
    }
}
