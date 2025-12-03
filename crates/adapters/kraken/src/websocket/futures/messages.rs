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

//! Data models for Kraken Futures WebSocket v1 API messages.

use nautilus_model::data::{
    IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick,
};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// Output message types from the Futures WebSocket handler.
#[derive(Clone, Debug)]
pub enum FuturesWsMessage {
    MarkPrice(MarkPriceUpdate),
    IndexPrice(IndexPriceUpdate),
    Quote(QuoteTick),
    Trade(TradeTick),
    BookDeltas(OrderBookDeltas),
}

/// Kraken Futures WebSocket feed types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KrakenFuturesFeed {
    Ticker,
    Trade,
    TradeSnapshot,
    Book,
    BookSnapshot,
    Heartbeat,
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
    pub event: String,
    #[serde(default)]
    pub message: Option<String>,
}

/// Info message from Kraken Futures WebSocket (sent on connection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesInfoMessage {
    pub event: String,
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
    pub side: Ustr,
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
    #[serde(default)]
    pub tick_size: Option<f64>,
    pub bids: Vec<KrakenFuturesBookLevel>,
    pub asks: Vec<KrakenFuturesBookLevel>,
}

/// Book delta from Kraken Futures WebSocket (uses snake_case).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesBookDelta {
    pub feed: KrakenFuturesFeed,
    pub product_id: Ustr,
    pub side: Ustr,
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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
}
