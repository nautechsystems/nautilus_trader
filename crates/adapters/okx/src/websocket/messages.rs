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

//! Data structures modelling OKX WebSocket request and response payloads.

use derive_builder::Builder;
use nautilus_model::{
    data::{Data, FundingRateUpdate, OrderBookDeltas},
    events::{AccountState, OrderCancelRejected, OrderModifyRejected, OrderRejected},
    instruments::InstrumentAny,
    reports::{FillReport, OrderStatusReport},
};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::enums::{OKXWsChannel, OKXWsOperation};
use crate::{
    common::{
        enums::{
            OKXAlgoOrderType, OKXBookAction, OKXCandleConfirm, OKXExecType, OKXInstrumentType,
            OKXOrderCategory, OKXOrderStatus, OKXOrderType, OKXPositionSide, OKXSide,
            OKXTargetCurrency, OKXTradeMode, OKXTriggerType,
        },
        parse::{
            deserialize_empty_string_as_none, deserialize_string_to_u64,
            deserialize_target_currency_as_none,
        },
    },
    websocket::enums::OKXSubscriptionEvent,
};

#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    Data(Vec<Data>),
    Deltas(OrderBookDeltas),
    FundingRates(Vec<FundingRateUpdate>),
    Instrument(Box<InstrumentAny>),
    AccountUpdate(AccountState),
    OrderRejected(OrderRejected),
    OrderCancelRejected(OrderCancelRejected),
    OrderModifyRejected(OrderModifyRejected),
    ExecutionReports(Vec<ExecutionReport>),
    Error(OKXWebSocketError),
    Raw(serde_json::Value), // Unhandled channels
    Reconnected,
}

/// Represents an OKX WebSocket error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python", pyo3::pyclass)]
pub struct OKXWebSocketError {
    /// Error code from OKX (e.g., "50101").
    pub code: String,
    /// Error message from OKX.
    pub message: String,
    /// Connection ID if available.
    pub conn_id: Option<String>,
    /// Timestamp when the error occurred.
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ExecutionReport {
    Order(OrderStatusReport),
    Fill(FillReport),
}

/// Generic WebSocket request for OKX trading commands.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXWsRequest<T> {
    /// Client request ID (required for order operations).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Operation type (order, cancel-order, amend-order).
    pub op: OKXWsOperation,
    /// Request effective deadline. Unix timestamp format in milliseconds.
    /// This is when the request itself expires, not related to order expiration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp_time: Option<String>,
    /// Arguments payload for the operation.
    pub args: Vec<T>,
}

/// OKX WebSocket authentication message.
#[derive(Debug, Serialize)]
pub struct OKXAuthentication {
    pub op: &'static str,
    pub args: Vec<OKXAuthenticationArg>,
}

/// OKX WebSocket authentication arguments.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXAuthenticationArg {
    pub api_key: String,
    pub passphrase: String,
    pub timestamp: String,
    pub sign: String,
}

