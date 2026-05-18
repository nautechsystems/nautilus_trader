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

//! Data models for Kraken WebSocket v2 API messages.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ustr::Ustr;

use super::enums::{
    KrakenExecType, KrakenLiquidityInd, KrakenWsChannel, KrakenWsMessageType, KrakenWsMethod,
    KrakenWsOrderStatus,
};
use crate::{
    common::enums::{KrakenOrderSide, KrakenOrderType, KrakenSpotTrigger, KrakenTimeInForce},
    websocket::spot_v2::level_3::messages::{KrakenL3Snapshot, KrakenL3UpdateData},
};

/// Output message types from the Kraken Spot v2 WebSocket handler.
#[derive(Clone, Debug)]
pub enum KrakenSpotWsMessage {
    Ticker(Vec<KrakenWsTickerData>),
    Trade(Vec<KrakenWsTradeData>),
    Book {
        data: Vec<KrakenWsBookData>,
        is_snapshot: bool,
    },
    Ohlc(Vec<KrakenWsOhlcData>),
    Execution(Vec<KrakenWsExecutionData>),
    OrderResponse(KrakenWsOrderResponse),
    L3Snapshot(KrakenL3Snapshot),
    L3Update(KrakenL3UpdateData),
    Reconnected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsRequest {
    pub method: KrakenWsMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<KrakenWsParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub req_id: Option<u64>,
}

/// Parameters for a Kraken WebSocket request, covering both channel subscriptions and order methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KrakenWsParams {
    /// Parameters for subscribe/unsubscribe channel requests.
    Channel(KrakenWsChannelParams),
    /// Parameters for the `add_order` method.
    AddOrder(KrakenWsAddOrderParams),
    /// Parameters for the `amend_order` method.
    AmendOrder(KrakenWsAmendOrderParams),
    /// Parameters for the `cancel_order` method.
    CancelOrder(KrakenWsCancelOrderParams),
    /// Parameters for the `batch_add` method.
    BatchAdd(KrakenWsBatchAddParams),
}

/// Parameters for channel subscribe/unsubscribe requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsChannelParams {
    /// Channel to subscribe or unsubscribe.
    pub channel: KrakenWsChannel,
    /// Symbols to subscribe for (market data channels).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Vec<Ustr>>,
    /// Whether to receive a snapshot on subscribe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<bool>,
    /// Order book depth (book channel only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    /// OHLC interval in minutes (ohlc channel only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<u32>,
    /// Event trigger filter (ticker channel, e.g. `"bbo"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_trigger: Option<String>,
    /// Authentication token (private channels).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Whether to include a snapshot of open orders (executions channel).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_orders: Option<bool>,
    /// Whether to include a snapshot of recent trades (executions channel).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_trades: Option<bool>,
}

/// Parameters for the `add_order` WebSocket method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsAddOrderParams {
    /// Order type (limit, market, etc.).
    pub order_type: KrakenOrderType,
    /// Order side (buy or sell).
    pub side: KrakenOrderSide,
    /// Order quantity in base currency.
    pub order_qty: f64,
    /// Trading pair symbol (e.g. `"BTC/USD"`).
    pub symbol: String,
    /// Authentication token.
    pub token: String,
    /// Limit price (required for limit orders).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<f64>,
    /// Time in force policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<KrakenTimeInForce>,
    /// Expiration timestamp for `GoodTilDate` orders. Required by Kraken whenever
    /// `time_in_force = GTD`. Accepts an RFC3339 timestamp (`"2026-12-31T23:59:59Z"`)
    /// or a relative duration (`"+30s"`, `"+1h"`, `"+2D"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<String>,
    /// Client-assigned order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// Whether the order must be a passive post-only order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_only: Option<bool>,
    /// Whether the order may only reduce an existing position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    /// Trigger parameters for stop/take-profit orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<KrakenWsTriggerParams>,
    /// Leverage multiplier for margin orders; omit for non-margin (cash) orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leverage: Option<u16>,
    /// Conditional close order attached to the parent order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditional: Option<KrakenWsConditionalParams>,
}

