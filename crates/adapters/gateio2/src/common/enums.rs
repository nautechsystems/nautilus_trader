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

//! Gate.io specific enums and types.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Gate.io market type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateioMarketType {
    /// Spot market
    Spot,
    /// Margin market
    Margin,
    /// USDT-settled perpetual futures
    Futures,
    /// Delivery futures
    Delivery,
    /// Options
    Options,
}

impl fmt::Display for GateioMarketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spot => write!(f, "spot"),
            Self::Margin => write!(f, "margin"),
            Self::Futures => write!(f, "futures"),
            Self::Delivery => write!(f, "delivery"),
            Self::Options => write!(f, "options"),
        }
    }
}

/// Gate.io order side
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateioOrderSide {
    /// Buy order
    #[serde(rename = "buy")]
    Buy,
    /// Sell order
    #[serde(rename = "sell")]
    Sell,
}

impl fmt::Display for GateioOrderSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Buy => write!(f, "buy"),
            Self::Sell => write!(f, "sell"),
        }
    }
}

/// Gate.io order type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateioOrderType {
    /// Limit order
    #[serde(rename = "limit")]
    Limit,
    /// Market order
    #[serde(rename = "market")]
    Market,
}

impl fmt::Display for GateioOrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit => write!(f, "limit"),
            Self::Market => write!(f, "market"),
        }
    }
}

/// Gate.io time in force
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GateioTimeInForce {
    /// Good Till Cancelled
    #[serde(rename = "gtc")]
    GTC,
    /// Immediate Or Cancel
    #[serde(rename = "ioc")]
    IOC,
    /// Post Only
    #[serde(rename = "poc")]
    POC,
    /// Fill Or Kill
    #[serde(rename = "fok")]
    FOK,
}

impl fmt::Display for GateioTimeInForce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GTC => write!(f, "gtc"),
            Self::IOC => write!(f, "ioc"),
            Self::POC => write!(f, "poc"),
            Self::FOK => write!(f, "fok"),
        }
    }
}

/// Gate.io order status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateioOrderStatus {
    /// Order is open and active
    #[serde(rename = "open")]
    Open,
    /// Order is closed (filled or cancelled)
    #[serde(rename = "closed")]
    Closed,
    /// Order is cancelled
    #[serde(rename = "cancelled")]
    Cancelled,
}

impl fmt::Display for GateioOrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Closed => write!(f, "closed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Gate.io account type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GateioAccountType {
    /// Spot account
    Spot,
    /// Margin account
    Margin,
    /// Futures account
    Futures,
    /// Delivery account
    Delivery,
    /// Options account
    Options,
    /// Unified account
    Unified,
}

impl fmt::Display for GateioAccountType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spot => write!(f, "spot"),
            Self::Margin => write!(f, "margin"),
            Self::Futures => write!(f, "futures"),
            Self::Delivery => write!(f, "delivery"),
            Self::Options => write!(f, "options"),
            Self::Unified => write!(f, "unified"),
        }
    }
}

/// Gate.io WebSocket channel types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GateioWsChannel {
    /// Spot market ticker channel
    SpotTicker { currency_pair: String },
    /// Spot order book channel
    SpotOrderBook { currency_pair: String },
    /// Spot trades channel
    SpotTrades { currency_pair: String },
    /// Spot user trades channel (authenticated)
    SpotUserTrades { currency_pair: Option<String> },
    /// Spot user orders channel (authenticated)
    SpotUserOrders { currency_pair: Option<String> },
    /// Futures tickers channel
    FuturesTicker { contract: String },
    /// Futures order book channel
    FuturesOrderBook { contract: String },
    /// Futures trades channel
    FuturesTrades { contract: String },
    /// Futures user trades channel (authenticated)
    FuturesUserTrades { contract: Option<String> },
    /// Futures user orders channel (authenticated)
    FuturesUserOrders { contract: Option<String> },
    /// Futures positions channel (authenticated)
    FuturesPositions { user_id: String },
}

impl fmt::Display for GateioWsChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpotTicker { currency_pair } => write!(f, "spot.tickers:{}", currency_pair),
            Self::SpotOrderBook { currency_pair } => write!(f, "spot.order_book:{}", currency_pair),
            Self::SpotTrades { currency_pair } => write!(f, "spot.trades:{}", currency_pair),
            Self::SpotUserTrades { currency_pair } => {
                if let Some(pair) = currency_pair {
                    write!(f, "spot.usertrades:{}", pair)
                } else {
                    write!(f, "spot.usertrades")
                }
            }
            Self::SpotUserOrders { currency_pair } => {
                if let Some(pair) = currency_pair {
                    write!(f, "spot.orders:{}", pair)
                } else {
                    write!(f, "spot.orders")
                }
            }
            Self::FuturesTicker { contract } => write!(f, "futures.tickers:{}", contract),
            Self::FuturesOrderBook { contract } => write!(f, "futures.order_book:{}", contract),
            Self::FuturesTrades { contract } => write!(f, "futures.trades:{}", contract),
            Self::FuturesUserTrades { contract } => {
                if let Some(c) = contract {
                    write!(f, "futures.usertrades:{}", c)
                } else {
                    write!(f, "futures.usertrades")
                }
            }
            Self::FuturesUserOrders { contract } => {
                if let Some(c) = contract {
                    write!(f, "futures.orders:{}", c)
                } else {
                    write!(f, "futures.orders")
                }
            }
            Self::FuturesPositions { user_id } => write!(f, "futures.positions:{}", user_id),
        }
    }
}
