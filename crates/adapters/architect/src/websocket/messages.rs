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

//! WebSocket message types for the Architect API.
//!
//! This module contains request and response message structures for both
//! market data and order management WebSocket streams.

use nautilus_core::serialization::serialize_decimal_as_str;
use nautilus_model::{
    data::{Bar, Data, OrderBookDeltas},
    events::{OrderCancelRejected, OrderRejected},
    identifiers::ClientOrderId,
    reports::{FillReport, OrderStatusReport},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::error::ArchitectWsErrorResponse;
use crate::common::{
    enums::{
        ArchitectCandleWidth, ArchitectMarketDataLevel, ArchitectOrderSide, ArchitectOrderStatus,
        ArchitectTimeInForce,
    },
    parse::deserialize_decimal_or_zero,
};

/// Subscribe request for market data.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectMdSubscribe {
    /// Client request ID for correlation.
    pub request_id: i64,
    /// Request type (always "subscribe").
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Instrument symbol.
    pub symbol: String,
    /// Market data level (LEVEL_1, LEVEL_2, LEVEL_3).
    pub level: ArchitectMarketDataLevel,
}

/// Unsubscribe request for market data.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectMdUnsubscribe {
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
pub struct ArchitectMdSubscribeCandles {
    /// Client request ID for correlation.
    pub request_id: i64,
    /// Request type (always "subscribe_candles").
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Instrument symbol.
    pub symbol: String,
    /// Candle width/interval.
    pub width: ArchitectCandleWidth,
}

/// Unsubscribe request for candle data.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectMdUnsubscribeCandles {
    /// Client request ID for correlation.
    pub request_id: i64,
    /// Request type (always "unsubscribe_candles").
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Instrument symbol.
    pub symbol: String,
    /// Candle width/interval.
    pub width: ArchitectCandleWidth,
}

/// Heartbeat message from market data WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectMdHeartbeat {
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
pub struct ArchitectMdTicker {
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
pub struct ArchitectMdTrade {
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
    /// Trade direction: "B" (buy) or "S" (sell).
    pub d: ArchitectOrderSide,
}

/// Candle/OHLCV message from market data WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectMdCandle {
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
    pub width: ArchitectCandleWidth,
}

/// Price level entry in order book.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectBookLevel {
    /// Price at this level.
    #[serde(deserialize_with = "deserialize_decimal_or_zero")]
    pub p: Decimal,
    /// Quantity at this level.
    pub q: i64,
}

/// Price level entry with individual order breakdown (L3).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectBookLevelL3 {
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
pub struct ArchitectMdBookL1 {
    /// Message type (always "1").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Instrument symbol.
    pub s: Ustr,
    /// Bid levels (typically just best bid).
    pub b: Vec<ArchitectBookLevel>,
    /// Ask levels (typically just best ask).
    pub a: Vec<ArchitectBookLevel>,
}

/// Level 2 order book update (aggregated price levels).
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectMdBookL2 {
    /// Message type (always "2").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Instrument symbol.
    pub s: Ustr,
    /// Bid levels.
    pub b: Vec<ArchitectBookLevel>,
    /// Ask levels.
    pub a: Vec<ArchitectBookLevel>,
}

/// Level 3 order book update (individual order quantities).
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectMdBookL3 {
    /// Message type (always "3").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Instrument symbol.
    pub s: Ustr,
    /// Bid levels with order breakdown.
    pub b: Vec<ArchitectBookLevelL3>,
    /// Ask levels with order breakdown.
    pub a: Vec<ArchitectBookLevelL3>,
}

/// Place order request via WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsPlaceOrder {
    /// Request ID for correlation.
    pub rid: i64,
    /// Message type (always "p").
    pub t: String,
    /// Instrument symbol.
    pub s: String,
    /// Order side: "B" (buy) or "S" (sell).
    pub d: ArchitectOrderSide,
    /// Order quantity.
    pub q: i64,
    /// Order price.
    #[serde(
        serialize_with = "serialize_decimal_as_str",
        deserialize_with = "deserialize_decimal_or_zero"
    )]
    pub p: Decimal,
    /// Time in force.
    pub tif: ArchitectTimeInForce,
    /// Post-only flag (maker-or-cancel).
    pub po: bool,
    /// Optional order tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// Cancel order request via WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsCancelOrder {
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
pub struct ArchitectWsGetOpenOrders {
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
pub struct ArchitectWsPlaceOrderResponse {
    /// Request ID matching the original request.
    pub rid: i64,
    /// Response result.
    pub res: ArchitectWsPlaceOrderResult,
}

/// Result payload for place order response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsPlaceOrderResult {
    /// Order ID of the placed order.
    pub oid: String,
}

