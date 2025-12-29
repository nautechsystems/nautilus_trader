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

//! Enumerations for Kraken WebSocket v2 API.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString, FromRepr};

#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenWsMethod {
    Subscribe,
    Unsubscribe,
    Ping,
    Pong,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenWsChannel {
    Ticker,
    #[serde(rename = "trade")]
    #[strum(serialize = "trade")]
    Trade,
    #[serde(rename = "book")]
    #[strum(serialize = "book")]
    Book,
    #[serde(rename = "ohlc")]
    #[strum(serialize = "ohlc")]
    Ohlc,
    #[serde(rename = "spread")]
    #[strum(serialize = "spread")]
    Spread,
    // Private channels
    #[serde(rename = "executions")]
    #[strum(serialize = "executions")]
    Executions,
    #[serde(rename = "balances")]
    #[strum(serialize = "balances")]
    Balances,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", eq, eq_int)
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenWsMessageType {
    Heartbeat,
    Status,
    Subscribe,
    Unsubscribe,
    Update,
    Snapshot,
    Error,
}

/// Execution type from the Kraken executions channel.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(ascii_case_insensitive, serialize_all = "snake_case")]
pub enum KrakenExecType {
    /// Order is pending submission to the exchange.
    #[serde(rename = "pending_new")]
    #[strum(serialize = "pending_new")]
    PendingNew,
    /// Order has been accepted by the exchange.
    New,
    /// Order has been partially or fully filled.
    Trade,
    /// Order has been completely filled.
    Filled,
    /// Iceberg order refill.
    #[serde(rename = "iceberg_refill")]
    #[strum(serialize = "iceberg_refill")]
    IcebergRefill,
    /// Order has been canceled.
    Canceled,
    /// Order has expired.
    Expired,
    /// Order has been amended (user-initiated modification).
    Amended,
    /// Order has been restated (engine-initiated adjustment).
    Restated,
    /// Order status update without state change.
    Status,
}

/// Order status from the Kraken WebSocket v2 executions channel.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(ascii_case_insensitive, serialize_all = "snake_case")]
pub enum KrakenWsOrderStatus {
    /// Order is pending submission.
    #[serde(rename = "pending_new")]
    #[strum(serialize = "pending_new")]
    PendingNew,
    /// Order has been accepted.
    New,
    /// Order has been partially filled.
    #[serde(rename = "partially_filled")]
    #[strum(serialize = "partially_filled")]
    PartiallyFilled,
    /// Order has been completely filled.
    Filled,
    /// Order has been canceled.
    Canceled,
    /// Order has expired.
    Expired,
    /// Conditional order has been triggered.
    Triggered,
}

/// Liquidity indicator from trade executions.
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum KrakenLiquidityInd {
    /// Maker (limit order that added liquidity).
    #[serde(rename = "m")]
    #[strum(serialize = "m")]
    Maker,
    /// Taker (order that removed liquidity).
    #[serde(rename = "t")]
    #[strum(serialize = "t")]
    Taker,
}
