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
