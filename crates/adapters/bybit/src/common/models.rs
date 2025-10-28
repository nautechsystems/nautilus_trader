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

//! Shared data transfer objects used across Bybit HTTP and WebSocket payloads.

use serde::{Deserialize, Serialize};

/// Generic wrapper that contains a list payload returned by Bybit.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitList<T> {
    /// Collection returned by the endpoint.
    pub list: Vec<T>,
}

/// Generic list result that also carries pagination cursor information.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitCursorList<T> {
    /// Collection returned by the endpoint.
    pub list: Vec<T>,
    /// Pagination cursor for the next page, when provided.
    pub next_page_cursor: Option<String>,
}

/// Common leverage filter that describes leverage bounds and step.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeverageFilter {
    /// Minimum leverage supported.
    pub min_leverage: String,
    /// Maximum leverage supported.
    pub max_leverage: String,
    /// Step between successive leverage values.
    pub leverage_step: String,
}

/// Price filter for linear/inverse contracts describing min/max and tick.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearPriceFilter {
    /// Minimum allowed order price.
    pub min_price: String,
    /// Maximum allowed order price.
    pub max_price: String,
    /// Tick size for price increments.
    pub tick_size: String,
}

/// Price filter for spot instruments (only tick size is provided).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotPriceFilter {
    /// Tick size for price increments.
    pub tick_size: String,
}

/// Lot size filter for spot instruments.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotLotSizeFilter {
    /// Base asset precision.
    pub base_precision: String,
    /// Quote asset precision.
    pub quote_precision: String,
    /// Minimum order quantity.
    pub min_order_qty: String,
    /// Maximum order quantity.
    pub max_order_qty: String,
    /// Minimum order notional.
    pub min_order_amt: String,
    /// Maximum order notional.
    pub max_order_amt: String,
}

/// Lot size filter for derivatives instruments.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearLotSizeFilter {
    /// Maximum order quantity.
    pub max_order_qty: String,
    /// Minimum order quantity.
    pub min_order_qty: String,
    /// Quantity increment step.
    pub qty_step: String,
    /// Maximum order quantity for post-only orders.
    #[serde(default)]
    pub post_only_max_order_qty: Option<String>,
    /// Maximum order quantity for market orders.
    #[serde(default)]
    pub max_mkt_order_qty: Option<String>,
    /// Minimum notional value.
    #[serde(default)]
    pub min_notional_value: Option<String>,
}

/// Lot size filter for option instruments.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionLotSizeFilter {
    /// Maximum order quantity.
    pub max_order_qty: String,
    /// Minimum order quantity.
    pub min_order_qty: String,
    /// Quantity increment step.
    pub qty_step: String,
}

/// Top-level response envelope returned by Bybit HTTP endpoints.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BybitResponse<T> {
    /// Return code (0 = success).
    pub ret_code: i64,
    /// Textual message accompanying the response.
    pub ret_msg: String,
    /// Actual payload returned by the endpoint.
    pub result: T,
    /// Additional metadata returned by some endpoints.
    #[serde(default)]
    pub ret_ext_info: Option<serde_json::Value>,
    /// Server time in milliseconds when the response was produced.
    #[serde(default)]
    pub time: Option<i64>,
}

/// Convenience alias for responses that return a simple list.
pub type BybitListResponse<T> = BybitResponse<BybitList<T>>;

/// Convenience alias for responses that return a cursor-based list.
pub type BybitCursorListResponse<T> = BybitResponse<BybitCursorList<T>>;