/// Parameters for the `amend_order` WebSocket method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsAmendOrderParams {
    /// Authentication token.
    pub token: String,
    /// Kraken order ID to amend (preferred over `cl_ord_id`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    /// Client-assigned order ID to amend (used when `order_id` is unavailable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// New order quantity (replaces the existing quantity).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_qty: Option<f64>,
    /// New limit price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<f64>,
    /// New trigger price (for conditional orders).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_price: Option<f64>,
}

/// Parameters for the `cancel_order` WebSocket method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsCancelOrderParams {
    /// Authentication token.
    pub token: String,
    /// One or more Kraken order IDs to cancel (preferred over `cl_ord_id`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<Vec<String>>,
    /// One or more client-assigned order IDs to cancel (used when `order_id` is unavailable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<Vec<String>>,
}

/// Parameters for the `batch_add` WebSocket method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsBatchAddParams {
    /// Trading pair symbol shared by all orders in the batch.
    pub symbol: String,
    /// List of orders to submit.
    pub orders: Vec<KrakenWsBatchAddOrder>,
    /// Authentication token.
    pub token: String,
}

/// A single order entry within a `batch_add` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsBatchAddOrder {
    /// Order type.
    pub order_type: KrakenOrderType,
    /// Order side.
    pub side: KrakenOrderSide,
    /// Order quantity.
    pub order_qty: f64,
    /// Limit price (required for limit orders).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<f64>,
    /// Client-assigned order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// Time in force policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<KrakenTimeInForce>,
    /// Expiration timestamp for `GoodTilDate` legs. Required by Kraken whenever
    /// `time_in_force = GTD`. RFC3339 timestamp or relative duration string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<String>,
    /// Whether the order must be a passive post-only order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_only: Option<bool>,
    /// Whether the order may only reduce an existing position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    /// Leverage multiplier for margin orders; omit for non-margin (cash) orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leverage: Option<u16>,
    /// Trigger parameters for stop-loss and take-profit order types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<KrakenWsTriggerParams>,
}

/// Trigger parameters for stop/take-profit order types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsTriggerParams {
    /// Reference price for the trigger.
    pub reference: KrakenSpotTrigger,
    /// Trigger price level.
    pub price: f64,
    /// Price direction for the trigger (above or below).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_type: Option<String>,
}

/// Conditional close order attached to a parent order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsConditionalParams {
    /// Order type for the conditional leg.
    pub order_type: KrakenOrderType,
    /// Limit price for the conditional leg.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<f64>,
    /// Stop price for the conditional leg.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_price: Option<f64>,
}

/// Response envelope for order-method WebSocket responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsOrderResponse {
    /// The method that triggered this response.
    pub method: KrakenWsMethod,
    /// Echo of the request ID (only present when the client sent one).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub req_id: Option<u64>,
    /// Whether the request succeeded.
    pub success: bool,
    /// ISO 8601 timestamp when the request was received.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in: Option<String>,
    /// ISO 8601 timestamp when the response was sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_out: Option<String>,
    /// Error message when `success` is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Result payload when `success` is `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<KrakenWsOrderResult>,
}

/// Result payload for single-order responses (`add_order`, `amend_order`, `cancel_order`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsOrderResult {
    /// Kraken-assigned order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    /// Client-assigned order ID echoed back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// Integer user reference echoed back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_userref: Option<i64>,
    /// Non-fatal warnings associated with the order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<Vec<String>>,
    /// Per-order results for `batch_add` responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orders: Option<Vec<KrakenWsBatchOrderResult>>,
}

