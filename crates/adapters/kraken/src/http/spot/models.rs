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

//! Data models for Kraken Spot HTTP API responses.

use indexmap::IndexMap;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, SeqAccess, Visitor},
};
use ustr::Ustr;

use crate::common::enums::{
    KrakenAssetClass, KrakenOrderSide, KrakenOrderStatus, KrakenOrderType, KrakenPairStatus,
    KrakenSpotTrigger, KrakenSystemStatus,
};

/// Wrapper for Kraken API responses.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct KrakenResponse<T> {
    pub error: Vec<String>,
    pub result: Option<T>,
}

// Balance Models

/// Response from Kraken Balance endpoint.
/// Maps currency codes (e.g., "USDT", "ETH") to their balance amounts as strings.
pub type BalanceResponse = IndexMap<String, String>;

/// Response from `POST /0/private/TradeBalance` (margin accounts only).
///
/// Distinct from [`BalanceResponse`]: wallet balances give currency amounts held; this gives
/// margin accounting metrics (equity, used margin, free margin) denominated in a single asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeBalanceResponse {
    pub eb: String, // equivalent balance (all currencies combined)
    pub tb: String, // trade balance (equity currency collateral)
    pub m: String,  // margin amount of open positions (used margin)
    pub uv: String, // unexecuted value of partly filled orders/positions
    pub n: String,  // unrealized net profit/loss of open positions
    pub c: String,  // cost basis of open positions
    pub v: String,  // current floating valuation of open positions
    pub e: String,  // equity = eb + n
    pub mf: String, // free margin = e - m
    #[serde(default)]
    pub ml: Option<String>, // margin level % (absent when no positions are open)
}

/// A single open spot margin position from `POST /0/private/OpenPositions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotOpenPosition {
    pub ordertxid: String,
    pub pair: String,
    pub time: f64,
    #[serde(rename = "type")]
    pub side: KrakenOrderSide,
    pub ordertype: KrakenOrderType,
    pub cost: String,
    pub fee: String,
    pub vol: String,
    pub vol_closed: String,
    pub margin: String,
    #[serde(default)]
    pub posstatus: Option<String>,
    #[serde(default)]
    pub value: Option<String>, // present when docalcs=true
    #[serde(default)]
    pub net: Option<String>, // present when docalcs=true
    #[serde(default)]
    pub terms: Option<String>,
    #[serde(default)]
    pub rollovertm: Option<String>,
    #[serde(default)]
    pub misc: Option<String>,
    #[serde(default)]
    pub oflags: Option<String>,
}

/// Response from `POST /0/private/OpenPositions`: maps position ID to position data.
///
/// Kraken returns `[]` (empty array) when there are no open positions, and a JSON object
/// (map) when positions exist. The custom deserializer handles both forms.
#[derive(Debug, Clone, Default)]
pub struct SpotOpenPositionsResponse(IndexMap<String, SpotOpenPosition>);

impl std::ops::Deref for SpotOpenPositionsResponse {
    type Target = IndexMap<String, SpotOpenPosition>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SpotOpenPositionsResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = SpotOpenPositionsResponse;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a map of open positions or an empty array")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut out = IndexMap::new();
                while let Some((k, v)) = map.next_entry::<String, SpotOpenPosition>()? {
                    out.insert(k, v);
                }
                Ok(SpotOpenPositionsResponse(out))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                if seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(serde::de::Error::custom(
                        "OpenPositions: expected empty array or object map, received non-empty array",
                    ));
                }
                Ok(SpotOpenPositionsResponse(IndexMap::new()))
            }
        }
        deserializer.deserialize_any(V)
    }
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

/// Order description from QueryOrders response (full details, required fields).
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

/// Order description from AddOrder response (simpler, with optional fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddOrderDescription {
    #[serde(default)]
    pub order: Option<String>,
    #[serde(default)]
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
    pub trigger: Option<KrakenSpotTrigger>,
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
    /// Average fill price (if returned by the API)
    #[serde(default)]
    pub avg_price: Option<String>,
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
    pub descr: Option<AddOrderDescription>,
    #[serde(default)]
    pub txid: Vec<String>,
    #[serde(default)]
    pub cl_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotBatchOrderResponse {
    #[serde(default)]
    pub descr: Option<AddOrderDescription>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub txid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotAddOrderBatchResponse {
    #[serde(default)]
    pub orders: Vec<SpotBatchOrderResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotCancelOrderResponse {
    pub count: i32,
    #[serde(default)]
    pub pending: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotCancelOrderBatchResponse {
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotEditOrderResponse {
    pub descr: Option<AddOrderDescription>,
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

/// Response from `POST /0/private/AmendOrder`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotAmendOrderResponse {
    /// The amend transaction ID.
    pub amend_id: String,
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

    #[rstest]
    fn test_open_positions_empty_array() {
        let result: SpotOpenPositionsResponse = serde_json::from_str("[]").unwrap();
        assert!(result.is_empty());
    }

    #[rstest]
    fn test_open_positions_empty_object() {
        let result: SpotOpenPositionsResponse = serde_json::from_str("{}").unwrap();
        assert!(result.is_empty());
    }

    #[rstest]
    fn test_open_positions_non_empty_array_errors() {
        let err =
            serde_json::from_str::<SpotOpenPositionsResponse>(r#"[{"posid": "123"}]"#).unwrap_err();
        assert!(
            err.to_string()
                .contains("OpenPositions: expected empty array or object map"),
            "unexpected error: {err}"
        );
    }
}
