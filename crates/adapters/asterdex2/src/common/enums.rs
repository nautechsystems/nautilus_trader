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

//! Asterdex specific enums and types.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Asterdex market type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AsterdexMarketType {
    /// Spot market
    Spot,
    /// Futures market
    Futures,
}

impl fmt::Display for AsterdexMarketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spot => write!(f, "spot"),
            Self::Futures => write!(f, "futures"),
        }
    }
}

/// Asterdex order side
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AsterdexOrderSide {
    Buy,
    Sell,
}

impl fmt::Display for AsterdexOrderSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Buy => write!(f, "BUY"),
            Self::Sell => write!(f, "SELL"),
        }
    }
}

/// Asterdex order type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AsterdexOrderType {
    Limit,
    Market,
    Stop,
    StopMarket,
    TakeProfit,
    TakeProfitMarket,
    TrailingStopMarket,
}

impl fmt::Display for AsterdexOrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit => write!(f, "LIMIT"),
            Self::Market => write!(f, "MARKET"),
            Self::Stop => write!(f, "STOP"),
            Self::StopMarket => write!(f, "STOP_MARKET"),
            Self::TakeProfit => write!(f, "TAKE_PROFIT"),
            Self::TakeProfitMarket => write!(f, "TAKE_PROFIT_MARKET"),
            Self::TrailingStopMarket => write!(f, "TRAILING_STOP_MARKET"),
        }
    }
}

/// Asterdex time in force
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AsterdexTimeInForce {
    /// Good Till Cancel
    Gtc,
    /// Immediate Or Cancel
    Ioc,
    /// Fill Or Kill
    Fok,
    /// Good Till Crossing (post-only)
    Gtx,
    /// Hidden order
    Hidden,
}

impl fmt::Display for AsterdexTimeInForce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gtc => write!(f, "GTC"),
            Self::Ioc => write!(f, "IOC"),
            Self::Fok => write!(f, "FOK"),
            Self::Gtx => write!(f, "GTX"),
            Self::Hidden => write!(f, "HIDDEN"),
        }
    }
}

/// Asterdex order status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AsterdexOrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
    Expired,
}

impl fmt::Display for AsterdexOrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::New => write!(f, "NEW"),
            Self::PartiallyFilled => write!(f, "PARTIALLY_FILLED"),
            Self::Filled => write!(f, "FILLED"),
            Self::Canceled => write!(f, "CANCELED"),
            Self::Rejected => write!(f, "REJECTED"),
            Self::Expired => write!(f, "EXPIRED"),
        }
    }
}

/// Asterdex position side (futures)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AsterdexPositionSide {
    Both,
    Long,
    Short,
}

impl fmt::Display for AsterdexPositionSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Both => write!(f, "BOTH"),
            Self::Long => write!(f, "LONG"),
            Self::Short => write!(f, "SHORT"),
        }
    }
}

/// Asterdex WebSocket channel types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AsterdexWsChannel {
    // Spot channels
    SpotAggTrade { symbol: String },
    SpotTrade { symbol: String },
    SpotKline { symbol: String, interval: String },
    SpotTicker { symbol: String },
    SpotBookTicker { symbol: String },
    SpotDepth { symbol: String, levels: Option<u16> },
    // Futures channels
    FuturesAggTrade { symbol: String },
    FuturesKline { symbol: String, interval: String },
    FuturesMarkPrice { symbol: String },
    FuturesTicker { symbol: String },
    FuturesBookTicker { symbol: String },
    FuturesDepth { symbol: String, levels: Option<u16> },
    // User data
    SpotUserData { listen_key: String },
    FuturesUserData { listen_key: String },
}

impl fmt::Display for AsterdexWsChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpotAggTrade { symbol } => write!(f, "{}@aggTrade", symbol.to_lowercase()),
            Self::SpotTrade { symbol } => write!(f, "{}@trade", symbol.to_lowercase()),
            Self::SpotKline { symbol, interval } => {
                write!(f, "{}@kline_{}", symbol.to_lowercase(), interval)
            }
            Self::SpotTicker { symbol } => write!(f, "{}@ticker", symbol.to_lowercase()),
            Self::SpotBookTicker { symbol } => write!(f, "{}@bookTicker", symbol.to_lowercase()),
            Self::SpotDepth { symbol, levels } => {
                if let Some(l) = levels {
                    write!(f, "{}@depth{}", symbol.to_lowercase(), l)
                } else {
                    write!(f, "{}@depth", symbol.to_lowercase())
                }
            }
            Self::FuturesAggTrade { symbol } => write!(f, "{}@aggTrade", symbol.to_lowercase()),
            Self::FuturesKline { symbol, interval } => {
                write!(f, "{}@kline_{}", symbol.to_lowercase(), interval)
            }
            Self::FuturesMarkPrice { symbol } => write!(f, "{}@markPrice", symbol.to_lowercase()),
            Self::FuturesTicker { symbol } => write!(f, "{}@ticker", symbol.to_lowercase()),
            Self::FuturesBookTicker { symbol } => {
                write!(f, "{}@bookTicker", symbol.to_lowercase())
            }
            Self::FuturesDepth { symbol, levels } => {
                if let Some(l) = levels {
                    write!(f, "{}@depth{}", symbol.to_lowercase(), l)
                } else {
                    write!(f, "{}@depth", symbol.to_lowercase())
                }
            }
            Self::SpotUserData { listen_key } => write!(f, "{}", listen_key),
            Self::FuturesUserData { listen_key } => write!(f, "{}", listen_key),
        }
    }
}

impl AsterdexWsChannel {
    /// Converts the channel to its WebSocket stream name
    pub fn to_stream_name(&self) -> String {
        self.to_string()
    }
}
