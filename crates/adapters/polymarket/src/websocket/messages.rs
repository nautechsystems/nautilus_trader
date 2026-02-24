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

//! WebSocket message types for the Polymarket CLOB API.

use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::{
    enums::{
        PolymarketEventType, PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOrderStatus,
        PolymarketOrderType, PolymarketOutcome, PolymarketTradeStatus,
    },
    models::PolymarketMakerOrder,
};

/// A user order status update from the WebSocket user channel.
///
/// References: <https://docs.polymarket.com/developers/CLOB/websocket/user-channel#order-message>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketUserOrder {
    pub asset_id: Ustr,
    pub associate_trades: Option<Vec<String>>,
    pub created_at: String,
    pub expiration: Option<String>,
    pub id: String,
    pub maker_address: Ustr,
    pub market: Ustr,
    pub order_owner: Ustr,
    pub order_type: PolymarketOrderType,
    pub original_size: String,
    pub outcome: PolymarketOutcome,
    pub owner: Ustr,
    pub price: String,
    pub side: PolymarketOrderSide,
    pub size_matched: String,
    pub status: PolymarketOrderStatus,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub event_type: PolymarketEventType,
}

/// A user trade update from the WebSocket user channel.
///
/// References: <https://docs.polymarket.com/developers/CLOB/websocket/user-channel>
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketUserTrade {
    pub asset_id: Ustr,
    pub bucket_index: u64,
    pub fee_rate_bps: String,
    pub id: String,
    pub last_update: String,
    pub maker_address: Ustr,
    pub maker_orders: Vec<PolymarketMakerOrder>,
    pub market: Ustr,
    pub match_time: String,
    pub outcome: PolymarketOutcome,
    pub owner: Ustr,
    pub price: String,
    pub side: PolymarketOrderSide,
    pub size: String,
    pub status: PolymarketTradeStatus,
    pub taker_order_id: String,
    pub timestamp: String,
    pub trade_owner: Ustr,
    pub trader_side: PolymarketLiquiditySide,
    #[serde(rename = "type")]
    pub event_type: PolymarketEventType,
}

/// A single price level in an order book snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketBookLevel {
    pub price: String,
    pub size: String,
}

/// An order book snapshot from the WebSocket market channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketBookSnapshot {
    pub market: Ustr,
    pub asset_id: Ustr,
    pub bids: Vec<PolymarketBookLevel>,
    pub asks: Vec<PolymarketBookLevel>,
    pub timestamp: String,
}

/// A single price change entry within a quotes message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketQuote {
    pub asset_id: Ustr,
    pub price: String,
    pub side: PolymarketOrderSide,
    pub size: String,
    pub hash: String,
    pub best_bid: String,
    pub best_ask: String,
}

/// A price change (quotes) message from the WebSocket market channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketQuotes {
    pub market: Ustr,
    pub price_changes: Vec<PolymarketQuote>,
    pub timestamp: String,
}

/// A last trade price message from the WebSocket market channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketTrade {
    pub market: Ustr,
    pub asset_id: Ustr,
    pub fee_rate_bps: String,
    pub price: String,
    pub side: PolymarketOrderSide,
    pub size: String,
    pub timestamp: String,
}

/// A tick size change notification from the WebSocket market channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketTickSizeChange {
    pub market: Ustr,
    pub asset_id: Ustr,
    pub new_tick_size: String,
    pub old_tick_size: String,
    pub timestamp: String,
}

/// An envelope for tagged WebSocket market channel messages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum MarketWsMessage {
    #[serde(rename = "book")]
    Book(PolymarketBookSnapshot),
    #[serde(rename = "price_change")]
    PriceChange(PolymarketQuotes),
    #[serde(rename = "last_trade_price")]
    LastTradePrice(PolymarketTrade),
    #[serde(rename = "tick_size_change")]
    TickSizeChange(PolymarketTickSizeChange),
}

/// An envelope for tagged WebSocket user channel messages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum UserWsMessage {
    #[serde(rename = "order")]
    Order(PolymarketUserOrder),
    #[serde(rename = "trade")]
    Trade(PolymarketUserTrade),
}
