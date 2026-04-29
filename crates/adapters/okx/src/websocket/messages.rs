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

//! Data structures modelling OKX WebSocket request and response payloads.

use derive_builder::Builder;
use nautilus_model::{
    data::{Data, FundingRateUpdate, InstrumentStatus, OrderBookDeltas},
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderExpired,
        OrderModifyRejected, OrderRejected, OrderTriggered, OrderUpdated,
    },
    identifiers::ClientOrderId,
    instruments::InstrumentAny,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::enums::{OKXWsChannel, OKXWsOperation};
use crate::{
    common::{
        enums::{
            OKXAlgoOrderType, OKXBookAction, OKXCandleConfirm, OKXExecType, OKXInstrumentType,
            OKXOrderCategory, OKXOrderStatus, OKXOrderType, OKXPositionSide, OKXPriceType,
            OKXQuickMarginType, OKXSelfTradePreventionMode, OKXSettlementState, OKXSide,
            OKXTargetCurrency, OKXTradeMode, OKXTriggerType,
        },
        models::OKXInstrument,
        parse::{
            deserialize_empty_string_as_none, deserialize_empty_ustr_as_none,
            deserialize_string_to_u64, deserialize_target_currency_as_none,
        },
    },
    websocket::enums::OKXSubscriptionEvent,
};

#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    Data(Vec<Data>),
    Deltas(OrderBookDeltas),
    FundingRates(Vec<FundingRateUpdate>),
    Instrument(Box<InstrumentAny>, Option<InstrumentStatus>),
    InstrumentStatus(InstrumentStatus),
    AccountUpdate(AccountState),
    PositionUpdate(PositionStatusReport),
    OrderAccepted(OrderAccepted),
    OrderCanceled(OrderCanceled),
    OrderExpired(OrderExpired),
    OrderRejected(OrderRejected),
    OrderCancelRejected(OrderCancelRejected),
    OrderModifyRejected(OrderModifyRejected),
    OrderTriggered(OrderTriggered),
    OrderUpdated(OrderUpdated),
    ExecutionReports(Vec<ExecutionReport>),
    Error(OKXWebSocketError),
    Raw(serde_json::Value), // Unhandled channels
    Reconnected,
    Authenticated,
}

/// Represents an OKX WebSocket error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python", pyo3::pyclass(from_py_object))]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.okx")
)]
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
#[expect(clippy::large_enum_variant)]
pub enum ExecutionReport {
    Order(OrderStatusReport),
    Fill(FillReport),
}

/// Output from the OKX WebSocket handler.
///
/// Contains venue-specific types only. Data parsing occurs in `PyOKXWebSocketClient`
/// (using an instruments cache), and execution parsing occurs in `execution.rs`
/// (using the system Cache for order lookups).
#[derive(Debug)]
pub enum OKXWsMessage {
    /// Order book snapshot or update.
    BookData {
        arg: OKXWebSocketArg,
        action: OKXBookAction,
        data: Vec<OKXBookMsg>,
    },
    /// Data from a non-book channel (trades, tickers, mark price, funding, candles, etc.).
    ChannelData {
        channel: OKXWsChannel,
        inst_id: Option<Ustr>,
        data: serde_json::Value,
    },
    /// Response to a WebSocket order command (place, cancel, amend, mass-cancel).
    OrderResponse {
        id: Option<String>,
        op: OKXWsOperation,
        code: String,
        msg: String,
        data: Vec<serde_json::Value>,
    },
    /// Order push channel updates.
    Orders(Vec<OKXOrderMsg>),
    /// Algo order push channel updates.
    AlgoOrders(Vec<OKXAlgoOrderMsg>),
    /// Account channel update (raw JSON).
    Account(serde_json::Value),
    /// Positions channel update (raw JSON).
    Positions(serde_json::Value),
    /// Instrument definition updates.
    Instruments(Vec<OKXInstrument>),
    /// A WebSocket send failed; carries context for emitting the appropriate rejection event.
    SendFailed {
        request_id: String,
        client_order_id: Option<ClientOrderId>,
        op: Option<OKXWsOperation>,
        error: String,
    },
    /// Error received from OKX.
    Error(OKXWebSocketError),
    /// WebSocket reconnected.
    Reconnected,
    /// WebSocket authenticated.
    Authenticated,
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXSubscriptionArg {
    pub channel: OKXWsChannel,
    pub inst_type: Option<OKXInstrumentType>,
    pub inst_family: Option<Ustr>,
    pub inst_id: Option<Ustr>,
}

/// OKX WebSocket message variants.
///
/// Uses custom deserialization that checks discriminant fields (event, op, action)
/// to determine the correct variant.
#[derive(Debug)]
pub enum OKXWsFrame {
    Login {
        event: String,
        code: String,
        msg: String,
        conn_id: String,
    },
    Subscription {
        event: OKXSubscriptionEvent,
        arg: OKXWebSocketArg,
        conn_id: String,
        code: Option<String>,
        msg: Option<String>,
    },
    ChannelConnCount {
        event: String,
        channel: OKXWsChannel,
        conn_count: String,
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
    Ping,
    Reconnected,
}

impl<'de> Deserialize<'de> for OKXWsFrame {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        // Deserialize to a map to inspect discriminant fields first
        let value = serde_json::Value::deserialize(deserializer)?;
        let obj = value
            .as_object()
            .ok_or_else(|| D::Error::custom("expected JSON object for OKXWsFrame"))?;

