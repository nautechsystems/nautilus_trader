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

//! Data models for Kraken Futures HTTP API responses.

use ahash::AHashMap;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::common::{
    enums::{
        KrakenApiResult, KrakenFillType, KrakenFuturesOrderEventType, KrakenFuturesOrderStatus,
        KrakenFuturesOrderType, KrakenInstrumentType, KrakenOrderSide, KrakenPositionSide,
        KrakenSendStatus, KrakenTriggerSide, KrakenTriggerSignal,
    },
    serialization::{decimal, decimal_map, deserialize_decimal_pair, optional_decimal},
};

// Futures Instruments Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesMarginLevel {
    /// Number of contracts (for inverse futures) or notional units (for flexible futures).
    /// The field name varies: `contracts` for inverse, `numNonContractUnits` for flexible.
    #[serde(alias = "numNonContractUnits", default, with = "decimal")]
    pub contracts: Decimal,
    #[serde(with = "decimal")]
    pub initial_margin: Decimal,
    #[serde(with = "decimal")]
    pub maintenance_margin: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesInstrument {
    pub symbol: String,
    #[serde(rename = "type")]
    pub instrument_type: KrakenInstrumentType,
    /// Only present for inverse futures, not for flexible futures.
    #[serde(default)]
    pub underlying: Option<String>,
    #[serde(with = "decimal")]
    pub tick_size: Decimal,
    #[serde(with = "decimal")]
    pub contract_size: Decimal,
    pub tradeable: bool,
    #[serde(default, with = "optional_decimal")]
    pub impact_mid_size: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub max_position_size: Option<Decimal>,
    pub opening_date: String,
    pub margin_levels: Vec<FuturesMarginLevel>,
    #[serde(default)]
    pub funding_rate_coefficient: Option<i32>,
    #[serde(default, with = "optional_decimal")]
    pub max_relative_funding_rate: Option<Decimal>,
    #[serde(default)]
    pub isin: Option<String>,
    pub contract_value_trade_precision: i32,
    pub post_only: bool,
    #[serde(default)]
    pub fee_schedule_uid: Option<String>,
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
    #[serde(default, with = "optional_decimal")]
    pub last: Option<Decimal>,
    #[serde(default)]
    pub last_time: Option<String>,
    pub tag: String,
    pub pair: String,
    #[serde(default, with = "optional_decimal")]
    pub mark_price: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub bid: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub bid_size: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub ask: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub ask_size: Option<Decimal>,
    #[serde(rename = "vol24h", default, with = "optional_decimal")]
    pub vol_24h: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub volume_quote: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub open_interest: Option<Decimal>,
    #[serde(rename = "open24h", default, with = "optional_decimal")]
    pub open_24h: Option<Decimal>,
    #[serde(rename = "high24h", default, with = "optional_decimal")]
    pub high_24h: Option<Decimal>,
    #[serde(rename = "low24h", default, with = "optional_decimal")]
    pub low_24h: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub last_size: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub funding_rate: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub funding_rate_prediction: Option<Decimal>,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default, with = "optional_decimal")]
    pub index_price: Option<Decimal>,
    #[serde(default)]
    pub post_only: bool,
    #[serde(rename = "change24h", default, with = "optional_decimal")]
    pub change_24h: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesTickersResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    pub tickers: Vec<FuturesTicker>,
}

// Futures Order Book Models

/// A `[price, qty]` pair from the Kraken Futures orderbook endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct FuturesOrderBookLevel {
    #[serde(serialize_with = "decimal::serialize")]
    pub price: Decimal,
    #[serde(serialize_with = "decimal::serialize")]
    pub qty: Decimal,
}

