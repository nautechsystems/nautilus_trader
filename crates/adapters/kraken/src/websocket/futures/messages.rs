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

//! Data models for Kraken Futures WebSocket v1 API messages.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use strum::{AsRefStr, EnumString};
use ustr::Ustr;

use crate::common::enums::{KrakenFillType, KrakenFuturesOrderType, KrakenOrderSide};

// Normalizes a float price field so `0.0` is treated as "no price set".
// Kraken Futures wire messages send a literal `0.0` for absent prices
// (e.g. `stop_price: 0.0` on pure limit orders) rather than omitting the
// field or sending `null`. Without this, downstream code would see
// `Some(0.0)` and emit bogus trigger prices on `OrderUpdated` events,
// which the order model rejects for non-stop order types.
fn deserialize_optional_price_zero_as_none<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<f64>::deserialize(deserializer)?;
    Ok(value.filter(|v| *v != 0.0))
}

/// Output message types from the Futures WebSocket handler.
#[derive(Clone, Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "Messages are ephemeral and immediately consumed"
)]
pub enum KrakenFuturesWsMessage {
    Ticker(KrakenFuturesTickerData),
    Trade(KrakenFuturesTradeData),
    BookSnapshot(KrakenFuturesBookSnapshot),
    BookDelta(KrakenFuturesBookDelta),
    OpenOrdersCancel(KrakenFuturesOpenOrdersCancel),
    OpenOrdersDelta(KrakenFuturesOpenOrdersDelta),
    FillsDelta(KrakenFuturesFillsDelta),
    Challenge(String),
    Reconnected,
}

/// Kraken Futures WebSocket feed types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, AsRefStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum KrakenFuturesFeed {
    Ticker,
    Trade,
    TradeSnapshot,
    Book,
    BookSnapshot,
    Heartbeat,
    OpenOrders,
    OpenOrdersSnapshot,
    Fills,
    FillsSnapshot,
}

/// Kraken Futures WebSocket subscription channel types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub enum KrakenFuturesChannel {
    Book,
    Deltas,
    Trades,
    Quotes,
    Mark,
    Index,
    Funding,
}

/// Kraken Futures WebSocket event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KrakenFuturesEvent {
    Subscribe,
    Unsubscribe,
    Subscribed,
    Unsubscribed,
    Info,
    Error,
    Alert,
    Challenge,
}

/// Message type classification for efficient routing.
/// Used to classify incoming WebSocket messages without full deserialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KrakenFuturesMessageType {
    // Private feeds (execution)
    OpenOrdersSnapshot,
    OpenOrdersCancel,
    OpenOrdersDelta,
    FillsSnapshot,
    FillsDelta,
    // Public feeds (market data)
    Ticker,
    TradeSnapshot,
    Trade,
    BookSnapshot,
    BookDelta,
    // Control messages
    Info,
    Pong,
    Subscribed,
    Unsubscribed,
    Challenge,
    Heartbeat,
    Error,
    Alert,
    Unknown,
}

#[must_use]
pub fn classify_futures_message(value: &Value) -> KrakenFuturesMessageType {
    if let Some(event) = value.get("event").and_then(|v| v.as_str()) {
        return match event {
            "info" => KrakenFuturesMessageType::Info,
            "pong" => KrakenFuturesMessageType::Pong,
            "subscribed" => KrakenFuturesMessageType::Subscribed,
            "unsubscribed" => KrakenFuturesMessageType::Unsubscribed,
            "challenge" => KrakenFuturesMessageType::Challenge,
            "error" => KrakenFuturesMessageType::Error,
            "alert" => KrakenFuturesMessageType::Alert,
            _ => KrakenFuturesMessageType::Unknown,
        };
    }

    if let Some(feed) = value.get("feed").and_then(|v| v.as_str()) {
        return match feed {
            "heartbeat" => KrakenFuturesMessageType::Heartbeat,
            "open_orders_snapshot" => KrakenFuturesMessageType::OpenOrdersSnapshot,
            "open_orders" => {
                // Cancel messages have is_cancel=true but no "order" object
                if value.get("is_cancel").and_then(|v| v.as_bool()) == Some(true) {
                    if value.get("order").is_some() {
                        KrakenFuturesMessageType::OpenOrdersDelta
                    } else {
                        KrakenFuturesMessageType::OpenOrdersCancel
                    }
                } else {
                    KrakenFuturesMessageType::OpenOrdersDelta
                }
            }
            "fills_snapshot" => KrakenFuturesMessageType::FillsSnapshot,
            "fills" => KrakenFuturesMessageType::FillsDelta,
            "ticker" => KrakenFuturesMessageType::Ticker,
            "trade_snapshot" => KrakenFuturesMessageType::TradeSnapshot,
            "trade" => KrakenFuturesMessageType::Trade,
            "book_snapshot" => KrakenFuturesMessageType::BookSnapshot,
            "book" => KrakenFuturesMessageType::BookDelta,
            _ => KrakenFuturesMessageType::Unknown,
        };
    }

    KrakenFuturesMessageType::Unknown
}