        // Check discriminant fields in priority order

        // 1. Check for "event" field - Login, Subscription, ChannelConnCount, or Error
        if let Some(event) = obj.get("event").and_then(|v| v.as_str()) {
            if event == "login" {
                return parse_login(obj);
            } else if event == "subscribe" || event == "unsubscribe" {
                return parse_subscription(obj);
            } else if event == "error" {
                // All error events (simple or subscription-related) go to parse_error
                // Extra fields like "arg" and "connId" are ignored
                return parse_error(obj);
            } else if obj.contains_key("channel") && obj.contains_key("connCount") {
                return parse_channel_conn_count(obj);
            }
        }

        // 2. Check for "op" field - OrderResponse
        if obj.contains_key("op") {
            return parse_order_response(obj);
        }

        // 3. Check for "action" field with "arg" - BookData
        if obj.contains_key("action") && obj.contains_key("arg") {
            return parse_book_data(obj);
        }

        // 4. Check for "arg" and "data" without "action" - Data
        if obj.contains_key("arg") && obj.contains_key("data") {
            return parse_data(obj);
        }

        // 5. Fallback to Error if it has "code" and "msg"
        if obj.contains_key("code") && obj.contains_key("msg") {
            return parse_error(obj);
        }

        Err(D::Error::custom(format!(
            "cannot determine OKXWsFrame variant from: {}",
            serde_json::to_string(&value).unwrap_or_default()
        )))
    }
}

fn parse_login<E: serde::de::Error>(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<OKXWsFrame, E> {
    Ok(OKXWsFrame::Login {
        event: obj
            .get("event")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("event"))?,
        code: obj
            .get("code")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("code"))?,
        msg: obj
            .get("msg")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("msg"))?,
        conn_id: obj
            .get("connId")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("connId"))?,
    })
}

fn parse_subscription<E: serde::de::Error>(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<OKXWsFrame, E> {
    let event_str = obj
        .get("event")
        .and_then(|v| v.as_str())
        .ok_or_else(|| E::missing_field("event"))?;

    let event: OKXSubscriptionEvent =
        serde_json::from_value(serde_json::Value::String(event_str.to_string()))
            .map_err(|e| E::custom(format!("invalid event: {e}")))?;

    let arg: OKXWebSocketArg = obj
        .get("arg")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| E::custom(format!("invalid arg: {e}")))?
        .ok_or_else(|| E::missing_field("arg"))?;

    Ok(OKXWsFrame::Subscription {
        event,
        arg,
        conn_id: obj
            .get("connId")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("connId"))?,
        code: obj.get("code").and_then(|v| v.as_str()).map(String::from),
        msg: obj.get("msg").and_then(|v| v.as_str()).map(String::from),
    })
}

fn parse_channel_conn_count<E: serde::de::Error>(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<OKXWsFrame, E> {
    let channel: OKXWsChannel = obj
        .get("channel")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| E::custom(format!("invalid channel: {e}")))?
        .ok_or_else(|| E::missing_field("channel"))?;

    Ok(OKXWsFrame::ChannelConnCount {
        event: obj
            .get("event")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("event"))?,
        channel,
        conn_count: obj
            .get("connCount")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("connCount"))?,
        conn_id: obj
            .get("connId")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("connId"))?,
    })
}

