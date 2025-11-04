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

//! Enums for Lighter-specific types.

use std::fmt;

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Lighter account type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum LighterAccountType {
    /// Standard account (fee-less).
    Standard,
    /// Premium account (0.2 bps maker, 2 bps taker fees).
    Premium,
}

/// Lighter order type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LighterOrderType {
    /// Limit order.
    Limit,
    /// Market order.
    Market,
    /// Stop loss order.
    StopLoss,
    /// Stop loss limit order.
    StopLossLimit,
    /// Take profit order.
    TakeProfit,
    /// Take profit limit order.
    TakeProfitLimit,
    /// TWAP (Time-Weighted Average Price) order.
    Twap,
}

impl fmt::Display for LighterOrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit => write!(f, "LIMIT"),
            Self::Market => write!(f, "MARKET"),
            Self::StopLoss => write!(f, "STOP_LOSS"),
            Self::StopLossLimit => write!(f, "STOP_LOSS_LIMIT"),
            Self::TakeProfit => write!(f, "TAKE_PROFIT"),
            Self::TakeProfitLimit => write!(f, "TAKE_PROFIT_LIMIT"),
            Self::Twap => write!(f, "TWAP"),
        }
    }
}

/// Lighter time-in-force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LighterTimeInForce {
    /// Good till canceled.
    #[serde(rename = "GTC")]
    #[strum(serialize = "GTC")]
    GoodTilCanceled,
    /// Immediate or cancel.
    #[serde(rename = "IOC")]
    #[strum(serialize = "IOC")]
    ImmediateOrCancel,
    /// Fill or kill.
    #[serde(rename = "FOK")]
    #[strum(serialize = "FOK")]
    FillOrKill,
    /// Post only.
    #[serde(rename = "POST_ONLY")]
    #[strum(serialize = "POST_ONLY")]
    PostOnly,
    /// Good till time.
    #[serde(rename = "GTT")]
    #[strum(serialize = "GTT")]
    GoodTillTime,
}

/// Lighter order side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum LighterOrderSide {
    /// Buy order.
    Buy,
    /// Sell order.
    Sell,
}

/// Lighter order status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LighterOrderStatus {
    /// Order is pending.
    Pending,
    /// Order is open.
    Open,
    /// Order is partially filled.
    PartiallyFilled,
    /// Order is filled.
    Filled,
    /// Order is canceled.
    Canceled,
    /// Order is rejected.
    Rejected,
    /// Order is expired.
    Expired,
}

/// Lighter instrument type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum LighterInstrumentType {
    /// Spot instrument.
    Spot,
    /// Perpetual futures.
    Perp,
}

/// Lighter WebSocket channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LighterWsChannel {
    /// Order book channel.
    OrderBook { market_id: u64 },
    /// Trades channel.
    Trades { market_id: u64 },
    /// Account updates channel.
    Account { account_id: u64 },
    /// Order updates channel.
    Orders { account_id: u64 },
}

impl fmt::Display for LighterWsChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OrderBook { market_id } => write!(f, "orderbook:{}", market_id),
            Self::Trades { market_id } => write!(f, "trades:{}", market_id),
            Self::Account { account_id } => write!(f, "account:{}", account_id),
            Self::Orders { account_id } => write!(f, "orders:{}", account_id),
        }
    }
}