impl<'de> serde::Deserialize<'de> for FuturesOrderBookLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let arr = deserialize_decimal_pair(deserializer)?;
        Ok(Self {
            price: arr.0,
            qty: arr.1,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderBookData {
    pub bids: Vec<FuturesOrderBookLevel>,
    pub asks: Vec<FuturesOrderBookLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderBookResponse {
    pub result: KrakenApiResult,
    pub order_book: FuturesOrderBookData,
    #[serde(default)]
    pub server_time: Option<String>,
}

// Futures Historical Funding Rates Models

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesHistoricalFundingRate {
    pub timestamp: String,
    #[serde(with = "decimal")]
    pub relative_funding_rate: Decimal,
    #[serde(with = "decimal")]
    pub funding_rate: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesHistoricalFundingRatesResponse {
    pub result: KrakenApiResult,
    pub rates: Vec<FuturesHistoricalFundingRate>,
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
    #[serde(default, with = "optional_decimal")]
    pub limit_price: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub stop_price: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub unfilled_size: Option<Decimal>,
    pub received_time: String,
    pub status: KrakenFuturesOrderStatus,
    #[serde(with = "decimal")]
    pub filled_size: Decimal,
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
    #[serde(default)]
    pub server_time: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub open_orders: Vec<FuturesOpenOrder>,
}

// Futures Order Events Models (v2 API)

/// Wrapper for an order event containing the order data and event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderEventWrapper {
    pub order: FuturesOrderEvent,
    #[serde(rename = "type")]
    pub event_type: KrakenFuturesOrderEventType,
    #[serde(default, with = "optional_decimal")]
    pub reduced_quantity: Option<Decimal>,
}

/// The actual order data within an order event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderEvent {
    pub order_id: String,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "type")]
    pub order_type: KrakenFuturesOrderType,
    pub symbol: String,
    pub side: KrakenOrderSide,
    #[serde(with = "decimal")]
    pub quantity: Decimal,
    #[serde(with = "decimal")]
    pub filled: Decimal,
    #[serde(default, with = "optional_decimal")]
    pub limit_price: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub stop_price: Option<Decimal>,
    pub timestamp: String,
    pub last_update_timestamp: String,
    #[serde(default)]
    pub reduce_only: bool,
}

