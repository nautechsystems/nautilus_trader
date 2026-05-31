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

//! Binance Spot public JSON WebSocket message types.

use nautilus_network::websocket::WebSocketClient;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    common::enums::{BinanceKlineInterval, BinanceWsMethod},
    spot::websocket::streams::messages::{
        BinanceWsErrorMsg, BinanceWsErrorResponse, BinanceWsResponse,
    },
};

/// Output message from the Spot public JSON WebSocket handler.
#[derive(Debug, Clone)]
pub enum BinanceSpotPublicWsMessage {
    /// Trade stream event.
    Trade(BinanceSpotTradeMsg),
    /// Best bid/ask stream event.
    BookTicker(BinanceSpotBookTickerMsg),
    /// Partial depth snapshot stream event.
    DepthSnapshot(BinanceSpotPartialDepthMsg),
    /// Kline/candlestick stream event.
    Kline(BinanceSpotKlineMsg),
    /// Server shutdown notice.
    ServerShutdown(BinanceSpotServerShutdownMsg),
    /// Raw JSON message (unhandled or unknown event).
    RawJson(serde_json::Value),
    /// Error from the server.
    Error(BinanceWsErrorMsg),
    /// WebSocket reconnected.
    Reconnected,
}

/// Commands sent from the outer client to the inner handler.
#[allow(
    missing_debug_implementations,
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum BinanceSpotPublicWsCommand {
    /// Set the WebSocket client after connection.
    SetClient(WebSocketClient),
    /// Disconnect and clean up.
    Disconnect,
    /// Subscribe to streams.
    Subscribe { streams: Vec<String> },
    /// Unsubscribe from streams.
    Unsubscribe { streams: Vec<String> },
}

/// Binance WebSocket subscription request.
#[derive(Debug, Clone, Serialize)]
pub struct BinanceWsSubscription {
    /// Request method.
    pub method: BinanceWsMethod,
    /// Stream names to subscribe/unsubscribe.
    pub params: Vec<String>,
    /// Request ID for correlation.
    pub id: u64,
}

impl BinanceWsSubscription {
    /// Create a subscribe request.
    #[must_use]
    pub fn subscribe(streams: Vec<String>, id: u64) -> Self {
        Self {
            method: BinanceWsMethod::Subscribe,
            params: streams,
            id,
        }
    }

    /// Create an unsubscribe request.
    #[must_use]
    pub fn unsubscribe(streams: Vec<String>, id: u64) -> Self {
        Self {
            method: BinanceWsMethod::Unsubscribe,
            params: streams,
            id,
        }
    }
}

/// Combined stream wrapper used by `/stream` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceCombinedStreamEvent {
    /// Stream name (e.g., `btcusdt@depth20`).
    pub stream: String,
    /// Payload data.
    pub data: serde_json::Value,
}

/// Trade stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotTradeMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Trade ID.
    #[serde(rename = "t")]
    pub trade_id: u64,
    /// Price.
    #[serde(rename = "p")]
    pub price: String,
    /// Quantity.
    #[serde(rename = "q")]
    pub quantity: String,
    /// Trade time in milliseconds.
    #[serde(rename = "T")]
    pub trade_time: i64,
    /// Is buyer the market maker.
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

/// Best bid/ask stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotBookTickerMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Order book update id.
    #[serde(rename = "u")]
    pub book_update_id: u64,
    /// Best bid price.
    #[serde(rename = "b")]
    pub best_bid_price: String,
    /// Best bid quantity.
    #[serde(rename = "B")]
    pub best_bid_qty: String,
    /// Best ask price.
    #[serde(rename = "a")]
    pub best_ask_price: String,
    /// Best ask quantity.
    #[serde(rename = "A")]
    pub best_ask_qty: String,
    /// Transaction time in milliseconds (if provided).
    #[serde(rename = "T")]
    pub transaction_time: Option<i64>,
}

/// Partial depth stream message with symbol inferred from stream name.
#[derive(Debug, Clone)]
pub struct BinanceSpotPartialDepthMsg {
    /// Symbol.
    pub symbol: Ustr,
    /// Last update ID.
    pub last_update_id: u64,
    /// Bid levels `[price, qty]`.
    pub bids: Vec<[String; 2]>,
    /// Ask levels `[price, qty]`.
    pub asks: Vec<[String; 2]>,
}

/// Raw partial depth payload from Spot JSON stream.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotPartialDepthPayload {
    /// Last update ID.
    #[serde(rename = "lastUpdateId")]
    pub last_update_id: u64,
    /// Bid levels `[price, qty]`.
    pub bids: Vec<[String; 2]>,
    /// Ask levels `[price, qty]`.
    pub asks: Vec<[String; 2]>,
}

/// Kline stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotKlineMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Kline data.
    #[serde(rename = "k")]
    pub kline: BinanceSpotKlineData,
}

/// Kline data within kline message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotKlineData {
    /// Kline start time.
    #[serde(rename = "t")]
    pub start_time: i64,
    /// Kline close time.
    #[serde(rename = "T")]
    pub close_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Kline interval.
    #[serde(rename = "i")]
    pub interval: BinanceKlineInterval,
    /// First trade ID.
    #[serde(rename = "f")]
    pub first_trade_id: i64,
    /// Last trade ID.
    #[serde(rename = "L")]
    pub last_trade_id: i64,
    /// Open price.
    #[serde(rename = "o")]
    pub open: String,
    /// Close price.
    #[serde(rename = "c")]
    pub close: String,
    /// High price.
    #[serde(rename = "h")]
    pub high: String,
    /// Low price.
    #[serde(rename = "l")]
    pub low: String,
    /// Base asset volume.
    #[serde(rename = "v")]
    pub volume: String,
    /// Number of trades.
    #[serde(rename = "n")]
    pub num_trades: i64,
    /// Is this kline closed.
    #[serde(rename = "x")]
    pub is_closed: bool,
}

/// Server shutdown event sent before Binance disconnects clients.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceSpotServerShutdownMsg {
    /// Event type (`"serverShutdown"`).
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
}

pub type BinanceSpotWsResponse = BinanceWsResponse;
pub type BinanceSpotWsErrorResponse = BinanceWsErrorResponse;
