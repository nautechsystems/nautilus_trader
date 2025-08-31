// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use serde::{Deserialize, Serialize};

use crate::common::enums::HyperliquidSide;

/// Represents metadata about available markets from `POST /info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidMeta {
    #[serde(default)]
    pub universe: Vec<HyperliquidAssetInfo>,
}

/// Represents asset information from the meta endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidAssetInfo {
    /// Asset name (e.g., "BTC").
    pub name: String,
    /// Number of decimal places for size.
    #[serde(rename = "szDecimals")]
    pub sz_decimals: u32,
}

/// Represents an L2 order book snapshot from `POST /info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidL2Book {
    /// Coin symbol.
    pub coin: String,
    /// Order book levels: [bids, asks].
    pub levels: Vec<Vec<HyperliquidLevel>>,
    /// Timestamp in milliseconds.
    pub time: u64,
}

/// Represents an order book level with price and size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidLevel {
    /// Price level.
    pub px: String,
    /// Size at this level.
    pub sz: String,
}

/// Represents user fills response from `POST /info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidFills {
    #[serde(default)]
    pub fills: Vec<HyperliquidFill>,
}

/// Represents an individual fill from user fills.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidFill {
    /// Coin symbol.
    pub coin: String,
    /// Fill price.
    pub px: String,
    /// Fill size.
    pub sz: String,
    /// Order side (buy/sell).
    pub side: HyperliquidSide,
    /// Fill timestamp in milliseconds.
    pub time: u64,
    /// Position size before this fill.
    #[serde(rename = "startPosition")]
    pub start_position: String,
    /// Directory (order book path).
    pub dir: String,
    /// Closed P&L from this fill.
    #[serde(rename = "closedPnl")]
    pub closed_pnl: String,
    /// Hash reference.
    pub hash: String,
    /// Order ID that generated this fill.
    pub oid: u64,
    /// Crossed status.
    pub crossed: bool,
    /// Fee paid for this fill.
    pub fee: String,
}

/// Represents order status response from `POST /info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidOrderStatus {
    #[serde(default)]
    pub statuses: Vec<HyperliquidOrderStatusEntry>,
}

/// Represents an individual order status entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidOrderStatusEntry {
    /// Order information.
    pub order: HyperliquidOrderInfo,
    /// Current status string.
    pub status: String,
    /// Status timestamp in milliseconds.
    #[serde(rename = "statusTimestamp")]
    pub status_timestamp: u64,
}

/// Represents order information within an order status entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperliquidOrderInfo {
    /// Coin symbol.
    pub coin: String,
    /// Order side (buy/sell).
    pub side: HyperliquidSide,
    /// Limit price.
    #[serde(rename = "limitPx")]
    pub limit_px: String,
    /// Order size.
    pub sz: String,
    /// Order ID.
    pub oid: u64,
    /// Order timestamp in milliseconds.
    pub timestamp: u64,
    /// Original order size.
    #[serde(rename = "origSz")]
    pub orig_sz: String,
}

/// Represents an exchange action request wrapper for `POST /exchange`.
#[derive(Debug, Clone, Serialize)]
pub struct HyperliquidExchangeRequest<T> {
    /// The action to perform.
    pub action: T,
    /// Request nonce for replay protection.
    #[serde(rename = "nonce")]
    pub nonce: u64,
    /// ECC signature over the action.
    #[serde(rename = "signature")]
    pub signature: String,
    /// Optional vault address for sub-account trading.
    #[serde(rename = "vaultAddress", skip_serializing_if = "Option::is_none")]
    pub vault_address: Option<String>,
}

impl<T> HyperliquidExchangeRequest<T>
where
    T: Serialize,
{
    /// Create a new exchange request with the given action.
    pub fn new(action: T, nonce: u64, signature: String) -> Self {
        Self {
            action,
            nonce,
            signature,
            vault_address: None,
        }
    }

    /// Create a new exchange request with vault address for sub-account trading.
    pub fn with_vault(action: T, nonce: u64, signature: String, vault_address: String) -> Self {
        Self {
            action,
            nonce,
            signature,
            vault_address: Some(vault_address),
        }
    }

    /// Convert to JSON value for signing purposes.
    pub fn to_sign_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

/// Represents an exchange response wrapper from `POST /exchange`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HyperliquidExchangeResponse {
    /// Successful response with status.
    Status {
        /// Status message.
        status: String,
        /// Response payload.
        response: serde_json::Value,
    },
    /// Error response.
    Error {
        /// Error message.
        error: String,
    },
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_meta_deserialization() {
        let json = r#"{"universe": [{"name": "BTC", "szDecimals": 5}]}"#;

        let meta: HyperliquidMeta = serde_json::from_str(json).unwrap();

        assert_eq!(meta.universe.len(), 1);
        assert_eq!(meta.universe[0].name, "BTC");
        assert_eq!(meta.universe[0].sz_decimals, 5);
    }

    #[rstest]
    fn test_l2_book_deserialization() {
        let json = r#"{"coin": "BTC", "levels": [[{"px": "50000", "sz": "1.5"}], [{"px": "50100", "sz": "2.0"}]], "time": 1234567890}"#;

        let book: HyperliquidL2Book = serde_json::from_str(json).unwrap();

        assert_eq!(book.coin, "BTC");
        assert_eq!(book.levels.len(), 2);
        assert_eq!(book.time, 1234567890);
    }

    #[rstest]
    fn test_exchange_response_deserialization() {
        let json = r#"{"status": "ok", "response": {"type": "order"}}"#;

        let response: HyperliquidExchangeResponse = serde_json::from_str(json).unwrap();

        match response {
            HyperliquidExchangeResponse::Status { status, .. } => assert_eq!(status, "ok"),
            _ => panic!("Expected status response"),
        }
    }
}