/// Cancel order response from WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsCancelOrderResponse {
    /// Request ID matching the original request.
    pub rid: i64,
    /// Response result.
    pub res: ArchitectWsCancelOrderResult,
}

/// Result payload for cancel order response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsCancelOrderResult {
    /// Whether the cancel request was received.
    pub cxl_rx: bool,
}

/// Open orders response from WebSocket.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsOpenOrdersResponse {
    /// Request ID matching the original request.
    pub rid: i64,
    /// List of open orders.
    pub res: Vec<ArchitectWsOrder>,
}

/// Order details in WebSocket messages.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsOrder {
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
    pub o: ArchitectOrderStatus,
    /// Order side.
    pub d: ArchitectOrderSide,
    /// Time in force.
    pub tif: ArchitectTimeInForce,
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
pub struct ArchitectWsHeartbeat {
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
pub struct ArchitectWsOrderAcknowledged {
    /// Message type (always "n").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: ArchitectWsOrder,
}

/// Trade execution details for fill events.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsTradeExecution {
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
    pub d: ArchitectOrderSide,
    /// Whether this was an aggressor (taker) order.
    pub agg: bool,
}

/// Order partially filled event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsOrderPartiallyFilled {
    /// Message type (always "p").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: ArchitectWsOrder,
    /// Trade execution details.
    pub xs: ArchitectWsTradeExecution,
}

/// Order filled event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsOrderFilled {
    /// Message type (always "f").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: ArchitectWsOrder,
    /// Trade execution details.
    pub xs: ArchitectWsTradeExecution,
}

/// Order canceled event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsOrderCanceled {
    /// Message type (always "c").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: ArchitectWsOrder,
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
pub struct ArchitectWsOrderRejected {
    /// Message type (always "j").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: ArchitectWsOrder,
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
pub struct ArchitectWsOrderExpired {
    /// Message type (always "x").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: ArchitectWsOrder,
}

/// Order replaced/amended event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsOrderReplaced {
    /// Message type (always "r").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: ArchitectWsOrder,
}

/// Order done for day event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsOrderDoneForDay {
    /// Message type (always "d").
    pub t: String,
    /// Timestamp (Unix epoch seconds).
    pub ts: i64,
    /// Transaction number.
    pub tn: i64,
    /// Event ID.
    pub eid: String,
    /// Order details.
    pub o: ArchitectWsOrder,
}

/// Cancel rejected event.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectWsCancelRejected {
    /// Message type (always "e").
    pub t: String,
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

/// Nautilus domain message emitted after parsing Architect WebSocket events.
///
/// This enum contains fully-parsed Nautilus domain objects ready for consumption
/// by the DataClient without additional processing.
#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    /// Market data (trades, quotes).
    Data(Vec<Data>),
    /// Order book deltas.
    Deltas(OrderBookDeltas),
    /// Bar/candle data.
    Bar(Bar),
    /// Heartbeat message.
    Heartbeat,
    /// Error from venue or client.
    Error(ArchitectWsError),
    /// WebSocket reconnected notification.
    Reconnected,
}

/// Nautilus domain message for Architect orders WebSocket.
///
/// This enum contains parsed messages from the WebSocket stream.
/// Raw variants contain Architect-specific types for further processing.
/// Nautilus variants contain fully-parsed domain objects.
#[derive(Debug, Clone)]
pub enum ArchitectOrdersWsMessage {
    /// Order status reports from order updates.
    OrderStatusReports(Vec<OrderStatusReport>),
    /// Fill reports from executions.
    FillReports(Vec<FillReport>),
    /// Order rejected event (from failed order submission).
    OrderRejected(OrderRejected),
    /// Order cancel rejected event (from failed cancel operation).
    OrderCancelRejected(OrderCancelRejected),
    /// Order acknowledged by exchange.
    OrderAcknowledged(ArchitectWsOrderAcknowledged),
    /// Order partially filled.
    OrderPartiallyFilled(ArchitectWsOrderPartiallyFilled),
    /// Order fully filled.
    OrderFilled(ArchitectWsOrderFilled),
    /// Order canceled.
    OrderCanceled(ArchitectWsOrderCanceled),
    /// Order rejected by exchange.
    OrderRejectedRaw(ArchitectWsOrderRejected),
    /// Order expired.
    OrderExpired(ArchitectWsOrderExpired),
    /// Order replaced/amended.
    OrderReplaced(ArchitectWsOrderReplaced),
    /// Order done for day.
    OrderDoneForDay(ArchitectWsOrderDoneForDay),
    /// Cancel request rejected.
    CancelRejected(ArchitectWsCancelRejected),
    /// Place order response.
    PlaceOrderResponse(ArchitectWsPlaceOrderResponse),
    /// Cancel order response.
    CancelOrderResponse(ArchitectWsCancelOrderResponse),
    /// Open orders response.
    OpenOrdersResponse(ArchitectWsOpenOrdersResponse),
    /// Error from venue or client.
    Error(ArchitectWsError),
    /// WebSocket reconnected notification.
    Reconnected,
    /// Authentication successful notification.
    Authenticated,
}