/// Per-order outcome within a `batch_add` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsBatchOrderResult {
    /// Whether this individual order succeeded.
    pub success: bool,
    /// Kraken-assigned order ID (present when `success` is `true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    /// Client-assigned order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// Error message (present when `success` is `false`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum KrakenWsResponse {
    #[serde(rename = "pong")]
    Pong(KrakenWsPong),
    #[serde(rename = "subscribe")]
    Subscribe(KrakenWsSubscribeResponse),
    #[serde(rename = "unsubscribe")]
    Unsubscribe(KrakenWsUnsubscribeResponse),
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsPong {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub req_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsSubscribeResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub req_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<KrakenWsSubscriptionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsUnsubscribeResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub req_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsSubscriptionResult {
    pub channel: KrakenWsChannel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsMessage {
    pub channel: KrakenWsChannel,
    #[serde(rename = "type")]
    pub event_type: KrakenWsMessageType,
    pub data: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Ustr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsTickerData {
    pub symbol: Ustr,
    pub bid: f64,
    pub bid_qty: f64,
    pub ask: f64,
    pub ask_qty: f64,
    pub last: f64,
    pub volume: f64,
    pub vwap: f64,
    pub low: f64,
    pub high: f64,
    pub change: f64,
    pub change_pct: f64,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsTradeData {
    pub symbol: Ustr,
    pub side: KrakenOrderSide,
    pub price: f64,
    pub qty: f64,
    pub ord_type: KrakenOrderType,
    pub trade_id: i64,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsBookData {
    pub symbol: Ustr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bids: Option<Vec<KrakenWsBookLevel>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asks: Option<Vec<KrakenWsBookLevel>>,
    pub checksum: Option<u32>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsBookLevel {
    pub price: f64,
    pub qty: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsOhlcData {
    pub symbol: Ustr,
    pub interval: u32,
    pub interval_begin: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub vwap: f64,
    pub trades: i64,
}

/// Execution message from the Kraken executions channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsExecutionData {
    /// Execution type.
    pub exec_type: KrakenExecType,
    /// Kraken order ID.
    pub order_id: String,
    /// Client order ID (if provided when order was submitted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// Trading pair symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Order side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<KrakenOrderSide>,
    /// Order type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type: Option<KrakenOrderType>,
    /// Order quantity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_qty: Option<f64>,
    /// Limit price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<f64>,
    /// Order status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_status: Option<KrakenWsOrderStatus>,
    /// Cumulative filled quantity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cum_qty: Option<f64>,
    /// Cumulative cost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cum_cost: Option<f64>,
    /// Average fill price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_price: Option<f64>,
    /// Time in force.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<KrakenTimeInForce>,
    /// Post only flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_only: Option<bool>,
    /// Reduce only flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    /// Event timestamp.
    pub timestamp: DateTime<Utc>,
    /// Execution/trade ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec_id: Option<String>,
    /// Last fill quantity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_qty: Option<f64>,
    /// Last fill price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_price: Option<f64>,
    /// Trade cost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    /// Liquidity indicator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidity_ind: Option<KrakenLiquidityInd>,
    /// Fees array.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fees: Option<Vec<KrakenWsFee>>,
    /// Fee in USD equivalent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_usd_equiv: Option<f64>,
    /// Cancel reason (when exec_type is Canceled/Expired).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Fee information from execution messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsFee {
    /// Fee asset.
    pub asset: String,
    /// Fee quantity.
    pub qty: f64,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn load_test_data(filename: &str) -> String {
        let path = format!("test_data/{filename}");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to load test data from {path}: {e}"))
    }

    #[rstest]
    fn test_parse_subscribe_response() {
        let data = load_test_data("ws_subscribe_response.json");
        let response: KrakenWsResponse =
            serde_json::from_str(&data).expect("Failed to parse subscribe response");

        match response {
            KrakenWsResponse::Subscribe(sub) => {
                assert!(sub.success);
                assert_eq!(sub.req_id, Some(1));
                assert!(sub.result.is_some());
                let result = sub.result.unwrap();
                assert_eq!(result.channel, KrakenWsChannel::Ticker);
            }
            _ => panic!("Expected Subscribe response"),
        }
    }

    #[rstest]
    fn test_parse_pong() {
        let data = load_test_data("ws_pong.json");
        let response: KrakenWsResponse = serde_json::from_str(&data).expect("Failed to parse pong");

        match response {
            KrakenWsResponse::Pong(pong) => {
                assert_eq!(pong.req_id, Some(42));
            }
            _ => panic!("Expected Pong response"),
        }
    }

    #[rstest]
    fn test_parse_ticker_snapshot() {
        let data = load_test_data("ws_ticker_snapshot.json");
        let message: KrakenWsMessage =
            serde_json::from_str(&data).expect("Failed to parse ticker snapshot");

        assert_eq!(message.channel, KrakenWsChannel::Ticker);
        assert_eq!(message.event_type, KrakenWsMessageType::Snapshot);
        assert!(!message.data.is_empty());

        let ticker: KrakenWsTickerData =
            serde_json::from_value(message.data[0].clone()).expect("Failed to parse ticker data");
        assert_eq!(ticker.symbol.as_str(), "BTC/USD");
        assert!(ticker.bid.is_finite() && ticker.bid > 0.0);
        assert!(ticker.ask.is_finite() && ticker.ask > 0.0);
        assert!(ticker.last.is_finite() && ticker.last > 0.0);
        assert_eq!(
            ticker.timestamp.timestamp_nanos_opt().unwrap(),
            1_671_960_659_123_456_000
        );
    }

    #[rstest]
    fn test_parse_trade_update() {
        let data = load_test_data("ws_trade_update.json");
        let message: KrakenWsMessage =
            serde_json::from_str(&data).expect("Failed to parse trade update");

        assert_eq!(message.channel, KrakenWsChannel::Trade);
        assert_eq!(message.event_type, KrakenWsMessageType::Update);
        assert_eq!(message.data.len(), 2);

        let trade: KrakenWsTradeData =
            serde_json::from_value(message.data[0].clone()).expect("Failed to parse trade data");
        assert_eq!(trade.symbol.as_str(), "BTC/USD");
        assert!(trade.price.is_finite() && trade.price > 0.0);
        assert!(trade.qty.is_finite() && trade.qty > 0.0);
        assert!(trade.trade_id > 0);
    }

    #[rstest]
    fn test_parse_book_snapshot() {
        let data = load_test_data("ws_book_snapshot.json");
        let message: KrakenWsMessage =
            serde_json::from_str(&data).expect("Failed to parse book snapshot");

        assert_eq!(message.channel, KrakenWsChannel::Book);
        assert_eq!(message.event_type, KrakenWsMessageType::Snapshot);

        let book: KrakenWsBookData =
            serde_json::from_value(message.data[0].clone()).expect("Failed to parse book data");
        assert_eq!(book.symbol.as_str(), "BTC/USD");
        assert!(book.bids.is_some());
        assert!(book.asks.is_some());
        assert!(book.checksum.is_some());
        assert_eq!(
            book.timestamp.timestamp_nanos_opt().unwrap(),
            1_696_613_755_440_295_000
        );

        let bids = book.bids.unwrap();
        assert_eq!(bids.len(), 3);
        assert!(bids[0].price.is_finite() && bids[0].price > 0.0);
        assert!(bids[0].qty.is_finite() && bids[0].qty > 0.0);
    }

    #[rstest]
    fn test_parse_book_update() {
        let data = load_test_data("ws_book_update.json");
        let message: KrakenWsMessage =
            serde_json::from_str(&data).expect("Failed to parse book update");

        assert_eq!(message.channel, KrakenWsChannel::Book);
        assert_eq!(message.event_type, KrakenWsMessageType::Update);

        let book: KrakenWsBookData =
            serde_json::from_value(message.data[0].clone()).expect("Failed to parse book data");
        assert_eq!(
            book.timestamp.timestamp_nanos_opt().unwrap(),
            1_696_613_755_440_295_000
        );
        assert!(book.checksum.is_some());
    }

    #[rstest]
    fn test_parse_ohlc_update() {
        let data = load_test_data("ws_ohlc_update.json");
        let message: KrakenWsMessage =
            serde_json::from_str(&data).expect("Failed to parse OHLC update");

        assert_eq!(message.channel, KrakenWsChannel::Ohlc);
        assert_eq!(message.event_type, KrakenWsMessageType::Update);

        let ohlc: KrakenWsOhlcData =
            serde_json::from_value(message.data[0].clone()).expect("Failed to parse OHLC data");
        assert_eq!(ohlc.symbol.as_str(), "BTC/USD");
        assert!(ohlc.open.is_finite() && ohlc.open > 0.0);
        assert!(ohlc.high.is_finite() && ohlc.high > 0.0);
        assert!(ohlc.low.is_finite() && ohlc.low > 0.0);
        assert!(ohlc.close.is_finite() && ohlc.close > 0.0);
        assert_eq!(ohlc.interval, 1);
        assert!(ohlc.trades > 0);
    }

    #[rstest]
    fn test_serialize_add_order_request() {
        let request = KrakenWsRequest {
            method: KrakenWsMethod::AddOrder,
            params: Some(KrakenWsParams::AddOrder(KrakenWsAddOrderParams {
                order_type: KrakenOrderType::Limit,
                side: KrakenOrderSide::Buy,
                order_qty: 0.01,
                symbol: "BTC/USD".to_string(),
                limit_price: Some(30000.0),
                time_in_force: Some(KrakenTimeInForce::GoodTilCancelled),
                expire_time: None,
                cl_ord_id: Some("O-20260505-000001".to_string()),
                post_only: Some(true),
                reduce_only: None,
                leverage: None,
                trigger: None,
                conditional: None,
                token: "TESTTOKEN".to_string(),
            })),
            req_id: Some(42),
        };

        let serialized = serde_json::to_string(&request).expect("Failed to serialize");
        let expected: serde_json::Value =
            serde_json::from_str(&load_test_data("ws_add_order_request.json"))
                .expect("Failed to parse fixture");
        let actual: serde_json::Value =
            serde_json::from_str(&serialized).expect("Failed to parse serialized");
        assert_eq!(actual, expected);
    }

    #[rstest]
    fn test_serialize_amend_order_request() {
        let request = KrakenWsRequest {
            method: KrakenWsMethod::AmendOrder,
            params: Some(KrakenWsParams::AmendOrder(KrakenWsAmendOrderParams {
                order_id: Some("OABCDE-12345-FGHIJ".to_string()),
                cl_ord_id: None,
                order_qty: Some(0.005),
                limit_price: None,
                trigger_price: None,
                token: "TESTTOKEN".to_string(),
            })),
            req_id: Some(43),
        };

        let serialized = serde_json::to_string(&request).expect("Failed to serialize");
        let expected: serde_json::Value =
            serde_json::from_str(&load_test_data("ws_amend_order_request.json"))
                .expect("Failed to parse fixture");
        let actual: serde_json::Value =
            serde_json::from_str(&serialized).expect("Failed to parse serialized");
        assert_eq!(actual, expected);
    }

    #[rstest]
    fn test_serialize_cancel_order_request() {
        let request = KrakenWsRequest {
            method: KrakenWsMethod::CancelOrder,
            params: Some(KrakenWsParams::CancelOrder(KrakenWsCancelOrderParams {
                order_id: Some(vec!["OABCDE-12345-FGHIJ".to_string()]),
                cl_ord_id: None,
                token: "TESTTOKEN".to_string(),
            })),
            req_id: Some(44),
        };

        let serialized = serde_json::to_string(&request).expect("Failed to serialize");
        let expected: serde_json::Value =
            serde_json::from_str(&load_test_data("ws_cancel_order_request.json"))
                .expect("Failed to parse fixture");
        let actual: serde_json::Value =
            serde_json::from_str(&serialized).expect("Failed to parse serialized");
        assert_eq!(actual, expected);
    }

    #[rstest]
    fn test_serialize_batch_add_request() {
        let request = KrakenWsRequest {
            method: KrakenWsMethod::BatchAdd,
            params: Some(KrakenWsParams::BatchAdd(KrakenWsBatchAddParams {
                symbol: "BTC/USD".to_string(),
                orders: vec![
                    KrakenWsBatchAddOrder {
                        order_type: KrakenOrderType::Limit,
                        side: KrakenOrderSide::Buy,
                        order_qty: 0.01,
                        limit_price: Some(30000.0),
                        cl_ord_id: Some("O-A".to_string()),
                        time_in_force: None,
                        expire_time: None,
                        post_only: None,
                        reduce_only: None,
                        leverage: None,
                        trigger: None,
                    },
                    KrakenWsBatchAddOrder {
                        order_type: KrakenOrderType::Limit,
                        side: KrakenOrderSide::Sell,
                        order_qty: 0.01,
                        limit_price: Some(31000.0),
                        cl_ord_id: Some("O-B".to_string()),
                        time_in_force: None,
                        expire_time: None,
                        post_only: None,
                        reduce_only: None,
                        leverage: None,
                        trigger: None,
                    },
                ],
                token: "TESTTOKEN".to_string(),
            })),
            req_id: Some(45),
        };

        let serialized = serde_json::to_string(&request).expect("Failed to serialize");
        let expected: serde_json::Value =
            serde_json::from_str(&load_test_data("ws_batch_add_request.json"))
                .expect("Failed to parse fixture");
        let actual: serde_json::Value =
            serde_json::from_str(&serialized).expect("Failed to parse serialized");
        assert_eq!(actual, expected);
    }

    #[rstest]
    fn test_add_order_params_serializes_expire_time_for_gtd() {
        let params = KrakenWsAddOrderParams {
            order_type: KrakenOrderType::Limit,
            side: KrakenOrderSide::Buy,
            order_qty: 0.01,
            symbol: "BTC/USD".to_string(),
            token: "TKN".to_string(),
            limit_price: Some(30000.0),
            time_in_force: Some(KrakenTimeInForce::GoodTilDate),
            expire_time: Some("2026-12-31T23:59:59+00:00".to_string()),
            cl_ord_id: None,
            post_only: None,
            reduce_only: None,
            leverage: None,
            trigger: None,
            conditional: None,
        };
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&params).expect("serialize"))
                .expect("json");

        assert_eq!(value["time_in_force"], "GTD");
        assert_eq!(value["expire_time"], "2026-12-31T23:59:59+00:00");
    }

    #[rstest]
    fn test_add_order_params_omits_expire_time_when_absent() {
        let params = KrakenWsAddOrderParams {
            order_type: KrakenOrderType::Limit,
            side: KrakenOrderSide::Buy,
            order_qty: 0.01,
            symbol: "BTC/USD".to_string(),
            token: "TKN".to_string(),
            limit_price: Some(30000.0),
            time_in_force: None,
            expire_time: None,
            cl_ord_id: None,
            post_only: None,
            reduce_only: None,
            leverage: None,
            trigger: None,
            conditional: None,
        };
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&params).expect("serialize"))
                .expect("json");

        assert!(value.get("expire_time").is_none());
    }

    #[rstest]
    fn test_batch_add_order_serializes_leverage_and_trigger() {
        let order = KrakenWsBatchAddOrder {
            order_type: KrakenOrderType::StopLossLimit,
            side: KrakenOrderSide::Buy,
            order_qty: 0.01,
            limit_price: Some(31000.0),
            cl_ord_id: Some("O-CONDITIONAL".to_string()),
            time_in_force: None,
            expire_time: None,
            post_only: None,
            reduce_only: None,
            leverage: Some(2),
            trigger: Some(KrakenWsTriggerParams {
                reference: KrakenSpotTrigger::Last,
                price: 30500.0,
                price_type: None,
            }),
        };
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&order).expect("serialize")).expect("json");

        assert_eq!(
            value["leverage"], 2,
            "leverage must be serialized for margin batch legs",
        );
        assert!(
            value.get("trigger").is_some(),
            "trigger must be serialized for conditional batch legs",
        );
        assert_eq!(value["trigger"]["reference"], "last");
        assert_eq!(value["trigger"]["price"], 30500.0);
    }

    #[rstest]
    fn test_batch_add_order_omits_leverage_and_trigger_when_absent() {
        let order = KrakenWsBatchAddOrder {
            order_type: KrakenOrderType::Limit,
            side: KrakenOrderSide::Buy,
            order_qty: 0.01,
            limit_price: Some(30000.0),
            cl_ord_id: Some("O-PLAIN".to_string()),
            time_in_force: None,
            expire_time: None,
            post_only: None,
            reduce_only: None,
            leverage: None,
            trigger: None,
        };
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&order).expect("serialize")).expect("json");

        assert!(
            value.get("leverage").is_none(),
            "leverage must be omitted when None"
        );
        assert!(
            value.get("trigger").is_none(),
            "trigger must be omitted when None"
        );
    }

    #[rstest]
    fn test_deserialize_add_order_response_success() {
        let data = load_test_data("ws_add_order_response_success.json");
        let response: KrakenWsOrderResponse =
            serde_json::from_str(&data).expect("Failed to parse add_order success response");

        assert_eq!(response.method, KrakenWsMethod::AddOrder);
        assert_eq!(response.req_id, Some(42));
        assert!(response.success);
        assert!(response.error.is_none());

        let result = response.result.expect("Expected result");
        assert_eq!(result.order_id.as_deref(), Some("OABCDE-12345-FGHIJ"));
        assert_eq!(result.cl_ord_id.as_deref(), Some("O-20260505-000001"));
        assert_eq!(result.order_userref, Some(0));
    }

    #[rstest]
    fn test_deserialize_add_order_response_failure() {
        let data = load_test_data("ws_add_order_response_failure.json");
        let response: KrakenWsOrderResponse =
            serde_json::from_str(&data).expect("Failed to parse add_order failure response");

        assert_eq!(response.method, KrakenWsMethod::AddOrder);
        assert_eq!(response.req_id, Some(99));
        assert!(!response.success);
        assert_eq!(response.error.as_deref(), Some("EOrder:Insufficient funds"));
        assert!(response.result.is_none());
    }

    #[rstest]
    fn test_deserialize_batch_add_response_partial() {
        let data = load_test_data("ws_batch_add_response_partial.json");
        let response: KrakenWsOrderResponse =
            serde_json::from_str(&data).expect("Failed to parse batch_add partial response");

        assert_eq!(response.method, KrakenWsMethod::BatchAdd);
        assert_eq!(response.req_id, Some(45));
        assert!(response.success);

        let result = response.result.expect("Expected result");
        let orders = result.orders.expect("Expected orders");
        assert_eq!(orders.len(), 2);

        assert!(orders[0].success);
        assert_eq!(orders[0].order_id.as_deref(), Some("O1"));
        assert_eq!(orders[0].cl_ord_id.as_deref(), Some("O-A"));
        assert!(orders[0].error.is_none());

        assert!(!orders[1].success);
        assert!(orders[1].order_id.is_none());
        assert_eq!(orders[1].cl_ord_id.as_deref(), Some("O-B"));
        assert_eq!(orders[1].error.as_deref(), Some("EOrder:Invalid price"));
    }
}
