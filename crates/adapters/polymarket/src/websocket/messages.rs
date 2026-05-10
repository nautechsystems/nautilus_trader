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
    #[serde(default)]
    pub best_bid: Option<String>,
    #[serde(default)]
    pub best_ask: Option<String>,
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

/// Event metadata embedded in a new market notification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketNewMarketEvent {
    pub id: String,
    pub ticker: String,
    pub slug: String,
    pub title: String,
    pub description: String,
}

/// A new market notification from the WebSocket market channel.
///
/// Only received when `subscribe_new_markets` is enabled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketNewMarket {
    pub id: String,
    pub question: String,
    pub market: Ustr,
    pub slug: String,
    pub description: String,
    pub assets_ids: Vec<String>,
    pub outcomes: Vec<String>,
    pub timestamp: String,
    pub tags: Vec<String>,
    pub condition_id: String,
    pub active: bool,
    pub clob_token_ids: Vec<String>,
    #[serde(default)]
    pub order_price_min_tick_size: Option<String>,
    #[serde(default)]
    pub group_item_title: Option<String>,
    #[serde(default)]
    pub event_message: Option<PolymarketNewMarketEvent>,
}

/// A market resolved notification from the WebSocket market channel.
///
/// Only received when `subscribe_new_markets` is enabled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketMarketResolved {
    pub id: String,
    pub market: Ustr,
    pub assets_ids: Vec<String>,
    pub winning_asset_id: String,
    pub winning_outcome: String,
    pub timestamp: String,
    pub tags: Vec<String>,
}

/// A best bid/ask notification from the WebSocket market channel.
///
/// Only received when `subscribe_new_markets` is enabled.
/// Data is already covered by existing PriceChange/Book handlers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolymarketBestBidAsk {
    pub market: Ustr,
    pub asset_id: Ustr,
    pub best_bid: String,
    pub best_ask: String,
    pub spread: String,
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
    #[serde(rename = "new_market")]
    NewMarket(Box<PolymarketNewMarket>),
    #[serde(rename = "market_resolved")]
    MarketResolved(PolymarketMarketResolved),
    #[serde(rename = "best_bid_ask")]
    BestBidAsk(PolymarketBestBidAsk),
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

/// Output message type from the Polymarket WebSocket handler.
#[derive(Debug)]
pub enum PolymarketWsMessage {
    Market(MarketWsMessage),
    User(UserWsMessage),
    /// Emitted when the underlying WebSocket reconnects.
    Reconnected,
}

/// Auth payload embedded in user-channel subscribe messages.
#[derive(Debug, Serialize)]
pub struct PolymarketWsAuth {
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

/// Initial market-channel subscribe request sent for a fresh WebSocket session.
///
/// Wire format: `{"assets_ids": [...], "type": "market"}`
/// When `custom_feature_enabled` is true, enables new market and market resolved events.
#[derive(Debug, Serialize)]
pub struct MarketInitialSubscribeRequest {
    pub assets_ids: Vec<String>,
    #[serde(rename = "type")]
    pub msg_type: &'static str,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub custom_feature_enabled: bool,
}

/// Incremental market-channel subscribe request sent after the initial session subscribe.
///
/// Wire format: `{"assets_ids": [...], "operation": "subscribe"}`
/// When `custom_feature_enabled` is true, enables new market and market resolved events.
#[derive(Debug, Serialize)]
pub struct MarketSubscribeRequest {
    pub assets_ids: Vec<String>,
    pub operation: &'static str,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub custom_feature_enabled: bool,
}

/// Market-channel dynamic unsubscribe request sent during an active session.
///
/// Wire format: `{"assets_ids": [...], "operation": "unsubscribe"}`
#[derive(Debug, Serialize)]
pub struct MarketUnsubscribeRequest {
    pub assets_ids: Vec<String>,
    pub operation: &'static str,
}

/// User-channel subscribe request sent on connect.
///
/// Wire format: `{"auth": {...}, "markets": [], "assets_ids": [], "type": "user"}`
#[derive(Debug, Serialize)]
pub struct UserSubscribeRequest {
    pub auth: PolymarketWsAuth,
    pub markets: Vec<String>,
    pub assets_ids: Vec<String>,
    #[serde(rename = "type")]
    pub msg_type: &'static str,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::enums::{
        PolymarketEventType, PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOrderStatus,
        PolymarketOrderType, PolymarketOutcome, PolymarketTradeStatus,
    };

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    #[rstest]
    fn test_book_snapshot() {
        let snap: PolymarketBookSnapshot = load("ws_book_snapshot.json");

        assert_eq!(
            snap.asset_id.as_str(),
            "71321045679252212594626385532706912750332728571942532289631379312455583992563"
        );
        assert_eq!(snap.bids.len(), 3);
        assert_eq!(snap.asks.len(), 3);
        assert_eq!(snap.bids[0].price, "0.48");
        assert_eq!(snap.bids[0].size, "500.0");
        assert_eq!(snap.asks[0].price, "0.53");
        assert_eq!(snap.timestamp, "1703875200000");
    }

