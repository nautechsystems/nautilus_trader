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

//! WebSocket message structures for Lighter.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::common::{
    enums::{LighterOrderSide, LighterOrderStatus},
    models::LighterOrderBook,
};

/// WebSocket subscription message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsSubscribe {
    /// Channel to subscribe to (e.g., "orderbook", "trades", "account").
    pub channel: String,
    /// Additional parameters (e.g., market_id, account_id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// WebSocket unsubscribe message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsUnsubscribe {
    /// Channel to unsubscribe from.
    pub channel: String,
    /// Additional parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// WebSocket order book update message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsOrderBookUpdate {
    /// Market ID.
    pub market_id: u64,
    /// Bid updates (price, quantity).
    pub bids: Vec<(Decimal, Decimal)>,
    /// Ask updates (price, quantity).
    pub asks: Vec<(Decimal, Decimal)>,
    /// Timestamp (Unix nanoseconds).
    pub timestamp: i64,
}

/// WebSocket trade message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsTrade {
    /// Trade ID.
    pub id: String,
    /// Market ID.
    pub market_id: u64,
    /// Trade price.
    pub price: Decimal,
    /// Trade quantity.
    pub quantity: Decimal,
    /// Trade side (from taker's perspective).
    pub side: LighterOrderSide,
    /// Trade timestamp (Unix nanoseconds).
    pub timestamp: i64,
}

/// WebSocket account update message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsAccountUpdate {
    /// Account ID.
    pub account_id: u64,
    /// Updated balances.
    pub balances: Vec<WsBalance>,
}

/// Balance update in account message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsBalance {
    /// Currency symbol.
    pub currency: String,
    /// Available balance.
    pub available: Decimal,
    /// Total balance.
    pub total: Decimal,
    /// Locked balance.
    pub locked: Decimal,
}

/// WebSocket order update message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsOrderUpdate {
    /// Order ID.
    pub id: String,
    /// Client order ID.
    pub client_order_id: Option<String>,
    /// Market ID.
    pub market_id: u64,
    /// Order side.
    pub side: LighterOrderSide,
    /// Order price.
    pub price: Decimal,
    /// Order quantity.
    pub quantity: Decimal,
    /// Filled quantity.
    pub filled_quantity: Decimal,
    /// Remaining quantity.
    pub remaining_quantity: Decimal,
    /// Order status.
    pub status: LighterOrderStatus,
    /// Update timestamp (Unix nanoseconds).
    pub timestamp: i64,
}

/// WebSocket message envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// Order book update.
    OrderBook(WsOrderBookUpdate),
    /// Trade update.
    Trade(WsTrade),
    /// Account update.
    Account(WsAccountUpdate),
    /// Order update.
    Order(WsOrderUpdate),
    /// Ping message.
    Ping,
    /// Pong message.
    Pong,
    /// Subscription confirmation.
    Subscribed {
        /// Channel subscribed to.
        channel: String,
    },
    /// Unsubscription confirmation.
    Unsubscribed {
        /// Channel unsubscribed from.
        channel: String,
    },
    /// Error message.
    Error {
        /// Error message.
        message: String,
    },
}
