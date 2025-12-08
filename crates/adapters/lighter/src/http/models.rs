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

/// Response envelope for depth snapshots (REST or WS-compatible).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OrderBookSnapshotResponse {
    Wrapped { order_book: LighterOrderBookDepth },
    Depth(LighterOrderBookDepth),
}

impl OrderBookSnapshotResponse {
    #[must_use]
    pub fn into_depth(self) -> LighterOrderBookDepth {
        match self {
            Self::Wrapped { order_book } => order_book,
            Self::Depth(depth) => depth,
        }
    }
}
