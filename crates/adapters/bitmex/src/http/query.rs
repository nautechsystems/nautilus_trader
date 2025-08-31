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

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use serde::{self, Deserialize, Serialize};
use serde_json::Value;

use crate::common::enums::{
    BitmexContingencyType, BitmexExecInstruction, BitmexOrderType, BitmexPegPriceType, BitmexSide,
    BitmexTimeInForce,
};

fn serialize_string_vec<S>(values: &Option<Vec<String>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match values {
        Some(vec) => serializer.serialize_str(&vec.join(",")),
        None => serializer.serialize_none(),
    }
}

/// Parameters for the GET /trade endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetTradeParams {
    /// Instrument symbol. Send a bare series (e.g., XBT) to get data for the nearest expiring contract in that series.  You can also send a timeframe, e.g. `XBT:quarterly`. Timeframes are `nearest`, `daily`, `weekly`, `monthly`, `quarterly`, `biquarterly`, and `perpetual`.
    pub symbol: Option<String>,
    /// Generic table filter. Send JSON key/value pairs, such as `{"key": "value"}`. You can key on individual fields, and do more advanced querying on timestamps. See the [Timestamp Docs](https://www.bitmex.com/app/restAPI#Timestamp-Filters) for more details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<Value>,
    /// Array of column names to fetch. If omitted, will return all columns.  Note that this method will always return item keys, even when not specified, so you may receive more columns that you expect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Value>,
    /// Number of results to fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
    /// Starting point for results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<i32>,
    /// If true, will sort results newest first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverse: Option<bool>,
    /// Starting date filter for results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<DateTime<Utc>>,
    /// Ending date filter for results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<DateTime<Utc>>,
}

/// Parameters for the GET /order endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetOrderParams {
    /// Instrument symbol. Send a bare series (e.g., XBT) to get data for the nearest expiring contract in that series.  You can also send a timeframe, e.g. `XBT:quarterly`. Timeframes are `nearest`, `daily`, `weekly`, `monthly`, `quarterly`, `biquarterly`, and `perpetual`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Generic table filter. Send JSON key/value pairs, such as `{"key": "value"}`. You can key on individual fields, and do more advanced querying on timestamps. See the [Timestamp Docs](https://www.bitmex.com/app/restAPI#Timestamp-Filters) for more details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<Value>,
    /// Array of column names to fetch. If omitted, will return all columns.  Note that this method will always return item keys, even when not specified, so you may receive more columns that you expect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Value>,
    /// Number of results to fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
    /// Starting point for results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<i32>,
    /// If true, will sort results newest first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverse: Option<bool>,
    /// Starting date filter for results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<DateTime<Utc>>,
    /// Ending date filter for results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<DateTime<Utc>>,
}

/// Parameters for the POST /order endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct PostOrderParams {
    /// Instrument symbol. e.g. 'XBTUSD'.
    pub symbol: String,
    /// Order side. Valid options: Buy, Sell. Defaults to 'Buy' unless `orderQty` is negative.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<BitmexSide>,
    /// Order quantity in units of the instrument (i.e. contracts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_qty: Option<u32>,
    /// Optional limit price for `Limit`, `StopLimit`, and `LimitIfTouched` orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    /// Optional quantity to display in the book. Use 0 for a fully hidden order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_qty: Option<u32>,
    /// Optional trigger price for `Stop`, `StopLimit`, `MarketIfTouched`, and `LimitIfTouched` orders. Use a price below the current price for stop-sell orders and buy-if-touched orders. Use `execInst` of `MarkPrice` or `LastPrice` to define the current price used for triggering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_px: Option<f64>,
    /// Optional Client Order ID. This clOrdID will come back on the order and any related executions.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "clOrdID")]
    pub cl_ord_id: Option<String>,
    /// Optional Client Order Link ID for contingent orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "clOrdLinkID")]
    pub cl_ord_link_id: Option<String>,
    /// Optional trailing offset from the current price for `Stop`, `StopLimit`, `MarketIfTouched`, and `LimitIfTouched` orders; use a negative offset for stop-sell orders and buy-if-touched orders. Optional offset from the peg price for 'Pegged' orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peg_offset_value: Option<f64>,
    /// Optional peg price type. Valid options: `LastPeg`, `MidPricePeg`, `MarketPeg`, `PrimaryPeg`, `TrailingStopPeg`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peg_price_type: Option<BitmexPegPriceType>,
    /// Order type. Valid options: Market, Limit, Stop, `StopLimit`, `MarketIfTouched`, `LimitIfTouched`, Pegged. Defaults to `Limit` when `price` is specified. Defaults to `Stop` when `stopPx` is specified. Defaults to `StopLimit` when `price` and `stopPx` are specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ord_type: Option<BitmexOrderType>,
    /// Time in force. Valid options: `Day`, `GoodTillCancel`, `ImmediateOrCancel`, `FillOrKill`. Defaults to `GoodTillCancel` for `Limit`, `StopLimit`, and `LimitIfTouched` orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<BitmexTimeInForce>,
    /// Optional execution instructions. Valid options: `ParticipateDoNotInitiate`, `AllOrNone`, `MarkPrice`, `IndexPrice`, `LastPrice`, `Close`, `ReduceOnly`, Fixed. `AllOrNone` instruction requires `displayQty` to be 0. `MarkPrice`, `IndexPrice` or `LastPrice` instruction valid for `Stop`, `StopLimit`, `MarketIfTouched`, and `LimitIfTouched` orders.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_exec_instructions"
    )]
    pub exec_inst: Option<Vec<BitmexExecInstruction>>,
    /// Deprecated: linked orders are not supported after 2018/11/10.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contingency_type: Option<BitmexContingencyType>,
    /// Optional order annotation. e.g. 'Take profit'.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