fn parse_order_response<E: serde::de::Error>(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<OKXWsFrame, E> {
    let op: OKXWsOperation = obj
        .get("op")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| E::custom(format!("invalid op: {e}")))?
        .ok_or_else(|| E::missing_field("op"))?;

    let data: Vec<serde_json::Value> = obj
        .get("data")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| E::custom(format!("invalid data: {e}")))?
        .unwrap_or_default();

    Ok(OKXWsFrame::OrderResponse {
        id: obj.get("id").and_then(|v| v.as_str()).map(String::from),
        op,
        code: obj
            .get("code")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("code"))?,
        msg: obj
            .get("msg")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("msg"))?,
        data,
    })
}

fn parse_book_data<E: serde::de::Error>(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<OKXWsFrame, E> {
    let arg: OKXWebSocketArg = obj
        .get("arg")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| E::custom(format!("invalid arg: {e}")))?
        .ok_or_else(|| E::missing_field("arg"))?;

    let action: OKXBookAction = obj
        .get("action")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| E::custom(format!("invalid action: {e}")))?
        .ok_or_else(|| E::missing_field("action"))?;

    let data: Vec<OKXBookMsg> = obj
        .get("data")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| E::custom(format!("invalid data: {e}")))?
        .ok_or_else(|| E::missing_field("data"))?;

    Ok(OKXWsFrame::BookData { arg, action, data })
}

fn parse_data<E: serde::de::Error>(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<OKXWsFrame, E> {
    let arg: OKXWebSocketArg = obj
        .get("arg")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| E::custom(format!("invalid arg: {e}")))?
        .ok_or_else(|| E::missing_field("arg"))?;

    let data = obj
        .get("data")
        .cloned()
        .ok_or_else(|| E::missing_field("data"))?;

    Ok(OKXWsFrame::Data { arg, data })
}

fn parse_error<E: serde::de::Error>(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<OKXWsFrame, E> {
    Ok(OKXWsFrame::Error {
        code: obj
            .get("code")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("code"))?,
        msg: obj
            .get("msg")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| E::missing_field("msg"))?,
    })
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
    /// Order source for ELP liquidity identification.
    #[serde(default)]
    pub source: Option<String>,
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
    /// Order source (0: normal, 1: ELP).
    #[serde(default)]
    pub source: Option<String>,
    /// Sequence ID for trade events.
    #[serde(default)]
    pub seq_id: Option<u64>,
}

/// Funding rate data for perpetual swaps.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXFundingRateMsg {
    /// Instrument type.
    #[serde(default)]
    pub inst_type: Option<OKXInstrumentType>,
    /// Instrument ID.
    pub inst_id: Ustr,
    /// Current funding rate.
    pub funding_rate: Ustr,
    /// Predicted next funding rate.
    pub next_funding_rate: Ustr,
    /// Minimum funding rate.
    #[serde(default)]
    pub min_funding_rate: Option<String>,
    /// Maximum funding rate.
    #[serde(default)]
    pub max_funding_rate: Option<String>,
    /// Settlement state.
    #[serde(default)]
    pub sett_state: OKXSettlementState,
    /// Settlement funding rate.
    #[serde(default)]
    pub sett_funding_rate: Option<String>,
    /// Current premium.
    #[serde(default)]
    pub premium: Option<String>,
    /// Funding rate calculation method.
    #[serde(default)]
    pub method: Option<String>,
    /// Funding time, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub funding_time: u64,
    /// Next funding time, Unix timestamp format in milliseconds (used to determine funding interval).
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub next_funding_time: u64,
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
    /// Instrument type.
    #[serde(default)]
    pub inst_type: Option<OKXInstrumentType>,
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
    /// Black-Scholes delta.
    #[serde(alias = "deltaBS")]
    pub delta_bs: String,
    /// Black-Scholes gamma.
    #[serde(alias = "gammaBS")]
    pub gamma_bs: String,
    /// Black-Scholes theta.
    #[serde(alias = "thetaBS")]
    pub theta_bs: String,
    /// Black-Scholes vega.
    #[serde(alias = "vegaBS")]
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
    /// Forward price.
    #[serde(default)]
    pub fwd_px: Option<String>,
    /// Mark price.
    #[serde(default)]
    pub mark_px: Option<String>,
    /// Volatility level.
    #[serde(default)]
    pub vol_lv: Option<String>,
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

pub use crate::common::models::OKXAttachedAlgoOrd;

/// Linked algo order metadata from order push updates.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXLinkedAlgoOrd {
    /// Parent algo order ID.
    #[serde(default)]
    pub algo_id: String,
}

