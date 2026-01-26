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

//! WebSocket message types for the AX Exchange API.
//!
//! This module contains request and response message structures for both
//! market data and order management WebSocket streams.

use nautilus_core::{
    UnixNanos,
    serialization::{
        deserialize_optional_decimal, serialize_decimal_as_str, serialize_optional_decimal_as_str,
    },
};
use nautilus_model::{
    data::{Bar, Data, OrderBookDeltas},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderExpired, OrderFilled, OrderRejected,
    },
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::Currency,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::error::AxWsErrorResponse;
use crate::common::{
    enums::{
        AxCandleWidth, AxMarketDataLevel, AxOrderSide, AxOrderStatus, AxOrderType, AxTimeInForce,
    },
    parse::deserialize_decimal_or_zero,
};

/// Nautilus domain message emitted after parsing Ax WebSocket events.
///
/// This enum contains fully-parsed Nautilus domain objects ready for consumption
/// by the DataClient without additional processing.
#[derive(Debug, Clone)]
pub enum NautilusDataWsMessage {
    /// Market data (trades, quotes).
    Data(Vec<Data>),
    /// Order book deltas.
    Deltas(OrderBookDeltas),
    /// Bar/candle data.
    Bar(Bar),
    /// Heartbeat message.
    Heartbeat,
    /// Error from venue or client.
    Error(AxWsError),
    /// WebSocket reconnected notification.
    Reconnected,
}

/// Nautilus domain messages for the Ax orders WebSocket.
///
/// This enum contains parsed messages from the WebSocket stream.
/// Variants contain fully-parsed Nautilus domain objects.
#[derive(Debug, Clone)]
pub enum NautilusExecWsMessage {
    /// Order accepted by the venue.
    OrderAccepted(OrderAccepted),
    /// Order filled (partial or complete).
    OrderFilled(Box<OrderFilled>),
    /// Order canceled.
    OrderCanceled(OrderCanceled),
    /// Order expired.
    OrderExpired(OrderExpired),
    /// Order rejected by venue.
    OrderRejected(OrderRejected),
    /// Order cancel rejected by venue.
    OrderCancelRejected(OrderCancelRejected),
    /// Order status reports from order updates.
    OrderStatusReports(Vec<OrderStatusReport>),
    /// Fill reports from executions.
    FillReports(Vec<FillReport>),
}

/// Subscribe request for market data.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxMdSubscribe {
    /// Client request ID for correlation.
    pub request_id: i64,
    /// Request type (always "subscribe").
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Instrument symbol.
    pub symbol: String,
    /// Market data level (LEVEL_1, LEVEL_2, LEVEL_3).
    pub level: AxMarketDataLevel,
}

/// Unsubscribe request for market data.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxMdUnsubscribe {
    /// Client request ID for correlation.
    pub request_id: i64,
    /// Request type (always "unsubscribe").
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Instrument symbol.
    pub symbol: String,
}

/// Subscribe request for candle data.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxMdSubscribeCandles {
    /// Client request ID for correlation.
    pub request_id: i64,
    /// Request type (always "subscribe_candles").
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Instrument symbol.
    pub symbol: String,
    /// Candle width/interval.
    pub width: AxCandleWidth,
}

/// Unsubscribe request for candle data.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxMdUnsubscribeCandles {
    /// Client request ID for correlation.
    pub request_id: i64,
    /// Request type (always "unsubscribe_candles").
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Instrument symbol.
    pub symbol: String,
    /// Candle width/interval.
    pub width: AxCandleWidth,
}

/// Heartbeat message from market data WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxMdHeartbeat {
    /// Message type (always "h").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
}

/// Ticker/statistics message from market data WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxMdTicker {
    /// Message type (always "s").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Instrument symbol.
    pub s: Ustr,
    /// Last price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub p: Decimal,
    /// Last quantity.
    pub q: i64,
    /// Open price (24h).
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub o: Decimal,
    /// Low price (24h).
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub l: Decimal,
    /// High price (24h).
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub h: Decimal,
    /// Volume (24h).
    pub v: i64,
    /// Open interest.
    #[serde(default)]
    pub oi: Option<i64>,
}

