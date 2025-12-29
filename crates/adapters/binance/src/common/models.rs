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

//! Shared data transfer objects used across Binance HTTP and WebSocket payloads.

use serde::{Deserialize, Serialize};

use crate::common::enums::BinanceFilterType;

/// Binance API error response structure.
///
/// Binance returns this format for error responses:
/// ```json
/// {"code": -1000, "msg": "An unknown error occurred"}
/// ```
#[derive(Clone, Debug, Deserialize)]
pub struct BinanceErrorResponse {
    /// Binance error code (negative number indicates error).
    pub code: i64,
    /// Error message describing the issue.
    pub msg: String,
}

/// Price filter for spot instruments.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/filters#price_filter>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotPriceFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Minimum allowed price.
    pub min_price: String,
    /// Maximum allowed price.
    pub max_price: String,
    /// Tick size for price increments.
    pub tick_size: String,
}

/// Lot size filter for spot instruments.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/filters#lot_size>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotLotSizeFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Minimum order quantity.
    pub min_qty: String,
    /// Maximum order quantity.
    pub max_qty: String,
    /// Quantity increment step.
    pub step_size: String,
}

/// Market lot size filter for spot instruments.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/filters#market_lot_size>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotMarketLotSizeFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Minimum order quantity.
    pub min_qty: String,
    /// Maximum order quantity.
    pub max_qty: String,
    /// Quantity increment step.
    pub step_size: String,
}

/// Notional filter for spot instruments.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/filters#notional>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotNotionalFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Minimum notional value.
    pub min_notional: String,
    /// Apply minimum to market orders.
    #[serde(default)]
    pub apply_min_to_market: Option<bool>,
    /// Maximum notional value.
    #[serde(default)]
    pub max_notional: Option<String>,
    /// Apply maximum to market orders.
    #[serde(default)]
    pub apply_max_to_market: Option<bool>,
    /// Average price in minutes.
    #[serde(default)]
    pub avg_price_mins: Option<i64>,
}

/// Percent price filter for spot instruments.
///
/// # References
/// - <https://developers.binance.com/docs/binance-spot-api-docs/filters#percent_price>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotPercentPriceFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Maximum price deviation above average.
    pub multiplier_up: String,
    /// Maximum price deviation below average.
    pub multiplier_down: String,
    /// Average price minutes window.
    #[serde(default)]
    pub avg_price_mins: Option<i64>,
}

/// Price filter for futures instruments (USD-M and COIN-M).
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Exchange-Information>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPriceFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Minimum allowed price.
    pub min_price: String,
    /// Maximum allowed price.
    pub max_price: String,
    /// Tick size for price increments.
    pub tick_size: String,
}

/// Lot size filter for futures instruments.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Exchange-Information>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesLotSizeFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Maximum order quantity.
    pub max_qty: String,
    /// Minimum order quantity.
    pub min_qty: String,
    /// Quantity increment step.
    pub step_size: String,
}

/// Market lot size filter for futures instruments.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Exchange-Information>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesMarketLotSizeFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Maximum order quantity.
    pub max_qty: String,
    /// Minimum order quantity.
    pub min_qty: String,
    /// Quantity increment step.
    pub step_size: String,
}

/// Min notional filter for futures instruments.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Exchange-Information>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesMinNotionalFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Minimum notional value.
    pub notional: String,
}

/// Percent price filter for futures instruments.
///
/// # References
/// - <https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Exchange-Information>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPercentPriceFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Maximum price deviation above mark price.
    pub multiplier_up: String,
    /// Maximum price deviation below mark price.
    pub multiplier_down: String,
    /// Multiplier decimal places.
    #[serde(default)]
    pub multiplier_decimal: Option<String>,
}

/// Max number of orders filter.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaxNumOrdersFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Maximum number of open orders.
    pub limit: i64,
}

/// Max number of algo orders filter.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaxNumAlgoOrdersFilter {
    /// Filter type identifier.
    pub filter_type: BinanceFilterType,
    /// Maximum number of algo orders.
    pub limit: i64,
}

/// Rate limit definition from exchange info.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceRateLimit {
    /// Type of rate limit (REQUEST_WEIGHT, ORDERS, RAW_REQUESTS).
    pub rate_limit_type: String,
    /// Time interval (SECOND, MINUTE, DAY).
    pub interval: String,
    /// Interval number.
    pub interval_num: i64,
    /// Request limit.
    pub limit: i64,
}