/// Order update message from WebSocket orders channel.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXOrderMsg {
    /// Accumulated filled size.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub acc_fill_sz: Option<String>,
    /// Algo order ID.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub algo_id: Option<String>,
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
    /// Attached child client order ID if surfaced at the top level.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub attach_algo_cl_ord_id: Option<String>,
    /// Attached TP/SL child order metadata.
    #[serde(default)]
    pub attach_algo_ords: Vec<OKXAttachedAlgoOrd>,
    /// Fee (cumulative).
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fee: Option<String>,
    /// Fee currency.
    pub fee_ccy: Ustr,
    /// Fee for this fill.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fill_fee: Option<String>,
    /// Fill fee currency.
    #[serde(default, deserialize_with = "deserialize_empty_ustr_as_none")]
    pub fill_fee_ccy: Option<Ustr>,
    /// Mark price at fill time.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fill_mark_px: Option<String>,
    /// Mark volatility at fill time (options).
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fill_mark_vol: Option<String>,
    /// Implied volatility at fill time (options).
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fill_px_vol: Option<String>,
    /// Fill price in USD (options).
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fill_px_usd: Option<String>,
    /// Forward price at fill time (options).
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fill_fwd_px: Option<String>,
    /// Fill notional in USD.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fill_notional_usd: Option<String>,
    /// PnL for this fill.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fill_pnl: Option<String>,
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
    /// Whether the TP order is a limit order.
    #[serde(default)]
    pub is_tp_limit: Option<String>,
    /// Leverage.
    pub lever: String,
    /// Linked algo order metadata.
    #[serde(default)]
    pub linked_algo_ord: Option<OKXLinkedAlgoOrd>,
    /// Notional value in USD.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub notional_usd: Option<String>,
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
    /// Price type (options).
    #[serde(default)]
    pub px_type: OKXPriceType,
    /// Price in USD (options).
    #[serde(default)]
    pub px_usd: Option<String>,
    /// Price in volatility (options).
    #[serde(default)]
    pub px_vol: Option<String>,
    /// Quick margin type.
    #[serde(default)]
    pub quick_mgn_type: OKXQuickMarginType,
    /// Rebate amount.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub rebate: Option<String>,
    /// Rebate currency.
    #[serde(default, deserialize_with = "deserialize_empty_ustr_as_none")]
    pub rebate_ccy: Option<Ustr>,
    /// Reduce only flag.
    pub reduce_only: String,
    /// Side.
    pub side: OKXSide,
    /// Stop-loss order price.
    #[serde(default)]
    pub sl_ord_px: Option<String>,
    /// Stop-loss trigger price.
    #[serde(default)]
    pub sl_trigger_px: Option<String>,
    /// Stop-loss trigger price type (last, mark, index).
    #[serde(default)]
    pub sl_trigger_px_type: Option<OKXTriggerType>,
    /// Order source.
    #[serde(default)]
    pub source: Option<String>,
    /// Order state.
    pub state: OKXOrderStatus,
    /// Self-trade prevention ID.
    #[serde(default)]
    pub stp_id: Option<String>,
    /// Self-trade prevention mode.
    #[serde(default)]
    pub stp_mode: OKXSelfTradePreventionMode,
    /// Execution type.
    pub exec_type: OKXExecType,
    /// Size.
    pub sz: String,
    /// Order tag.
    #[serde(default)]
    pub tag: Option<String>,
    /// Trade mode.
    pub td_mode: OKXTradeMode,
    /// Target currency (base_ccy or quote_ccy). Empty for margin modes.
    #[serde(default, deserialize_with = "deserialize_target_currency_as_none")]
    pub tgt_ccy: Option<OKXTargetCurrency>,
    /// Take-profit order price.
    #[serde(default)]
    pub tp_ord_px: Option<String>,
    /// Take-profit trigger price.
    #[serde(default)]
    pub tp_trigger_px: Option<String>,
    /// Take-profit trigger price type (last, mark, index).
    #[serde(default)]
    pub tp_trigger_px_type: Option<OKXTriggerType>,
    /// Trade ID.
    pub trade_id: String,
    /// Last update time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub u_time: u64,
    /// Amend result code.
    #[serde(default)]
    pub amend_result: Option<String>,
    /// Request ID (for amend responses).
    #[serde(default)]
    pub req_id: Option<String>,
    /// Error code.
    #[serde(default)]
    pub code: Option<String>,
    /// Error message.
    #[serde(default)]
    pub msg: Option<String>,
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
    /// Algo order type (trigger, move_order_stop, oco, iceberg, twap).
    pub ord_type: OKXAlgoOrderType,
    /// Order state.
    pub state: OKXOrderStatus,
    /// Side.
    pub side: OKXSide,
    /// Position side.
    pub pos_side: OKXPositionSide,
    /// Size.
    #[serde(default)]
    pub sz: String,
    /// Trigger price.
    #[serde(default)]
    pub trigger_px: String,
    /// Trigger price type (last, mark, index).
    #[serde(default)]
    pub trigger_px_type: OKXTriggerType,
    /// Stop-loss trigger price for conditional close orders.
    #[serde(default)]
    pub sl_trigger_px: String,
    /// Stop-loss order price for conditional close orders.
    #[serde(default)]
    pub sl_ord_px: String,
    /// Stop-loss trigger price type (last, mark, index).
    #[serde(default)]
    pub sl_trigger_px_type: OKXTriggerType,
    /// Take-profit trigger price for conditional close orders.
    #[serde(default)]
    pub tp_trigger_px: String,
    /// Take-profit order price for conditional close orders.
    #[serde(default)]
    pub tp_ord_px: String,
    /// Take-profit trigger price type (last, mark, index).
    #[serde(default)]
    pub tp_trigger_px_type: OKXTriggerType,
    /// Order price (-1 for market orders).
    #[serde(default)]
    pub ord_px: String,
    /// Trade mode.
    pub td_mode: OKXTradeMode,
    /// Leverage.
    pub lever: String,
    /// Reduce only flag.
    #[serde(default)]
    pub reduce_only: String,
    /// Fraction of the position to close for close-order algos.
    #[serde(default)]
    pub close_fraction: String,
    /// Actual filled price.
    #[serde(default)]
    pub actual_px: String,
    /// Actual filled size.
    #[serde(default)]
    pub actual_sz: String,
    /// Notional USD value.
    #[serde(default)]
    pub notional_usd: String,
    /// Creation time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub c_time: u64,
    /// Update time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub u_time: u64,
    /// Trigger time (empty until triggered).
    #[serde(default)]
    pub trigger_time: String,
    /// Tag.
    #[serde(default)]
    pub tag: String,
    /// Callback price ratio for trailing stop (e.g. "0.01" for 1%).
    #[serde(default)]
    pub callback_ratio: String,
    /// Callback price spread for trailing stop (absolute distance).
    #[serde(default)]
    pub callback_spread: String,
    /// Activation price for trailing stop.
    #[serde(default)]
    pub active_px: String,
    /// Currency.
    #[serde(default, deserialize_with = "deserialize_empty_ustr_as_none")]
    pub ccy: Option<Ustr>,
    /// Target currency (base_ccy or quote_ccy).
    #[serde(default, deserialize_with = "deserialize_target_currency_as_none")]
    pub tgt_ccy: Option<OKXTargetCurrency>,
    /// Fee amount.
    #[serde(default)]
    pub fee: Option<String>,
    /// Fee currency.
    #[serde(default, deserialize_with = "deserialize_empty_ustr_as_none")]
    pub fee_ccy: Option<Ustr>,
    /// Trigger order type (fok, ioc).
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub advance_ord_type: Option<String>,
}

