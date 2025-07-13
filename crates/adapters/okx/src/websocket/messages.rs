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

use derive_builder::Builder;
use nautilus_model::{
    data::{Data, OrderBookDeltas},
    events::{AccountState, OrderRejected},
    instruments::InstrumentAny,
    reports::{FillReport, OrderStatusReport},
};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::enums::{OKXWsChannel, OKXWsOperation};
use crate::{
    common::{
        enums::{
            OKXBookAction, OKXCandleConfirm, OKXExecType, OKXInstrumentType, OKXOrderStatus,
            OKXOrderType, OKXPositionSide, OKXSide, OKXTradeMode,
        },
        parse::{deserialize_empty_string_as_none, deserialize_string_to_u64},
    },
    websocket::enums::OKXSubscriptionEvent,
};

#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    Data(Vec<Data>),
    Deltas(OrderBookDeltas),
    Instrument(Box<InstrumentAny>),
    AccountUpdate(AccountState),
    OrderRejected(OrderRejected),
    ExecutionReports(Vec<ExecutionReport>),
    Error(OKXWebSocketError),
    Raw(serde_json::Value), // Unhandled channels
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
    /// Arguments payload for the operation.
    pub args: Vec<T>,
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
    Subscription {
        event: OKXSubscriptionEvent,
        arg: OKXWebSocketArg,
        #[serde(rename = "connId")]
        conn_id: String,
    },
    Login {
        event: String,
        code: String,
        msg: String,
        #[serde(rename = "connId")]
        conn_id: String,
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
        op: String,
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
    pub funding_rate: String,
    /// Predicted next funding rate.
    pub next_funding_rate: String,
    /// Next funding time, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub funding_time: u64,
    /// Message timestamp, Unix timestamp format in milliseconds.
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
    pub title: String,
    /// Status type: planned or scheduled.
    #[serde(rename = "type")]
    pub status_type: String,
    /// System maintenance state: canceled, completed, pending, ongoing.
    pub state: String,
    /// Expected completion timestamp.
    pub end_time: Option<String>,
    /// Planned start timestamp.
    pub begin_time: Option<String>,
    /// Service involved.
    pub service_type: Option<String>,
    /// Reason for status change.
    pub reason: Option<String>,
    /// Timestamp of the data generation, Unix timestamp format in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub ts: u64,
}

/// Order update message from WebSocket orders channel.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OKXOrderMsg {
    /// Accumulated filled size.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub acc_fill_sz: Option<String>,
    /// Algorithm client order ID.
    #[serde(default)]
    pub algo_cl_ord_id: Option<String>,
    /// Algorithm ID.
    #[serde(default)]
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
    /// Category.
    pub category: String,
    /// Currency.
    pub ccy: String,
    /// Client order ID.
    pub cl_ord_id: String,
    /// Fee.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub fee: Option<String>,
    /// Fee currency.
    pub fee_ccy: String,
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
    pub ord_type: String,
    /// Profit and loss.
    pub pnl: String,
    /// Position side.
    pub pos_side: String,
    /// Price.
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
    pub td_mode: String,
    /// Trade ID.
    pub trade_id: String,
    /// Last update time, Unix timestamp in milliseconds.
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub u_time: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_websocket_arg() {
        let json_str = r#"{"channel":"instruments","instType":"SPOT"}"#;

        let result: Result<OKXWebSocketArg, _> = serde_json::from_str(json_str);
        match result {
            Ok(arg) => {
                assert_eq!(
                    arg.channel,
                    crate::websocket::enums::OKXWsChannel::Instruments
                );
                assert_eq!(
                    arg.inst_type,
                    Some(crate::common::enums::OKXInstrumentType::Spot)
                );
                assert_eq!(arg.inst_id, None);
            }
            Err(e) => {
                panic!("Failed to deserialize WebSocket arg: {e}");
            }
        }
    }

    #[test]
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
                assert_eq!(
                    msg.arg.channel,
                    crate::websocket::enums::OKXWsChannel::Instruments
                );
                assert_eq!(msg.conn_id, "380cfa6a");
            }
            Err(e) => {
                panic!("Failed to deserialize subscribe message directly: {e}");
            }
        }
    }

    #[test]
    fn test_deserialize_subscribe_confirmation() {
        let json_str = r#"{"event":"subscribe","arg":{"channel":"instruments","instType":"SPOT"},"connId":"380cfa6a"}"#;

        let result: Result<OKXWebSocketEvent, _> = serde_json::from_str(json_str);
        match result {
            Ok(msg) => {
                if let OKXWebSocketEvent::Subscription {
                    event,
                    arg,
                    conn_id,
                } = msg
                {
                    assert_eq!(event, OKXSubscriptionEvent::Subscribe);
                    assert_eq!(arg.channel, OKXWsChannel::Instruments);
                    assert_eq!(conn_id, "380cfa6a");
                } else {
                    panic!("Expected Subscribe variant, got: {msg:?}");
                }
            }
            Err(e) => {
                panic!("Failed to deserialize subscription confirmation: {e}");
            }
        }
    }

    #[test]
    fn test_deserialize_subscribe_with_inst_id() {
        let json_str = r#"{"event":"subscribe","arg":{"channel":"candle1m","instId":"ETH-USDT"},"connId":"358602f5"}"#;

        let result: Result<OKXWebSocketEvent, _> = serde_json::from_str(json_str);
        match result {
            Ok(msg) => {
                if let OKXWebSocketEvent::Subscription {
                    event,
                    arg,
                    conn_id,
                } = msg
                {
                    assert_eq!(event, OKXSubscriptionEvent::Subscribe);
                    assert_eq!(arg.channel, OKXWsChannel::Candle1Minute);
                    assert_eq!(conn_id, "358602f5");
                } else {
                    panic!("Expected Subscribe variant, got: {msg:?}");
                }
            }
            Err(e) => {
                panic!("Failed to deserialize subscription confirmation: {e}");
            }
        }
    }

    #[test]
    fn test_channel_serialization_for_logging() {
        // Test that we can serialize channel enums to their string representations for logging
        use crate::websocket::enums::OKXWsChannel;

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
    pub inst_id: String,
    /// Trading mode: cash, isolated, cross.
    pub td_mode: OKXTradeMode,
    /// Margin currency (only for isolated margin).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ccy: Option<String>,
    /// Unique client order ID.
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
    pub tgt_ccy: Option<String>,
}

/// Parameters for WebSocket cancel order operation (instType not included).
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsCancelOrderParams {
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: String,
    /// Exchange-assigned order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ord_id: Option<String>,
    /// User-assigned client order ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// Position side: long, short, net (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos_side: Option<OKXPositionSide>,
    /// Margin currency (only for margin trades).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ccy: Option<String>,
}

/// Parameters for WebSocket amend order operation (instType not included).
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct WsAmendOrderParams {
    /// Instrument ID, e.g. "BTC-USDT".
    pub inst_id: String,
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
    pub px: Option<String>,
    /// New order size (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sz: Option<String>,
    /// Position side: long, short, net (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos_side: Option<OKXPositionSide>,
    /// Margin currency (only for margin trades).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ccy: Option<String>,
}
