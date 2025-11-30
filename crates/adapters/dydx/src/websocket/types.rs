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

//! WebSocket content type definitions for dYdX channels.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nautilus_model::enums::OrderSide;
use serde::{Deserialize, Serialize};

/// Trade message from v4_trades channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxTrade {
    /// Trade ID.
    pub id: String,
    /// Order side (BUY/SELL).
    pub side: OrderSide,
    /// Trade size.
    pub size: String,
    /// Trade price.
    pub price: String,
    /// Trade timestamp.
    pub created_at: DateTime<Utc>,
    /// Order type.
    #[serde(rename = "type")]
    pub order_type: String,
    /// Block height (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_height: Option<String>,
}

/// Contents of v4_trades channel_data message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxTradeContents {
    /// Array of trades.
    pub trades: Vec<DydxTrade>,
}

/// Candle/bar data from v4_candles channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxCandle {
    /// Base token volume.
    pub base_token_volume: String,
    /// Close price.
    pub close: String,
    /// High price.
    pub high: String,
    /// Low price.
    pub low: String,
    /// Open price.
    pub open: String,
    /// Resolution/timeframe.
    pub resolution: String,
    /// Start time.
    pub started_at: DateTime<Utc>,
    /// Starting open interest.
    pub starting_open_interest: String,
    /// Market ticker.
    pub ticker: String,
    /// Number of trades.
    pub trades: i64,
    /// USD volume.
    pub usd_volume: String,
    /// Orderbook mid price at close (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orderbook_mid_price_close: Option<String>,
    /// Orderbook mid price at open (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orderbook_mid_price_open: Option<String>,
}

/// Order book price level (price, size tuple).
pub type PriceLevel = (String, String);

/// Contents of v4_orderbook channel_data/channel_batch_data messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxOrderbookContents {
    /// Bid price levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bids: Option<Vec<PriceLevel>>,
    /// Ask price levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asks: Option<Vec<PriceLevel>>,
}

/// Price level for orderbook snapshot (structured format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxPriceLevel {
    /// Price.
    pub price: String,
    /// Size.
    pub size: String,
}

/// Contents of v4_orderbook subscribed (snapshot) message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxOrderbookSnapshotContents {
    /// Bid price levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bids: Option<Vec<DydxPriceLevel>>,
    /// Ask price levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asks: Option<Vec<DydxPriceLevel>>,
}

/// Oracle price data for a market.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxOraclePriceMarket {
    /// Oracle price.
    pub oracle_price: String,
}

/// Contents of v4_markets channel_data message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DydxMarketsContents {
    /// Oracle prices by market symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oracle_prices: Option<HashMap<String, DydxOraclePriceMarket>>,
}