    #[rstest]
    fn test_book_snapshot_roundtrip() {
        let snap: PolymarketBookSnapshot = load("ws_book_snapshot.json");
        let json = serde_json::to_string(&snap).unwrap();
        let snap2: PolymarketBookSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, snap2);
    }

    #[rstest]
    fn test_quotes() {
        let quotes: PolymarketQuotes = load("ws_quotes.json");

        assert_eq!(quotes.price_changes.len(), 2);
        assert_eq!(quotes.price_changes[0].side, PolymarketOrderSide::Buy);
        assert_eq!(quotes.price_changes[0].price, "0.51");
        assert_eq!(quotes.price_changes[0].best_bid.as_deref(), Some("0.51"));
        assert_eq!(quotes.price_changes[0].best_ask.as_deref(), Some("0.52"));
        assert_eq!(quotes.price_changes[1].side, PolymarketOrderSide::Sell);
        assert_eq!(quotes.timestamp, "1703875201000");
    }

    #[rstest]
    fn test_last_trade() {
        let trade: PolymarketTrade = load("ws_last_trade.json");

        assert_eq!(trade.price, "0.51");
        assert_eq!(trade.size, "25.0");
        assert_eq!(trade.side, PolymarketOrderSide::Buy);
        assert_eq!(trade.fee_rate_bps, "0");
        assert_eq!(trade.timestamp, "1703875202000");
    }

    #[rstest]
    fn test_tick_size_change() {
        let msg: PolymarketTickSizeChange = load("ws_tick_size_change.json");

        assert_eq!(msg.new_tick_size, "0.01");
        assert_eq!(msg.old_tick_size, "0.1");
        assert_eq!(msg.timestamp, "1703875210000");
    }

    #[rstest]
    fn test_user_order_placement() {
        let order: PolymarketUserOrder = load("ws_user_order_placement.json");

        assert_eq!(order.event_type, PolymarketEventType::Placement);
        assert_eq!(order.status, PolymarketOrderStatus::Live);
        assert_eq!(order.side, PolymarketOrderSide::Buy);
        assert_eq!(order.order_type, PolymarketOrderType::GTC);
        assert_eq!(order.outcome, PolymarketOutcome::yes());
        assert_eq!(order.original_size, "100.0");
        assert_eq!(order.size_matched, "0.0");
        assert!(order.associate_trades.is_none());
        assert!(order.expiration.is_none());
    }

    #[rstest]
    fn test_user_order_update() {
        let order: PolymarketUserOrder = load("ws_user_order_update.json");

        assert_eq!(order.event_type, PolymarketEventType::Update);
        assert_eq!(order.size_matched, "25.0");
        assert_eq!(
            order.associate_trades.as_deref(),
            Some(&["trade-0xabcdef1234".to_string()][..])
        );
    }

    #[rstest]
    fn test_user_order_cancellation() {
        let order: PolymarketUserOrder = load("ws_user_order_cancellation.json");

        assert_eq!(order.event_type, PolymarketEventType::Cancellation);
        assert_eq!(order.status, PolymarketOrderStatus::Canceled);
        assert_eq!(order.size_matched, "0.0");
    }

    #[rstest]
    fn test_user_trade() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");

        assert_eq!(trade.event_type, PolymarketEventType::Trade);
        assert_eq!(trade.status, PolymarketTradeStatus::Confirmed);
        assert_eq!(trade.side, PolymarketOrderSide::Buy);
        assert_eq!(trade.trader_side, PolymarketLiquiditySide::Taker);
        assert_eq!(trade.price, "0.5");
        assert_eq!(trade.size, "25.0");
        assert_eq!(trade.fee_rate_bps, "0");
        assert_eq!(trade.bucket_index, 1);
        assert_eq!(trade.maker_orders.len(), 1);
        assert_eq!(
            trade.taker_order_id,
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12"
        );
    }

    #[rstest]
    fn test_market_ws_message_book() {
        let msg: MarketWsMessage = load("ws_market_book_msg.json");

        assert!(matches!(msg, MarketWsMessage::Book(_)));
        if let MarketWsMessage::Book(snap) = msg {
            assert_eq!(snap.bids.len(), 2);
            assert_eq!(snap.asks.len(), 2);
            assert_eq!(snap.timestamp, "1703875200000");
        }
    }

    #[rstest]
    fn test_market_ws_message_price_change() {
        let msg: MarketWsMessage = load("ws_market_price_change_msg.json");

        assert!(matches!(msg, MarketWsMessage::PriceChange(_)));
        if let MarketWsMessage::PriceChange(quotes) = msg {
            assert_eq!(quotes.price_changes.len(), 1);
        }
    }

    #[rstest]
    fn test_market_ws_message_last_trade_price() {
        let msg: MarketWsMessage = load("ws_market_last_trade_msg.json");

        assert!(matches!(msg, MarketWsMessage::LastTradePrice(_)));
        if let MarketWsMessage::LastTradePrice(trade) = msg {
            assert_eq!(trade.price, "0.51");
        }
    }

    #[rstest]
    fn test_market_ws_message_tick_size_change() {
        let msg: MarketWsMessage = load("ws_market_tick_size_msg.json");

        assert!(matches!(msg, MarketWsMessage::TickSizeChange(_)));
        if let MarketWsMessage::TickSizeChange(change) = msg {
            assert_eq!(change.new_tick_size, "0.01");
            assert_eq!(change.old_tick_size, "0.1");
        }
    }

    #[rstest]
    fn test_user_ws_message_order() {
        let msg: UserWsMessage = load("ws_user_order_msg.json");

        assert!(matches!(msg, UserWsMessage::Order(_)));
        if let UserWsMessage::Order(order) = msg {
            assert_eq!(order.event_type, PolymarketEventType::Placement);
            assert_eq!(order.side, PolymarketOrderSide::Buy);
        }
    }

    #[rstest]
    fn test_user_ws_message_trade() {
        let msg: UserWsMessage = load("ws_user_trade_msg.json");

        assert!(matches!(msg, UserWsMessage::Trade(_)));
        if let UserWsMessage::Trade(trade) = msg {
            assert_eq!(trade.event_type, PolymarketEventType::Trade);
            assert_eq!(trade.status, PolymarketTradeStatus::Confirmed);
        }
    }

    #[rstest]
    fn test_market_ws_message_new_market() {
        let msg: MarketWsMessage = load("ws_market_new_market_msg.json");

        assert!(matches!(msg, MarketWsMessage::NewMarket(_)));
        if let MarketWsMessage::NewMarket(nm) = msg {
            assert_eq!(nm.id, "1031769");
            assert_eq!(nm.slug, "nvda-above-240-on-january-30-2026");
            assert_eq!(
                nm.condition_id,
                "0x311d0c4b6671ab54af4970c06fcf58662516f5168997bdda209ec3db5aa6b0c1"
            );
            assert!(nm.active);
            assert_eq!(nm.outcomes.len(), 2);
            assert_eq!(nm.clob_token_ids.len(), 2);
            assert_eq!(nm.order_price_min_tick_size.as_deref(), Some("0.01"));

            let event = nm
                .event_message
                .as_ref()
                .expect("event_message should be parsed");
            assert_eq!(event.id, "125819");
            assert_eq!(event.ticker, "nvda-above-in-january-2026");
            assert_eq!(event.slug, "nvda-above-in-january-2026");
            assert_eq!(
                event.title,
                "Will NVIDIA (NVDA) close above ___ end of January?"
            );
        }
    }

    #[rstest]
    fn test_market_ws_message_resolved() {
        let msg: MarketWsMessage = load("ws_market_resolved_msg.json");

        assert!(matches!(msg, MarketWsMessage::MarketResolved(_)));
        if let MarketWsMessage::MarketResolved(mr) = msg {
            assert_eq!(mr.id, "1031769");
            assert_eq!(mr.winning_outcome, "Yes");
            assert_eq!(mr.assets_ids.len(), 2);
            assert_eq!(
                mr.winning_asset_id,
                "76043073756653678226373981964075571318267289248134717369284518995922789326425"
            );
        }
    }

    #[rstest]
    fn test_market_ws_message_best_bid_ask() {
        let msg: MarketWsMessage = load("ws_market_best_bid_ask_msg.json");

        assert!(matches!(msg, MarketWsMessage::BestBidAsk(_)));
        if let MarketWsMessage::BestBidAsk(bba) = msg {
            assert_eq!(bba.best_bid, "0.73");
            assert_eq!(bba.best_ask, "0.77");
            assert_eq!(bba.spread, "0.04");
        }
    }
}
