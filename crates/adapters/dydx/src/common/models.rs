//! Common types and models for the dYdX adapter.

use serde::{Deserialize, Serialize};

/// dYdX account information.
///
/// Represents a Cosmos SDK account with its address, account number,
/// and current sequence (nonce) for transaction ordering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxAccount {
    /// Cosmos SDK address (dydx...).
    pub address: String,
    /// Account number from the blockchain.
    pub account_number: u64,
    /// Current sequence number (nonce) for transactions.
    pub sequence: u64,
}