/// Trade message from market data WebSocket.
///
/// Note: Uses same "s" message type as ticker but with different fields.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxMdTrade {
    /// Message type (always "s").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Instrument symbol.
    pub s: Ustr,
    /// Trade price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub p: Decimal,
    /// Trade quantity.
    pub q: i64,
    /// Trade direction: "B" (buy) or "S" (sell). Optional for some message types.
    #[serde(default)]
    pub d: Option<AxOrderSide>,
}

/// Candle/OHLCV message from market data WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxMdCandle {
    /// Message type (always "c").
    pub t: String,
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Candle timestamp (Unix epoch).
    pub ts: i64,
    /// Open price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub open: Decimal,
    /// Low price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub low: Decimal,
    /// High price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub high: Decimal,
    /// Close price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub close: Decimal,
    /// Total volume.
    pub volume: i64,
    /// Buy volume.
    pub buy_volume: i64,
    /// Sell volume.
    pub sell_volume: i64,
    /// Candle width/interval.
    pub width: AxCandleWidth,
}

/// Price level entry in order book.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxBookLevel {
    /// Price at this level.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub p: Decimal,
    /// Quantity at this level.
    pub q: i64,
}

/// Price level entry with individual order breakdown (L3).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxBookLevelL3 {
    /// Price at this level.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub p: Decimal,
    /// Total quantity at this level.
    pub q: i64,
    /// Individual order quantities at this price.
    pub o: Vec<i64>,
}

/// Level 1 order book update (best bid/ask).
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxMdBookL1 {
    /// Message type (always "1").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Instrument symbol.
    pub s: Ustr,
    /// Bid levels (typically just best bid).
    pub b: Vec<AxBookLevel>,
    /// Ask levels (typically just best ask).
    pub a: Vec<AxBookLevel>,
}

/// Level 2 order book update (aggregated price levels).
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxMdBookL2 {
    /// Message type (always "2").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Instrument symbol.
    pub s: Ustr,
    /// Bid levels.
    pub b: Vec<AxBookLevel>,
    /// Ask levels.
    pub a: Vec<AxBookLevel>,
}

/// Level 3 order book update (individual order quantities).
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxMdBookL3 {
    /// Message type (always "3").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Instrument symbol.
    pub s: Ustr,
    /// Bid levels with order breakdown.
    pub b: Vec<AxBookLevelL3>,
    /// Ask levels with order breakdown.
    pub a: Vec<AxBookLevelL3>,
}

/// Place order request via WebSocket.
///
/// # References
/// - <https://docs.architect.co/sdk-reference/order-entry>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsPlaceOrder {
    /// Request ID for correlation.
    pub rid: i64,
    /// Message type (always "p").
    pub t: String,
    /// Instrument symbol.
    pub s: String,
    /// Order side: "B" (buy) or "S" (sell).
    pub d: AxOrderSide,
    /// Order quantity.
    pub q: i64,
    /// Order price (limit price).
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_or_zero"
    )]
    pub p: Decimal,
    /// Time in force.
    pub tif: AxTimeInForce,
    /// Post-only flag (maker-or-cancel).
    pub po: bool,
    /// Optional order tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Order type (defaults to LIMIT if not specified).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type: Option<AxOrderType>,
    /// Trigger price for stop-loss orders.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_decimal_as_str",
        deserialize_with = "deserialize_optional_decimal",
        default
    )]
    pub trigger_price: Option<Decimal>,
}

/// Cancel order request via WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsCancelOrder {
    /// Request ID for correlation.
    pub rid: i64,
    /// Message type (always "x").
    pub t: String,
    /// Order ID to cancel.
    pub oid: String,
}

/// Get open orders request via WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsGetOpenOrders {
    /// Request ID for correlation.
    pub rid: i64,
    /// Message type (always "o").
    pub t: String,
}

/// Place order response from WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsPlaceOrderResponse {
    /// Request ID matching the original request.
    pub rid: i64,
    /// Response result.
    pub res: AxWsPlaceOrderResult,
}

/// Result payload for place order response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsPlaceOrderResult {
    /// Order ID of the placed order.
    pub oid: String,
}