/// Parameters for WebSocket place order operation.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsAttachAlgoOrdParams {
    /// Attached algo client order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attach_algo_cl_ord_id: Option<String>,
    /// Stop-loss trigger price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_trigger_px: Option<String>,
    /// Stop-loss order price (`-1` for market).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_ord_px: Option<String>,
    /// Stop-loss trigger price type (last, mark, index).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_trigger_px_type: Option<OKXTriggerType>,
    /// Take-profit trigger price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_trigger_px: Option<String>,
    /// Take-profit order price (`-1` for market).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_ord_px: Option<String>,
    /// Take-profit trigger price type (last, mark, index).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_trigger_px_type: Option<OKXTriggerType>,
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
    /// Instrument ID code (numeric). Replaced `instId` for WebSocket order operations.
    pub inst_id_code: u64,
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
    /// Price in USD, only applicable to options. Mutually exclusive with `px` and `px_vol`.
    #[builder(default)]
    #[serde(rename = "pxUsd", skip_serializing_if = "Option::is_none")]
    pub px_usd: Option<String>,
    /// Price in implied volatility (1 = 100%), only applicable to options.
    /// Mutually exclusive with `px` and `px_usd`.
    #[builder(default)]
    #[serde(rename = "pxVol", skip_serializing_if = "Option::is_none")]
    pub px_vol: Option<String>,
    /// Reduce-only flag.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    /// Whether to close the entire position.
    #[builder(default)]
    #[serde(rename = "closePosition", skip_serializing_if = "Option::is_none")]
    pub close_position: Option<bool>,
    /// Target currency for net orders.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tgt_ccy: Option<OKXTargetCurrency>,
    /// Order tag for categorization.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Attached TP/SL orders submitted with the parent order.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attach_algo_ords: Option<Vec<WsAttachAlgoOrdParams>>,
}

