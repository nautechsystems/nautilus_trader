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

//! Data models for Kraken WebSocket v2 API messages.

use nautilus_model::data::{Data, OrderBookDeltas};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ustr::Ustr;

use super::enums::{KrakenWsChannel, KrakenWsMessageType, KrakenWsMethod};
use crate::common::enums::{KrakenOrderSide, KrakenOrderType};

/// Nautilus WebSocket message types for Kraken adapter.
#[derive(Clone, Debug)]
pub enum NautilusWsMessage {
    Data(Vec<Data>),
    Deltas(OrderBookDeltas),
}

// Request messages

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsRequest {
    pub method: KrakenWsMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<KrakenWsParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub req_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsParams {
    pub channel: KrakenWsChannel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Vec<Ustr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

// Response messages

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

// Data messages

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsMessage {
    pub channel: KrakenWsChannel,
    #[serde(rename = "type")]
    pub event_type: KrakenWsMessageType,
    pub data: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Ustr>,
}

// Ticker data

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
}

// Trade data

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsTradeData {
    pub symbol: Ustr,
    pub side: KrakenOrderSide,
    pub price: f64,
    pub qty: f64,
    pub ord_type: KrakenOrderType,
    pub trade_id: i64,
    pub timestamp: String,
}

// Order book data

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsBookData {
    pub symbol: Ustr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bids: Option<Vec<KrakenWsBookLevel>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asks: Option<Vec<KrakenWsBookLevel>>,
    pub checksum: Option<u32>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsBookLevel {
    pub price: f64,
    pub qty: f64,
}

// OHLC data

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenWsOhlcData {
    pub symbol: Ustr,
    pub interval: u32,
    pub timestamp: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub vwap: f64,
    pub trades: i64,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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
        assert!(book.timestamp.is_some());
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
}