/// Subscribe/unsubscribe request for Kraken Futures WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesRequest {
    pub event: KrakenFuturesEvent,
    pub feed: KrakenFuturesFeed,
    pub product_ids: Vec<String>,
}

/// Response to a subscription request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesSubscriptionResponse {
    pub event: KrakenFuturesEvent,
    pub feed: KrakenFuturesFeed,
    pub product_ids: Vec<String>,
}

/// Error response from Kraken Futures WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesErrorResponse {
    pub event: KrakenFuturesEvent,
    #[serde(default)]
    pub message: Option<String>,
}

/// Info message from Kraken Futures WebSocket (sent on connection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesInfoMessage {
    pub event: KrakenFuturesEvent,
    pub version: i32,
}

/// Heartbeat message from Kraken Futures WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesHeartbeat {
    pub feed: KrakenFuturesFeed,
    pub time: i64,
}

/// Ticker data from Kraken Futures WebSocket (uses snake_case).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesTickerData {
    pub feed: KrakenFuturesFeed,
    pub product_id: Ustr,
    #[serde(default)]
    pub time: Option<i64>,
    #[serde(default)]
    pub bid: Option<f64>,
    #[serde(default)]
    pub ask: Option<f64>,
    #[serde(default)]
    pub bid_size: Option<f64>,
    #[serde(default)]
    pub ask_size: Option<f64>,
    #[serde(default)]
    pub last: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub volume_quote: Option<f64>,
    #[serde(default, rename = "openInterest")]
    pub open_interest: Option<f64>,
    #[serde(default)]
    pub index: Option<f64>,
    #[serde(default, rename = "markPrice")]
    pub mark_price: Option<f64>,
    #[serde(default)]
    pub change: Option<f64>,
    #[serde(default)]
    pub open: Option<f64>,
    #[serde(default)]
    pub high: Option<f64>,
    #[serde(default)]
    pub low: Option<f64>,
    #[serde(default)]
    pub funding_rate: Option<f64>,
    #[serde(default)]
    pub funding_rate_prediction: Option<f64>,
    #[serde(default)]
    pub relative_funding_rate: Option<f64>,
    #[serde(default)]
    pub relative_funding_rate_prediction: Option<f64>,
    #[serde(default)]
    pub next_funding_rate_time: Option<f64>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub pair: Option<String>,
    #[serde(default)]
    pub leverage: Option<String>,
    #[serde(default)]
    pub dtm: Option<i64>,
    #[serde(default, rename = "maturityTime")]
    pub maturity_time: Option<i64>,
    #[serde(default)]
    pub suspended: Option<bool>,
    #[serde(default)]
    pub post_only: Option<bool>,
}

/// Trade data from Kraken Futures WebSocket (uses snake_case).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesTradeData {
    pub feed: KrakenFuturesFeed,
    pub product_id: Ustr,
    #[serde(default)]
    pub uid: Option<String>,
    pub side: KrakenOrderSide,
    #[serde(rename = "type", default)]
    pub trade_type: Option<String>,
    pub seq: i64,
    pub time: i64,
    pub qty: f64,
    pub price: f64,
}

/// Trade snapshot from Kraken Futures WebSocket (sent on subscription).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesTradeSnapshot {
    pub feed: KrakenFuturesFeed,
    pub product_id: Ustr,
    pub trades: Vec<KrakenFuturesTradeData>,
}