#[derive(Debug, Serialize)]
pub struct OKXSubscription {
    pub op: OKXWsOperation,
    pub args: Vec<OKXSubscriptionArg>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXSubscriptionArg {
    pub channel: OKXWsChannel,
    pub inst_type: Option<OKXInstrumentType>,
    pub inst_family: Option<Ustr>,
    pub inst_id: Option<Ustr>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OKXWebSocketEvent {
    Login {
        event: String,
        code: String,
        msg: String,
        #[serde(rename = "connId")]
        conn_id: String,
    },
    Subscription {
        event: OKXSubscriptionEvent,
        arg: OKXWebSocketArg,
        #[serde(rename = "connId")]
        conn_id: String,
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        msg: Option<String>,
    },
    ChannelConnCount {
        event: String,
        channel: OKXWsChannel,
        #[serde(rename = "connCount")]
        conn_count: String,
        #[serde(rename = "connId")]
        conn_id: String,
    },
    OrderResponse {
        id: Option<String>,
        op: OKXWsOperation,
        code: String,
        msg: String,
        data: Vec<serde_json::Value>,
    },
    BookData {
        arg: OKXWebSocketArg,
        action: OKXBookAction,
        data: Vec<OKXBookMsg>,
    },
    Data {
        arg: OKXWebSocketArg,
        data: serde_json::Value,
    },
    Error {
        code: String,
        msg: String,
    },
    #[serde(skip)]
    Ping,
    #[serde(skip)]
    Reconnected,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXWebSocketArg {
    /// Channel name that pushed the data.
    pub channel: OKXWsChannel,
    #[serde(default)]
    pub inst_id: Option<Ustr>,
    #[serde(default)]
    pub inst_type: Option<OKXInstrumentType>,
    #[serde(default)]
    pub inst_family: Option<Ustr>,
    #[serde(default)]
    pub bar: Option<Ustr>,
}

/// Ticker data for an instrument.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXTickerMsg {
    /// Instrument type, e.g. "SPOT", "SWAP".
    pub inst_type: OKXInstrumentType,
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: Ustr,
    /// Last traded price.
    #[serde(rename = "last")]
    pub last_px: String,
    /// Last traded size.
    pub last_sz: String,
    /// Best ask price.
    pub ask_px: String,
    /// Best ask size.
    pub ask_sz: String,
    /// Best bid price.
    pub bid_px: String,
    /// Best bid size.
    pub bid_sz: String,
    /// 24-hour opening price.
    pub open24h: String,
    /// 24-hour highest price.
    pub high24h: String,
    /// 24-hour lowest price.
    pub low24h: String,
    /// 24-hour trading volume in quote currency.
    pub vol_ccy_24h: String,
    /// 24-hour trading volume.
    pub vol24h: String,
    /// The opening price of the day (UTC 0).
    pub sod_utc0: String,
    /// The opening price of the day (UTC 8).
    pub sod_utc8: String,
    /// Timestamp of the data generation, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Represents a single order in the order book.
#[derive(Debug, Serialize, Deserialize)]
pub struct OrderBookEntry {
    /// Price of the order.
    pub price: String,
    /// Size of the order.
    pub size: String,
    /// Number of liquidated orders.
    pub liquidated_orders_count: String,
    /// Total number of orders at this price.
    pub orders_count: String,
}

/// Order book data for an instrument.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXBookMsg {
    /// Order book asks [price, size, liquidated orders count, orders count].
    pub asks: Vec<OrderBookEntry>,
    /// Order book bids [price, size, liquidated orders count, orders count].
    pub bids: Vec<OrderBookEntry>,
    /// Checksum value.
    pub checksum: Option<i64>,
    /// Sequence ID of the last sent message. Only applicable to books, books-l2-tbt, books50-l2-tbt.
    pub prev_seq_id: Option<i64>,
    /// Sequence ID of the current message, implementation details below.
    pub seq_id: u64,
    /// Order book generation time, Unix timestamp format in milliseconds, e.g. 1597026383085.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Trade data for an instrument.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXTradeMsg {
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Trade ID.
    pub trade_id: String,
    /// Trade price.
    pub px: String,
    /// Trade size.
    pub sz: String,
    /// Trade direction (buy or sell).
    pub side: OKXSide,
    /// Count.
    pub count: String,
    /// Trade timestamp, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Funding rate data for perpetual swaps.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXFundingRateMsg {
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Current funding rate.
    pub funding_rate: Ustr,
    /// Predicted next funding rate.
    pub next_funding_rate: Ustr,
    /// Next funding time, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub funding_time: u64,
    /// Message timestamp, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Mark price data for perpetual swaps.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXMarkPriceMsg {
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Current mark price.
    pub mark_px: String,
    /// Timestamp of the data generation, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Index price data.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXIndexPriceMsg {
    /// Index name, e.g. "BTC-USD".
    pub inst_id: Ustr,
    /// Latest index price.
    pub idx_px: String,
    /// 24-hour highest price.
    pub high24h: String,
    /// 24-hour lowest price.
    pub low24h: String,
    /// 24-hour opening price.
    pub open24h: String,
    /// The opening price of the day (UTC 0).
    pub sod_utc0: String,
    /// The opening price of the day (UTC 8).
    pub sod_utc8: String,
    /// Timestamp of the data generation, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Price limit data (upper and lower limits).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXPriceLimitMsg {
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Buy limit price.
    pub buy_lmt: String,
    /// Sell limit price.
    pub sell_lmt: String,
    /// Timestamp of the data generation, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Candlestick data for an instrument.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXCandleMsg {
    /// Candlestick timestamp, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
    /// Opening price.
    pub o: String,
    /// Highest price.
    pub h: String,
    /// Lowest price.
    pub l: String,
    /// Closing price.
    pub c: String,
    /// Trading volume in contracts.
    pub vol: String,
    /// Trading volume in quote currency.
    pub vol_ccy: String,
    pub vol_ccy_quote: String,
    /// Whether this is a completed candle.
    pub confirm: OKXCandleConfirm,
}

/// Open interest data.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXOpenInterestMsg {
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Open interest in contracts.
    pub oi: String,
    /// Open interest in quote currency.
    pub oi_ccy: String,
    /// Timestamp of the data generation, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Option market data summary.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXOptionSummaryMsg {
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Underlying.
    pub uly: String,
    /// Delta.
    pub delta: String,
    /// Gamma.
    pub gamma: String,
    /// Theta.
    pub theta: String,
    /// Vega.
    pub vega: String,
    /// Black-Scholes implied volatility delta.
    pub delta_bs: String,
    /// Black-Scholes implied volatility gamma.
    pub gamma_bs: String,
    /// Black-Scholes implied volatility theta.
    pub theta_bs: String,
    /// Black-Scholes implied volatility vega.
    pub vega_bs: String,
    /// Realized volatility.
    pub real_vol: String,
    /// Bid volatility.
    pub bid_vol: String,
    /// Ask volatility.
    pub ask_vol: String,
    /// Mark volatility.
    pub mark_vol: String,
    /// Leverage.
    pub lever: String,
    /// Timestamp of the data generation, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Estimated delivery/exercise price data.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXEstimatedPriceMsg {
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Estimated settlement price.
    pub settle_px: String,
    /// Timestamp of the data generation, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Platform status updates.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXStatusMsg {
    /// System maintenance status.
    pub title: Ustr,
    /// Status type: planned or scheduled.
    #[serde(rename = "type")]
    pub status_type: Ustr,
    /// System maintenance state: canceled, completed, pending, ongoing.
    pub state: Ustr,
    /// Expected completion timestamp.
    pub end_time: Option<String>,
    /// Planned start timestamp.
    pub begin_time: Option<String>,
    /// Service involved.
    pub service_type: Option<Ustr>,
    /// Reason for status change.
    pub reason: Option<String>,
    /// Timestamp of the data generation, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Order update message from WebSocket orders channel.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXOrderMsg {
    /// Accumulated filled size.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub acc_fill_sz: Option<String>,
    /// Average price.
    pub avg_px: String,
    /// Creation time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub c_time: u64,
    /// Cancel source.
    #[serde(default)]
    pub cancel_source: Option<String>,
    /// Cancel source reason.
    #[serde(default)]
    pub cancel_source_reason: Option<String>,
    /// Order category (normal, liquidation, ADL, etc.).
    pub category: OKXOrderCategory,
    /// Currency.
    pub ccy: Ustr,
    /// Client order ID.
    pub cl_ord_id: String,
    /// Parent algo client order ID if present.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub algo_cl_ord_id: Option<String>,
    /// Fee.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fee: Option<String>,
    /// Fee currency.
    pub fee_ccy: Ustr,
    /// Fill price.
    pub fill_px: String,
    /// Fill size.
    pub fill_sz: String,
    /// Fill time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub fill_time: u64,
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Instrument type.
    pub inst_type: OKXInstrumentType,
    /// Leverage.
    pub lever: String,
    /// Order ID.
    pub ord_id: Ustr,
    /// Order type.
    pub ord_type: OKXOrderType,
    /// Profit and loss.
    pub pnl: String,
    /// Position side.
    pub pos_side: OKXPositionSide,
    /// Price (algo orders use ordPx instead).
    #[serde(default)]
    pub px: String,
    /// Reduce only flag.
    pub reduce_only: String,
    /// Side.
    pub side: OKXSide,
    /// Order state.
    pub state: OKXOrderStatus,
    /// Execution type.
    pub exec_type: OKXExecType,
    /// Size.
    pub sz: String,
    /// Trade mode.
    pub td_mode: OKXTradeMode,
    /// Target currency (base_ccy or quote_ccy). Empty for margin modes.
    #[serde(default, deserialize_with = "deserialize_target_currency_as_none")]
    pub tgt_ccy: Option<OKXTargetCurrency>,
    /// Trade ID.
    pub trade_id: String,
    /// Last update time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub u_time: u64,
}

/// Represents an algo order message from WebSocket updates.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXAlgoOrderMsg {
    /// Algorithm ID.
    pub algo_id: String,
    /// Algorithm client order ID.
    #[serde(default)]
    pub algo_cl_ord_id: String,
    /// Client order ID (empty for algo orders until triggered).
    pub cl_ord_id: String,
    /// Order ID (empty until algo order is triggered).
    pub ord_id: String,
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Instrument type.
    pub inst_type: OKXInstrumentType,
    /// Order type (always "trigger" for conditional orders).
    pub ord_type: OKXOrderType,
    /// Order state.
    pub state: OKXOrderStatus,
    /// Side.
    pub side: OKXSide,
    /// Position side.
    pub pos_side: OKXPositionSide,
    /// Size.
    pub sz: String,
    /// Trigger price.
    pub trigger_px: String,
    /// Trigger price type (last, mark, index).
    pub trigger_px_type: OKXTriggerType,
    /// Order price (-1 for market orders).
    pub ord_px: String,
    /// Trade mode.
    pub td_mode: OKXTradeMode,
    /// Leverage.
    pub lever: String,
    /// Reduce only flag.
    pub reduce_only: String,
    /// Actual filled price.
    pub actual_px: String,
    /// Actual filled size.
    pub actual_sz: String,
    /// Notional USD value.
    pub notional_usd: String,
    /// Creation time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub c_time: u64,
    /// Update time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub u_time: u64,
    /// Trigger time (empty until triggered).
    pub trigger_time: String,
    /// Tag.
    #[serde(default)]
    pub tag: String,
}

