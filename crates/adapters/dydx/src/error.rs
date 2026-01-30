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

//! Error handling for the dYdX adapter.
//!
//! This module provides error types for all dYdX operations, including
//! HTTP, WebSocket, and gRPC errors.

use thiserror::Error;

use crate::{http::error::DydxHttpError, websocket::error::DydxWsError};

/// Result type for dYdX operations.
pub type DydxResult<T> = Result<T, DydxError>;

/// The main error type for all dYdX adapter operations.
#[derive(Debug, Error)]
pub enum DydxError {
    /// HTTP client errors.
    #[error("HTTP error: {0}")]
    Http(#[from] DydxHttpError),

    /// WebSocket connection errors.
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] DydxWsError),

    /// gRPC errors from Cosmos SDK node.
    #[error("gRPC error: {0}")]
    Grpc(#[from] Box<tonic::Status>),

    /// Transaction signing errors.
    #[error("Signing error: {0}")]
    Signing(String),

    /// Protocol buffer encoding errors.
    #[error("Encoding error: {0}")]
    Encoding(#[from] prost::EncodeError),

    /// Protocol buffer decoding errors.
    #[error("Decoding error: {0}")]
    Decoding(#[from] prost::DecodeError),

    /// JSON serialization/deserialization errors.
    #[error("JSON error: {message}")]
    Json {
        message: String,
        /// The raw JSON that failed to parse, if available.
        raw: Option<String>,
    },

    /// Configuration errors.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Invalid data errors.
    #[error("Invalid data: {0}")]
    InvalidData(String),

    /// Invalid order side error.
    #[error("Invalid order side: {0}")]
    InvalidOrderSide(String),

    /// Unsupported order type error.
    #[error("Unsupported order type: {0}")]
    UnsupportedOrderType(String),

    /// Feature not yet implemented.
    #[error("Not implemented: {0}")]
    NotImplemented(String),

    /// Order construction and submission errors.
    #[error("Order error: {0}")]
    Order(String),

    /// Parsing errors (e.g., string to number conversions).
    #[error("Parse error: {0}")]
    Parse(String),

    /// Wallet and account derivation errors.
    #[error("Wallet error: {0}")]
    Wallet(String),

    /// Nautilus core errors.
    #[error("Nautilus error: {0}")]
    Nautilus(#[from] anyhow::Error),
}

/// Cosmos SDK error code for account sequence mismatch.
/// See: https://github.com/cosmos/cosmos-sdk/blob/main/types/errors/errors.go
const COSMOS_ERROR_CODE_SEQUENCE_MISMATCH: u32 = 32;

impl DydxError {
    /// Returns true if this error is a Cosmos SDK account sequence mismatch (code 32).
    ///
    /// Sequence mismatch occurs when:
    /// - Multiple transactions race for the same sequence number
    /// - A transaction was submitted but not yet included in a block
    /// - The local sequence counter is out of sync with chain state
    ///
    /// These errors are typically recoverable by resyncing the sequence from chain
    /// and rebuilding the transaction.
    #[must_use]
    pub fn is_sequence_mismatch(&self) -> bool {
        match self {
            // Check for code=32 pattern in error messages
            Self::Grpc(status) => {
                let msg = status.message();
                Self::message_indicates_sequence_mismatch(msg)
            }
            Self::Nautilus(e) => {
                let msg = format!("{e:?}");
                Self::message_indicates_sequence_mismatch(&msg)
            }
            _ => false,
        }
    }

    /// Checks if an error message indicates a sequence mismatch.
    ///
    /// Matches both the error code pattern (code=32) and the descriptive text.
    fn message_indicates_sequence_mismatch(msg: &str) -> bool {
        // Check for Cosmos SDK error code 32
        msg.contains(&format!("code={COSMOS_ERROR_CODE_SEQUENCE_MISMATCH}"))
            || msg.contains("account sequence mismatch")
    }

    /// Returns true if this error is likely transient and worth retrying.
    ///
    /// Transient errors include:
    /// - Sequence mismatch (recoverable by resync)
    /// - Network timeouts
    /// - Temporary node unavailability
    #[must_use]
    pub fn is_transient(&self) -> bool {
        if self.is_sequence_mismatch() {
            return true;
        }

        match self {
            Self::Grpc(status) => {
                matches!(
                    status.code(),
                    tonic::Code::Unavailable
                        | tonic::Code::DeadlineExceeded
                        | tonic::Code::ResourceExhausted
                )
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_sequence_mismatch_from_code_pattern() {
        // Simulate error message from grpc/client.rs broadcast_tx
        let err = DydxError::Nautilus(anyhow::anyhow!(
            "Transaction broadcast failed: code=32, log=account sequence mismatch, expected 15, received 14"
        ));
        assert!(err.is_sequence_mismatch());
    }

    #[rstest]
    fn test_sequence_mismatch_from_text_pattern() {
        let err = DydxError::Nautilus(anyhow::anyhow!(
            "account sequence mismatch: expected 100, received 99"
        ));
        assert!(err.is_sequence_mismatch());
    }

    #[rstest]
    fn test_sequence_mismatch_grpc_error() {
        let status =
            tonic::Status::invalid_argument("account sequence mismatch, expected 42, received 41");
        let err = DydxError::Grpc(Box::new(status));
        assert!(err.is_sequence_mismatch());
    }

    #[rstest]
    fn test_non_sequence_error_not_matched() {
        let err = DydxError::Nautilus(anyhow::anyhow!("insufficient funds"));
        assert!(!err.is_sequence_mismatch());
    }

    #[rstest]
    fn test_other_error_variants_not_matched() {
        let err = DydxError::Config("bad config".to_string());
        assert!(!err.is_sequence_mismatch());

        let err = DydxError::Order("order rejected".to_string());
        assert!(!err.is_sequence_mismatch());
    }

    #[rstest]
    fn test_is_transient_sequence_mismatch() {
        let err = DydxError::Nautilus(anyhow::anyhow!("account sequence mismatch"));
        assert!(err.is_transient());
    }

    #[rstest]
    fn test_is_transient_unavailable() {
        let status = tonic::Status::unavailable("node unavailable");
        let err = DydxError::Grpc(Box::new(status));
        assert!(err.is_transient());
    }

    #[rstest]
    fn test_is_transient_deadline_exceeded() {
        let status = tonic::Status::deadline_exceeded("timeout");
        let err = DydxError::Grpc(Box::new(status));
        assert!(err.is_transient());
    }

    #[rstest]
    fn test_is_not_transient_permission_denied() {
        let status = tonic::Status::permission_denied("unauthorized");
        let err = DydxError::Grpc(Box::new(status));
        assert!(!err.is_transient());
    }

    #[rstest]
    fn test_is_not_transient_config_error() {
        let err = DydxError::Config("invalid".to_string());
        assert!(!err.is_transient());
    }
}