/// Book snapshot from Kraken Futures WebSocket (uses snake_case).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesBookSnapshot {
    pub feed: KrakenFuturesFeed,
    pub product_id: Ustr,
    pub timestamp: i64,
    pub seq: i64,
    #[serde(default, rename = "tickSize")]
    pub tick_size: Option<f64>,
    pub bids: Vec<KrakenFuturesBookLevel>,
    pub asks: Vec<KrakenFuturesBookLevel>,
}

/// Book delta from Kraken Futures WebSocket (uses snake_case).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesBookDelta {
    pub feed: KrakenFuturesFeed,
    pub product_id: Ustr,
    pub side: KrakenOrderSide,
    pub seq: i64,
    pub price: f64,
    pub qty: f64,
    pub timestamp: i64,
}

/// Price level in order book.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesBookLevel {
    pub price: f64,
    pub qty: f64,
}

/// Challenge request for WebSocket authentication.
#[derive(Debug, Clone, Serialize)]
pub struct KrakenFuturesChallengeRequest {
    pub event: KrakenFuturesEvent,
    pub api_key: String,
}

/// Challenge response from WebSocket.
#[derive(Debug, Clone, Deserialize)]
pub struct KrakenFuturesChallengeResponse {
    pub event: KrakenFuturesEvent,
    pub message: String,
}

/// Authenticated subscription request for private feeds.
#[derive(Debug, Clone, Serialize)]
pub struct KrakenFuturesPrivateSubscribeRequest {
    pub event: KrakenFuturesEvent,
    pub feed: KrakenFuturesFeed,
    pub api_key: String,
    pub original_challenge: String,
    pub signed_challenge: String,
}

/// Open order from Kraken Futures WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesOpenOrder {
    pub instrument: Ustr,
    pub time: i64,
    pub last_update_time: i64,
    pub qty: f64,
    pub filled: f64,
    /// Limit price. Optional for stop/trigger orders which only have stop_price.
    #[serde(default, deserialize_with = "deserialize_optional_price_zero_as_none")]
    pub limit_price: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_optional_price_zero_as_none")]
    pub stop_price: Option<f64>,
    #[serde(rename = "type")]
    pub order_type: KrakenFuturesOrderType,
    pub order_id: String,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    /// 0 = buy, 1 = sell
    pub direction: i32,
    #[serde(default)]
    pub reduce_only: bool,
    #[serde(default, rename = "triggerSignal")]
    pub trigger_signal: Option<String>,
}

/// Open orders snapshot from Kraken Futures WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesOpenOrdersSnapshot {
    pub feed: KrakenFuturesFeed,
    #[serde(default)]
    pub account: Option<String>,
    pub orders: Vec<KrakenFuturesOpenOrder>,
}

/// Open orders delta/update from Kraken Futures WebSocket.
/// Used when full order details are provided (new orders, updates).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesOpenOrdersDelta {
    pub feed: KrakenFuturesFeed,
    pub order: KrakenFuturesOpenOrder,
    pub is_cancel: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

impl KrakenFuturesOpenOrdersDelta {
    /// Returns whether this delta represents a fill-driven removal from the book.
    ///
    /// Kraken Futures sends an open_orders delta with `is_cancel=true` and a
    /// `full_fill`/`partial_fill` reason when an order leaves the book because
    /// it filled. The actual fill data arrives via the fills feed, so callers
    /// must skip these deltas to avoid emitting a spurious `OrderCanceled`
    /// event before the real `OrderFilled`.
    #[must_use]
    pub fn is_fill_driven_cancel(&self) -> bool {
        self.is_cancel && matches!(self.reason.as_deref(), Some("full_fill" | "partial_fill"))
    }
}

/// Open orders cancel notification from Kraken Futures WebSocket.
/// Used when an order is canceled - contains only order identifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesOpenOrdersCancel {
    pub feed: KrakenFuturesFeed,
    pub order_id: String,
    pub cli_ord_id: Option<String>,
    pub is_cancel: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Fill from Kraken Futures WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesFill {
    #[serde(alias = "product_id")]
    pub instrument: Option<Ustr>,
    pub time: i64,
    pub price: f64,
    pub qty: f64,
    pub order_id: String,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    pub fill_id: String,
    pub fill_type: KrakenFillType,
    /// true = buy, false = sell
    pub buy: bool,
    #[serde(default)]
    pub fee_paid: Option<f64>,
    #[serde(default)]
    pub fee_currency: Option<String>,
}