/// Parameters for WebSocket place order operation.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsPostOrderParams {
    /// Instrument type: SPOT, MARGIN, SWAP, FUTURES, OPTION (optional for WebSocket).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inst_type: Option<OKXInstrumentType>,
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: Ustr,
    /// Trading mode: cash, isolated, cross.
    pub td_mode: OKXTradeMode,
    /// Margin currency (only for isolated margin).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ccy: Option<Ustr>,
    /// Unique client order ID.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// Order side: buy or sell.
    pub side: OKXSide,
    /// Position side: long, short, net (optional).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos_side: Option<OKXPositionSide>,
    /// Order type: limit, market, post_only, fok, ioc, etc.
    pub ord_type: OKXOrderType,
    /// Order size.
    pub sz: String,
    /// Order price (required for limit orders).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub px: Option<String>,
    /// Reduce-only flag.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    /// Target currency for net orders.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tgt_ccy: Option<OKXTargetCurrency>,
    /// Order tag for categorization.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// Parameters for WebSocket cancel order operation (instType not included).
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsCancelOrderParams {
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: Ustr,
    /// Exchange-assigned order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ord_id: Option<String>,
    /// User-assigned client order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
}

/// Parameters for WebSocket mass cancel operation.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsMassCancelParams {
    /// Instrument type.
    pub inst_type: OKXInstrumentType,
    /// Instrument family, e.g. "BTC-USD", "BTC-USDT".
    pub inst_family: Ustr,
}

