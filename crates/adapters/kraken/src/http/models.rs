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
    KrakenApiResult, KrakenAssetClass, KrakenFillType, KrakenFuturesOrderStatus,
    KrakenFuturesOrderType, KrakenOrderSide, KrakenOrderStatus, KrakenOrderType, KrakenPairStatus,
    KrakenPositionSide, KrakenSystemStatus, KrakenTriggerSignal,
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

// Futures Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesMarginLevel {
    /// Number of contracts (for inverse futures) or notional units (for flexible futures).
    /// The field name varies: `contracts` for inverse, `numNonContractUnits` for flexible.
    #[serde(alias = "numNonContractUnits", default)]
    pub contracts: f64,
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
    /// Only present for inverse futures, not for flexible futures.
    #[serde(default)]
    pub underlying: Option<String>,
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
    pub result: KrakenApiResult,
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
    pub result: KrakenApiResult,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesOpenOrder {
    pub order_id: String,
    pub symbol: String,
    pub side: KrakenOrderSide,
    #[serde(rename = "orderType")]
    pub order_type: KrakenFuturesOrderType,
    #[serde(rename = "limitPrice", default)]
    pub limit_price: Option<f64>,
    #[serde(rename = "stopPrice", default)]
    pub stop_price: Option<f64>,
    #[serde(rename = "unfilledSize")]
    pub unfilled_size: f64,
    #[serde(rename = "receivedTime")]
    pub received_time: String,
    pub status: KrakenFuturesOrderStatus,
    #[serde(rename = "filledSize")]
    pub filled_size: f64,
    #[serde(rename = "reduceOnly", default)]
    pub reduce_only: Option<bool>,
    #[serde(rename = "lastUpdateTime")]
    pub last_update_time: String,
    #[serde(rename = "triggerSignal", default)]
    pub trigger_signal: Option<KrakenTriggerSignal>,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesOpenOrdersResponse {
    pub result: KrakenApiResult,
    #[serde(rename = "serverTime")]
    pub server_time: String,
    #[serde(rename = "openOrders")]
    pub open_orders: Vec<FuturesOpenOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesOrderEvent {
    pub order_id: String,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "type")]
    pub order_type: KrakenFuturesOrderType,
    pub symbol: String,
    pub side: KrakenOrderSide,
    pub quantity: f64,
    pub filled: f64,
    #[serde(rename = "limitPrice", default)]
    pub limit_price: Option<f64>,
    #[serde(rename = "stopPrice", default)]
    pub stop_price: Option<f64>,
    #[serde(rename = "timestamp")]
    pub timestamp: String,
    #[serde(rename = "lastUpdateTimestamp")]
    pub last_update_timestamp: String,
    #[serde(default)]
    pub reduce_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesOrderEventsResponse {
    pub result: KrakenApiResult,
    #[serde(rename = "serverTime")]
    pub server_time: String,
    pub elements: Vec<FuturesOrderEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesFill {
    pub fill_id: String,
    pub symbol: String,
    pub side: KrakenOrderSide,
    pub order_id: String,
    #[serde(rename = "fillTime")]
    pub fill_time: String,
    pub size: f64,
    pub price: f64,
    #[serde(rename = "fillType")]
    pub fill_type: KrakenFillType,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "fee_paid", default)]
    pub fee_paid: Option<f64>,
    #[serde(rename = "fee_currency", default)]
    pub fee_currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesFillsResponse {
    pub result: KrakenApiResult,
    #[serde(rename = "serverTime")]
    pub server_time: String,
    pub fills: Vec<FuturesFill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesPosition {
    pub side: KrakenPositionSide,
    pub symbol: String,
    pub price: f64,
    #[serde(rename = "fillTime")]
    pub fill_time: String,
    pub size: f64,
    #[serde(rename = "unrealizedFunding", default)]
    pub unrealized_funding: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesOpenPositionsResponse {
    pub result: KrakenApiResult,
    #[serde(rename = "serverTime")]
    pub server_time: String,
    #[serde(rename = "openPositions")]
    pub open_positions: Vec<FuturesPosition>,
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

// Futures Order Execution Models

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesSendOrderResponse {
    pub result: KrakenApiResult,
    #[serde(rename = "serverTime")]
    pub server_time: String,
    #[serde(rename = "sendStatus")]
    pub send_status: FuturesSendStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesSendStatus {
    #[serde(default)]
    pub order_id: Option<String>,
    pub status: String,
    #[serde(default)]
    pub order_events: Option<Vec<FuturesOrderEvent>>,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesCancelOrderResponse {
    pub result: KrakenApiResult,
    #[serde(rename = "serverTime")]
    pub server_time: String,
    #[serde(rename = "cancelStatus")]
    pub cancel_status: FuturesCancelStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesCancelStatus {
    pub status: String,
    #[serde(default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesEditOrderResponse {
    pub result: KrakenApiResult,
    #[serde(rename = "serverTime")]
    pub server_time: String,
    #[serde(rename = "editStatus")]
    pub edit_status: FuturesEditStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesEditStatus {
    pub status: String,
    #[serde(default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesBatchOrderResponse {
    pub result: KrakenApiResult,
    #[serde(rename = "serverTime")]
    pub server_time: String,
    #[serde(rename = "batchStatus")]
    pub batch_status: Vec<FuturesSendStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesCancelAllOrdersResponse {
    pub result: KrakenApiResult,
    #[serde(rename = "serverTime")]
    pub server_time: String,
    #[serde(rename = "cancelStatus")]
    pub cancel_status: FuturesCancelAllStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesCancelAllStatus {
    pub status: String,
    #[serde(rename = "cancelledOrders", default)]
    pub cancelled_orders: Vec<CancelledOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelledOrder {
    #[serde(default)]
    pub order_id: Option<String>,
}

/// Response from the Kraken Futures public executions endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPublicExecutionsResponse {
    pub elements: Vec<FuturesPublicExecutionElement>,
    #[serde(default)]
    pub len: Option<i64>,
    #[serde(default)]
    pub continuation_token: Option<String>,
}

/// A single execution element from the public executions response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesPublicExecutionElement {
    pub uid: String,
    pub timestamp: i64,
    pub event: FuturesPublicExecutionEvent,
}

/// The event wrapper containing the execution details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesPublicExecutionEvent {
    #[serde(rename = "Execution")]
    pub execution: FuturesPublicExecutionWrapper,
}

/// Wrapper containing the actual execution data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPublicExecutionWrapper {
    pub execution: FuturesPublicExecution,
    #[serde(default)]
    pub taker_reduced_quantity: Option<String>,
}

/// The actual execution/trade data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPublicExecution {
    pub uid: String,
    pub maker_order: FuturesPublicOrder,
    pub taker_order: FuturesPublicOrder,
    pub timestamp: i64,
    pub quantity: String,
    pub price: String,
    #[serde(default)]
    pub mark_price: Option<String>,
    #[serde(default)]
    pub limit_filled: Option<bool>,
    #[serde(default)]
    pub usd_value: Option<String>,
}

/// Order information within an execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPublicOrder {
    pub uid: String,
    pub tradeable: String,
    pub direction: String,
    pub quantity: String,
    pub timestamp: i64,
    #[serde(default)]
    pub limit_price: Option<String>,
    #[serde(default)]
    pub order_type: Option<String>,
    #[serde(default)]
    pub reduce_only: Option<bool>,
    #[serde(default)]
    pub last_update_timestamp: Option<i64>,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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

    #[rstest]
    fn test_parse_futures_open_orders() {
        let data = load_test_data("http_futures_open_orders.json");
        let response: FuturesOpenOrdersResponse =
            serde_json::from_str(&data).expect("Failed to parse futures open orders");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.open_orders.len(), 3);

        let order = &response.open_orders[0];
        assert_eq!(order.order_id, "2ce038ae-c144-4de7-a0f1-82f7f4fca864");
        assert_eq!(order.symbol, "PI_ETHUSD");
        assert_eq!(order.side, KrakenOrderSide::Buy);
        assert_eq!(order.order_type, KrakenFuturesOrderType::Limit);
        assert_eq!(order.limit_price, Some(1200.0));
        assert_eq!(order.unfilled_size, 100.0);
        assert_eq!(order.filled_size, 0.0);
    }

    #[rstest]
    fn test_parse_futures_fills() {
        let data = load_test_data("http_futures_fills.json");
        let response: FuturesFillsResponse =
            serde_json::from_str(&data).expect("Failed to parse futures fills");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.fills.len(), 3);

        let fill = &response.fills[0];
        assert_eq!(fill.fill_id, "cad76f07-814e-4dc6-8478-7867407b6bff");
        assert_eq!(fill.symbol, "PI_XBTUSD");
        assert_eq!(fill.side, KrakenOrderSide::Buy);
        assert_eq!(fill.size, 5000.0);
        assert_eq!(fill.price, 27937.5);
        assert_eq!(fill.fill_type, KrakenFillType::Maker);
        assert_eq!(fill.fee_currency, Some("BTC".to_string()));
    }

    #[rstest]
    fn test_parse_futures_open_positions() {
        let data = load_test_data("http_futures_open_positions.json");
        let response: FuturesOpenPositionsResponse =
            serde_json::from_str(&data).expect("Failed to parse futures open positions");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.open_positions.len(), 2);

        let position = &response.open_positions[0];
        assert_eq!(position.side, KrakenPositionSide::Short);
        assert_eq!(position.symbol, "PI_XBTUSD");
        assert_eq!(position.size, 8000.0);
        assert!(position.unrealized_funding.is_some());
    }
}