/// Fills snapshot from Kraken Futures WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesFillsSnapshot {
    pub feed: KrakenFuturesFeed,
    #[serde(default)]
    pub account: Option<String>,
    pub fills: Vec<KrakenFuturesFill>,
}

/// Fills delta/update from Kraken Futures WebSocket.
/// Note: Kraken sends fills updates in array format (same as snapshot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesFillsDelta {
    pub feed: KrakenFuturesFeed,
    #[serde(default)]
    pub username: Option<String>,
    pub fills: Vec<KrakenFuturesFill>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_deserialize_ticker_data() {
        // Kraken Futures WebSocket uses snake_case (unlike the REST API which uses camelCase)
        let json = r#"{
            "feed": "ticker",
            "product_id": "PI_XBTUSD",
            "time": 1700000000000,
            "bid": 90650.5,
            "ask": 90651.0,
            "bid_size": 10.5,
            "ask_size": 8.2,
            "last": 90650.8,
            "volume": 1234567.89,
            "index": 90648.5,
            "markPrice": 90649.2,
            "funding_rate": 0.0001,
            "openInterest": 50000000.0
        }"#;

        let ticker: KrakenFuturesTickerData = serde_json::from_str(json).unwrap();
        assert_eq!(ticker.feed, KrakenFuturesFeed::Ticker);
        assert_eq!(ticker.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(ticker.bid, Some(90650.5));
        assert_eq!(ticker.ask, Some(90651.0));
        assert_eq!(ticker.index, Some(90648.5));
        assert_eq!(ticker.mark_price, Some(90649.2));
        assert_eq!(ticker.funding_rate, Some(0.0001));
    }

    #[rstest]
    fn test_serialize_subscribe_request() {
        let request = KrakenFuturesRequest {
            event: KrakenFuturesEvent::Subscribe,
            feed: KrakenFuturesFeed::Ticker,
            product_ids: vec!["PI_XBTUSD".to_string()],
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"event\":\"subscribe\""));
        assert!(json.contains("\"feed\":\"ticker\""));
        assert!(json.contains("PI_XBTUSD"));
    }

    #[rstest]
    fn test_deserialize_ticker_from_fixture() {
        let json = include_str!("../../../test_data/ws_futures_ticker.json");
        let ticker: KrakenFuturesTickerData = serde_json::from_str(json).unwrap();

        assert_eq!(ticker.feed, KrakenFuturesFeed::Ticker);
        assert_eq!(ticker.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(ticker.bid, Some(21978.5));
        assert_eq!(ticker.ask, Some(21987.0));
        assert_eq!(ticker.bid_size, Some(2536.0));
        assert_eq!(ticker.ask_size, Some(13948.0));
        assert_eq!(ticker.index, Some(21984.54));
        assert_eq!(ticker.mark_price, Some(21979.68641534714));
        assert!(ticker.funding_rate.is_some());
    }

    #[rstest]
    fn test_deserialize_trade_from_fixture() {
        let json = include_str!("../../../test_data/ws_futures_trade.json");
        let trade: KrakenFuturesTradeData = serde_json::from_str(json).unwrap();

        assert_eq!(trade.feed, KrakenFuturesFeed::Trade);
        assert_eq!(trade.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(trade.side, KrakenOrderSide::Sell);
        assert_eq!(trade.qty, 15000.0);
        assert_eq!(trade.price, 34969.5);
        assert_eq!(trade.seq, 653355);
    }

    #[rstest]
    fn test_deserialize_trade_snapshot_from_fixture() {
        let json = include_str!("../../../test_data/ws_futures_trade_snapshot.json");
        let snapshot: KrakenFuturesTradeSnapshot = serde_json::from_str(json).unwrap();

        assert_eq!(snapshot.feed, KrakenFuturesFeed::TradeSnapshot);
        assert_eq!(snapshot.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(snapshot.trades.len(), 2);
        assert_eq!(snapshot.trades[0].price, 34893.0);
        assert_eq!(snapshot.trades[1].price, 34891.0);
    }

    #[rstest]
    fn test_deserialize_book_snapshot_from_fixture() {
        let json = include_str!("../../../test_data/ws_futures_book_snapshot.json");
        let snapshot: KrakenFuturesBookSnapshot = serde_json::from_str(json).unwrap();

        assert_eq!(snapshot.feed, KrakenFuturesFeed::BookSnapshot);
        assert_eq!(snapshot.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(snapshot.bids.len(), 2);
        assert_eq!(snapshot.asks.len(), 2);
        assert_eq!(snapshot.bids[0].price, 34892.5);
        assert_eq!(snapshot.asks[0].price, 34911.5);
    }

    #[rstest]
    fn test_deserialize_book_delta_from_fixture() {
        let json = include_str!("../../../test_data/ws_futures_book_delta.json");
        let delta: KrakenFuturesBookDelta = serde_json::from_str(json).unwrap();

        assert_eq!(delta.feed, KrakenFuturesFeed::Book);
        assert_eq!(delta.product_id, Ustr::from("PI_XBTUSD"));
        assert_eq!(delta.side, KrakenOrderSide::Sell);
        assert_eq!(delta.price, 34981.0);
        assert_eq!(delta.qty, 0.0); // Delete action
    }

    #[rstest]
    fn test_deserialize_open_orders_snapshot_from_fixture() {
        let json = include_str!("../../../test_data/ws_futures_open_orders_snapshot.json");
        let snapshot: KrakenFuturesOpenOrdersSnapshot = serde_json::from_str(json).unwrap();

        assert_eq!(snapshot.feed, KrakenFuturesFeed::OpenOrdersSnapshot);
        assert_eq!(snapshot.orders.len(), 1);
        assert_eq!(snapshot.orders[0].instrument, Ustr::from("PI_XBTUSD"));
        assert_eq!(snapshot.orders[0].qty, 1000.0);
        assert_eq!(
            snapshot.orders[0].order_type,
            KrakenFuturesOrderType::StopLower
        );
    }

    #[rstest]
    fn test_deserialize_open_orders_delta_from_fixture() {
        let json = include_str!("../../../test_data/ws_futures_open_orders_delta.json");
        let delta: KrakenFuturesOpenOrdersDelta = serde_json::from_str(json).unwrap();

        assert_eq!(delta.feed, KrakenFuturesFeed::OpenOrders);
        assert!(!delta.is_cancel);
        assert_eq!(delta.order.instrument, Ustr::from("PI_XBTUSD"));
        assert_eq!(delta.order.qty, 304.0);
        assert_eq!(delta.order.limit_price, Some(10640.0));
        // Kraken sends stop_price: 0.0 on pure limit orders. The zero-as-none
        // deserializer maps that back to None so downstream code does not emit
        // a bogus trigger_price, which the order model rejects for limit types.
        assert_eq!(delta.order.stop_price, None);
    }

    #[rstest]
    fn test_deserialize_open_orders_delta_full_fill_is_fill_driven_cancel() {
        // Regression for the spurious OrderCanceled bug: Kraken sends a delta with
        // is_cancel=true, qty=0, filled=full, reason="full_fill" when an order leaves
        // the book because it filled. The delta must be classified as fill-driven so
        // the execution path skips it and lets the FillsDelta carry the actual fill.
        let json = include_str!("../../../test_data/ws_futures_open_orders_delta_full_fill.json");
        let delta: KrakenFuturesOpenOrdersDelta = serde_json::from_str(json).unwrap();

        assert!(delta.is_cancel);
        assert_eq!(delta.reason.as_deref(), Some("full_fill"));
        assert_eq!(delta.order.qty, 0.0);
        assert_eq!(delta.order.filled, 0.0001);
        assert!(delta.is_fill_driven_cancel());
    }

    #[rstest]
    #[case::placement(false, None, false)]
    #[case::user_cancel(true, Some("cancelled_by_user"), false)]
    #[case::post_only_reject(true, Some("post_order_failed_because_it_would_filled"), false)]
    #[case::full_fill(true, Some("full_fill"), true)]
    #[case::partial_fill(true, Some("partial_fill"), true)]
    #[case::cancel_no_reason(true, None, false)]
    fn test_open_orders_delta_is_fill_driven_cancel(
        #[case] is_cancel: bool,
        #[case] reason: Option<&'static str>,
        #[case] expected: bool,
    ) {
        let delta = KrakenFuturesOpenOrdersDelta {
            feed: KrakenFuturesFeed::OpenOrders,
            order: KrakenFuturesOpenOrder {
                instrument: Ustr::from("PF_XBTUSD"),
                time: 0,
                last_update_time: 0,
                qty: 0.0001,
                filled: 0.0,
                limit_price: Some(70_000.0),
                stop_price: None,
                order_type: KrakenFuturesOrderType::Limit,
                order_id: "test".to_string(),
                cli_ord_id: None,
                direction: 0,
                reduce_only: false,
                trigger_signal: None,
            },
            is_cancel,
            reason: reason.map(str::to_string),
        };

        assert_eq!(delta.is_fill_driven_cancel(), expected);
    }

    #[rstest]
    fn test_deserialize_open_orders_cancel_from_fixture() {
        let json = include_str!("../../../test_data/ws_futures_open_orders_cancel.json");
        let cancel: KrakenFuturesOpenOrdersCancel = serde_json::from_str(json).unwrap();

        assert_eq!(cancel.feed, KrakenFuturesFeed::OpenOrders);
        assert!(cancel.is_cancel);
        assert_eq!(cancel.order_id, "660c6b23-8007-48c1-a7c9-4893f4572e8c");
        assert_eq!(cancel.reason, Some("cancelled_by_user".to_string()));
        assert!(cancel.cli_ord_id.is_none()); // Not in docs example
    }

    #[rstest]
    fn test_deserialize_fills_snapshot_from_fixture() {
        let json = include_str!("../../../test_data/ws_futures_fills_snapshot.json");
        let snapshot: KrakenFuturesFillsSnapshot = serde_json::from_str(json).unwrap();

        assert_eq!(snapshot.feed, KrakenFuturesFeed::FillsSnapshot);
        assert_eq!(snapshot.fills.len(), 2);
        assert_eq!(
            snapshot.fills[0].instrument,
            Some(Ustr::from("FI_XBTUSD_200925"))
        );
        assert!(snapshot.fills[0].buy);
        assert_eq!(snapshot.fills[0].fill_type, KrakenFillType::Maker);
    }

    #[rstest]
    fn test_classify_ticker_message() {
        let json = include_str!("../../../test_data/ws_futures_ticker.json");
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::Ticker
        );
    }

    #[rstest]
    fn test_classify_trade_message() {
        let json = include_str!("../../../test_data/ws_futures_trade.json");
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::Trade
        );
    }

    #[rstest]
    fn test_classify_trade_snapshot_message() {
        let json = include_str!("../../../test_data/ws_futures_trade_snapshot.json");
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::TradeSnapshot
        );
    }

    #[rstest]
    fn test_classify_book_snapshot_message() {
        let json = include_str!("../../../test_data/ws_futures_book_snapshot.json");
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::BookSnapshot
        );
    }

    #[rstest]
    fn test_classify_book_delta_message() {
        let json = include_str!("../../../test_data/ws_futures_book_delta.json");
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::BookDelta
        );
    }

    #[rstest]
    fn test_classify_open_orders_delta_message() {
        let json = include_str!("../../../test_data/ws_futures_open_orders_delta.json");
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::OpenOrdersDelta
        );
    }

    #[rstest]
    fn test_classify_open_orders_cancel_message() {
        let json = include_str!("../../../test_data/ws_futures_open_orders_cancel.json");
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::OpenOrdersCancel
        );
    }

    #[rstest]
    fn test_classify_heartbeat_message() {
        let json = r#"{"feed":"heartbeat","time":1700000000000}"#;
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::Heartbeat
        );
    }

    #[rstest]
    fn test_classify_info_event() {
        let json = r#"{"event":"info","version":1}"#;
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::Info
        );
    }

    #[rstest]
    fn test_classify_subscribed_event() {
        let json = r#"{"event":"subscribed","feed":"ticker","product_ids":["PI_XBTUSD"]}"#;
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::Subscribed
        );
    }

    #[rstest]
    fn test_classify_error_event() {
        let json = r#"{"event":"error","message":"Unknown product_id"}"#;
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::Error
        );
    }

    #[rstest]
    fn test_classify_alert_event() {
        let json = r#"{"event":"alert","message":"Rate limit exceeded"}"#;
        let value: Value = serde_json::from_str(json).unwrap();
        assert_eq!(
            classify_futures_message(&value),
            KrakenFuturesMessageType::Alert
        );
    }
}
