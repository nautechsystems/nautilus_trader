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

use crate::common::enums::{
    CoinbaseIntxAlgoStrategy, CoinbaseIntxInstrumentType, CoinbaseIntxOrderEventType,
    CoinbaseIntxOrderType, CoinbaseIntxSTPMode, CoinbaseIntxSide, CoinbaseIntxTimeInForce,
};

/// Parameters for creating a new order.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct CreateOrderParams {
    /// Portfolio identifier.
    pub portfolio: String,
    /// Unique client-assigned order identifier.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<String>,
    /// Side of the transaction (BUY/SELL).
    pub side: CoinbaseIntxSide,
    /// Amount in base asset units.
    pub size: String,
    /// Instrument identifier (name, ID, or UUID).
    pub instrument: String,
    /// Type of order.
    #[serde(rename = "type")]
    pub order_type: CoinbaseIntxOrderType,
    /// Time in force for the order.
    #[builder(default)]
    pub tif: CoinbaseIntxTimeInForce,
    /// Price limit in quote asset units.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// Market price that activates a stop order.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,
    /// Limit price for TP/SL stop leg orders.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_limit_price: Option<String>,
    /// Expiration time for GTT orders.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<DateTime<Utc>>,
    /// Self-trade prevention mode.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stp_mode: Option<CoinbaseIntxSTPMode>,
    /// Whether order must rest on the book.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_only: Option<bool>,
    /// Whether order must close existing position.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_only: Option<bool>,
    /// Algorithmic trading strategy.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algo_strategy: Option<CoinbaseIntxAlgoStrategy>,
}

/// Parameters for retrieving a single order.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetOrderParams {
    /// Portfolio UUID or ID.
    pub portfolio: String,
}

/// Parameters for querying orders.
#[derive(Clone, Debug, Serialize, Deserialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetOrdersParams {
    /// Portfolio UUID or ID.
    pub portfolio: String,
    /// Instrument identifier (name, UUID, or ID).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrument: Option<String>,
    /// Type of instrument ("SPOT" or "PERPETUAL_FUTURE").
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrument_type: Option<String>,
    /// Client-provided order identifier.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<String>,
    /// Type of the most recent order event.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<CoinbaseIntxOrderEventType>,
    /// Type of order.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type: Option<CoinbaseIntxOrderType>,
    /// Order side (BUY/SELL).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<CoinbaseIntxSide>,
    /// Maximum event time for results (ISO-8601).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_datetime: Option<DateTime<Utc>>,
    /// Number of results to return (default: 25, max: 100).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_limit: Option<u32>,
    /// Number of results to skip.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_offset: Option<u32>,
}

/// Parameters for retrieving a single order.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CancelOrderParams {
    /// Portfolio UUID or ID.
    pub portfolio: String,
}

/// Parameters for canceling orders.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct CancelOrdersParams {
    /// Portfolio UUID or ID.
    pub portfolio: String,
    /// Instrument identifier (name, UUID, or ID).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrument: Option<String>,
    /// Order side.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<CoinbaseIntxSide>,
    /// Type of instrument.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrument_type: Option<CoinbaseIntxInstrumentType>,
}

/// Parameters for modifying an existing order.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct ModifyOrderParams {
    /// Portfolio UUID or ID (must match original order).
    pub portfolio: String,
    /// Client-assigned unique identifier for the modified order.
    pub client_order_id: String,
    /// New price limit in quote asset units (for limit and stop limit orders).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// New market price that activates a stop order.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,
    /// New amount in base asset units.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
}

/// Parameters for querying portfolio fills.
#[derive(Clone, Debug, Serialize, Deserialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct GetPortfolioFillsParams {
    /// A specific order for which to fetch fills identified by order ID.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    /// Fetch fills for all orders with the given client order ID.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<String>,
    /// The maximum `event_time` for results. Can be used in pagination to keep result set static.
    /// Uses ISO-8601 format (e.g., 2023-03-16T23:59:53Z).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_datetime: Option<DateTime<Utc>>,
    /// The number of results to return (defaults to 25 with a max supported value of 100).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_limit: Option<u32>,
    /// The number of results from the beginning to skip past.
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_offset: Option<u32>,
    /// The minimum `event_time` for results. Uses ISO-8601 format (e.g., 2023-03-16T23:59:53Z).
    #[builder(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_from: Option<DateTime<Utc>>,
}
