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

//! Binance Futures WebSocket message types.
//!
//! Futures streams use standard JSON encoding (not SBE like Spot).

use nautilus_model::{
    data::{Data, OrderBookDeltas},
    instruments::InstrumentAny,
};
use nautilus_network::websocket::WebSocketClient;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::{
    BinanceFuturesOrderType, BinanceKlineInterval, BinanceOrderStatus, BinanceSide,
    BinanceTimeInForce, BinanceWsMethod,
};

/// Output message from the Futures WebSocket handler.
#[derive(Debug, Clone)]
pub enum NautilusFuturesWsMessage {
    /// Market data (trades, quotes).
    Data(Vec<Data>),
    /// Order book deltas.
    Deltas(OrderBookDeltas),
    /// Instrument update.
    Instrument(Box<InstrumentAny>),
    /// Error from the server.
    Error(BinanceFuturesWsErrorMsg),
    /// Raw JSON message (for debugging or unhandled types).
    RawJson(serde_json::Value),
    /// WebSocket reconnected - subscriptions should be restored.
    Reconnected,
}

/// Error message from Binance Futures WebSocket.
#[derive(Debug, Clone)]
pub struct BinanceFuturesWsErrorMsg {
    /// Error code from Binance.
    pub code: i64,
    /// Error message.
    pub msg: String,
}

/// Handler command for client-handler communication.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum BinanceFuturesHandlerCommand {
    /// Set the WebSocket client reference.
    SetClient(WebSocketClient),
    /// Disconnect from the WebSocket.
    Disconnect,
    /// Initialize instruments in the handler cache.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the handler cache.
    UpdateInstrument(InstrumentAny),
    /// Subscribe to streams.
    Subscribe { streams: Vec<String> },
    /// Unsubscribe from streams.
    Unsubscribe { streams: Vec<String> },
}

// ------------------------------------------------------------------------------------------------
// JSON Stream Messages from Binance Futures WebSocket
// ------------------------------------------------------------------------------------------------

/// Aggregate trade stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesAggTradeMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Aggregate trade ID.
    #[serde(rename = "a")]
    pub agg_trade_id: u64,
    /// Price.
    #[serde(rename = "p")]
    pub price: String,
    /// Quantity.
    #[serde(rename = "q")]
    pub quantity: String,
    /// First trade ID.
    #[serde(rename = "f")]
    pub first_trade_id: u64,
    /// Last trade ID.
    #[serde(rename = "l")]
    pub last_trade_id: u64,
    /// Trade time in milliseconds.
    #[serde(rename = "T")]
    pub trade_time: i64,
    /// Is buyer the market maker.
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

/// Trade stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesTradeMsg {
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

/// Order book depth update stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesDepthUpdateMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Transaction time in milliseconds.
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// First update ID.
    #[serde(rename = "U")]
    pub first_update_id: u64,
    /// Final update ID.
    #[serde(rename = "u")]
    pub final_update_id: u64,
    /// Previous final update ID.
    #[serde(rename = "pu")]
    pub prev_final_update_id: u64,
    /// Bids [price, quantity].
    #[serde(rename = "b")]
    pub bids: Vec<[String; 2]>,
    /// Asks [price, quantity].
    #[serde(rename = "a")]
    pub asks: Vec<[String; 2]>,
}

/// Mark price stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesMarkPriceMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Mark price.
    #[serde(rename = "p")]
    pub mark_price: String,
    /// Index price.
    #[serde(rename = "i")]
    pub index_price: String,
    /// Estimated settle price.
    #[serde(rename = "P")]
    pub estimated_settle_price: String,
    /// Funding rate.
    #[serde(rename = "r")]
    pub funding_rate: String,
    /// Next funding time in milliseconds.
    #[serde(rename = "T")]
    pub next_funding_time: i64,
}

/// Book ticker stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesBookTickerMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Update ID.
    #[serde(rename = "u")]
    pub update_id: u64,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Transaction time in milliseconds.
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
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
}

/// Kline/candlestick stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesKlineMsg {
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
    pub kline: BinanceFuturesKlineData,
}

/// Kline data within kline message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesKlineData {
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
    /// Quote asset volume.
    #[serde(rename = "q")]
    pub quote_volume: String,
    /// Taker buy base asset volume.
    #[serde(rename = "V")]
    pub taker_buy_volume: String,
    /// Taker buy quote asset volume.
    #[serde(rename = "Q")]
    pub taker_buy_quote_volume: String,
}

/// Liquidation order stream message.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesLiquidationMsg {
    /// Event type.
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Order data.
    #[serde(rename = "o")]
    pub order: BinanceFuturesLiquidationOrder,
}

/// Liquidation order details.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesLiquidationOrder {
    /// Symbol.
    #[serde(rename = "s")]
    pub symbol: Ustr,
    /// Order side.
    #[serde(rename = "S")]
    pub side: BinanceSide,
    /// Order type.
    #[serde(rename = "o")]
    pub order_type: BinanceFuturesOrderType,
    /// Time in force.
    #[serde(rename = "f")]
    pub time_in_force: BinanceTimeInForce,
    /// Original quantity.
    #[serde(rename = "q")]
    pub original_qty: String,
    /// Price.
    #[serde(rename = "p")]
    pub price: String,
    /// Average price.
    #[serde(rename = "ap")]
    pub average_price: String,
    /// Order status.
    #[serde(rename = "X")]
    pub status: BinanceOrderStatus,
    /// Last filled quantity.
    #[serde(rename = "l")]
    pub last_filled_qty: String,
    /// Accumulated filled quantity.
    #[serde(rename = "z")]
    pub accumulated_qty: String,
    /// Trade time in milliseconds.
    #[serde(rename = "T")]
    pub trade_time: i64,
}

// ------------------------------------------------------------------------------------------------
// Subscription Request/Response
// ------------------------------------------------------------------------------------------------

/// WebSocket subscription request.
#[derive(Debug, Clone, Serialize)]
pub struct BinanceFuturesWsSubscribeRequest {
    /// Request method.
    pub method: BinanceWsMethod,
    /// Stream names to subscribe.
    pub params: Vec<String>,
    /// Request ID.
    pub id: u64,
}

/// WebSocket subscription response.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesWsSubscribeResponse {
    /// Response result (null on success).
    pub result: Option<serde_json::Value>,
    /// Request ID echoed back.
    pub id: u64,
}

/// WebSocket error response.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesWsErrorResponse {
    /// Error code.
    pub code: i64,
    /// Error message.
    pub msg: String,
    /// Request ID if available.
    pub id: Option<u64>,
}