/// Represents an error event surfaced by the WebSocket client.
#[derive(Debug, Clone)]
pub struct ArchitectWsError {
    /// Error code from Architect.
    pub code: Option<String>,
    /// Human readable message.
    pub message: String,
    /// Optional request ID related to the failure.
    pub request_id: Option<i64>,
}

impl ArchitectWsError {
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

impl From<ArchitectWsErrorResponse> for ArchitectWsError {
    fn from(resp: ArchitectWsErrorResponse) -> Self {
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
    /// Client order ID for correlation.
    pub client_order_id: ClientOrderId,
    /// Instrument symbol.
    pub symbol: Ustr,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_md_subscribe_serialization() {
        let msg = ArchitectMdSubscribe {
            request_id: 2,
            msg_type: "subscribe".to_string(),
            symbol: "BTCUSD-PERP".to_string(),
            level: ArchitectMarketDataLevel::Level2,
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
        let msg = ArchitectMdUnsubscribe {
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
        let msg = ArchitectMdSubscribeCandles {
            request_id: 4,
            msg_type: "subscribe_candles".to_string(),
            symbol: "BTCUSD-PERP".to_string(),
            width: ArchitectCandleWidth::Minutes1,
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
        let msg = ArchitectMdUnsubscribeCandles {
            request_id: 5,
            msg_type: "unsubscribe_candles".to_string(),
            symbol: "BTCUSD-PERP".to_string(),
            width: ArchitectCandleWidth::Minutes1,
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
        let msg = ArchitectWsPlaceOrder {
            rid: 1,
            t: "p".to_string(),
            s: "BTCUSD-PERP".to_string(),
            d: ArchitectOrderSide::Buy,
            q: 100,
            p: dec!(50000.50),
            tif: ArchitectTimeInForce::Gtc,
            po: false,
            tag: Some("trade001".to_string()),
        };

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["rid"], 1);
        assert_eq!(parsed["t"], "p");
        assert_eq!(parsed["s"], "BTCUSD-PERP");
        assert_eq!(parsed["d"], "B");
        assert_eq!(parsed["q"], 100);
        assert_eq!(parsed["p"], "50000.5");
        assert_eq!(parsed["tif"], "GTC");
        assert_eq!(parsed["po"], false);
        assert_eq!(parsed["tag"], "trade001");
    }

    #[rstest]
    fn test_ws_cancel_order_serialization() {
        let msg = ArchitectWsCancelOrder {
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
        let msg = ArchitectWsGetOpenOrders {
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
        let msg: ArchitectMdHeartbeat = serde_json::from_str(json).unwrap();
        assert_eq!(msg.t, "h");
    }

    #[rstest]
    fn test_load_md_ticker_from_file() {
        let json = include_str!("../../test_data/ws_md_ticker.json");
        let msg: ArchitectMdTicker = serde_json::from_str(json).unwrap();
        assert_eq!(msg.s.as_str(), "BTCUSD-PERP");
    }

    #[rstest]
    fn test_load_md_trade_from_file() {
        let json = include_str!("../../test_data/ws_md_trade.json");
        let msg: ArchitectMdTrade = serde_json::from_str(json).unwrap();
        assert_eq!(msg.d, ArchitectOrderSide::Buy);
    }

    #[rstest]
    fn test_load_md_candle_from_file() {
        let json = include_str!("../../test_data/ws_md_candle.json");
        let msg: ArchitectMdCandle = serde_json::from_str(json).unwrap();
        assert_eq!(msg.width, ArchitectCandleWidth::Minutes1);
    }

    #[rstest]
    fn test_load_md_book_l1_from_file() {
        let json = include_str!("../../test_data/ws_md_book_l1.json");
        let msg: ArchitectMdBookL1 = serde_json::from_str(json).unwrap();
        assert_eq!(msg.b.len(), 1);
        assert_eq!(msg.a.len(), 1);
    }

    #[rstest]
    fn test_load_md_book_l2_from_file() {
        let json = include_str!("../../test_data/ws_md_book_l2.json");
        let msg: ArchitectMdBookL2 = serde_json::from_str(json).unwrap();
        assert_eq!(msg.b.len(), 3);
        assert_eq!(msg.a.len(), 3);
    }

    #[rstest]
    fn test_load_md_book_l3_from_file() {
        let json = include_str!("../../test_data/ws_md_book_l3.json");
        let msg: ArchitectMdBookL3 = serde_json::from_str(json).unwrap();
        assert_eq!(msg.b.len(), 2);
        assert!(!msg.b[0].o.is_empty());
    }

    #[rstest]
    fn test_load_order_place_response_from_file() {
        let json = include_str!("../../test_data/ws_order_place_response.json");
        let msg: ArchitectWsPlaceOrderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(msg.res.oid, "O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
    }

    #[rstest]
    fn test_load_order_cancel_response_from_file() {
        let json = include_str!("../../test_data/ws_order_cancel_response.json");
        let msg: ArchitectWsCancelOrderResponse = serde_json::from_str(json).unwrap();
        assert!(msg.res.cxl_rx);
    }

    #[rstest]
    fn test_load_order_open_orders_response_from_file() {
        let json = include_str!("../../test_data/ws_order_open_orders_response.json");
        let msg: ArchitectWsOpenOrdersResponse = serde_json::from_str(json).unwrap();
        assert_eq!(msg.res.len(), 1);
    }

    #[rstest]
    fn test_load_order_heartbeat_from_file() {
        let json = include_str!("../../test_data/ws_order_heartbeat.json");
        let msg: ArchitectWsHeartbeat = serde_json::from_str(json).unwrap();
        assert_eq!(msg.t, "h");
    }

    #[rstest]
    fn test_load_order_acknowledged_from_file() {
        let json = include_str!("../../test_data/ws_order_acknowledged.json");
        let msg: ArchitectWsOrderAcknowledged = serde_json::from_str(json).unwrap();
        assert_eq!(msg.t, "n");
    }

    #[rstest]
    fn test_load_order_filled_from_file() {
        let json = include_str!("../../test_data/ws_order_filled.json");
        let msg: ArchitectWsOrderFilled = serde_json::from_str(json).unwrap();
        assert_eq!(msg.o.o, ArchitectOrderStatus::Filled);
    }

    #[rstest]
    fn test_load_order_partially_filled_from_file() {
        let json = include_str!("../../test_data/ws_order_partially_filled.json");
        let msg: ArchitectWsOrderPartiallyFilled = serde_json::from_str(json).unwrap();
        assert_eq!(msg.xs.q, 50);
    }

    #[rstest]
    fn test_load_order_canceled_from_file() {
        let json = include_str!("../../test_data/ws_order_canceled.json");
        let msg: ArchitectWsOrderCanceled = serde_json::from_str(json).unwrap();
        assert_eq!(msg.xr, "USER_REQUESTED");
    }

    #[rstest]
    fn test_load_order_rejected_from_file() {
        let json = include_str!("../../test_data/ws_order_rejected.json");
        let msg: ArchitectWsOrderRejected = serde_json::from_str(json).unwrap();
        assert_eq!(msg.r, "INSUFFICIENT_MARGIN");
    }

    #[rstest]
    fn test_load_order_expired_from_file() {
        let json = include_str!("../../test_data/ws_order_expired.json");
        let msg: ArchitectWsOrderExpired = serde_json::from_str(json).unwrap();
        assert_eq!(msg.o.tif, ArchitectTimeInForce::Ioc);
    }

    #[rstest]
    fn test_load_order_replaced_from_file() {
        let json = include_str!("../../test_data/ws_order_replaced.json");
        let msg: ArchitectWsOrderReplaced = serde_json::from_str(json).unwrap();
        assert_eq!(msg.t, "r");
        assert_eq!(msg.o.p, dec!(50500.00));
    }

    #[rstest]
    fn test_load_order_done_for_day_from_file() {
        let json = include_str!("../../test_data/ws_order_done_for_day.json");
        let msg: ArchitectWsOrderDoneForDay = serde_json::from_str(json).unwrap();
        assert_eq!(msg.t, "d");
        assert_eq!(msg.o.xq, 50);
    }

    #[rstest]
    fn test_load_cancel_rejected_from_file() {
        let json = include_str!("../../test_data/ws_cancel_rejected.json");
        let msg: ArchitectWsCancelRejected = serde_json::from_str(json).unwrap();
        assert_eq!(msg.r, "ORDER_NOT_FOUND");
    }
}
