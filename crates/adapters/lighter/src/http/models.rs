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

use std::{collections::HashMap, str::FromStr};

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};

/// Response for `/nextNonce`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NextNonceResponse {
    /// Result code (200 on success).
    pub code: i32,
    /// The next nonce value for the given account/key.
    pub nonce: i64,
    /// Optional message field returned by the API.
    #[serde(default)]
    pub message: Option<String>,
}

/// Response for `/sendTx`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SendTxResponse {
    /// Result code (200 on success).
    pub code: i32,
    /// Optional message (e.g., rate-limit hints).
    #[serde(default)]
    pub message: Option<String>,
    /// Transaction hash returned by the API.
    #[serde(default)]
    pub tx_hash: Option<String>,
    /// Optional predicted execution time in milliseconds.
    #[serde(default)]
    pub predicted_execution_time_ms: Option<i64>,
}

/// Account order entry returned by `/accountActiveOrders`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LighterActiveOrder {
    #[serde(default)]
    pub order_index: Option<i64>,
    #[serde(default)]
    pub client_order_index: Option<i64>,
    #[serde(default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub client_order_id: Option<String>,
    #[serde(default)]
    pub market_index: Option<u32>,
    #[serde(default)]
    pub owner_account_index: Option<i64>,
    #[serde(default)]
    pub initial_base_amount: Option<String>,
    #[serde(default)]
    pub price: Option<String>,
    #[serde(default)]
    pub nonce: Option<i64>,
    #[serde(default)]
    pub remaining_base_amount: Option<String>,
    #[serde(default)]
    pub is_ask: Option<bool>,
    #[serde(default)]
    pub base_size: Option<i64>,
    #[serde(default)]
    pub base_price: Option<i64>,
    #[serde(default)]
    pub filled_base_amount: Option<String>,
    #[serde(default)]
    pub filled_quote_amount: Option<String>,
    #[serde(default)]
    pub side: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub time_in_force: Option<String>,
    #[serde(default)]
    pub reduce_only: Option<bool>,
    #[serde(default)]
    pub trigger_price: Option<String>,
    #[serde(default)]
    pub order_expiry: Option<i64>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub trigger_status: Option<String>,
    #[serde(default)]
    pub trigger_time: Option<i64>,
    #[serde(default)]
    pub parent_order_index: Option<i64>,
    #[serde(default)]
    pub parent_order_id: Option<String>,
    #[serde(default)]
    pub to_trigger_order_id_0: Option<String>,
    #[serde(default)]
    pub to_trigger_order_id_1: Option<String>,
    #[serde(default)]
    pub to_cancel_order_id_0: Option<String>,
    #[serde(default)]
    pub block_height: Option<i64>,
    #[serde(default)]
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

/// Response for `/accountActiveOrders`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccountActiveOrdersResponse {
    /// Result code (200 on success).
    pub code: i32,
    /// Order entries currently active for the account/market.
    #[serde(default)]
    pub orders: Vec<LighterActiveOrder>,
    /// Optional message field returned by the API.
    #[serde(default)]
    pub message: Option<String>,
}

/// Response for `/account` (simplified for PR3 reconciliation).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccountResponse {
    /// Result code (200 on success).
    pub code: i32,
    /// Optional total accounts count.
    #[serde(default)]
    pub total: Option<i64>,
    /// Raw accounts payload (left as JSON values for future mapping).
    #[serde(default)]
    pub accounts: Vec<serde_json::Value>,
    /// Optional message field returned by the API.
    #[serde(default)]
    pub message: Option<String>,
}

use crate::data::models::LighterOrderBookDepth;

/// Response envelope for `GET /orderBooks`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OrderBooksResponse {
    /// Response is wrapped in an `order_books` (or `orderBooks`) field.
    Wrapped {
        #[serde(default, alias = "orderBooks")]
        order_books: Vec<LighterOrderBook>,
    },
    /// Response is a bare array of order book entries.
    Flat(Vec<LighterOrderBook>),
}