/// Parameters for WebSocket amend order operation (instType not included).
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsAmendOrderParams {
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: Ustr,
    /// Exchange-assigned order ID (optional if using clOrdId).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ord_id: Option<String>,
    /// User-assigned client order ID (optional if using ordId).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// New client order ID for the amended order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_cl_ord_id: Option<String>,
    /// New order price (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_px: Option<String>,
    /// New order size (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_sz: Option<String>,
}

/// Parameters for WebSocket algo order placement.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsPostAlgoOrderParams {
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: Ustr,
    /// Trading mode: cash, isolated, cross.
    pub td_mode: OKXTradeMode,
    /// Order side: buy or sell.
    pub side: OKXSide,
    /// Order type: trigger (for stop orders).
    pub ord_type: OKXAlgoOrderType,
    /// Order size.
    pub sz: String,
    /// Client order ID (optional).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// Position side: long, short, net (optional).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos_side: Option<OKXPositionSide>,
    /// Trigger price for stop/conditional orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_px: Option<String>,
    /// Trigger price type: last, index, mark.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_px_type: Option<OKXTriggerType>,
    /// Order price (for limit orders after trigger).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_px: Option<String>,
    /// Reduce-only flag.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    /// Order tag for categorization.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// Parameters for WebSocket cancel algo order operation.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsCancelAlgoOrderParams {
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: Ustr,
    /// Algo order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algo_id: Option<String>,
    /// Client algo order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algo_cl_ord_id: Option<String>,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_core::time::get_atomic_clock_realtime;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_deserialize_websocket_arg() {
        let json_str = r#"{"channel":"instruments","instType":"SPOT"}"#;

        let result: Result<OKXWebSocketArg, _> = serde_json::from_str(json_str);
        match result {
            Ok(arg) => {
                assert_eq!(arg.channel, OKXWsChannel::Instruments);
                assert_eq!(arg.inst_type, Some(OKXInstrumentType::Spot));
                assert_eq!(arg.inst_id, None);
            }
            Err(e) => {
                panic!("Failed to deserialize WebSocket arg: {e}");
            }
        }
    }

    #[rstest]
    fn test_deserialize_subscribe_variant_direct() {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SubscribeMsg {
            event: String,
            arg: OKXWebSocketArg,
            conn_id: String,
        }

        let json_str = r#"{"event":"subscribe","arg":{"channel":"instruments","instType":"SPOT"},"connId":"380cfa6a"}"#;

        let result: Result<SubscribeMsg, _> = serde_json::from_str(json_str);
        match result {
            Ok(msg) => {
                assert_eq!(msg.event, "subscribe");
                assert_eq!(msg.arg.channel, OKXWsChannel::Instruments);
                assert_eq!(msg.conn_id, "380cfa6a");
            }
            Err(e) => {
                panic!("Failed to deserialize subscribe message directly: {e}");
            }
        }
    }

    #[rstest]
    fn test_deserialize_subscribe_confirmation() {
        let json_str = r#"{"event":"subscribe","arg":{"channel":"instruments","instType":"SPOT"},"connId":"380cfa6a"}"#;

        let result: Result<OKXWebSocketEvent, _> = serde_json::from_str(json_str);
        match result {
            Ok(msg) => {
                if let OKXWebSocketEvent::Subscription {
                    event,
                    arg,
                    conn_id,
                    ..
                } = msg
                {
                    assert_eq!(event, OKXSubscriptionEvent::Subscribe);
                    assert_eq!(arg.channel, OKXWsChannel::Instruments);
                    assert_eq!(conn_id, "380cfa6a");
                } else {
                    panic!("Expected Subscribe variant, was: {msg:?}");
                }
            }
            Err(e) => {
                panic!("Failed to deserialize subscription confirmation: {e}");
            }
        }
    }

    #[rstest]
    fn test_deserialize_subscribe_with_inst_id() {
        let json_str = r#"{"event":"subscribe","arg":{"channel":"candle1m","instId":"ETH-USDT"},"connId":"358602f5"}"#;

        let result: Result<OKXWebSocketEvent, _> = serde_json::from_str(json_str);
        match result {
            Ok(msg) => {
                if let OKXWebSocketEvent::Subscription {
                    event,
                    arg,
                    conn_id,
                    ..
                } = msg
                {
                    assert_eq!(event, OKXSubscriptionEvent::Subscribe);
                    assert_eq!(arg.channel, OKXWsChannel::Candle1Minute);
                    assert_eq!(conn_id, "358602f5");
                } else {
                    panic!("Expected Subscribe variant, was: {msg:?}");
                }
            }
            Err(e) => {
                panic!("Failed to deserialize subscription confirmation: {e}");
            }
        }
    }

    #[rstest]
    fn test_channel_serialization_for_logging() {
        let channel = OKXWsChannel::Candle1Minute;
        let serialized = serde_json::to_string(&channel).unwrap();
        let cleaned = serialized.trim_matches('"').to_string();
        assert_eq!(cleaned, "candle1m");

        let channel = OKXWsChannel::BboTbt;
        let serialized = serde_json::to_string(&channel).unwrap();
        let cleaned = serialized.trim_matches('"').to_string();
        assert_eq!(cleaned, "bbo-tbt");

        let channel = OKXWsChannel::Trades;
        let serialized = serde_json::to_string(&channel).unwrap();
        let cleaned = serialized.trim_matches('"').to_string();
        assert_eq!(cleaned, "trades");
    }

    #[rstest]
    fn test_order_response_with_enum_operation() {
        let json_str = r#"{"id":"req-123","op":"order","code":"0","msg":"","data":[]}"#;
        let result: Result<OKXWebSocketEvent, _> = serde_json::from_str(json_str);
        match result {
            Ok(OKXWebSocketEvent::OrderResponse {
                id,
                op,
                code,
                msg,
                data,
            }) => {
                assert_eq!(id, Some("req-123".to_string()));
                assert_eq!(op, OKXWsOperation::Order);
                assert_eq!(code, "0");
                assert_eq!(msg, "");
                assert!(data.is_empty());
            }
            Ok(other) => panic!("Expected OrderResponse, was: {other:?}"),
            Err(e) => panic!("Failed to deserialize: {e}"),
        }

        let json_str = r#"{"id":"cancel-456","op":"cancel-order","code":"50001","msg":"Order not found","data":[]}"#;
        let result: Result<OKXWebSocketEvent, _> = serde_json::from_str(json_str);
        match result {
            Ok(OKXWebSocketEvent::OrderResponse {
                id,
                op,
                code,
                msg,
                data,
            }) => {
                assert_eq!(id, Some("cancel-456".to_string()));
                assert_eq!(op, OKXWsOperation::CancelOrder);
                assert_eq!(code, "50001");
                assert_eq!(msg, "Order not found");
                assert!(data.is_empty());
            }
            Ok(other) => panic!("Expected OrderResponse, was: {other:?}"),
            Err(e) => panic!("Failed to deserialize: {e}"),
        }

        let json_str = r#"{"id":"amend-789","op":"amend-order","code":"50002","msg":"Invalid price","data":[]}"#;
        let result: Result<OKXWebSocketEvent, _> = serde_json::from_str(json_str);
        match result {
            Ok(OKXWebSocketEvent::OrderResponse {
                id,
                op,
                code,
                msg,
                data,
            }) => {
                assert_eq!(id, Some("amend-789".to_string()));
                assert_eq!(op, OKXWsOperation::AmendOrder);
                assert_eq!(code, "50002");
                assert_eq!(msg, "Invalid price");
                assert!(data.is_empty());
            }
            Ok(other) => panic!("Expected OrderResponse, was: {other:?}"),
            Err(e) => panic!("Failed to deserialize: {e}"),
        }
    }

    #[rstest]
    fn test_operation_enum_serialization() {
        let op = OKXWsOperation::Order;
        let serialized = serde_json::to_string(&op).unwrap();
        assert_eq!(serialized, "\"order\"");

        let op = OKXWsOperation::CancelOrder;
        let serialized = serde_json::to_string(&op).unwrap();
        assert_eq!(serialized, "\"cancel-order\"");

        let op = OKXWsOperation::AmendOrder;
        let serialized = serde_json::to_string(&op).unwrap();
        assert_eq!(serialized, "\"amend-order\"");

        let op = OKXWsOperation::Subscribe;
        let serialized = serde_json::to_string(&op).unwrap();
        assert_eq!(serialized, "\"subscribe\"");
    }

    #[rstest]
    fn test_order_response_parsing() {
        let success_response = r#"{
            "id": "req-123",
            "op": "order",
            "code": "0",
            "msg": "",
            "data": [{"sMsg": "Order placed successfully"}]
        }"#;

        let parsed: OKXWebSocketEvent = serde_json::from_str(success_response).unwrap();

        match parsed {
            OKXWebSocketEvent::OrderResponse {
                id,
                op,
                code,
                msg,
                data,
            } => {
                assert_eq!(id, Some("req-123".to_string()));
                assert_eq!(op, OKXWsOperation::Order);
                assert_eq!(code, "0");
                assert_eq!(msg, "");
                assert_eq!(data.len(), 1);
            }
            _ => panic!("Expected OrderResponse variant"),
        }

        let failure_response = r#"{
            "id": "req-456",
            "op": "cancel-order",
            "code": "50001",
            "msg": "Order not found",
            "data": [{"sMsg": "Order with client order ID not found"}]
        }"#;

        let parsed: OKXWebSocketEvent = serde_json::from_str(failure_response).unwrap();

        match parsed {
            OKXWebSocketEvent::OrderResponse {
                id,
                op,
                code,
                msg,
                data,
            } => {
                assert_eq!(id, Some("req-456".to_string()));
                assert_eq!(op, OKXWsOperation::CancelOrder);
                assert_eq!(code, "50001");
                assert_eq!(msg, "Order not found");
                assert_eq!(data.len(), 1);
            }
            _ => panic!("Expected OrderResponse variant"),
        }
    }

    #[rstest]
    fn test_subscription_event_parsing() {
        let subscription_json = r#"{
            "event": "subscribe",
            "arg": {
                "channel": "tickers",
                "instId": "BTC-USDT"
            },
            "connId": "a4d3ae55"
        }"#;

        let parsed: OKXWebSocketEvent = serde_json::from_str(subscription_json).unwrap();

        match parsed {
            OKXWebSocketEvent::Subscription {
                event,
                arg,
                conn_id,
                ..
            } => {
                assert_eq!(
                    event,
                    crate::websocket::enums::OKXSubscriptionEvent::Subscribe
                );
                assert_eq!(arg.channel, OKXWsChannel::Tickers);
                assert_eq!(arg.inst_id, Some(Ustr::from("BTC-USDT")));
                assert_eq!(conn_id, "a4d3ae55");
            }
            _ => panic!("Expected Subscription variant"),
        }
    }

    #[rstest]
    fn test_login_event_parsing() {
        let login_success = r#"{
            "event": "login",
            "code": "0",
            "msg": "Login successful",
            "connId": "a4d3ae55"
        }"#;

        let parsed: OKXWebSocketEvent = serde_json::from_str(login_success).unwrap();

        match parsed {
            OKXWebSocketEvent::Login {
                event,
                code,
                msg,
                conn_id,
            } => {
                assert_eq!(event, "login");
                assert_eq!(code, "0");
                assert_eq!(msg, "Login successful");
                assert_eq!(conn_id, "a4d3ae55");
            }
            _ => panic!("Expected Login variant, was: {:?}", parsed),
        }
    }

    #[rstest]
    fn test_error_event_parsing() {
        let error_json = r#"{
            "code": "60012",
            "msg": "Invalid request"
        }"#;

        let parsed: OKXWebSocketEvent = serde_json::from_str(error_json).unwrap();

        match parsed {
            OKXWebSocketEvent::Error { code, msg } => {
                assert_eq!(code, "60012");
                assert_eq!(msg, "Invalid request");
            }
            _ => panic!("Expected Error variant"),
        }
    }

    #[rstest]
    fn test_websocket_request_serialization() {
        let request = OKXWsRequest {
            id: Some("req-123".to_string()),
            op: OKXWsOperation::Order,
            args: vec![serde_json::json!({
                "instId": "BTC-USDT",
                "tdMode": "cash",
                "side": "buy",
                "ordType": "market",
                "sz": "0.1"
            })],
            exp_time: None,
        };

        let serialized = serde_json::to_string(&request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();

        assert_eq!(parsed["id"], "req-123");
        assert_eq!(parsed["op"], "order");
        assert!(parsed["args"].is_array());
        assert_eq!(parsed["args"].as_array().unwrap().len(), 1);
    }

    #[rstest]
    fn test_subscription_request_serialization() {
        let subscription = OKXSubscription {
            op: OKXWsOperation::Subscribe,
            args: vec![OKXSubscriptionArg {
                channel: OKXWsChannel::Tickers,
                inst_type: Some(crate::common::enums::OKXInstrumentType::Spot),
                inst_family: None,
                inst_id: Some(Ustr::from("BTC-USDT")),
            }],
        };

        let serialized = serde_json::to_string(&subscription).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();

        assert_eq!(parsed["op"], "subscribe");
        assert!(parsed["args"].is_array());
        assert_eq!(parsed["args"][0]["channel"], "tickers");
        assert_eq!(parsed["args"][0]["instType"], "SPOT");
        assert_eq!(parsed["args"][0]["instId"], "BTC-USDT");
    }

    #[rstest]
    fn test_error_message_extraction() {
        let responses = vec![
            (
                r#"{
                "id": "req-123",
                "op": "order",
                "code": "50001",
                "msg": "Order failed",
                "data": [{"sMsg": "Insufficient balance"}]
            }"#,
                "Insufficient balance",
            ),
            (
                r#"{
                "id": "req-456",
                "op": "cancel-order",
                "code": "50002",
                "msg": "Cancel failed",
                "data": [{}]
            }"#,
                "Cancel failed",
            ),
        ];

        for (response_json, expected_msg) in responses {
            let parsed: OKXWebSocketEvent = serde_json::from_str(response_json).unwrap();

            match parsed {
                OKXWebSocketEvent::OrderResponse {
                    id: _,
                    op: _,
                    code,
                    msg,
                    data,
                } => {
                    assert_ne!(code, "0"); // Error response

                    // Extract error message with fallback logic
                    let error_msg = data
                        .first()
                        .and_then(|d| d.get("sMsg"))
                        .and_then(|s| s.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or(&msg);

                    assert_eq!(error_msg, expected_msg);
                }
                _ => panic!("Expected OrderResponse variant"),
            }
        }
    }

    #[rstest]
    fn test_book_data_parsing() {
        let book_data_json = r#"{
            "arg": {
                "channel": "books",
                "instId": "BTC-USDT"
            },
            "action": "snapshot",
            "data": [{
                "asks": [["50000.0", "0.1", "0", "1"]],
                "bids": [["49999.0", "0.2", "0", "1"]],
                "ts": "1640995200000",
                "checksum": 123456789,
                "seqId": 1000
            }]
        }"#;

        let parsed: OKXWebSocketEvent = serde_json::from_str(book_data_json).unwrap();

        match parsed {
            OKXWebSocketEvent::BookData { arg, action, data } => {
                assert_eq!(arg.channel, OKXWsChannel::Books);
                assert_eq!(arg.inst_id, Some(Ustr::from("BTC-USDT")));
                assert_eq!(
                    action,
                    super::super::super::common::enums::OKXBookAction::Snapshot
                );
                assert_eq!(data.len(), 1);
            }
            _ => panic!("Expected BookData variant"),
        }
    }

    #[rstest]
    fn test_data_event_parsing() {
        let data_json = r#"{
            "arg": {
                "channel": "trades",
                "instId": "BTC-USDT"
            },
            "data": [{
                "instId": "BTC-USDT",
                "tradeId": "12345",
                "px": "50000.0",
                "sz": "0.1",
                "side": "buy",
                "ts": "1640995200000"
            }]
        }"#;

        let parsed: OKXWebSocketEvent = serde_json::from_str(data_json).unwrap();

        match parsed {
            OKXWebSocketEvent::Data { arg, data } => {
                assert_eq!(arg.channel, OKXWsChannel::Trades);
                assert_eq!(arg.inst_id, Some(Ustr::from("BTC-USDT")));
                assert!(data.is_array());
            }
            _ => panic!("Expected Data variant"),
        }
    }

    #[rstest]
    fn test_nautilus_message_variants() {
        let clock = get_atomic_clock_realtime();
        let ts_init = clock.get_time_ns();

        let error = OKXWebSocketError {
            code: "60012".to_string(),
            message: "Invalid request".to_string(),
            conn_id: None,
            timestamp: ts_init.as_u64(),
        };
        let error_msg = NautilusWsMessage::Error(error);

        match error_msg {
            NautilusWsMessage::Error(e) => {
                assert_eq!(e.code, "60012");
                assert_eq!(e.message, "Invalid request");
            }
            _ => panic!("Expected Error variant"),
        }

        let raw_scenarios = vec![
            ::serde_json::json!({"unknown": "data"}),
            ::serde_json::json!({"channel": "unsupported", "data": [1, 2, 3]}),
            ::serde_json::json!({"complex": {"nested": {"structure": true}}}),
        ];

        for raw_data in raw_scenarios {
            let raw_msg = NautilusWsMessage::Raw(raw_data.clone());

            match raw_msg {
                NautilusWsMessage::Raw(data) => {
                    assert_eq!(data, raw_data);
                }
                _ => panic!("Expected Raw variant"),
            }
        }
    }

    #[rstest]
    fn test_order_response_parsing_success() {
        let order_response_json = r#"{
            "id": "req-123",
            "op": "order",
            "code": "0",
            "msg": "",
            "data": [{"sMsg": "Order placed successfully"}]
        }"#;

        let parsed: OKXWebSocketEvent = serde_json::from_str(order_response_json).unwrap();

        match parsed {
            OKXWebSocketEvent::OrderResponse {
                id,
                op,
                code,
                msg,
                data,
            } => {
                assert_eq!(id, Some("req-123".to_string()));
                assert_eq!(op, OKXWsOperation::Order);
                assert_eq!(code, "0");
                assert_eq!(msg, "");
                assert_eq!(data.len(), 1);
            }
            _ => panic!("Expected OrderResponse variant"),
        }
    }

    #[rstest]
    fn test_order_response_parsing_failure() {
        let order_response_json = r#"{
            "id": "req-456",
            "op": "cancel-order",
            "code": "50001",
            "msg": "Order not found",
            "data": [{"sMsg": "Order with client order ID not found"}]
        }"#;

        let parsed: OKXWebSocketEvent = serde_json::from_str(order_response_json).unwrap();

        match parsed {
            OKXWebSocketEvent::OrderResponse {
                id,
                op,
                code,
                msg,
                data,
            } => {
                assert_eq!(id, Some("req-456".to_string()));
                assert_eq!(op, OKXWsOperation::CancelOrder);
                assert_eq!(code, "50001");
                assert_eq!(msg, "Order not found");
                assert_eq!(data.len(), 1);
            }
            _ => panic!("Expected OrderResponse variant"),
        }
    }

    #[rstest]
    fn test_message_request_serialization() {
        let request = OKXWsRequest {
            id: Some("req-123".to_string()),
            op: OKXWsOperation::Order,
            args: vec![::serde_json::json!({
                "instId": "BTC-USDT",
                "tdMode": "cash",
                "side": "buy",
                "ordType": "market",
                "sz": "0.1"
            })],
            exp_time: None,
        };

        let serialized = serde_json::to_string(&request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();

        assert_eq!(parsed["id"], "req-123");
        assert_eq!(parsed["op"], "order");
        assert!(parsed["args"].is_array());
        assert_eq!(parsed["args"].as_array().unwrap().len(), 1);
    }
}
