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

//! Enums for dYdX WebSocket operations and channels.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString, FromRepr};

/// WebSocket operation types for dYdX.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DydxWsOperation {
    /// Subscribe to a channel.
    Subscribe,
    /// Unsubscribe from a channel.
    Unsubscribe,
    /// Ping keepalive message.
    Ping,
    /// Pong response to ping.
    Pong,
}

/// dYdX WebSocket channel identifiers.
///
/// # References
///
/// <https://docs.dydx.trade/developers/indexer/websockets>
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DydxWsChannel {
    /// Market data for all markets.
    #[serde(rename = "v4_markets")]
    #[strum(serialize = "v4_markets")]
    Markets,
    /// Trade stream for specific market.
    #[serde(rename = "v4_trades")]
    #[strum(serialize = "v4_trades")]
    Trades,
    /// Order book snapshots and updates.
    #[serde(rename = "v4_orderbook")]
    #[strum(serialize = "v4_orderbook")]
    Orderbook,
    /// Candlestick/kline data.
    #[serde(rename = "v4_candles")]
    #[strum(serialize = "v4_candles")]
    Candles,
    /// Subaccount updates (orders, fills, positions).
    #[serde(rename = "v4_subaccounts")]
    #[strum(serialize = "v4_subaccounts")]
    Subaccounts,
    /// Block height updates from chain.
    #[serde(rename = "v4_block_height")]
    #[strum(serialize = "v4_block_height")]
    BlockHeight,
}

impl DydxWsChannel {
    /// Returns `true` if this is a private channel requiring authentication.
    #[must_use]
    pub const fn is_private(&self) -> bool {
        matches!(self, Self::Subaccounts)
    }

    /// Returns `true` if this is a public channel.
    #[must_use]
    pub const fn is_public(&self) -> bool {
        !self.is_private()
    }
}