impl OrderBooksResponse {
    /// Consume the response and return the contained order books.
    #[must_use]
    pub fn into_books(self) -> Vec<LighterOrderBook> {
        match self {
            Self::Wrapped { order_books } => order_books,
            Self::Flat(entries) => entries,
        }
    }
}

impl Default for OrderBooksResponse {
    fn default() -> Self {
        Self::Wrapped {
            order_books: Vec::new(),
        }
    }
}

/// Order book metadata entry returned by Lighter.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LighterOrderBook {
    /// Market identifier (used for subscriptions and REST).
    #[serde(alias = "market_id", alias = "marketIndex")]
    pub market_index: u32,

    /// Venue-provided symbol (often the base asset name).
    #[serde(default, alias = "symbol")]
    pub symbol: Option<String>,

    /// Optional base token name/code (if separate from `symbol`).
    #[serde(default, alias = "base_token", alias = "baseToken", alias = "base")]
    pub base_token: Option<String>,

    /// Optional quote token code (defaults to USD when absent).
    #[serde(default, alias = "quote_token", alias = "quoteToken", alias = "quote")]
    pub quote_token: Option<String>,

    /// Price decimal precision supported by the market.
    #[serde(
        default,
        alias = "supported_price_decimals",
        alias = "price_decimals",
        alias = "priceDecimals"
    )]
    pub supported_price_decimals: Option<u32>,

    /// Size decimal precision supported by the market.
    #[serde(
        default,
        alias = "supported_size_decimals",
        alias = "size_decimals",
        alias = "sizeDecimals"
    )]
    pub supported_size_decimals: Option<u32>,

    /// Minimum base amount per order.
    #[serde(
        default,
        alias = "min_base_amount",
        alias = "minBaseAmount",
        deserialize_with = "deserialize_decimal_opt"
    )]
    pub min_base_amount: Option<Decimal>,

    /// Tick size (optional; derived from decimals when absent).
    #[serde(
        default,
        alias = "tick_size",
        alias = "tickSize",
        deserialize_with = "deserialize_decimal_opt"
    )]
    pub tick_size: Option<Decimal>,

    /// Minimum size increment (optional; derived from decimals when absent).
    #[serde(
        default,
        alias = "lot_size",
        alias = "lotSize",
        deserialize_with = "deserialize_decimal_opt"
    )]
    pub lot_size: Option<Decimal>,

    /// Whether the market is currently active/tradeable.
    #[serde(default)]
    pub active: Option<bool>,

    /// Preserve any additional fields for debugging/telemetry.
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

fn deserialize_decimal_opt<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(serde_json::Value::Number(num)) => Decimal::from_str(&num.to_string())
            .map(Some)
            .map_err(serde::de::Error::custom),
        Some(serde_json::Value::String(s)) => Decimal::from_str(s.as_str())
            .map(Some)
            .map_err(serde::de::Error::custom),
        Some(other) => Err(serde::de::Error::custom(format!(
            "expected decimal-compatible value, found {other}"
        ))),
    }
}

/// Response envelope for depth snapshots from `/orderBookOrders` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderBookSnapshotResponse {
    #[serde(default)]
    pub code: Option<i32>,
    #[serde(default)]
    pub asks: Vec<crate::data::models::LighterBookLevel>,
    #[serde(default)]
    pub bids: Vec<crate::data::models::LighterBookLevel>,
    #[serde(default)]
    pub total_asks: Option<u64>,
    #[serde(default)]
    pub total_bids: Option<u64>,
}

impl OrderBookSnapshotResponse {
    #[must_use]
    pub fn into_depth(self) -> LighterOrderBookDepth {
        LighterOrderBookDepth {
            code: self.code,
            asks: self.asks,
            bids: self.bids,
            offset: None,
            nonce: None,
        }
    }
}