/// Parameters for WebSocket cancel order operation (instType not included).
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsCancelOrderParams {
    /// Instrument ID code (numeric). Replaced `instId` for WebSocket order operations.
    pub inst_id_code: u64,
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
    /// Instrument ID code (numeric). Replaced `instId` for WebSocket order operations.
    pub inst_id_code: u64,
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
    /// New price in USD, only applicable to options. Must match the pricing mode used at placement.
    #[serde(rename = "newPxUsd", skip_serializing_if = "Option::is_none")]
    pub new_px_usd: Option<String>,
    /// New price in implied volatility, only applicable to options.
    /// Must match the pricing mode used at placement.
    #[serde(rename = "newPxVol", skip_serializing_if = "Option::is_none")]
    pub new_px_vol: Option<String>,
    /// New order size (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_sz: Option<String>,
}

/// Parameters for WebSocket algo order placement.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsPostAlgoOrderParams {
    /// Instrument ID code (numeric). Replaced `instId` for WebSocket order operations.
    pub inst_id_code: u64,
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
    /// Callback rate for trailing stop (e.g., "0.01" for 1%).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_ratio: Option<String>,
    /// Callback spread for trailing stop (fixed price distance).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_spread: Option<String>,
    /// Activation price for trailing stop.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_px: Option<String>,
}