fn serialize_exec_instructions<S>(
    instructions: &Option<Vec<BitmexExecInstruction>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match instructions {
        Some(inst) => {
            let joined = inst
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            serializer.serialize_str(&joined)
        }
        None => serializer.serialize_none(),
    }
}

/// Parameters for the DELETE /order endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct DeleteOrderParams {
    /// Client Order ID(s). See POST /order.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_string_vec",
        rename = "orderID"
    )]
    pub order_id: Option<Vec<String>>,
    /// Client Order ID(s). See POST /order.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_string_vec",
        rename = "clOrdID"
    )]
    pub cl_ord_id: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional cancellation annotation. e.g. 'Spread Exceeded'.
    pub text: Option<String>,
}

/// Parameters for the PUT /order endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct PutOrderParams {
    /// Order ID
    #[serde(rename = "orderID")]
    pub order_id: Option<String>,
    /// Client Order ID. See POST /order.
    #[serde(rename = "origClOrdID")]
    pub orig_cl_ord_id: Option<String>,
    /// Optional new Client Order ID, requires `origClOrdID`.
    #[serde(rename = "clOrdID")]
    pub cl_ord_id: Option<String>,
    /// Optional order quantity in units of the instrument (i.e. contracts).
    pub order_qty: Option<u32>,
    /// Optional leaves quantity in units of the instrument (i.e. contracts). Useful for amending partially filled orders.
    pub leaves_qty: Option<u32>,
    /// Optional limit price for `Limit`, `StopLimit`, and `LimitIfTouched` orders.
    pub price: Option<f64>,
    /// Optional trigger price for `Stop`, `StopLimit`, `MarketIfTouched`, and `LimitIfTouched` orders. Use a price below the current price for stop-sell orders and buy-if-touched orders.
    pub stop_px: Option<f64>,
    /// Optional trailing offset from the current price for `Stop`, `StopLimit`, `MarketIfTouched`, and `LimitIfTouched` orders; use a negative offset for stop-sell orders and buy-if-touched orders. Optional offset from the peg price for 'Pegged' orders.
    pub peg_offset_value: Option<f64>,
    /// Optional amend annotation. e.g. 'Adjust skew'.
    pub text: Option<String>,
}

/// Parameters for the GET /execution/tradeHistory endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetExecutionParams {
    /// Instrument symbol. Send a bare series (e.g. XBT) to get data for the nearest expiring contract in that series.  You can also send a timeframe, e.g. `XBT:quarterly`. Timeframes are `nearest`, `daily`, `weekly`, `monthly`, `quarterly`, `biquarterly`, and `perpetual`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Generic table filter. Send JSON key/value pairs, such as `{"key": "value"}`. You can key on individual fields, and do more advanced querying on timestamps. See the [Timestamp Docs](https://www.bitmex.com/app/restAPI#Timestamp-Filters) for more details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<Value>,
    /// Array of column names to fetch. If omitted, will return all columns.  Note that this method will always return item keys, even when not specified, so you may receive more columns that you expect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Value>,
    /// Number of results to fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
    /// Starting point for results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<i32>,
    /// If true, will sort results newest first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverse: Option<bool>,
    /// Starting date filter for results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<DateTime<Utc>>,
    /// Ending date filter for results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<DateTime<Utc>>,
}

/// Parameters for the GET /position endpoint.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
#[serde(rename_all = "camelCase")]
pub struct GetPositionParams {
    /// Generic table filter. Send JSON key/value pairs, such as `{"key": "value"}`. You can key on individual fields, and do more advanced querying on timestamps. See the [Timestamp Docs](https://www.bitmex.com/app/restAPI#Timestamp-Filters) for more details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<Value>,
    /// Array of column names to fetch. If omitted, will return all columns.  Note that this method will always return item keys, even when not specified, so you may receive more columns that you expect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Value>,
    /// Number of results to fetch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
}
