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

//! Data models for Kraken Spot HTTP API responses.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::{
    KrakenAssetClass, KrakenOrderSide, KrakenOrderStatus, KrakenOrderType, KrakenPairStatus,
    KrakenSystemStatus,
};

/// Wrapper for Kraken API responses.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct KrakenResponse<T> {
    pub error: Vec<String>,
    pub result: Option<T>,
}

// Asset Pairs (Instruments) Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPairInfo {
    pub altname: Ustr,
    pub wsname: Option<Ustr>,
    pub aclass_base: KrakenAssetClass,
    pub base: Ustr,
    pub aclass_quote: KrakenAssetClass,
    pub quote: Ustr,
    pub cost_decimals: u8,
    pub pair_decimals: u8,
    pub lot_decimals: u8,
    pub lot_multiplier: i32,
    #[serde(default)]
    pub leverage_buy: Vec<i32>,
    #[serde(default)]
    pub leverage_sell: Vec<i32>,
    #[serde(default)]
    pub fees: Vec<(i32, f64)>,
    #[serde(default)]
    pub fees_maker: Vec<(i32, f64)>,
    pub fee_volume_currency: Option<Ustr>,
    pub margin_call: Option<i32>,
    pub margin_stop: Option<i32>,
    pub ordermin: Option<String>,
    pub costmin: Option<String>,
    pub tick_size: Option<String>,
    pub status: Option<KrakenPairStatus>,
    #[serde(default)]
    pub long_position_limit: Option<i64>,
    #[serde(default)]
    pub short_position_limit: Option<i64>,
}

pub type AssetPairsResponse = IndexMap<String, AssetPairInfo>;

// Ticker Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerInfo {
    #[serde(rename = "a")]
    pub ask: Vec<String>,
    #[serde(rename = "b")]
    pub bid: Vec<String>,
    #[serde(rename = "c")]
    pub last: Vec<String>,
    #[serde(rename = "v")]
    pub volume: Vec<String>,
    #[serde(rename = "p")]
    pub vwap: Vec<String>,
    #[serde(rename = "t")]
    pub trades: Vec<i64>,
    #[serde(rename = "l")]
    pub low: Vec<String>,
    #[serde(rename = "h")]
    pub high: Vec<String>,
    #[serde(rename = "o")]
    pub open: String,
}

pub type TickerResponse = IndexMap<String, TickerInfo>;

// OHLC (Candlestick) Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OhlcData {
    pub time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub vwap: String,
    pub volume: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OhlcResponse {
    pub last: i64,
    #[serde(flatten)]
    pub data: IndexMap<String, Vec<Vec<serde_json::Value>>>,
}

// Trades Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeData {
    pub price: String,
    pub volume: String,
    pub time: f64,
    pub side: KrakenOrderSide,
    pub order_type: KrakenOrderType,
    pub misc: String,
    #[serde(default)]
    pub trade_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradesResponse {
    pub last: String,
    #[serde(flatten)]
    pub data: IndexMap<String, Vec<Vec<serde_json::Value>>>,
}

// Order Book Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookLevel {
    pub price: String,
    pub volume: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookData {
    pub asks: Vec<Vec<serde_json::Value>>,
    pub bids: Vec<Vec<serde_json::Value>>,
}

pub type OrderBookResponse = IndexMap<String, OrderBookData>;

// System Status Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub status: KrakenSystemStatus,
    pub timestamp: String,
}

// Server Time Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerTime {
    pub unixtime: i64,
    pub rfc1123: String,
}

// WebSocket Token Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketToken {
    pub token: String,
    pub expires: i32,
}

