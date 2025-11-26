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

//! Data models for Kraken HTTP API responses.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::{
    KrakenAssetClass, KrakenOrderSide, KrakenOrderType, KrakenPairStatus, KrakenSystemStatus,
};

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

// Futures Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesMarginLevel {
    pub contracts: i64,
    #[serde(rename = "initialMargin")]
    pub initial_margin: f64,
    #[serde(rename = "maintenanceMargin")]
    pub maintenance_margin: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesInstrument {
    pub symbol: String,
    #[serde(rename = "type")]
    pub instrument_type: String,
    pub underlying: String,
    #[serde(rename = "tickSize")]
    pub tick_size: f64,
    #[serde(rename = "contractSize")]
    pub contract_size: f64,
    pub tradeable: bool,
    #[serde(rename = "impactMidSize")]
    pub impact_mid_size: f64,
    #[serde(rename = "maxPositionSize")]
    pub max_position_size: f64,
    #[serde(rename = "openingDate")]
    pub opening_date: String,
    #[serde(rename = "marginLevels")]
    pub margin_levels: Vec<FuturesMarginLevel>,
    #[serde(rename = "fundingRateCoefficient", default)]
    pub funding_rate_coefficient: Option<i32>,
    #[serde(rename = "maxRelativeFundingRate", default)]
    pub max_relative_funding_rate: Option<f64>,
    #[serde(default)]
    pub isin: Option<String>,
    #[serde(rename = "contractValueTradePrecision")]
    pub contract_value_trade_precision: i32,
    #[serde(rename = "postOnly")]
    pub post_only: bool,
    #[serde(rename = "feeScheduleUid")]
    pub fee_schedule_uid: String,
    pub mtf: bool,
    pub base: String,
    pub quote: String,
    pub pair: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesInstrumentsResponse {
    pub result: String,
    pub instruments: Vec<FuturesInstrument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesTicker {
    pub symbol: String,
    pub last: f64,
    #[serde(rename = "lastTime")]
    pub last_time: String,
    pub tag: String,
    pub pair: String,
    #[serde(rename = "markPrice")]
    pub mark_price: f64,
    pub bid: f64,
    #[serde(rename = "bidSize")]
    pub bid_size: f64,
    pub ask: f64,
    #[serde(rename = "askSize")]
    pub ask_size: f64,
    #[serde(rename = "vol24h")]
    pub vol_24h: f64,
    #[serde(rename = "volumeQuote")]
    pub volume_quote: f64,
    #[serde(rename = "openInterest")]
    pub open_interest: f64,
    #[serde(rename = "open24h")]
    pub open_24h: f64,
    #[serde(rename = "high24h")]
    pub high_24h: f64,
    #[serde(rename = "low24h")]
    pub low_24h: f64,
    #[serde(rename = "lastSize")]
    pub last_size: f64,
    #[serde(rename = "fundingRate", default)]
    pub funding_rate: Option<f64>,
    #[serde(rename = "fundingRatePrediction", default)]
    pub funding_rate_prediction: Option<f64>,
    pub suspended: bool,
    #[serde(rename = "indexPrice")]
    pub index_price: f64,
    #[serde(rename = "postOnly")]
    pub post_only: bool,
    #[serde(rename = "change24h")]
    pub change_24h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesTickersResponse {
    pub result: String,
    #[serde(rename = "serverTime")]
    pub server_time: String,
    pub tickers: Vec<FuturesTicker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesCandle {
    pub time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesCandlesResponse {
    pub candles: Vec<FuturesCandle>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::http::client::KrakenResponse;

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
