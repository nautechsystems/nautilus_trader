// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Strongly-typed WebSocket subscription topic builders.
//!
//! Topic wire format: `"{symbol}@{stream_type}"` — e.g. `"BTC-USD@depth20"`.

use std::fmt;

/// Order book depth levels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DepthLevel {
    D5,
    #[default]
    D10,
    D20,
}

impl DepthLevel {
    fn as_str(self) -> &'static str {
        match self {
            DepthLevel::D5 => "5",
            DepthLevel::D10 => "10",
            DepthLevel::D20 => "20",
        }
    }
}

/// A typed Bullet WebSocket subscription topic.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Topic {
    /// `{symbol}@depth{levels}` — incremental L2 book deltas.
    Depth { symbol: String, levels: DepthLevel },
    /// `{symbol}@bookTicker` — best bid/ask.
    BookTicker { symbol: String },
    /// `{symbol}@aggTrade` — aggregated trades.
    AggTrade { symbol: String },
    /// `{symbol}@markPrice` — mark price + funding rate.
    MarkPrice { symbol: String },
    /// `user.orders@{address}` — authenticated order updates.
    UserOrders { address: String },
}

impl Topic {
    /// L2 depth stream (incremental).
    pub fn depth(symbol: impl Into<String>, levels: DepthLevel) -> Self {
        Self::Depth { symbol: symbol.into(), levels }
    }

    /// Best bid/ask stream.
    pub fn book_ticker(symbol: impl Into<String>) -> Self {
        Self::BookTicker { symbol: symbol.into() }
    }

    /// Aggregated trade stream.
    pub fn agg_trade(symbol: impl Into<String>) -> Self {
        Self::AggTrade { symbol: symbol.into() }
    }

    /// Mark price / funding rate stream.
    pub fn mark_price(symbol: impl Into<String>) -> Self {
        Self::MarkPrice { symbol: symbol.into() }
    }

    /// Authenticated order update stream for an address.
    pub fn user_orders(address: impl Into<String>) -> Self {
        Self::UserOrders { address: address.into() }
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Depth { symbol, levels } => {
                write!(f, "{symbol}@depth{}", levels.as_str())
            }
            Self::BookTicker { symbol } => write!(f, "{symbol}@bookTicker"),
            Self::AggTrade { symbol } => write!(f, "{symbol}@aggTrade"),
            Self::MarkPrice { symbol } => write!(f, "{symbol}@markPrice"),
            Self::UserOrders { address } => write!(f, "{address}@user.orders"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_strings() {
        assert_eq!(
            Topic::depth("BTC-USD", DepthLevel::D20).to_string(),
            "BTC-USD@depth20"
        );
        assert_eq!(
            Topic::book_ticker("ETH-USD").to_string(),
            "ETH-USD@bookTicker"
        );
        assert_eq!(
            Topic::agg_trade("SOL-USD").to_string(),
            "SOL-USD@aggTrade"
        );
        assert_eq!(
            Topic::mark_price("BTC-USD").to_string(),
            "BTC-USD@markPrice"
        );
        assert_eq!(
            Topic::user_orders("abc123").to_string(),
            "abc123@user.orders"
        );
    }
}