// Spot Private Trading Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderDescription {
    pub pair: String,
    #[serde(rename = "type")]
    pub order_side: KrakenOrderSide,
    pub ordertype: KrakenOrderType,
    pub price: String,
    pub price2: String,
    pub leverage: String,
    pub order: String,
    pub close: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotOrder {
    pub refid: Option<String>,
    pub userref: Option<i64>,
    pub status: KrakenOrderStatus,
    pub opentm: f64,
    pub starttm: Option<f64>,
    pub expiretm: Option<f64>,
    pub descr: OrderDescription,
    pub vol: String,
    pub vol_exec: String,
    pub cost: String,
    pub fee: String,
    pub price: String,
    pub stopprice: Option<String>,
    pub limitprice: Option<String>,
    pub trigger: Option<String>,
    pub misc: String,
    pub oflags: String,
    #[serde(default)]
    pub trades: Option<Vec<String>>,
    #[serde(default)]
    pub closetm: Option<f64>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub ratecount: Option<i32>,
    #[serde(default)]
    pub cl_ord_id: Option<String>,
    #[serde(default)]
    pub amended: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotOpenOrdersResult {
    pub open: IndexMap<String, SpotOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotClosedOrdersResult {
    pub closed: IndexMap<String, SpotOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotTrade {
    pub ordertxid: String,
    pub postxid: String,
    pub pair: String,
    pub time: f64,
    #[serde(rename = "type")]
    pub trade_type: KrakenOrderSide,
    pub ordertype: KrakenOrderType,
    pub price: String,
    pub cost: String,
    pub fee: String,
    pub vol: String,
    pub margin: String,
    pub leverage: Option<String>,
    pub misc: String,
    #[serde(default)]
    pub trade_id: Option<i64>,
    #[serde(default)]
    pub maker: Option<bool>,
    #[serde(default)]
    pub ledgers: Option<Vec<String>>,
    #[serde(default)]
    pub posstatus: Option<String>,
    #[serde(default)]
    pub cprice: Option<String>,
    #[serde(default)]
    pub ccost: Option<String>,
    #[serde(default)]
    pub cfee: Option<String>,
    #[serde(default)]
    pub cvol: Option<String>,
    #[serde(default)]
    pub cmargin: Option<String>,
    #[serde(default)]
    pub net: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotTradesHistoryResult {
    pub trades: IndexMap<String, SpotTrade>,
    pub count: i32,
}

// Spot Order Execution Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotAddOrderResponse {
    pub descr: Option<OrderDescription>,
    #[serde(default)]
    pub txid: Vec<String>,
    #[serde(default)]
    pub cl_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotCancelOrderResponse {
    pub count: i32,
    #[serde(default)]
    pub pending: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotEditOrderResponse {
    pub descr: Option<OrderDescription>,
    pub txid: Option<String>,
    #[serde(default)]
    pub originaltxid: Option<String>,
    #[serde(default)]
    pub volume: Option<String>,
    #[serde(default)]
    pub price: Option<String>,
    #[serde(default)]
    pub price2: Option<String>,
    #[serde(default)]
    pub orders_cancelled: Option<i32>,
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
    fn test_parse_server_time() {
        let data = load_test_data("http_server_time.json");
        let response: KrakenResponse<ServerTime> =
            serde_json::from_str(&data).expect("Failed to parse server time");

        assert!(response.error.is_empty());
        let result = response.result.expect("Missing result");
        assert!(result.unixtime > 0);
        assert!(!result.rfc1123.is_empty());
    }

    #[rstest]
    fn test_parse_system_status() {
        let data = load_test_data("http_system_status.json");
        let response: KrakenResponse<SystemStatus> =
            serde_json::from_str(&data).expect("Failed to parse system status");

        assert!(response.error.is_empty());
        let result = response.result.expect("Missing result");
        assert!(!result.timestamp.is_empty());
    }

    #[rstest]
    fn test_parse_asset_pairs() {
        let data = load_test_data("http_asset_pairs.json");
        let response: KrakenResponse<AssetPairsResponse> =
            serde_json::from_str(&data).expect("Failed to parse asset pairs");

        assert!(response.error.is_empty());
        let result = response.result.expect("Missing result");
        assert!(!result.is_empty());

        let pair = result.get("XBTUSDT").expect("XBTUSDT pair not found");
        assert_eq!(pair.altname.as_str(), "XBTUSDT");
        assert_eq!(pair.base.as_str(), "XXBT");
        assert_eq!(pair.quote.as_str(), "USDT");
        assert!(pair.wsname.is_some());
    }

    #[rstest]
    fn test_parse_ticker() {
        let data = load_test_data("http_ticker.json");
        let response: KrakenResponse<TickerResponse> =
            serde_json::from_str(&data).expect("Failed to parse ticker");

        assert!(response.error.is_empty());
        let result = response.result.expect("Missing result");
        assert!(!result.is_empty());

        let ticker = result.get("XBTUSDT").expect("XBTUSDT ticker not found");
        assert_eq!(ticker.ask.len(), 3);
        assert_eq!(ticker.bid.len(), 3);
        assert_eq!(ticker.last.len(), 2);
    }

    #[rstest]
    fn test_parse_ohlc() {
        let data = load_test_data("http_ohlc.json");
        let response: KrakenResponse<serde_json::Value> =
            serde_json::from_str(&data).expect("Failed to parse OHLC");

        assert!(response.error.is_empty());
        assert!(response.result.is_some());
    }

    #[rstest]
    fn test_parse_order_book() {
        let data = load_test_data("http_order_book.json");
        let response: KrakenResponse<OrderBookResponse> =
            serde_json::from_str(&data).expect("Failed to parse order book");

        assert!(response.error.is_empty());
        let result = response.result.expect("Missing result");
        assert!(!result.is_empty());

        let book = result.get("XBTUSDT").expect("XBTUSDT order book not found");
        assert!(!book.asks.is_empty());
        assert!(!book.bids.is_empty());
    }

    #[rstest]
    fn test_parse_trades() {
        let data = load_test_data("http_trades.json");
        let response: KrakenResponse<TradesResponse> =
            serde_json::from_str(&data).expect("Failed to parse trades");

        assert!(response.error.is_empty());
        let result = response.result.expect("Missing result");
        assert!(!result.data.is_empty());
    }
}
