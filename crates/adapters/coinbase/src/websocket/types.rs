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

//! WebSocket message types for Coinbase Advanced Trade API.

use serde::{Deserialize, Serialize};

/// WebSocket channel types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Heartbeats channel - keeps connection alive
    Heartbeats,
    /// Candles channel - real-time OHLCV data
    Candles,
    /// Market trades channel - real-time trades
    MarketTrades,
    /// Status channel - product status updates
    Status,
    /// Ticker channel - real-time price updates
    Ticker,
    /// Ticker batch channel - batched price updates every 5s
    TickerBatch,
    /// Level2 order book channel
    Level2,
    /// User channel - user-specific updates (requires auth)
    User,
    /// Futures balance summary channel (requires auth)
    FuturesBalanceSummary,
}

impl Channel {
    /// Returns true if this channel requires authentication
    #[must_use]
    pub fn requires_auth(&self) -> bool {
        matches!(self, Self::User | Self::FuturesBalanceSummary)
    }

    /// Returns the channel name as a string
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Heartbeats => "heartbeats",
            Self::Candles => "candles",
            Self::MarketTrades => "market_trades",
            Self::Status => "status",
            Self::Ticker => "ticker",
            Self::TickerBatch => "ticker_batch",
            Self::Level2 => "level2",
            Self::User => "user",
            Self::FuturesBalanceSummary => "futures_balance_summary",
        }
    }
}

/// Subscribe request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRequest {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_ids: Option<Vec<String>>,
    pub channel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwt: Option<String>,
}

impl SubscribeRequest {
    /// Create a new subscribe request without authentication
    #[must_use]
    pub fn new(product_ids: Vec<String>, channel: Channel) -> Self {
        Self {
            msg_type: "subscribe".to_string(),
            product_ids: Some(product_ids),
            channel: channel.as_str().to_string(),
            jwt: None,
        }
    }

    /// Create a new subscribe request for heartbeats (no product_ids needed)
    #[must_use]
    pub fn new_heartbeats() -> Self {
        Self {
            msg_type: "subscribe".to_string(),
            product_ids: None,
            channel: "heartbeats".to_string(),
            jwt: None,
        }
    }

    /// Add JWT authentication
    #[must_use]
    pub fn with_jwt(mut self, jwt: String) -> Self {
        self.jwt = Some(jwt);
        self
    }
}

/// Unsubscribe request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeRequest {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_ids: Option<Vec<String>>,
    pub channel: String,
}

impl UnsubscribeRequest {
    /// Create a new unsubscribe request
    #[must_use]
    pub fn new(product_ids: Vec<String>, channel: Channel) -> Self {
        Self {
            msg_type: "unsubscribe".to_string(),
            product_ids: Some(product_ids),
            channel: channel.as_str().to_string(),
        }
    }

    /// Create a new unsubscribe request for heartbeats (no product_ids needed)
    #[must_use]
    pub fn new_heartbeats() -> Self {
        Self {
            msg_type: "unsubscribe".to_string(),
            product_ids: None,
            channel: "heartbeats".to_string(),
        }
    }
}

/// WebSocket message from server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "channel")]
pub enum WebSocketMessage {
    /// Subscriptions message
    #[serde(rename = "subscriptions")]
    Subscriptions {
        events: Vec<SubscriptionEvent>,
    },
    /// Heartbeat message
    #[serde(rename = "heartbeats")]
    Heartbeats {
        events: Vec<HeartbeatEvent>,
    },
    /// Candles message
    #[serde(rename = "candles")]
    Candles {
        events: Vec<CandleEvent>,
    },
    /// Market trades message
    #[serde(rename = "market_trades")]
    MarketTrades {
        events: Vec<MarketTradeEvent>,
    },
    /// Status message
    #[serde(rename = "status")]
    Status {
        events: Vec<StatusEvent>,
    },
    /// Ticker message
    #[serde(rename = "ticker")]
    Ticker {
        events: Vec<TickerEvent>,
    },
    /// Ticker batch message
    #[serde(rename = "ticker_batch")]
    TickerBatch {
        events: Vec<TickerEvent>,
    },
    /// Level2 message
    #[serde(rename = "l2_data", alias = "level2")]
    Level2 {
        events: Vec<Level2Event>,
    },
    /// User message
    #[serde(rename = "user")]
    User {
        events: Vec<UserEvent>,
    },
    /// Futures balance summary message
    #[serde(rename = "futures_balance_summary")]
    FuturesBalanceSummary {
        events: Vec<FuturesBalanceEvent>,
    },
}

/// Subscription event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionEvent {
    pub subscriptions: serde_json::Value,
}

/// Heartbeat event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatEvent {
    pub current_time: String,
    pub heartbeat_counter: u64,
}

/// Candle event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandleEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub candles: Vec<Candle>,
}

/// Candle data (OHLCV)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub start: String,
    pub high: String,
    pub low: String,
    pub open: String,
    pub close: String,
    pub volume: String,
    pub product_id: String,
}

/// Ticker event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub tickers: Vec<Ticker>,
}

/// Ticker data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticker {
    #[serde(rename = "type")]
    pub ticker_type: String,
    pub product_id: String,
    pub price: String,
    pub volume_24_h: String,
    pub low_24_h: String,
    pub high_24_h: String,
    pub low_52_w: String,
    pub high_52_w: String,
    pub price_percent_chg_24_h: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_bid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_bid_quantity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_ask: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_ask_quantity: Option<String>,
}

/// Level2 event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Level2Event {
    #[serde(rename = "type")]
    pub event_type: String,
    pub product_id: String,
    pub updates: Vec<Level2Update>,
}

/// Level2 update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Level2Update {
    pub side: String,
    pub event_time: String,
    pub price_level: String,
    pub new_quantity: String,
}

/// Market trade event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketTradeEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub trades: Vec<Trade>,
}

/// Trade data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub trade_id: String,
    pub product_id: String,
    pub price: String,
    pub size: String,
    pub side: String,
    pub time: String,
}

/// User event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orders: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub positions: Option<Vec<serde_json::Value>>,
}

/// Status event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub products: Vec<ProductStatus>,
}

/// Product status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductStatus {
    pub product_type: String,
    pub id: String,
    pub base_currency: String,
    pub quote_currency: String,
    pub base_increment: String,
    pub quote_increment: String,
    pub display_name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_market_funds: Option<String>,
}

/// Futures balance event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesBalanceEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub futures_balance_summary: serde_json::Value,
}

