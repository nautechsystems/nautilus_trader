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

//! Enumerations mapping dYdX v4 concepts onto idiomatic Nautilus variants.

use chrono::{DateTime, Utc};
use cosmrs::tendermint::{Error, chain::Id};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display};

/// [Chain ID](https://docs.dydx.xyz/nodes/network-constants#chain-id)
/// serves as a unique chain identifier to prevent replay attacks.
///
/// See also [Cosmos ecosystem](https://cosmos.directory/).
#[derive(Debug, Eq, PartialEq, Clone, Display, AsRefStr, Deserialize, Serialize)]
pub enum DydxChainId {
    /// Testnet.
    #[strum(serialize = "dydx-testnet-4")]
    #[serde(rename = "dydx-testnet-4")]
    Testnet4,
    /// Mainnet.
    #[strum(serialize = "dydx-mainnet-1")]
    #[serde(rename = "dydx-mainnet-1")]
    Mainnet1,
}

impl TryFrom<DydxChainId> for Id {
    type Error = Error;

    fn try_from(chain_id: DydxChainId) -> Result<Self, Self::Error> {
        chain_id.as_ref().parse()
    }
}

/// Order [expiration types](https://docs.dydx.xyz/concepts/trading/orders#comparison).
#[derive(Clone, Debug)]
pub enum DydxOrderGoodUntil {
    /// Block expiration is used for short-term orders.
    /// The order expires after the specified block height.
    Block(u32),
    /// Time expiration is used for long-term orders.
    /// The order expires at the specified timestamp.
    Time(DateTime<Utc>),
}

/// Order type enumeration.
#[derive(Clone, Debug)]
pub enum DydxOrderType {
    /// Limit order.
    Limit,
    /// Market order.
    Market,
    /// Stop limit order.
    StopLimit,
    /// Stop market order.
    StopMarket,
    /// Take profit order.
    TakeProfit,
    /// Take profit market order.
    TakeProfitMarket,
}

/// Order flags indicating order lifetime and execution type.
#[derive(Clone, Debug)]
pub enum DydxOrderFlags {
    /// Short-term order (expires by block height).
    ShortTerm,
    /// Long-term order (expires by timestamp).
    LongTerm,
    /// Conditional order (triggered by trigger price).
    Conditional,
}

/// Market parameters required for price and size quantizations.
///
/// These quantizations are required for `Order` placement.
/// See also [how to interpret block data for trades](https://docs.dydx.exchange/api_integration-guides/how_to_interpret_block_data_for_trades).
#[derive(Clone, Debug)]
pub struct DydxOrderMarketParams {
    /// Atomic resolution.
    pub atomic_resolution: i32,
    /// CLOB pair ID.
    pub clob_pair_id: u32,
    /// Oracle price.
    pub oracle_price: Option<Decimal>,
    /// Quantum conversion exponent.
    pub quantum_conversion_exponent: i32,
    /// Step base quantums.
    pub step_base_quantums: u64,
    /// Subticks per tick.
    pub subticks_per_tick: u32,
}