/// Response from the Kraken Futures order events v2 endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderEventsResponse {
    #[serde(default)]
    pub server_time: Option<String>,
    #[serde(default)]
    pub order_events: Vec<FuturesOrderEventWrapper>,
    #[serde(default)]
    pub continuation_token: Option<String>,
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
    #[serde(with = "decimal")]
    pub size: Decimal,
    #[serde(with = "decimal")]
    pub price: Decimal,
    pub fill_type: KrakenFillType,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "fee_paid", default, with = "optional_decimal")]
    pub fee_paid: Option<Decimal>,
    #[serde(rename = "fee_currency", default)]
    pub fee_currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesFillsResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
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
    #[serde(with = "decimal")]
    pub price: Decimal,
    pub fill_time: String,
    #[serde(with = "decimal")]
    pub size: Decimal,
    #[serde(default, with = "optional_decimal")]
    pub unrealized_funding: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOpenPositionsResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
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
    #[serde(default)]
    pub server_time: Option<String>,
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
    pub order_events: Option<Vec<FuturesSendOrderEvent>>,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "receivedTime", default)]
    pub received_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesSendOrderEvent {
    #[serde(rename = "type")]
    pub event_type: KrakenFuturesOrderEventType,
    #[serde(default)]
    pub order: Option<FuturesOrderEventData>,
    #[serde(default)]
    pub order_trigger: Option<FuturesOrderTriggerData>,
    #[serde(default, with = "optional_decimal")]
    pub reduced_quantity: Option<Decimal>,
    // Execution event fields
    #[serde(rename = "executionId", default)]
    pub execution_id: Option<String>,
    #[serde(default, with = "optional_decimal")]
    pub price: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub amount: Option<Decimal>,
    #[serde(rename = "orderPriorEdit", default)]
    pub order_prior_edit: Option<Box<FuturesOrderEventData>>,
    #[serde(rename = "orderPriorExecution", default)]
    pub order_prior_execution: Option<Box<FuturesOrderEventData>>,
    #[serde(rename = "takerReducedQuantity", default, with = "optional_decimal")]
    pub taker_reduced_quantity: Option<Decimal>,
    // Reject event fields
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub uid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderEventData {
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "cliOrdId", default)]
    pub cli_ord_id: Option<String>,
    #[serde(rename = "type")]
    pub order_type: KrakenFuturesOrderType,
    pub symbol: String,
    pub side: KrakenOrderSide,
    #[serde(with = "decimal")]
    pub quantity: Decimal,
    #[serde(with = "decimal")]
    pub filled: Decimal,
    #[serde(rename = "limitPrice", default, with = "optional_decimal")]
    pub limit_price: Option<Decimal>,
    #[serde(rename = "stopPrice", default, with = "optional_decimal")]
    pub stop_price: Option<Decimal>,
    pub timestamp: String,
    #[serde(rename = "lastUpdateTimestamp")]
    pub last_update_timestamp: String,
    #[serde(rename = "reduceOnly", default)]
    pub reduce_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderTriggerData {
    pub uid: String,
    #[serde(rename = "clientId", default)]
    pub client_id: Option<String>,
    #[serde(rename = "type")]
    pub order_type: KrakenFuturesOrderType,
    pub symbol: String,
    pub side: KrakenOrderSide,
    #[serde(with = "decimal")]
    pub quantity: Decimal,
    #[serde(rename = "limitPrice", default, with = "optional_decimal")]
    pub limit_price: Option<Decimal>,
    #[serde(rename = "limitPriceOffsetValue", default, with = "optional_decimal")]
    pub limit_price_offset_value: Option<Decimal>,
    #[serde(rename = "limitPriceOffsetUnit", default)]
    pub limit_price_offset_unit: Option<String>,
    #[serde(rename = "triggerPrice")]
    #[serde(with = "decimal")]
    pub trigger_price: Decimal,
    #[serde(rename = "triggerSide")]
    pub trigger_side: KrakenTriggerSide,
    #[serde(rename = "triggerSignal")]
    pub trigger_signal: KrakenTriggerSignal,
    #[serde(rename = "reduceOnly", default)]
    pub reduce_only: bool,
    pub timestamp: String,
    #[serde(rename = "lastUpdateTimestamp")]
    pub last_update_timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelOrderResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    pub cancel_status: FuturesCancelStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelStatus {
    pub status: KrakenSendStatus,
    #[serde(rename = "order_id", default)]
    pub order_id: Option<String>,
    #[serde(rename = "cli_ord_id", default)]
    pub cli_ord_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesEditOrderResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
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
    #[serde(default)]
    pub server_time: Option<String>,
    pub batch_status: Vec<FuturesSendStatus>,
}

/// Response for batch cancel operations via `/derivatives/api/v3/batchorder`.
///
/// When sending only cancel operations, the response has a different format
/// with individual cancel status items.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesBatchCancelResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub batch_status: Vec<FuturesBatchCancelStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesBatchCancelStatus {
    #[serde(default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    #[serde(default)]
    pub status: Option<KrakenSendStatus>,
    #[serde(default)]
    pub cancel_status: Option<FuturesCancelStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelAllOrdersResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub server_time: Option<String>,
    pub cancel_status: FuturesCancelAllStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelAllStatus {
    pub status: KrakenSendStatus,
    #[serde(default)]
    pub cancelled_orders: Vec<CancelledOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelledOrder {
    #[serde(rename = "order_id", default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub cli_ord_id: Option<String>,
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

// Futures Accounts Models

/// Response from the Kraken Futures accounts endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesAccountsResponse {
    pub result: KrakenApiResult,
    #[serde(default)]
    pub accounts: AHashMap<String, FuturesAccount>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub server_time: Option<String>,
}

/// Kraken Futures account type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KrakenFuturesAccountType {
    /// Multi-collateral margin account (flex).
    MultiCollateralMarginAccount,
    /// Single-collateral margin account.
    MarginAccount,
    /// Cash account (no margin).
    CashAccount,
    /// Unknown account type.
    #[serde(other)]
    Unknown,
}

/// A Kraken Futures account (margin or multi-collateral).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesAccount {
    #[serde(rename = "type")]
    pub account_type: KrakenFuturesAccountType,
    /// Balances for margin accounts (symbol -> amount).
    #[serde(default, with = "decimal_map")]
    pub balances: AHashMap<String, Decimal>,
    /// Currencies for flex/multi-collateral accounts.
    #[serde(default)]
    pub currencies: AHashMap<String, FuturesFlexCurrency>,
    /// Auxiliary info for margin accounts.
    #[serde(default)]
    pub auxiliary: Option<FuturesAuxiliary>,
    /// Margin requirements.
    #[serde(default)]
    pub margin_requirements: Option<FuturesMarginRequirements>,
    /// Portfolio value (for flex accounts).
    #[serde(default, with = "optional_decimal")]
    pub portfolio_value: Option<Decimal>,
    /// Available margin (for flex accounts).
    #[serde(default, with = "optional_decimal")]
    pub available_margin: Option<Decimal>,
    /// Initial margin (for flex accounts).
    #[serde(default, with = "optional_decimal")]
    pub initial_margin: Option<Decimal>,
    /// PnL (for flex accounts).
    #[serde(default, with = "optional_decimal")]
    pub pnl: Option<Decimal>,
}