/// Cancel order response from WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsCancelOrderResponse {
    /// Request ID matching the original request.
    pub rid: i64,
    /// Response result.
    pub res: AxWsCancelOrderResult,
}

/// Result payload for cancel order response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsCancelOrderResult {
    /// Whether the cancel request was received.
    pub cxl_rx: bool,
}

/// Open orders response from WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsOpenOrdersResponse {
    /// Request ID matching the original request.
    pub rid: i64,
    /// List of open orders.
    pub res: Vec<AxWsOrder>,
}

/// Error response from the Ax orders WebSocket.
///
/// Returned when a request fails (e.g., insufficient margin, invalid order).
#[derive(Clone, Debug, Deserialize)]
pub struct AxWsOrderErrorResponse {
    /// Request ID matching the original request.
    pub rid: i64,
    /// Error details.
    pub err: AxWsOrderError,
}

/// Error details in an error response.
#[derive(Clone, Debug, Deserialize)]
pub struct AxWsOrderError {
    /// Error code (e.g., 400).
    pub code: i64,
    /// Error message.
    pub msg: String,
}

/// List subscription response from the Ax orders WebSocket.
///
/// Returned when subscribing to order updates, contains a list ID for the subscription.
#[derive(Clone, Debug, Deserialize)]
pub struct AxWsListResponse {
    /// Request ID matching the original request.
    pub rid: i64,
    /// Response result.
    pub res: AxWsListResult,
}

/// List subscription result payload.
#[derive(Clone, Debug, Deserialize)]
pub struct AxWsListResult {
    /// List subscription ID.
    pub li: String,
    /// Order data (null on initial subscription, array of orders otherwise).
    #[serde(default)]
    pub o: Option<Vec<AxWsOrder>>,
}

/// Order details in WebSocket messages.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsOrder {
    /// Order ID.
    pub oid: String,
    /// User ID.
    pub u: String,
    /// Instrument symbol.
    pub s: Ustr,
    /// Order price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub p: Decimal,
    /// Order quantity.
    pub q: i64,
    /// Executed quantity.
    pub xq: i64,
    /// Remaining quantity.
    pub rq: i64,
    /// Order status.
    pub o: AxOrderStatus,
    /// Order side.
    pub d: AxOrderSide,
    /// Time in force.
    pub tif: AxTimeInForce,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Optional order tag.
    #[serde(default)]
    pub tag: Option<String>,
}

/// Heartbeat event from orders WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsHeartbeat {
    /// Message type (always "h").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
}

/// Order acknowledged event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsOrderAcknowledged {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: AxWsOrder,
}

/// Trade execution details for fill events.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsTradeExecution {
    /// Trade ID.
    pub tid: String,
    /// Instrument symbol.
    pub s: Ustr,
    /// Executed quantity.
    pub q: i64,
    /// Execution price.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub p: Decimal,
    /// Trade direction.
    pub d: AxOrderSide,
    /// Whether this was an aggressor (taker) order.
    pub agg: bool,
}

/// Order partially filled event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsOrderPartiallyFilled {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: AxWsOrder,
    /// Trade execution details.
    pub xs: AxWsTradeExecution,
}

/// Order filled event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsOrderFilled {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: AxWsOrder,
    /// Trade execution details.
    pub xs: AxWsTradeExecution,
}

/// Order canceled event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsOrderCanceled {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: AxWsOrder,
    /// Cancellation reason.
    pub xr: String,
    /// Cancellation text/description.
    #[serde(default)]
    pub txt: Option<String>,
}

/// Order rejected event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsOrderRejected {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: AxWsOrder,
    /// Rejection reason code.
    pub r: String,
    /// Rejection text/description.
    #[serde(default)]
    pub txt: Option<String>,
}

/// Order expired event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsOrderExpired {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: AxWsOrder,
}

/// Order replaced/amended event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsOrderReplaced {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: AxWsOrder,
}

/// Order done for day event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsOrderDoneForDay {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: AxWsOrder,
}

/// Cancel rejected event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AxWsCancelRejected {
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Order ID that failed to cancel.
    pub oid: String,
    /// Rejection reason code.
    pub r: String,
    /// Rejection text/description.
    #[serde(default)]
    pub txt: Option<String>,
}

