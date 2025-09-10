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

//! WebSocket message structures for Delta Exchange.

use chrono::{DateTime, Utc};
use nautilus_model::{
    data::{Data, OrderBookDeltas},
    events::OrderEventAny,
    instruments::InstrumentAny,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::enums::{DeltaExchangeWsChannel, WsMessageType, WsOperation};
use crate::common::{
    enums::{
        DeltaExchangeOrderEventType, DeltaExchangeOrderState, DeltaExchangeOrderType,
        DeltaExchangePositionEventType, DeltaExchangeSide,
    },
    parse::{parse_decimal_or_zero, parse_empty_string_as_none, parse_optional_decimal},
};

/// Represents different types of messages that can be sent to Nautilus.
#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    /// Market data update.
    Data(Data),
    /// Multiple data updates.
    DataVec(Vec<Data>),
    /// Order book deltas.
    Deltas(OrderBookDeltas),
    /// Instrument definition.
    Instrument(InstrumentAny),
    /// Order event.
    OrderEvent(OrderEventAny),
    /// Raw message for debugging.
    Raw(String),
}

/// Subscription request message.
#[derive(Debug, Serialize)]
pub struct DeltaExchangeSubscription {
    /// Operation type (subscribe/unsubscribe).
    #[serde(rename = "type")]
    pub op: WsOperation,
    /// List of channels to subscribe to.
    pub channels: Vec<DeltaExchangeWsChannel>,
    /// Product symbols (optional, for channel-specific subscriptions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols: Option<Vec<Ustr>>,
}

/// Authentication message for private channels.
#[derive(Debug, Serialize)]
pub struct DeltaExchangeAuth {
    /// Operation type (always "auth").
    #[serde(rename = "type")]
    pub op: WsOperation,
    /// API key.
    pub api_key: Ustr,
    /// Timestamp in milliseconds.
    pub timestamp: u64,
    /// HMAC signature.
    pub signature: String,
}

/// Generic WebSocket message from Delta Exchange.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum DeltaExchangeWsMessage {
    /// Authentication response.
    Auth(DeltaExchangeWsAuthMsg),
    /// Subscription confirmation.
    Subscription(DeltaExchangeWsSubscriptionMsg),
    /// Error message.
    Error(DeltaExchangeWsErrorMsg),
    /// Ticker update.
    Ticker(DeltaExchangeWsTickerMsg),
    /// Order book snapshot.
    OrderBookSnapshot(DeltaExchangeWsOrderBookSnapshotMsg),
    /// Order book update.
    OrderBookUpdate(DeltaExchangeWsOrderBookUpdateMsg),
    /// Trade update.
    Trade(DeltaExchangeWsTradeMsg),
    /// Candlestick update.
    Candle(DeltaExchangeWsCandleMsg),
    /// Mark price update.
    MarkPrice(DeltaExchangeWsMarkPriceMsg),
    /// Funding rate update.
    FundingRate(DeltaExchangeWsFundingRateMsg),
    /// Order update.
    Order(DeltaExchangeWsOrderMsg),
    /// Position update.
    Position(DeltaExchangeWsPositionMsg),
    /// User trade update.
    UserTrade(DeltaExchangeWsUserTradeMsg),
    /// Margin update.
    Margin(DeltaExchangeWsMarginMsg),
    /// Heartbeat/ping message.
    Ping(DeltaExchangeWsPingMsg),
}

/// Authentication response message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsAuthMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub success: bool,
    pub message: Option<String>,
}

/// Subscription confirmation message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsSubscriptionMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub channels: Vec<DeltaExchangeWsChannel>,
    pub symbols: Option<Vec<Ustr>>,
    pub success: bool,
    pub message: Option<String>,
}

/// Error message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsErrorMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub code: String,
    pub message: String,
    pub channel: Option<DeltaExchangeWsChannel>,
    pub symbol: Option<Ustr>,
}

/// Ticker update message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsTickerMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub symbol: Ustr,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub price: Option<Decimal>,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub change_24h: Option<Decimal>,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub high_24h: Option<Decimal>,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub low_24h: Option<Decimal>,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub volume_24h: Option<Decimal>,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub bid: Option<Decimal>,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub ask: Option<Decimal>,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub mark_price: Option<Decimal>,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub open_interest: Option<Decimal>,
    pub timestamp: u64,
}

/// Order book level.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsOrderBookLevel {
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub price: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub size: Decimal,
}

/// Order book snapshot message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsOrderBookSnapshotMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub symbol: Ustr,
    pub buy: Vec<DeltaExchangeWsOrderBookLevel>,
    pub sell: Vec<DeltaExchangeWsOrderBookLevel>,
    pub last_sequence_no: u64,
    pub timestamp: u64,
}

/// Order book update message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsOrderBookUpdateMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub symbol: Ustr,
    pub buy: Vec<DeltaExchangeWsOrderBookLevel>,
    pub sell: Vec<DeltaExchangeWsOrderBookLevel>,
    pub sequence_no: u64,
    pub prev_sequence_no: u64,
    pub timestamp: u64,
}

/// Trade message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsTradeMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub symbol: Ustr,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub price: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub size: Decimal,
    pub buyer_role: String,
    pub timestamp: u64,
}

/// Candlestick message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsCandleMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub symbol: Ustr,
    pub resolution: String,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub open: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub high: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub low: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub close: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub volume: Decimal,
    pub timestamp: u64,
}

/// Mark price message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsMarkPriceMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub symbol: Ustr,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub mark_price: Decimal,
    pub timestamp: u64,
}

/// Funding rate message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsFundingRateMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub symbol: Ustr,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub funding_rate: Decimal,
    pub next_funding_time: u64,
    pub timestamp: u64,
}

/// Order update message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsOrderMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub event_type: DeltaExchangeOrderEventType,
    pub id: u64,
    pub product_id: u64,
    pub product_symbol: Ustr,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub size: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub unfilled_size: Decimal,
    pub side: DeltaExchangeSide,
    pub order_type: DeltaExchangeOrderType,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub limit_price: Option<Decimal>,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub stop_price: Option<Decimal>,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub paid_price: Option<Decimal>,
    pub state: DeltaExchangeOrderState,
    #[serde(deserialize_with = "parse_empty_string_as_none")]
    pub client_order_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Position update message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsPositionMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub event_type: DeltaExchangePositionEventType,
    pub product_id: u64,
    pub product_symbol: Ustr,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub size: Decimal,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub entry_price: Option<Decimal>,
    #[serde(deserialize_with = "parse_optional_decimal")]
    pub mark_price: Option<Decimal>,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub unrealized_pnl: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub realized_pnl: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub margin: Decimal,
    pub updated_at: DateTime<Utc>,
}

/// User trade message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsUserTradeMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub id: u64,
    pub order_id: u64,
    pub product_id: u64,
    pub product_symbol: Ustr,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub size: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub price: Decimal,
    pub side: DeltaExchangeSide,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub commission: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub realized_pnl: Decimal,
    pub role: String,
    #[serde(deserialize_with = "parse_empty_string_as_none")]
    pub client_order_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Margin update message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsMarginMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub asset_id: u64,
    pub asset_symbol: Ustr,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub available_balance: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub order_margin: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub position_margin: Decimal,
    #[serde(deserialize_with = "parse_decimal_or_zero")]
    pub balance: Decimal,
    pub updated_at: DateTime<Utc>,
}

/// Ping/heartbeat message.
#[derive(Debug, Deserialize)]
pub struct DeltaExchangeWsPingMsg {
    #[serde(rename = "type")]
    pub message_type: WsMessageType,
    pub timestamp: u64,
}
