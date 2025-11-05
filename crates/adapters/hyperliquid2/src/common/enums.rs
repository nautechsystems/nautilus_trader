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

//! Hyperliquid enumeration types.

use serde::{Deserialize, Serialize};

/// Hyperliquid order side
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HyperliquidOrderSide {
    /// Buy order
    #[serde(rename = "A")]
    Buy,
    /// Sell order
    #[serde(rename = "B")]
    Sell,
}

/// Hyperliquid order type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HyperliquidOrderType {
    /// Limit order
    #[serde(rename = "limit")]
    Limit,
    /// Market order
    #[serde(rename = "market")]
    Market,
    /// Stop market order
    #[serde(rename = "stopMarket")]
    StopMarket,
    /// Stop limit order
    #[serde(rename = "stopLimit")]
    StopLimit,
    /// Take profit market order
    #[serde(rename = "takeProfitMarket")]
    TakeProfitMarket,
    /// Take profit limit order
    #[serde(rename = "takeProfitLimit")]
    TakeProfitLimit,
}

/// Hyperliquid time in force
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HyperliquidTimeInForce {
    /// Good till cancel
    #[serde(rename = "Gtc")]
    Gtc,
    /// Immediate or cancel
    #[serde(rename = "Ioc")]
    Ioc,
    /// Fill or kill
    #[serde(rename = "Alo")]
    Alo, // Add liquidity only (post-only)
}

/// Hyperliquid order status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HyperliquidOrderStatus {
    /// Order is open
    Open,
    /// Order is filled
    Filled,
    /// Order is canceled
    Canceled,
    /// Order is rejected
    Rejected,
    /// Order triggered
    Triggered,
}

/// Hyperliquid WebSocket channel type
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HyperliquidWsChannel {
    /// Subscribe to all market data
    AllMids,
    /// Subscribe to trades for a specific coin
    Trades {
        /// Trading pair (e.g., "BTC")
        coin: String,
    },
    /// Subscribe to L2 order book
    L2Book {
        /// Trading pair (e.g., "BTC")
        coin: String,
    },
    /// Subscribe to candles
    Candle {
        /// Trading pair (e.g., "BTC")
        coin: String,
        /// Interval (e.g., "1m", "5m", "1h")
        interval: String,
    },
    /// Subscribe to user events
    User {
        /// User address
        user: String,
    },
    /// Subscribe to user fills
    UserFills {
        /// User address
        user: String,
    },
}

impl HyperliquidWsChannel {
    /// Returns the channel name for subscription
    pub fn channel_name(&self) -> String {
        match self {
            Self::AllMids => "allMids".to_string(),
            Self::Trades { coin } => format!("trades@{}", coin),
            Self::L2Book { coin } => format!("l2Book@{}", coin),
            Self::Candle { coin, interval } => format!("candle@{}@{}", coin, interval),
            Self::User { user } => format!("user@{}", user),
            Self::UserFills { user } => format!("userFills@{}", user),
        }
    }
}

/// WebSocket subscription status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubscriptionStatus {
    /// Subscription pending
    Pending,
    /// Subscription active
    Subscribed,
    /// Unsubscription pending
    Unsubscribing,
    /// Unsubscribed
    Unsubscribed,
}