/// Internal raw message from the Ax orders WebSocket.
///
/// This enum uses serde's tagged deserialization to automatically
/// discriminate between different event types based on the "t" field.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "t")]
pub(crate) enum AxWsOrderEvent {
    /// Heartbeat message.
    #[serde(rename = "h")]
    Heartbeat,
    /// Order acknowledged.
    #[serde(rename = "n")]
    Acknowledged(AxWsOrderAcknowledged),
    /// Order partially filled.
    #[serde(rename = "p")]
    PartiallyFilled(AxWsOrderPartiallyFilled),
    /// Order filled.
    #[serde(rename = "f")]
    Filled(AxWsOrderFilled),
    /// Order canceled.
    #[serde(rename = "c")]
    Canceled(AxWsOrderCanceled),
    /// Order rejected.
    #[serde(rename = "j")]
    Rejected(AxWsOrderRejected),
    /// Order expired.
    #[serde(rename = "x")]
    Expired(AxWsOrderExpired),
    /// Order replaced.
    #[serde(rename = "r")]
    Replaced(AxWsOrderReplaced),
    /// Order done for day.
    #[serde(rename = "d")]
    DoneForDay(AxWsOrderDoneForDay),
    /// Cancel rejected.
    #[serde(rename = "e")]
    CancelRejected(AxWsCancelRejected),
}

/// Internal raw response from the Ax orders WebSocket.
///
/// Response messages have "rid" and "res" fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum AxWsOrderResponse {
    /// Place order response (res has "oid").
    PlaceOrder(AxWsPlaceOrderResponse),
    /// Cancel order response (res has "cxl_rx").
    CancelOrder(AxWsCancelOrderResponse),
    /// Open orders response (res is array).
    OpenOrders(AxWsOpenOrdersResponse),
    /// List subscription response (res has "li").
    List(AxWsListResponse),
}

/// Internal raw message from the Ax orders WebSocket.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum AxWsRawMessage {
    /// Error response message (has "rid" and "err").
    Error(AxWsOrderErrorResponse),
    /// Response message (has "rid" and "res").
    Response(AxWsOrderResponse),
    /// Event message (has "t" field).
    Event(Box<AxWsOrderEvent>),
}

/// Ax-specific messages for the orders WebSocket.
///
/// This enum contains response and control messages from the WebSocket stream.
#[derive(Debug, Clone)]
pub enum AxOrdersWsMessage {
    /// Nautilus domain messages parsed from order events.
    Nautilus(NautilusExecWsMessage),
    /// Place order response.
    PlaceOrderResponse(AxWsPlaceOrderResponse),
    /// Cancel order response.
    CancelOrderResponse(AxWsCancelOrderResponse),
    /// Open orders response.
    OpenOrdersResponse(AxWsOpenOrdersResponse),
    /// Error from venue or client.
    Error(AxWsError),
    /// WebSocket reconnected notification.
    Reconnected,
    /// Authentication successful notification.
    Authenticated,
}

/// Represents an error event surfaced by the WebSocket client.
#[derive(Debug, Clone)]
pub struct AxWsError {
    /// Error code from Ax.
    pub code: Option<String>,
    /// Human readable message.
    pub message: String,
    /// Optional request ID related to the failure.
    pub request_id: Option<i64>,
}

impl AxWsError {
    /// Creates a new error with the provided message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            code: None,
            message: message.into(),
            request_id: None,
        }
    }

    /// Creates a new error with code and message.
    #[must_use]
    pub fn with_code(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: Some(code.into()),
            message: message.into(),
            request_id: None,
        }
    }
}

impl From<AxWsOrderErrorResponse> for AxWsError {
    fn from(resp: AxWsOrderErrorResponse) -> Self {
        Self {
            code: Some(resp.err.code.to_string()),
            message: resp.err.msg,
            request_id: Some(resp.rid),
        }
    }
}

impl From<AxWsErrorResponse> for AxWsError {
    fn from(resp: AxWsErrorResponse) -> Self {
        Self {
            code: resp.code,
            message: resp.message.unwrap_or_else(|| "Unknown error".to_string()),
            request_id: resp.rid,
        }
    }
}

