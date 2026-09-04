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

//! Betfair stream client error types.

/// Represents stream client errors for the Betfair adapter.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BetfairStreamError {
    /// Failed to establish a connection.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    /// Stream authentication failed.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),
    /// Stream protocol error (unexpected message format).
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Connection or read timeout.
    #[error("Timeout: {0}")]
    Timeout(String),
    /// Connection was lost.
    #[error("Disconnected: {0}")]
    Disconnected(String),
}

impl From<serde_json::Error> for BetfairStreamError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        BetfairStreamError::ConnectionFailed("refused".to_string()),
        "Connection failed: refused"
    )]
    #[case(
        BetfairStreamError::AuthenticationFailed("invalid token".to_string()),
        "Authentication failed: invalid token"
    )]
    #[case(
        BetfairStreamError::ProtocolError("bad frame".to_string()),
        "Protocol error: bad frame"
    )]
    #[case(
        BetfairStreamError::JsonError("parse error".to_string()),
        "JSON error: parse error"
    )]
    #[case(
        BetfairStreamError::Timeout("read".to_string()),
        "Timeout: read"
    )]
    #[case(
        BetfairStreamError::Disconnected("reset".to_string()),
        "Disconnected: reset"
    )]
    fn test_display(#[case] error: BetfairStreamError, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn test_from_serde_error() {
        let json_err = serde_json::from_str::<String>("bad").unwrap_err();
        let err: BetfairStreamError = json_err.into();
        assert!(matches!(err, BetfairStreamError::JsonError(_)));
    }
}
