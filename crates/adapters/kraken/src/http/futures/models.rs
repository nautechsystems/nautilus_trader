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

//! Data models for Kraken Futures HTTP API responses.

use serde::{Deserialize, Serialize};

use crate::common::enums::{
    KrakenApiResult, KrakenFillType, KrakenFuturesOrderStatus, KrakenFuturesOrderType,
    KrakenOrderSide, KrakenPositionSide, KrakenTriggerSignal,
};

// Futures Instruments Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesMarginLevel {
    /// Number of contracts (for inverse futures) or notional units (for flexible futures).
    /// The field name varies: `contracts` for inverse, `numNonContractUnits` for flexible.
    #[serde(alias = "numNonContractUnits", default)]
    pub contracts: f64,
    pub initial_margin: f64,
    pub maintenance_margin: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesInstrument {
    pub symbol: String,
    #[serde(rename = "type")]
    pub instrument_type: String,
    /// Only present for inverse futures, not for flexible futures.
    #[serde(default)]
    pub underlying: Option<String>,
    pub tick_size: f64,
    pub contract_size: f64,
    pub tradeable: bool,
    #[serde(default)]
    pub impact_mid_size: Option<f64>,
    #[serde(default)]
    pub max_position_size: Option<f64>,
    pub opening_date: String,
    pub margin_levels: Vec<FuturesMarginLevel>,
    #[serde(default)]
    pub funding_rate_coefficient: Option<i32>,
    #[serde(default)]
    pub max_relative_funding_rate: Option<f64>,
    #[serde(default)]
    pub isin: Option<String>,
    pub contract_value_trade_precision: i32,
    pub post_only: bool,
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

// Futures Ticker Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesTicker {
    pub symbol: String,
    pub last: f64,
    pub last_time: String,
    pub tag: String,
    pub pair: String,
    pub mark_price: f64,
    pub bid: f64,
    pub bid_size: f64,
    pub ask: f64,
    pub ask_size: f64,
    #[serde(rename = "vol24h")]
    pub vol_24h: f64,
    pub volume_quote: f64,
    pub open_interest: f64,
    #[serde(rename = "open24h")]
    pub open_24h: f64,
    #[serde(rename = "high24h")]
    pub high_24h: f64,
    #[serde(rename = "low24h")]
    pub low_24h: f64,
    pub last_size: f64,
    #[serde(default)]
    pub funding_rate: Option<f64>,
    #[serde(default)]
    pub funding_rate_prediction: Option<f64>,
    pub suspended: bool,
    pub index_price: f64,
    pub post_only: bool,
    #[serde(rename = "change24h")]
    pub change_24h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesTickersResponse {
    pub result: KrakenApiResult,
    pub server_time: String,
    pub tickers: Vec<FuturesTicker>,
}

// Futures OHLC (Candles) Models

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

// Futures Open Orders Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOpenOrder {
    #[serde(rename = "order_id")]
    pub order_id: String,
    pub symbol: String,
    pub side: KrakenOrderSide,
    pub order_type: KrakenFuturesOrderType,
    #[serde(default)]
    pub limit_price: Option<f64>,
    #[serde(default)]
    pub stop_price: Option<f64>,
    pub unfilled_size: f64,
    pub received_time: String,
    pub status: KrakenFuturesOrderStatus,
    pub filled_size: f64,
    #[serde(default)]
    pub reduce_only: Option<bool>,
    pub last_update_time: String,
    #[serde(default)]
    pub trigger_signal: Option<KrakenTriggerSignal>,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOpenOrdersResponse {
    pub result: KrakenApiResult,
    pub server_time: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub open_orders: Vec<FuturesOpenOrder>,
}

// Futures Order Events Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderEvent {
    #[serde(rename = "order_id")]
    pub order_id: String,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "type")]
    pub order_type: KrakenFuturesOrderType,
    pub symbol: String,
    pub side: KrakenOrderSide,
    pub quantity: f64,
    pub filled: f64,
    #[serde(default)]
    pub limit_price: Option<f64>,
    #[serde(default)]
    pub stop_price: Option<f64>,
    pub timestamp: String,
    pub last_update_timestamp: String,
    #[serde(rename = "reduce_only", default)]
    pub reduce_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderEventsResponse {
    pub result: KrakenApiResult,
    pub server_time: String,
    pub elements: Vec<FuturesOrderEvent>,
}

// Futures Fills Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesFill {
    #[serde(rename = "fill_id")]
    pub fill_id: String,
    pub symbol: String,
    pub side: KrakenOrderSide,
    #[serde(rename = "order_id")]
    pub order_id: String,
    pub fill_time: String,
    pub size: f64,
    pub price: f64,
    pub fill_type: KrakenFillType,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "fee_paid", default)]
    pub fee_paid: Option<f64>,
    #[serde(rename = "fee_currency", default)]
    pub fee_currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesFillsResponse {
    pub result: KrakenApiResult,
    pub server_time: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub fills: Vec<FuturesFill>,
}

// Futures Positions Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPosition {
    pub side: KrakenPositionSide,
    pub symbol: String,
    pub price: f64,
    pub fill_time: String,
    pub size: f64,
    #[serde(default)]
    pub unrealized_funding: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOpenPositionsResponse {
    pub result: KrakenApiResult,
    pub server_time: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub open_positions: Vec<FuturesPosition>,
}

// Futures Order Execution Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesSendOrderResponse {
    pub result: KrakenApiResult,
    pub server_time: String,
    #[serde(default)]
    pub error: Option<String>,
    pub send_status: Option<FuturesSendStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesSendStatus {
    #[serde(rename = "order_id", default)]
    pub order_id: Option<String>,
    pub status: String,
    #[serde(default)]
    pub order_events: Option<Vec<FuturesOrderEvent>>,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelOrderResponse {
    pub result: KrakenApiResult,
    pub server_time: String,
    pub cancel_status: FuturesCancelStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelStatus {
    pub status: String,
    #[serde(rename = "order_id", default)]
    pub order_id: Option<String>,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesEditOrderResponse {
    pub result: KrakenApiResult,
    pub server_time: String,
    pub edit_status: FuturesEditStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesEditStatus {
    pub status: String,
    #[serde(rename = "order_id", default)]
    pub order_id: Option<String>,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesBatchOrderResponse {
    pub result: KrakenApiResult,
    pub server_time: String,
    pub batch_status: Vec<FuturesSendStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelAllOrdersResponse {
    pub result: KrakenApiResult,
    pub server_time: String,
    pub cancel_status: FuturesCancelAllStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelAllStatus {
    pub status: String,
    #[serde(default)]
    pub cancelled_orders: Vec<CancelledOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelledOrder {
    #[serde(rename = "order_id", default)]
    pub order_id: Option<String>,
}

// Futures Public Executions Models

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

    fn load_test_data(filename: &str) -> String {
        let path = format!("test_data/{filename}");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to load test data from {path}: {e}"))
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