/// Metadata for pending order operations.
///
/// Used to correlate order responses with the original request.
#[derive(Debug, Clone)]
pub struct OrderMetadata {
    /// Trader ID for event generation.
    pub trader_id: TraderId,
    /// Strategy ID for event generation.
    pub strategy_id: StrategyId,
    /// Instrument ID for event generation.
    pub instrument_id: InstrumentId,
    /// Client order ID for correlation.
    pub client_order_id: ClientOrderId,
    /// Venue order ID (populated after acknowledgment).
    pub venue_order_id: Option<VenueOrderId>,
    /// Original order timestamp.
    pub ts_init: UnixNanos,
    /// Instrument size precision for quantity conversion.
    pub size_precision: u8,
    /// Instrument price precision for price conversion.
    pub price_precision: u8,
    /// Quote currency for the instrument.
    pub quote_currency: Currency,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_md_subscribe_serialization() {
        let msg = AxMdSubscribe {
            request_id: 2,
            msg_type: "subscribe".to_string(),
            symbol: "BTCUSD-PERP".to_string(),
            level: AxMarketDataLevel::Level2,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["request_id"], 2);
        assert_eq!(parsed["type"], "subscribe");
        assert_eq!(parsed["symbol"], "BTCUSD-PERP");
        assert_eq!(parsed["level"], "LEVEL_2");
    }

    #[rstest]
    fn test_md_unsubscribe_serialization() {
        let msg = AxMdUnsubscribe {
            request_id: 3,
            msg_type: "unsubscribe".to_string(),
            symbol: "BTCUSD-PERP".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["request_id"], 3);
        assert_eq!(parsed["type"], "unsubscribe");
        assert_eq!(parsed["symbol"], "BTCUSD-PERP");
    }

    #[rstest]
    fn test_md_subscribe_candles_serialization() {
        let msg = AxMdSubscribeCandles {
            request_id: 4,
            msg_type: "subscribe_candles".to_string(),
            symbol: "BTCUSD-PERP".to_string(),
            width: AxCandleWidth::Minutes1,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["request_id"], 4);
        assert_eq!(parsed["type"], "subscribe_candles");
        assert_eq!(parsed["symbol"], "BTCUSD-PERP");
        assert_eq!(parsed["width"], "1m");
    }

    #[rstest]
    fn test_md_unsubscribe_candles_serialization() {
        let msg = AxMdUnsubscribeCandles {
            request_id: 5,
            msg_type: "unsubscribe_candles".to_string(),
            symbol: "BTCUSD-PERP".to_string(),
            width: AxCandleWidth::Minutes1,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["request_id"], 5);
        assert_eq!(parsed["type"], "unsubscribe_candles");
        assert_eq!(parsed["symbol"], "BTCUSD-PERP");
        assert_eq!(parsed["width"], "1m");
    }

    #[rstest]
    fn test_ws_place_order_serialization() {
        let msg = AxWsPlaceOrder {
            rid: 1,
            t: "p".to_string(),
            s: "BTCUSD-PERP".to_string(),
            d: AxOrderSide::Buy,
            q: 100,
            p: dec!(50000.50),
            tif: AxTimeInForce::Gtc,
            po: false,
            tag: Some("trade001".to_string()),
            order_type: None,
            trigger_price: None,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["rid"], 1);
        assert_eq!(parsed["t"], "p");
        assert_eq!(parsed["s"], "BTCUSD-PERP");
        assert_eq!(parsed["d"], "B");
        assert_eq!(parsed["q"], 100);
        assert_eq!(parsed["p"], "50000.50");
        assert_eq!(parsed["tif"], "GTC");
        assert_eq!(parsed["po"], false);
        assert_eq!(parsed["tag"], "trade001");
        assert!(parsed.get("order_type").is_none());
        assert!(parsed.get("trigger_price").is_none());
    }