/// Currency info for flex/multi-collateral accounts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesFlexCurrency {
    #[serde(with = "decimal")]
    pub quantity: Decimal,
    #[serde(default, with = "optional_decimal")]
    pub value: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub collateral: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub available: Option<Decimal>,
}

/// Auxiliary account info for margin accounts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesAuxiliary {
    #[serde(default, with = "optional_decimal")]
    pub usd: Option<Decimal>,
    /// Portfolio value.
    #[serde(default, with = "optional_decimal")]
    pub pv: Option<Decimal>,
    /// Profit/loss.
    #[serde(default, with = "optional_decimal")]
    pub pnl: Option<Decimal>,
    /// Available funds.
    #[serde(default, with = "optional_decimal")]
    pub af: Option<Decimal>,
    #[serde(default, with = "optional_decimal")]
    pub funding: Option<Decimal>,
}

/// Margin requirements for an account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesMarginRequirements {
    /// Initial margin.
    #[serde(default, with = "optional_decimal")]
    pub im: Option<Decimal>,
    /// Maintenance margin.
    #[serde(default, with = "optional_decimal")]
    pub mm: Option<Decimal>,
    /// Liquidation threshold.
    #[serde(default, with = "optional_decimal")]
    pub lt: Option<Decimal>,
    /// Termination threshold.
    #[serde(default, with = "optional_decimal")]
    pub tt: Option<Decimal>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn load_test_data(filename: &str) -> String {
        let path = format!("test_data/{filename}");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to load test data from {path}: {e}"))
    }

    #[rstest]
    fn test_parse_futures_cancel_all_orders_with_no_orders_to_cancel_status() {
        // Regression for the venue response shape that broke parsing in production:
        // the `cancelStatus.status` field is `noOrdersToCancel` even when one or more
        // orders were canceled in the same call. The `cancelledOrders` array carries
        // the actual canceled order ids, so the parser must accept this status.
        let raw = r#"{
            "result": "success",
            "cancelStatus": {
                "receivedTime": "2026-04-10T13:17:23.291Z",
                "cancelOnly": "PF_XBTUSD",
                "status": "noOrdersToCancel",
                "cancelledOrders": [
                    {
                        "order_id": "a182b1c0-cd01-4d1c-853b-605e936f412b",
                        "cliOrdId": "5f173994-f660-4809-b97a-586221fe5926"
                    }
                ],
                "orderEvents": []
            },
            "serverTime": "2026-04-10T13:17:23.291Z"
        }"#;

        let response: FuturesCancelAllOrdersResponse =
            serde_json::from_str(raw).expect("Failed to parse cancel-all response");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(
            response.cancel_status.status,
            KrakenSendStatus::NoOrdersToCancel
        );
        assert_eq!(response.cancel_status.cancelled_orders.len(), 1);
        assert_eq!(
            response.cancel_status.cancelled_orders[0]
                .order_id
                .as_deref(),
            Some("a182b1c0-cd01-4d1c-853b-605e936f412b")
        );
        assert_eq!(
            response.cancel_status.cancelled_orders[0]
                .cli_ord_id
                .as_deref(),
            Some("5f173994-f660-4809-b97a-586221fe5926")
        );
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
        assert_eq!(order.limit_price, Some(dec!(1200)));
        assert_eq!(order.unfilled_size, Some(dec!(100)));
        assert_eq!(order.filled_size, dec!(0));

        let trigger_order = &response.open_orders[1];
        assert_eq!(
            trigger_order.order_id,
            "c8135f52-2a86-4e26-b629-43cc37da9dbf"
        );
        assert_eq!(trigger_order.order_type, KrakenFuturesOrderType::TakeProfit);
        assert_eq!(trigger_order.symbol, "PI_XBTUSD");
        assert_eq!(trigger_order.side, KrakenOrderSide::Buy);
        assert_eq!(trigger_order.limit_price, None);
        assert_eq!(trigger_order.stop_price, Some(dec!(1880.4)));
        assert_eq!(trigger_order.unfilled_size, None);
        assert_eq!(trigger_order.received_time, "2023-04-07T15:14:25.995Z");
        assert_eq!(trigger_order.status, KrakenFuturesOrderStatus::Untouched);
        assert_eq!(trigger_order.filled_size, dec!(0));
        assert_eq!(trigger_order.reduce_only, Some(true));
        assert_eq!(trigger_order.last_update_time, "2023-04-07T15:14:25.995Z");
        assert_eq!(
            trigger_order.trigger_signal,
            Some(KrakenTriggerSignal::Last)
        );
        assert_eq!(trigger_order.cli_ord_id, None);
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
        assert_eq!(fill.size, dec!(5000));
        assert_eq!(fill.price, dec!(27937.5));
        assert_eq!(fill.fill_type, KrakenFillType::Maker);
        assert_eq!(fill.fee_currency, Some("BTC".to_string()));
        assert_eq!(response.fills[1].fill_type, KrakenFillType::Taker);
        assert_eq!(response.fills[2].fill_type, KrakenFillType::Assignee);
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
        assert_eq!(position.size, dec!(8000));
        assert!(position.unrealized_funding.is_some());
    }

    #[rstest]
    fn test_parse_futures_orderbook() {
        let data = load_test_data("http_futures_orderbook.json");
        let response: FuturesOrderBookResponse =
            serde_json::from_str(&data).expect("Failed to parse futures orderbook");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.order_book.bids.len(), 3);
        assert_eq!(response.order_book.asks.len(), 3);

        let best_bid = &response.order_book.bids[0];
        assert_eq!(best_bid.price, dec!(105900));
        assert_eq!(best_bid.qty, dec!(0.5));

        let best_ask = &response.order_book.asks[0];
        assert_eq!(best_ask.price, dec!(105950));
        assert_eq!(best_ask.qty, dec!(0.3));
    }

    #[rstest]
    fn test_parse_futures_historical_funding_rates() {
        let data = load_test_data("http_futures_historical_funding_rates.json");
        let response: FuturesHistoricalFundingRatesResponse =
            serde_json::from_str(&data).expect("Failed to parse historical funding rates");

        assert_eq!(response.result, KrakenApiResult::Success);
        assert_eq!(response.rates.len(), 3);

        let rate = &response.rates[0];
        assert_eq!(rate.timestamp, "2025-07-11T08:00:00.000Z");
        assert_eq!(rate.relative_funding_rate, dec!(0.0001));
        assert_eq!(rate.funding_rate, dec!(0.00005));

        let negative_rate = &response.rates[1];
        assert_eq!(negative_rate.relative_funding_rate, dec!(-0.00005));
    }

    #[rstest]
    fn test_parse_futures_orderbook_preserves_decimal_precision() {
        let data = load_test_data("http_futures_orderbook_precision.json");
        let level: FuturesOrderBookLevel = serde_json::from_str(&data).unwrap();

        assert_eq!(level.price, dec!(0.1234567890123456789012345678));
        assert_eq!(level.qty, dec!(123456789.123456789));
    }

    #[rstest]
    fn test_parse_futures_order_events_uses_enum_event_type() {
        let data = load_test_data("http_futures_order_events.json");
        let response: FuturesOrderEventsResponse =
            serde_json::from_str(&data).expect("Failed to parse futures order events");

        assert_eq!(response.order_events.len(), 3);
        assert_eq!(
            response.order_events[0].event_type,
            KrakenFuturesOrderEventType::Place
        );
        assert_eq!(
            response.order_events[1].event_type,
            KrakenFuturesOrderEventType::Fill
        );
        assert_eq!(
            response.order_events[2].event_type,
            KrakenFuturesOrderEventType::Cancel
        );
    }

    #[rstest]
    fn test_parse_futures_order_events_tolerates_unknown_enum_values() {
        // Regression for Kraken Futures change log 2026-05-18: an `"unknown"`
        // value anywhere in the batch must not fail the surrounding response.
        let data = load_test_data("http_futures_order_events_unknown.json");
        let response: FuturesOrderEventsResponse =
            serde_json::from_str(&data).expect("Failed to parse futures order events with unknown");

        assert_eq!(response.order_events.len(), 1);
        assert_eq!(
            response.order_events[0].order.order_type,
            KrakenFuturesOrderType::Unknown
        );
    }

    #[rstest]
    fn test_parse_futures_order_trigger_data_tolerates_unknown_enum_values() {
        // Trigger payload uses non-optional enums, so an `"unknown"` on
        // triggerSide / triggerSignal must not fail the sendStatus batch.
        let data = load_test_data("http_send_order_futures_unknown_trigger.json");
        let response: FuturesSendOrderResponse =
            serde_json::from_str(&data).expect("Failed to parse send-order response with unknown");

        let send_status = response.send_status.expect("sendStatus missing");
        let order_events = send_status.order_events.expect("orderEvents missing");
        let trigger = order_events[0]
            .order_trigger
            .as_ref()
            .expect("orderTrigger missing");

        assert_eq!(trigger.order_type, KrakenFuturesOrderType::Unknown);
        assert_eq!(trigger.trigger_side, KrakenTriggerSide::Unknown);
        assert_eq!(trigger.trigger_signal, KrakenTriggerSignal::Unknown);
    }

    #[rstest]
    fn test_parse_futures_send_order_execution_event_uses_enum_event_type() {
        let data = r#"
        {
          "result": "success",
          "sendStatus": {
            "status": "placed",
            "orderEvents": [
              {
                "type": "EXECUTION",
                "executionId": "c8a35168-8d52-4609-944f-3f32bb0d5c77",
                "price": 35000.5,
                "amount": 1.25,
                "orderPriorExecution": {
                  "orderId": "c8a35168-8d52-4609-944f-3f32bb0d5c77",
                  "cliOrdId": "test-order-001",
                  "type": "lmt",
                  "symbol": "PI_XBTUSD",
                  "side": "buy",
                  "quantity": 2.0,
                  "filled": 0.0,
                  "limitPrice": 35000.5,
                  "timestamp": "2024-01-15T10:30:45.123Z",
                  "lastUpdateTimestamp": "2024-01-15T10:30:45.123Z",
                  "reduceOnly": false
                }
              }
            ]
          }
        }
        "#;
        let response: FuturesSendOrderResponse =
            serde_json::from_str(data).expect("Failed to parse futures send order response");

        let send_status = response.send_status.expect("sendStatus missing");
        let order_events = send_status.order_events.expect("orderEvents missing");

        assert_eq!(order_events.len(), 1);
        assert_eq!(
            order_events[0].event_type,
            KrakenFuturesOrderEventType::Execution
        );
    }
}
