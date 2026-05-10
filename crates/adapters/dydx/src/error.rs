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

/// Cosmos SDK error code for transaction already in mempool cache (`ErrTxInMempoolCache`).
///
/// Returned when the exact same transaction bytes (same hash) are submitted to a node
/// that already has the transaction in its mempool cache. For short-term dYdX orders,
/// this is benign -- the original transaction is already queued for processing.
pub const COSMOS_ERROR_CODE_TX_IN_MEMPOOL_CACHE: u32 = 19;

const COSMOS_ERROR_CODE_SEQUENCE_MISMATCH: u32 = 32;

/// dYdX CLOB error code for duplicate cancel in memclob.
///
/// Returned when a cancel message is submitted for an order that already has a pending
/// cancel with a greater-than-or-equal `GoodTilBlock`. This is benign for short-term
/// cancel operations -- the previous cancel is already queued and will be processed.
///
/// Common scenario: overlapping `cancel_all_orders` waves from a grid MM strategy.
pub const DYDX_ERROR_CODE_CANCEL_ALREADY_IN_MEMCLOB: u32 = 9;

/// dYdX CLOB error code for cancelling a non-existent order.
///
/// Returned when attempting to cancel an order that has already been filled, expired,
/// or previously cancelled. This is benign -- the order is already gone.
pub const DYDX_ERROR_CODE_ORDER_DOES_NOT_EXIST: u32 = 3006;

const DYDX_ERROR_CODE_ALL_OF_FAILED: u32 = 104;

impl DydxError {
    /// Returns true if this error is a sequence mismatch (code=32 or code=104 with sequence hint).
    ///
    /// Sequence mismatch occurs when:
    /// - Multiple transactions race for the same sequence number
    /// - A transaction was submitted but not yet included in a block
    /// - The local sequence counter is out of sync with chain state
    ///
    /// On dYdX v4, sequence mismatches can manifest as either:
    /// - code=32: Standard Cosmos SDK "account sequence mismatch"
    /// - code=104: dYdX authenticator "signature verification failed; please verify sequence"
    ///
    /// These errors are typically recoverable by resyncing the sequence from chain
    /// and rebuilding the transaction.
    #[must_use]
    pub fn is_sequence_mismatch(&self) -> bool {
        match self {
            Self::Grpc(status) => {
                let msg = status.message();
                Self::message_indicates_sequence_mismatch(msg)
            }
            Self::Nautilus(e) => {
                let msg = e.to_string();
                Self::message_indicates_sequence_mismatch(&msg)
            }
            _ => false,
        }
    }

    fn message_indicates_sequence_mismatch(msg: &str) -> bool {
        // Standard Cosmos SDK error code 32
        if msg.contains(&format!("code={COSMOS_ERROR_CODE_SEQUENCE_MISMATCH}"))
            || msg.contains("account sequence mismatch")
        {
            return true;
        }
        // dYdX authenticator error code 104 with sequence hint
        msg.contains(&format!("code={DYDX_ERROR_CODE_ALL_OF_FAILED}")) && msg.contains("sequence")
    }

    /// Returns true if this error indicates the transaction is already in the mempool (code=19).
    ///
    /// This is benign for short-term orders -- the transaction was already accepted by the
    /// mempool on a previous submission and will be processed. Callers can safely treat
    /// this as success.
    #[must_use]
    pub fn is_tx_in_mempool(&self) -> bool {
        match self {
            Self::Nautilus(e) => {
                let msg = e.to_string();
                msg.contains(&format!("code={COSMOS_ERROR_CODE_TX_IN_MEMPOOL_CACHE}"))
                    || msg.contains("tx already in mempool")
            }
            _ => false,
        }
    }

    /// Returns true if this error indicates a duplicate cancel already in the memclob (code=9).
    ///
    /// dYdX rejects cancel messages when an existing cancel for the same order has a
    /// greater-than-or-equal `GoodTilBlock`. The original cancel will be processed.
    #[must_use]
    pub fn is_cancel_already_in_memclob(&self) -> bool {
        match self {
            Self::Nautilus(e) => {
                let msg = e.to_string();
                msg.contains(&format!("code={DYDX_ERROR_CODE_CANCEL_ALREADY_IN_MEMCLOB}"))
                    && msg.contains("cancel already exists")
            }
            _ => false,
        }
    }

    /// Returns true if this error indicates the order to cancel does not exist (code=3006).
    ///
    /// The order was already filled, expired, or previously cancelled.
    #[must_use]
    pub fn is_order_does_not_exist(&self) -> bool {
        match self {
            Self::Nautilus(e) => {
                let msg = e.to_string();
                msg.contains(&format!("code={DYDX_ERROR_CODE_ORDER_DOES_NOT_EXIST}"))
                    || msg.contains("Order Id to cancel does not exist")
            }
            _ => false,
        }
    }

    /// Returns true if this error is benign for short-term cancel operations.
    ///
    /// Benign cancel errors occur during overlapping cancel waves (common in grid MM):
    /// - code=19: Transaction already in mempool cache (duplicate tx bytes)
    /// - code=9: Cancel already exists in memclob with >= GoodTilBlock
    /// - code=3006: Order to cancel does not exist (already filled/expired/cancelled)
    #[must_use]
    pub fn is_benign_cancel_error(&self) -> bool {
        self.is_tx_in_mempool()
            || self.is_cancel_already_in_memclob()
            || self.is_order_does_not_exist()
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
    fn test_sequence_mismatch_dydx_authenticator_code_104() {
        let err = DydxError::Nautilus(anyhow::anyhow!(
            "Transaction broadcast failed: code=104, log=authentication failed for message 0, \
             authenticator id 966, type AllOf: signature verification failed; \
             please verify account number (0), sequence (545) and chain-id (dydx-mainnet-1): \
             Signature verification failed: AllOf verification failed"
        ));
        assert!(err.is_sequence_mismatch());
    }

    #[rstest]
    fn test_code_104_without_sequence_not_matched() {
        // code=104 without "sequence" in the message should NOT match
        let err = DydxError::Nautilus(anyhow::anyhow!(
            "Transaction broadcast failed: code=104, log=authentication failed: invalid pubkey"
        ));
        assert!(!err.is_sequence_mismatch());
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

    #[rstest]
    fn test_benign_cancel_tx_in_mempool() {
        let err = DydxError::Nautilus(anyhow::anyhow!(
            "Transaction broadcast failed: code=19, tx already in mempool cache"
        ));
        assert!(err.is_tx_in_mempool());
        assert!(err.is_benign_cancel_error());
    }

    #[rstest]
    fn test_benign_cancel_already_in_memclob() {
        let err = DydxError::Nautilus(anyhow::anyhow!(
            "Transaction broadcast failed: code=9, cancel already exists in memclob with >= GoodTilBlock"
        ));
        assert!(err.is_cancel_already_in_memclob());
        assert!(err.is_benign_cancel_error());
    }

    #[rstest]
    fn test_benign_cancel_order_does_not_exist() {
        let err = DydxError::Nautilus(anyhow::anyhow!(
            "Transaction broadcast failed: code=3006, Order Id to cancel does not exist"
        ));
        assert!(err.is_order_does_not_exist());
        assert!(err.is_benign_cancel_error());
    }

    #[rstest]
    fn test_non_benign_error_not_treated_as_benign() {
        let err = DydxError::Nautilus(anyhow::anyhow!("insufficient funds"));
        assert!(!err.is_benign_cancel_error());
    }

    #[rstest]
    fn test_benign_cancel_non_nautilus_variant() {
        let err = DydxError::Order("order rejected".to_string());
        assert!(!err.is_benign_cancel_error());
    }
}