    #[rstest]
    fn test_ws_place_stop_loss_order_serialization() {
        let msg = AxWsPlaceOrder {
            rid: 2,
            t: "p".to_string(),
            s: "BTCUSD-PERP".to_string(),
            d: AxOrderSide::Sell,
            q: 50,
            p: dec!(48000.00),
            tif: AxTimeInForce::Gtc,
            po: false,
            tag: None,
            order_type: Some(AxOrderType::StopLossLimit),
            trigger_price: Some(dec!(49000.00)),
        };

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["rid"], 2);
        assert_eq!(parsed["order_type"], "STOP_LOSS_LIMIT");
        assert_eq!(parsed["trigger_price"], "49000.00");
    }

    #[rstest]
    fn test_ws_cancel_order_serialization() {
        let msg = AxWsCancelOrder {
            rid: 2,
            t: "x".to_string(),
            oid: "O-01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["rid"], 2);
        assert_eq!(parsed["t"], "x");
        assert_eq!(parsed["oid"], "O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
    }

    #[rstest]
    fn test_ws_get_open_orders_serialization() {
        let msg = AxWsGetOpenOrders {
            rid: 3,
            t: "o".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["rid"], 3);
        assert_eq!(parsed["t"], "o");
    }

    #[rstest]
    fn test_load_md_heartbeat_from_file() {
        let json = include_str!("../../test_data/ws_md_heartbeat.json");
        let msg: AxMdHeartbeat = serde_json::from_str(json).unwrap();
        assert_eq!(msg.t, "h");
    }

    #[rstest]
    fn test_load_md_ticker_from_file() {
        let json = include_str!("../../test_data/ws_md_ticker.json");
        let msg: AxMdTicker = serde_json::from_str(json).unwrap();
        assert_eq!(msg.s.as_str(), "BTCUSD-PERP");
    }

    #[rstest]
    fn test_load_md_trade_from_file() {
        let json = include_str!("../../test_data/ws_md_trade.json");
        let msg: AxMdTrade = serde_json::from_str(json).unwrap();
        assert_eq!(msg.d, Some(AxOrderSide::Buy));
    }

    #[rstest]
    fn test_load_md_candle_from_file() {
        let json = include_str!("../../test_data/ws_md_candle.json");
        let msg: AxMdCandle = serde_json::from_str(json).unwrap();
        assert_eq!(msg.width, AxCandleWidth::Minutes1);
    }

    #[rstest]
    fn test_load_md_book_l1_from_file() {
        let json = include_str!("../../test_data/ws_md_book_l1.json");
        let msg: AxMdBookL1 = serde_json::from_str(json).unwrap();
        assert_eq!(msg.b.len(), 1);
        assert_eq!(msg.a.len(), 1);
    }

    #[rstest]
    fn test_load_md_book_l2_from_file() {
        let json = include_str!("../../test_data/ws_md_book_l2.json");
        let msg: AxMdBookL2 = serde_json::from_str(json).unwrap();
        assert_eq!(msg.b.len(), 3);
        assert_eq!(msg.a.len(), 3);
    }

    #[rstest]
    fn test_load_md_book_l3_from_file() {
        let json = include_str!("../../test_data/ws_md_book_l3.json");
        let msg: AxMdBookL3 = serde_json::from_str(json).unwrap();
        assert_eq!(msg.b.len(), 2);
        assert!(!msg.b[0].o.is_empty());
    }

    #[rstest]
    fn test_load_order_place_response_from_file() {
        let json = include_str!("../../test_data/ws_order_place_response.json");
        let msg: AxWsPlaceOrderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(msg.res.oid, "O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
    }

    #[rstest]
    fn test_load_order_cancel_response_from_file() {
        let json = include_str!("../../test_data/ws_order_cancel_response.json");
        let msg: AxWsCancelOrderResponse = serde_json::from_str(json).unwrap();
        assert!(msg.res.cxl_rx);
    }

    #[rstest]
    fn test_load_order_open_orders_response_from_file() {
        let json = include_str!("../../test_data/ws_order_open_orders_response.json");
        let msg: AxWsOpenOrdersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(msg.res.len(), 1);
    }

    #[rstest]
    fn test_load_order_heartbeat_from_file() {
        let json = include_str!("../../test_data/ws_order_heartbeat.json");
        let msg: AxWsHeartbeat = serde_json::from_str(json).unwrap();
        assert_eq!(msg.ts, 1609459200);
    }

    #[rstest]
    fn test_load_order_acknowledged_from_file() {
        let json = include_str!("../../test_data/ws_order_acknowledged.json");
        let msg: AxWsOrderAcknowledged = serde_json::from_str(json).unwrap();
        assert_eq!(msg.o.oid, "O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
    }

    #[rstest]
    fn test_load_order_filled_from_file() {
        let json = include_str!("../../test_data/ws_order_filled.json");
        let msg: AxWsOrderFilled = serde_json::from_str(json).unwrap();
        assert_eq!(msg.o.o, AxOrderStatus::Filled);
    }

    #[rstest]
    fn test_load_order_partially_filled_from_file() {
        let json = include_str!("../../test_data/ws_order_partially_filled.json");
        let msg: AxWsOrderPartiallyFilled = serde_json::from_str(json).unwrap();
        assert_eq!(msg.xs.q, 50);
    }

    #[rstest]
    fn test_load_order_canceled_from_file() {
        let json = include_str!("../../test_data/ws_order_canceled.json");
        let msg: AxWsOrderCanceled = serde_json::from_str(json).unwrap();
        assert_eq!(msg.xr, "USER_REQUESTED");
    }

    #[rstest]
    fn test_load_order_rejected_from_file() {
        let json = include_str!("../../test_data/ws_order_rejected.json");
        let msg: AxWsOrderRejected = serde_json::from_str(json).unwrap();
        assert_eq!(msg.r, "INSUFFICIENT_MARGIN");
    }

    #[rstest]
    fn test_load_order_expired_from_file() {
        let json = include_str!("../../test_data/ws_order_expired.json");
        let msg: AxWsOrderExpired = serde_json::from_str(json).unwrap();
        assert_eq!(msg.o.tif, AxTimeInForce::Ioc);
    }

    #[rstest]
    fn test_load_order_replaced_from_file() {
        let json = include_str!("../../test_data/ws_order_replaced.json");
        let msg: AxWsOrderReplaced = serde_json::from_str(json).unwrap();
        assert_eq!(msg.o.p, dec!(50500.00));
    }

    #[rstest]
    fn test_load_order_done_for_day_from_file() {
        let json = include_str!("../../test_data/ws_order_done_for_day.json");
        let msg: AxWsOrderDoneForDay = serde_json::from_str(json).unwrap();
        assert_eq!(msg.o.xq, 50);
    }

    #[rstest]
    fn test_load_cancel_rejected_from_file() {
        let json = include_str!("../../test_data/ws_cancel_rejected.json");
        let msg: AxWsCancelRejected = serde_json::from_str(json).unwrap();
        assert_eq!(msg.r, "ORDER_NOT_FOUND");
    }

    #[rstest]
    fn test_load_order_error_response_from_file() {
        let json = include_str!("../../test_data/ws_order_error_response.json");
        let msg: AxWsOrderErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(msg.rid, 1);
        assert_eq!(msg.err.code, 400);
        assert!(msg.err.msg.contains("initial margin"));
    }

    #[rstest]
    fn test_load_order_list_response_from_file() {
        let json = include_str!("../../test_data/ws_order_list_response.json");
        let msg: AxWsListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(msg.rid, 0);
        assert_eq!(msg.res.li, "01KCQM-4WP1-0000");
        assert!(msg.res.o.is_none());
    }

    #[rstest]
    fn test_load_order_list_response_with_orders_from_file() {
        let json = include_str!("../../test_data/ws_order_list_response_with_orders.json");
        let msg: AxWsListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(msg.rid, 0);
        assert_eq!(msg.res.li, "01KCQM-4WP1-0000");
        let orders = msg.res.o.unwrap();
        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].oid, "O-01KF4QM3VVJEDH98ZVNS1PCSBB");
        assert_eq!(orders[1].oid, "O-01KF4QM3K9FJZWYA02JF9Y1FJA");
    }

    #[rstest]
    fn test_raw_message_error_variant() {
        let json = include_str!("../../test_data/ws_order_error_response.json");
        let msg: AxWsRawMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, AxWsRawMessage::Error(_)));
    }

    #[rstest]
    fn test_raw_message_list_response_variant() {
        let json = include_str!("../../test_data/ws_order_list_response.json");
        let msg: AxWsRawMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(
            msg,
            AxWsRawMessage::Response(AxWsOrderResponse::List(_))
        ));
    }
}