/// Parameters for WebSocket cancel algo order operation.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsCancelAlgoOrderParams {
    /// Instrument ID code (numeric). Replaced `instId` for WebSocket order operations.
    pub inst_id_code: u64,
    /// Algo order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algo_id: Option<String>,
    /// Client algo order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algo_cl_ord_id: Option<String>,
}

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

        let result: Result<OKXWsFrame, _> = serde_json::from_str(json_str);
        match result {
            Ok(msg) => {
                if let OKXWsFrame::Subscription {
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

        let result: Result<OKXWsFrame, _> = serde_json::from_str(json_str);
        match result {
            Ok(msg) => {
                if let OKXWsFrame::Subscription {
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
        let result: Result<OKXWsFrame, _> = serde_json::from_str(json_str);
        match result {
            Ok(OKXWsFrame::OrderResponse {
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
        let result: Result<OKXWsFrame, _> = serde_json::from_str(json_str);
        match result {
            Ok(OKXWsFrame::OrderResponse {
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
        let result: Result<OKXWsFrame, _> = serde_json::from_str(json_str);
        match result {
            Ok(OKXWsFrame::OrderResponse {
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

        let parsed: OKXWsFrame = serde_json::from_str(success_response).unwrap();

        match parsed {
            OKXWsFrame::OrderResponse {
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

        let parsed: OKXWsFrame = serde_json::from_str(failure_response).unwrap();

        match parsed {
            OKXWsFrame::OrderResponse {
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

        let parsed: OKXWsFrame = serde_json::from_str(subscription_json).unwrap();

        match parsed {
            OKXWsFrame::Subscription {
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

        let parsed: OKXWsFrame = serde_json::from_str(login_success).unwrap();

        match parsed {
            OKXWsFrame::Login {
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
            _ => panic!("Expected Login variant, was: {parsed:?}"),
        }
    }

    #[rstest]
    fn test_error_event_parsing() {
        let error_json = r#"{
            "code": "60012",
            "msg": "Invalid request"
        }"#;

        let parsed: OKXWsFrame = serde_json::from_str(error_json).unwrap();

        match parsed {
            OKXWsFrame::Error { code, msg } => {
                assert_eq!(code, "60012");
                assert_eq!(msg, "Invalid request");
            }
            _ => panic!("Expected Error variant"),
        }
    }

    #[rstest]
    fn test_error_event_with_event_field_parsing() {
        // OKX sends error events with "event":"error" field (e.g., login failures)
        let error_json = r#"{
            "event": "error",
            "code": "60018",
            "msg": "Invalid sign"
        }"#;

        let parsed: OKXWsFrame = serde_json::from_str(error_json).unwrap();

        match parsed {
            OKXWsFrame::Error { code, msg } => {
                assert_eq!(code, "60018");
                assert_eq!(msg, "Invalid sign");
            }
            _ => panic!("Expected Error variant, was: {parsed:?}"),
        }
    }

    #[rstest]
    fn test_subscription_error_with_arg_field_parsing() {
        // OKX sends subscription errors with arg field (channel subscription failures)
        let error_json = r#"{
            "event": "error",
            "arg": {"channel": "tickers", "instId": "INVALID-INST"},
            "code": "60012",
            "msg": "Invalid request: channel not found",
            "connId": "a4d3ae55"
        }"#;

        let parsed: OKXWsFrame = serde_json::from_str(error_json).unwrap();

        match parsed {
            OKXWsFrame::Error { code, msg } => {
                assert_eq!(code, "60012");
                assert_eq!(msg, "Invalid request: channel not found");
            }
            _ => panic!("Expected Error variant, was: {parsed:?}"),
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
                inst_type: Some(OKXInstrumentType::Spot),
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
            let parsed: OKXWsFrame = serde_json::from_str(response_json).unwrap();

            match parsed {
                OKXWsFrame::OrderResponse {
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

        let parsed: OKXWsFrame = serde_json::from_str(book_data_json).unwrap();

        match parsed {
            OKXWsFrame::BookData { arg, action, data } => {
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

        let parsed: OKXWsFrame = serde_json::from_str(data_json).unwrap();

        match parsed {
            OKXWsFrame::Data { arg, data } => {
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

        let parsed: OKXWsFrame = serde_json::from_str(order_response_json).unwrap();

        match parsed {
            OKXWsFrame::OrderResponse {
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

        let parsed: OKXWsFrame = serde_json::from_str(order_response_json).unwrap();

        match parsed {
            OKXWsFrame::OrderResponse {
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

    #[rstest]
    fn test_ws_post_order_params_serializes_inst_id_code() {
        use super::WsPostOrderParamsBuilder;
        use crate::common::enums::{OKXOrderType, OKXSide, OKXTradeMode};

        let params = WsPostOrderParamsBuilder::default()
            .inst_id_code(10459u64)
            .td_mode(OKXTradeMode::Cross)
            .side(OKXSide::Buy)
            .ord_type(OKXOrderType::Limit)
            .sz("0.01".to_string())
            .px("50000".to_string())
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();

        assert!(json.contains("\"instIdCode\":10459"));
        assert!(!json.contains("\"instId\""));
    }

    #[rstest]
    fn test_ws_post_order_params_serializes_attached_tp_sl() {
        use super::{WsAttachAlgoOrdParamsBuilder, WsPostOrderParamsBuilder};
        use crate::common::enums::{OKXOrderType, OKXSide, OKXTradeMode, OKXTriggerType};

        let params = WsPostOrderParamsBuilder::default()
            .inst_id_code(10459u64)
            .td_mode(OKXTradeMode::Cross)
            .side(OKXSide::Buy)
            .ord_type(OKXOrderType::Limit)
            .sz("0.01".to_string())
            .px("50000".to_string())
            .attach_algo_ords(vec![
                WsAttachAlgoOrdParamsBuilder::default()
                    .attach_algo_cl_ord_id("O-bracket-sl")
                    .sl_trigger_px("39000")
                    .sl_ord_px("-1")
                    .sl_trigger_px_type(OKXTriggerType::Last)
                    .build()
                    .unwrap(),
                WsAttachAlgoOrdParamsBuilder::default()
                    .attach_algo_cl_ord_id("O-bracket-tp")
                    .tp_trigger_px("41000")
                    .tp_ord_px("-1")
                    .tp_trigger_px_type(OKXTriggerType::Last)
                    .build()
                    .unwrap(),
            ])
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();

        assert!(json.contains("\"attachAlgoOrds\""));
        assert!(json.contains("\"attachAlgoClOrdId\":\"O-bracket-sl\""));
        assert!(json.contains("\"slTriggerPx\":\"39000\""));
        assert!(json.contains("\"slOrdPx\":\"-1\""));
        assert!(json.contains("\"attachAlgoClOrdId\":\"O-bracket-tp\""));
        assert!(json.contains("\"tpTriggerPx\":\"41000\""));
        assert!(json.contains("\"tpOrdPx\":\"-1\""));
    }

    #[rstest]
    fn test_ws_cancel_order_params_serializes_inst_id_code() {
        use super::WsCancelOrderParamsBuilder;

        let params = WsCancelOrderParamsBuilder::default()
            .inst_id_code(10461u64)
            .ord_id("12345678".to_string())
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();

        assert!(json.contains("\"instIdCode\":10461"));
        assert!(!json.contains("\"instId\""));
        assert!(json.contains("\"ordId\":\"12345678\""));
    }

    #[rstest]
    fn test_ws_amend_order_params_serializes_inst_id_code() {
        use super::WsAmendOrderParamsBuilder;

        let params = WsAmendOrderParamsBuilder::default()
            .inst_id_code(10459u64)
            .cl_ord_id("client123".to_string())
            .new_px("51000".to_string())
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();

        assert!(json.contains("\"instIdCode\":10459"));
        assert!(!json.contains("\"instId\""));
        assert!(json.contains("\"newPx\":\"51000\""));
    }

    #[rstest]
    fn test_ws_post_algo_order_params_serializes_inst_id_code() {
        use super::WsPostAlgoOrderParamsBuilder;
        use crate::common::enums::{OKXAlgoOrderType, OKXSide, OKXTradeMode, OKXTriggerType};

        let params = WsPostAlgoOrderParamsBuilder::default()
            .inst_id_code(10459u64)
            .td_mode(OKXTradeMode::Cross)
            .side(OKXSide::Buy)
            .ord_type(OKXAlgoOrderType::Trigger)
            .sz("0.01".to_string())
            .trigger_px("48000".to_string())
            .trigger_px_type(OKXTriggerType::Last)
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();

        assert!(json.contains("\"instIdCode\":10459"));
        assert!(!json.contains("\"instId\""));
        assert!(json.contains("\"triggerPx\":\"48000\""));
    }

    #[rstest]
    fn test_ws_cancel_algo_order_params_serializes_inst_id_code() {
        let params = WsCancelAlgoOrderParams {
            inst_id_code: 10459,
            algo_id: Some("987654321".to_string()),
            algo_cl_ord_id: None,
        };

        let json = serde_json::to_string(&params).unwrap();

        assert!(json.contains("\"instIdCode\":10459"));
        assert!(!json.contains("\"instId\""));
        assert!(json.contains("\"algoId\":\"987654321\""));
    }

    #[rstest]
    fn test_ws_post_order_params_serializes_px_usd() {
        use super::WsPostOrderParamsBuilder;
        use crate::common::enums::{OKXOrderType, OKXSide, OKXTradeMode};

        let params = WsPostOrderParamsBuilder::default()
            .inst_id_code(10459u64)
            .td_mode(OKXTradeMode::Cross)
            .side(OKXSide::Buy)
            .ord_type(OKXOrderType::Limit)
            .sz("1".to_string())
            .px_usd("100.5".to_string())
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"pxUsd\":\"100.5\""));
        assert!(!json.contains("\"pxVol\""));
        assert!(!json.contains("\"px\":"));
    }

    #[rstest]
    fn test_ws_post_order_params_serializes_px_vol() {
        use super::WsPostOrderParamsBuilder;
        use crate::common::enums::{OKXOrderType, OKXSide, OKXTradeMode};

        let params = WsPostOrderParamsBuilder::default()
            .inst_id_code(10459u64)
            .td_mode(OKXTradeMode::Cross)
            .side(OKXSide::Buy)
            .ord_type(OKXOrderType::Limit)
            .sz("1".to_string())
            .px_vol("0.55".to_string())
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"pxVol\":\"0.55\""));
        assert!(!json.contains("\"pxUsd\""));
        assert!(!json.contains("\"px\":"));
    }

    #[rstest]
    fn test_ws_amend_order_params_serializes_new_px_usd() {
        use super::WsAmendOrderParamsBuilder;

        let params = WsAmendOrderParamsBuilder::default()
            .inst_id_code(10459u64)
            .cl_ord_id("client123".to_string())
            .new_px_usd("105.0".to_string())
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"newPxUsd\":\"105.0\""));
        assert!(!json.contains("\"newPx\":"));
        assert!(!json.contains("\"newPxVol\""));
    }

    #[rstest]
    fn test_ws_amend_order_params_serializes_new_px_vol() {
        use super::WsAmendOrderParamsBuilder;

        let params = WsAmendOrderParamsBuilder::default()
            .inst_id_code(10459u64)
            .cl_ord_id("client123".to_string())
            .new_px_vol("0.60".to_string())
            .build()
            .unwrap();

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"newPxVol\":\"0.60\""));
        assert!(!json.contains("\"newPx\":"));
        assert!(!json.contains("\"newPxUsd\""));
    }
}
