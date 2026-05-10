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

//! WebSocket message types for the Coinbase Advanced Trade API.
//!
//! All incoming messages share an envelope with `channel`, `timestamp`,
//! `sequence_num`, and a channel-specific `events` array. Outgoing
//! subscription messages use a flat format with `type`, `product_ids`,
//! `channel`, and `jwt`.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::{
    enums::{
        CoinbaseContractExpiryType, CoinbaseMarginLevel, CoinbaseMarginWindowType,
        CoinbaseOrderSide, CoinbaseOrderStatus, CoinbaseOrderType, CoinbaseProductStatus,
        CoinbaseProductType, CoinbaseRiskManagedBy, CoinbaseTimeInForce, CoinbaseTriggerStatus,
        CoinbaseWsChannel,
    },
    parse::deserialize_decimal_from_str,
};

/// Subscribe or unsubscribe request sent to the WebSocket.
///
/// Public channels (`level2`, `market_trades`, `ticker`, etc.) do not require
/// a JWT. Set `jwt` to `None` for unauthenticated subscriptions; the field
/// is omitted from the serialized JSON.
#[derive(Debug, Clone, Serialize)]
pub struct CoinbaseWsSubscription {
    /// `"subscribe"` or `"unsubscribe"`.
    #[serde(rename = "type")]
    pub msg_type: CoinbaseWsAction,
    /// Product IDs to subscribe to (omitted for channel-level subscriptions).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub product_ids: Vec<Ustr>,
    /// Channel name (subscription-side, e.g. `level2`).
    pub channel: CoinbaseWsChannel,
    /// JWT for authentication (required for `user` and `futures_balance_summary`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwt: Option<String>,
}

/// WebSocket subscription action type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoinbaseWsAction {
    Subscribe,
    Unsubscribe,
}

/// Top-level WebSocket message dispatched by channel.
///
/// Uses serde internally-tagged enum on the `channel` field so each variant
/// deserializes only the events relevant to that channel.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "channel")]
pub enum CoinbaseWsMessage {
    /// Order book snapshot or incremental update.
    #[serde(rename = "l2_data")]
    L2Data {
        timestamp: String,
        sequence_num: u64,
        events: Vec<WsL2DataEvent>,
    },

    /// Market trade executions.
    #[serde(rename = "market_trades")]
    MarketTrades {
        timestamp: String,
        sequence_num: u64,
        events: Vec<WsMarketTradesEvent>,
    },

    /// Price ticker for a single product.
    #[serde(rename = "ticker")]
    Ticker {
        timestamp: String,
        sequence_num: u64,
        events: Vec<WsTickerEvent>,
    },

    /// Batched ticker updates for multiple products.
    #[serde(rename = "ticker_batch")]
    TickerBatch {
        timestamp: String,
        sequence_num: u64,
        events: Vec<WsTickerEvent>,
    },

    /// OHLC candle updates.
    #[serde(rename = "candles")]
    Candles {
        timestamp: String,
        sequence_num: u64,
        events: Vec<WsCandlesEvent>,
    },

    /// User order status updates.
    ///
    /// The feed handler deserializes this channel but ignores it until the
    /// execution client is wired.
    #[serde(rename = "user")]
    User {
        timestamp: String,
        sequence_num: u64,
        events: Vec<WsUserEvent>,
    },

    /// Connection heartbeat.
    #[serde(rename = "heartbeats")]
    Heartbeats {
        timestamp: String,
        sequence_num: u64,
        events: Vec<WsHeartbeatEvent>,
    },

    /// Futures balance summary (requires auth).
    ///
    /// The feed handler deserializes this channel but ignores it until account
    /// state handling is added.
    #[serde(rename = "futures_balance_summary")]
    FuturesBalanceSummary {
        timestamp: String,
        sequence_num: u64,
        events: Vec<WsFuturesBalanceSummaryEvent>,
    },

    /// System status updates.
    ///
    /// The feed handler deserializes this channel but ignores it until venue
    /// status handling is added.
    #[serde(rename = "status")]
    Status {
        timestamp: String,
        sequence_num: u64,
        events: Vec<WsStatusEvent>,
    },

    /// Subscription confirmation.
    #[serde(rename = "subscriptions")]
    Subscriptions {
        timestamp: String,
        sequence_num: u64,
        events: Vec<WsSubscriptionsEvent>,
    },
}

/// L2 data event containing book updates.
#[derive(Debug, Clone, Deserialize)]
pub struct WsL2DataEvent {
    /// `"snapshot"` for initial state, `"update"` for incremental.
    #[serde(rename = "type")]
    pub event_type: WsEventType,
    pub product_id: Ustr,
    pub updates: Vec<WsL2Update>,
}

/// A single order book level update.
#[derive(Debug, Clone, Deserialize)]
pub struct WsL2Update {
    pub side: WsBookSide,
    pub event_time: String,
    pub price_level: String,
    pub new_quantity: String,
}

/// Book side in L2 data messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WsBookSide {
    Bid,
    Offer,
}

/// Market trades event.
#[derive(Debug, Clone, Deserialize)]
pub struct WsMarketTradesEvent {
    /// `"snapshot"` or `"update"`.
    #[serde(rename = "type")]
    pub event_type: WsEventType,
    pub trades: Vec<WsTrade>,
}

/// A single trade from the market_trades channel.
#[derive(Debug, Clone, Deserialize)]
pub struct WsTrade {
    pub trade_id: String,
    pub product_id: Ustr,
    pub price: String,
    pub size: String,
    pub side: CoinbaseOrderSide,
    pub time: String,
}

/// Ticker event.
#[derive(Debug, Clone, Deserialize)]
pub struct WsTickerEvent {
    /// `"snapshot"` or `"update"`.
    #[serde(rename = "type")]
    pub event_type: WsEventType,
    pub tickers: Vec<WsTicker>,
}

/// Ticker data for a single product.
#[derive(Debug, Clone, Deserialize)]
pub struct WsTicker {
    pub product_id: Ustr,
    pub price: String,
    pub volume_24_h: String,
    pub low_24_h: String,
    pub high_24_h: String,
    #[serde(default)]
    pub low_52_w: String,
    #[serde(default)]
    pub high_52_w: String,
    pub price_percent_chg_24_h: String,
    pub best_bid: String,
    pub best_bid_quantity: String,
    pub best_ask: String,
    pub best_ask_quantity: String,
}

/// Candles event.
#[derive(Debug, Clone, Deserialize)]
pub struct WsCandlesEvent {
    /// `"snapshot"` or `"update"`.
    #[serde(rename = "type")]
    pub event_type: WsEventType,
    pub candles: Vec<WsCandle>,
}

/// A single candle from the candles channel.
#[derive(Debug, Clone, Deserialize)]
pub struct WsCandle {
    pub start: String,
    pub high: String,
    pub low: String,
    pub open: String,
    pub close: String,
    pub volume: String,
    pub product_id: Ustr,
}

/// User event containing order status updates.
#[derive(Debug, Clone, Deserialize)]
pub struct WsUserEvent {
    /// `"snapshot"` or `"update"`.
    #[serde(rename = "type")]
    pub event_type: WsEventType,
    pub orders: Vec<WsOrderUpdate>,
}

/// Order status update from the user channel.
#[derive(Debug, Clone, Deserialize)]
pub struct WsOrderUpdate {
    pub order_id: String,
    pub client_order_id: String,
    pub contract_expiry_type: CoinbaseContractExpiryType,
    pub cumulative_quantity: String,
    pub leaves_quantity: String,
    pub avg_price: String,
    pub total_fees: String,
    pub status: CoinbaseOrderStatus,
    pub product_id: Ustr,
    pub product_type: CoinbaseProductType,
    pub creation_time: String,
    pub order_side: CoinbaseOrderSide,
    pub order_type: CoinbaseOrderType,
    pub risk_managed_by: CoinbaseRiskManagedBy,
    pub time_in_force: CoinbaseTimeInForce,
    pub trigger_status: CoinbaseTriggerStatus,
    #[serde(default)]
    pub cancel_reason: String,
    #[serde(default)]
    pub reject_reason: String,
    #[serde(default)]
    pub total_value_after_fees: String,
}

/// Heartbeat event.
#[derive(Debug, Clone, Deserialize)]
pub struct WsHeartbeatEvent {
    pub current_time: String,
    pub heartbeat_counter: u64,
}

/// Futures balance summary event.
#[derive(Debug, Clone, Deserialize)]
pub struct WsFuturesBalanceSummaryEvent {
    #[serde(rename = "type")]
    pub event_type: WsEventType,
    pub fcm_balance_summary: WsFcmBalanceSummary,
}

/// Futures balance summary snapshot.
#[derive(Debug, Clone, Deserialize)]
pub struct WsFcmBalanceSummary {
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub futures_buying_power: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub total_usd_balance: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub cbi_usd_balance: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub cfm_usd_balance: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub total_open_orders_hold_amount: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub unrealized_pnl: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub daily_realized_pnl: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub initial_margin: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub available_margin: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub liquidation_threshold: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub liquidation_buffer_amount: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub liquidation_buffer_percentage: Decimal,
    pub intraday_margin_window_measure: WsMarginWindowMeasure,
    pub overnight_margin_window_measure: WsMarginWindowMeasure,
}

/// Margin window summary inside a futures balance snapshot.
#[derive(Debug, Clone, Deserialize)]
pub struct WsMarginWindowMeasure {
    pub margin_window_type: CoinbaseMarginWindowType,
    pub margin_level: CoinbaseMarginLevel,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub initial_margin: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub maintenance_margin: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub liquidation_buffer_percentage: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub total_hold: Decimal,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub futures_buying_power: Decimal,
}

/// Status channel event.
#[derive(Debug, Clone, Deserialize)]
pub struct WsStatusEvent {
    #[serde(rename = "type")]
    pub event_type: WsEventType,
    #[serde(default)]
    pub products: Vec<WsStatusProduct>,
}

/// Status channel product snapshot.
#[derive(Debug, Clone, Deserialize)]
pub struct WsStatusProduct {
    pub product_type: CoinbaseProductType,
    pub id: Ustr,
    pub base_currency: Ustr,
    pub quote_currency: Ustr,
    pub base_increment: String,
    pub quote_increment: String,
    pub display_name: String,
    pub status: CoinbaseProductStatus,
    pub status_message: String,
    #[serde(deserialize_with = "deserialize_decimal_from_str")]
    pub min_market_funds: Decimal,
}

/// Subscription confirmation event.
#[derive(Debug, Clone, Deserialize)]
pub struct WsSubscriptionsEvent {
    pub subscriptions: HashMap<CoinbaseWsChannel, Vec<Ustr>>,
}

/// Event type discriminator for snapshot vs incremental update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WsEventType {
    Snapshot,
    Update,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_fixture;

    #[rstest]
    fn test_deserialize_l2_snapshot() {
        let json = load_test_fixture("ws_l2_data_snapshot.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();

        match msg {
            CoinbaseWsMessage::L2Data {
                timestamp,
                sequence_num,
                events,
            } => {
                assert!(!timestamp.is_empty());
                assert_eq!(sequence_num, 0);
                assert_eq!(events.len(), 1);

                let event = &events[0];
                assert_eq!(event.event_type, WsEventType::Snapshot);
                assert_eq!(event.product_id, "BTC-USD");
                assert!(!event.updates.is_empty());

                let bid = event
                    .updates
                    .iter()
                    .find(|u| u.side == WsBookSide::Bid)
                    .expect("should have a bid update");
                assert!(!bid.price_level.is_empty());
                assert!(!bid.new_quantity.is_empty());
            }
            other => panic!("Expected L2Data, was {other:?}"),
        }
    }

    #[rstest]
    fn test_deserialize_l2_update() {
        let json = load_test_fixture("ws_l2_data_update.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();

        match msg {
            CoinbaseWsMessage::L2Data {
                sequence_num,
                events,
                ..
            } => {
                assert!(sequence_num > 0);
                assert_eq!(events[0].event_type, WsEventType::Update);
            }
            other => panic!("Expected L2Data, was {other:?}"),
        }
    }

    #[rstest]
    fn test_deserialize_market_trades() {
        let json = load_test_fixture("ws_market_trades.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();

        match msg {
            CoinbaseWsMessage::MarketTrades { events, .. } => {
                assert_eq!(events.len(), 1);
                assert!(!events[0].trades.is_empty());

                let trade = &events[0].trades[0];
                assert_eq!(trade.product_id, "BTC-USD");
                assert!(!trade.price.is_empty());
                assert!(!trade.size.is_empty());
                assert!(!trade.trade_id.is_empty());
            }
            other => panic!("Expected MarketTrades, was {other:?}"),
        }
    }

    #[rstest]
    fn test_deserialize_ticker() {
        let json = load_test_fixture("ws_ticker.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();

        match msg {
            CoinbaseWsMessage::Ticker { events, .. } => {
                assert_eq!(events.len(), 1);
                assert!(!events[0].tickers.is_empty());

                let ticker = &events[0].tickers[0];
                assert_eq!(ticker.product_id, "BTC-USD");
                assert!(!ticker.best_bid.is_empty());
                assert!(!ticker.best_ask.is_empty());
                assert!(!ticker.best_bid_quantity.is_empty());
                assert!(!ticker.best_ask_quantity.is_empty());
            }
            other => panic!("Expected Ticker, was {other:?}"),
        }
    }

    #[rstest]
    fn test_deserialize_candles() {
        let json = load_test_fixture("ws_candles.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();

        match msg {
            CoinbaseWsMessage::Candles { events, .. } => {
                assert_eq!(events.len(), 1);
                assert!(!events[0].candles.is_empty());

                let candle = &events[0].candles[0];
                assert_eq!(candle.product_id, "BTC-USD");
                assert!(!candle.open.is_empty());
                assert!(!candle.high.is_empty());
                assert!(!candle.low.is_empty());
                assert!(!candle.close.is_empty());
                assert!(!candle.volume.is_empty());
            }
            other => panic!("Expected Candles, was {other:?}"),
        }
    }

    #[rstest]
    fn test_deserialize_user_order_update() {
        let json = load_test_fixture("ws_user.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();

        match msg {
            CoinbaseWsMessage::User { events, .. } => {
                assert_eq!(events.len(), 1);
                assert!(!events[0].orders.is_empty());

                let order = &events[0].orders[0];
                assert!(!order.order_id.is_empty());
                assert_eq!(order.product_id, "BTC-USD");
                assert_eq!(order.status, CoinbaseOrderStatus::Open);
                assert_eq!(order.order_side, CoinbaseOrderSide::Buy);
                assert_eq!(order.order_type, CoinbaseOrderType::Limit);
                assert_eq!(
                    order.contract_expiry_type,
                    CoinbaseContractExpiryType::Unknown
                );
                assert_eq!(order.product_type, CoinbaseProductType::Spot);
                assert_eq!(order.risk_managed_by, CoinbaseRiskManagedBy::Unknown);
                assert_eq!(order.time_in_force, CoinbaseTimeInForce::GoodUntilCancelled);
                assert_eq!(
                    order.trigger_status,
                    CoinbaseTriggerStatus::InvalidOrderType
                );
            }
            other => panic!("Expected User, was {other:?}"),
        }
    }

    #[rstest]
    fn test_deserialize_heartbeat() {
        let json = load_test_fixture("ws_heartbeats.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();

        match msg {
            CoinbaseWsMessage::Heartbeats { events, .. } => {
                assert_eq!(events.len(), 1);
                assert!(!events[0].current_time.is_empty());
                assert!(events[0].heartbeat_counter > 0);
            }
            other => panic!("Expected Heartbeats, was {other:?}"),
        }
    }

    #[rstest]
    fn test_deserialize_status_channel() {
        let json = r#"{
          "channel": "status",
          "client_id": "",
          "timestamp": "2023-02-09T20:29:49.753424311Z",
          "sequence_num": 0,
          "events": [
            {
              "type": "snapshot",
              "products": [
                {
                  "product_type": "SPOT",
                  "id": "BTC-USD",
                  "base_currency": "BTC",
                  "quote_currency": "USD",
                  "base_increment": "0.00000001",
                  "quote_increment": "0.01",
                  "display_name": "BTC/USD",
                  "status": "online",
                  "status_message": "",
                  "min_market_funds": "1"
                }
              ]
            }
          ]
        }"#;
        let msg: CoinbaseWsMessage = serde_json::from_str(json).unwrap();

        match msg {
            CoinbaseWsMessage::Status { events, .. } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].event_type, WsEventType::Snapshot);
                assert_eq!(events[0].products.len(), 1);
                let product = &events[0].products[0];
                assert_eq!(product.id, "BTC-USD");
                assert_eq!(product.product_type, CoinbaseProductType::Spot);
                assert_eq!(product.status, CoinbaseProductStatus::Online);
                assert_eq!(product.min_market_funds, Decimal::ONE);
            }
            other => panic!("Expected Status, was {other:?}"),
        }
    }

    #[rstest]
    fn test_deserialize_futures_balance_summary_channel() {
        let json = r#"{
          "channel": "futures_balance_summary",
          "client_id": "",
          "timestamp": "2023-02-09T20:33:57.609931463Z",
          "sequence_num": 0,
          "events": [
            {
              "type": "snapshot",
              "fcm_balance_summary": {
                "futures_buying_power": "100.00",
                "total_usd_balance": "200.00",
                "cbi_usd_balance": "300.00",
                "cfm_usd_balance": "400.00",
                "total_open_orders_hold_amount": "500.00",
                "unrealized_pnl": "600.00",
                "daily_realized_pnl": "0",
                "initial_margin": "700.00",
                "available_margin": "800.00",
                "liquidation_threshold": "900.00",
                "liquidation_buffer_amount": "1000.00",
                "liquidation_buffer_percentage": "1000",
                "intraday_margin_window_measure": {
                  "margin_window_type": "FCM_MARGIN_WINDOW_TYPE_INTRADAY",
                  "margin_level": "MARGIN_LEVEL_TYPE_BASE",
                  "initial_margin": "100.00",
                  "maintenance_margin": "200.00",
                  "liquidation_buffer_percentage": "1000",
                  "total_hold": "100.00",
                  "futures_buying_power": "400.00"
                },
                "overnight_margin_window_measure": {
                  "margin_window_type": "FCM_MARGIN_WINDOW_TYPE_OVERNIGHT",
                  "margin_level": "MARGIN_LEVEL_TYPE_BASE",
                  "initial_margin": "300.00",
                  "maintenance_margin": "200.00",
                  "liquidation_buffer_percentage": "1000",
                  "total_hold": "-30.00",
                  "futures_buying_power": "2000.00"
                }
              }
            }
          ]
        }"#;
        let msg: CoinbaseWsMessage = serde_json::from_str(json).unwrap();

        match msg {
            CoinbaseWsMessage::FuturesBalanceSummary { events, .. } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].event_type, WsEventType::Snapshot);
                let summary = &events[0].fcm_balance_summary;
                assert_eq!(summary.futures_buying_power, Decimal::from(100));
                assert_eq!(summary.daily_realized_pnl, Decimal::ZERO);
                assert_eq!(
                    summary.intraday_margin_window_measure.margin_window_type,
                    CoinbaseMarginWindowType::Intraday
                );
                assert_eq!(
                    summary.overnight_margin_window_measure.margin_level,
                    CoinbaseMarginLevel::Base
                );
                assert_eq!(
                    summary.overnight_margin_window_measure.total_hold,
                    "-30.00".parse::<Decimal>().unwrap()
                );
            }
            other => panic!("Expected FuturesBalanceSummary, was {other:?}"),
        }
    }

    #[rstest]
    fn test_deserialize_subscriptions() {
        let json = load_test_fixture("ws_subscriptions.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();

        match msg {
            CoinbaseWsMessage::Subscriptions { events, .. } => {
                assert_eq!(events.len(), 1);
                assert_eq!(
                    events[0].subscriptions.get(&CoinbaseWsChannel::Level2),
                    Some(&vec![Ustr::from("BTC-USD")])
                );
                assert_eq!(
                    events[0]
                        .subscriptions
                        .get(&CoinbaseWsChannel::MarketTrades),
                    Some(&vec![Ustr::from("BTC-USD"), Ustr::from("ETH-USD")])
                );
            }
            other => panic!("Expected Subscriptions, was {other:?}"),
        }
    }

    #[rstest]
    fn test_serialize_subscribe_request_with_jwt() {
        let sub = CoinbaseWsSubscription {
            msg_type: CoinbaseWsAction::Subscribe,
            product_ids: vec![Ustr::from("BTC-USD")],
            channel: CoinbaseWsChannel::User,
            jwt: Some("test-jwt-token".to_string()),
        };

        let json = serde_json::to_value(&sub).unwrap();
        assert_eq!(json["type"], "subscribe");
        assert_eq!(json["channel"], "user");
        assert_eq!(json["product_ids"][0], "BTC-USD");
        assert_eq!(json["jwt"], "test-jwt-token");
    }

    #[rstest]
    fn test_serialize_subscribe_request_public_omits_jwt() {
        let sub = CoinbaseWsSubscription {
            msg_type: CoinbaseWsAction::Subscribe,
            product_ids: vec![Ustr::from("BTC-USD")],
            channel: CoinbaseWsChannel::Level2,
            jwt: None,
        };

        let json = serde_json::to_value(&sub).unwrap();
        assert_eq!(json["type"], "subscribe");
        assert_eq!(json["channel"], "level2");
        assert!(json.get("jwt").is_none());
    }

    #[rstest]
    fn test_serialize_unsubscribe_request() {
        let sub = CoinbaseWsSubscription {
            msg_type: CoinbaseWsAction::Unsubscribe,
            product_ids: vec![Ustr::from("ETH-USD")],
            channel: CoinbaseWsChannel::MarketTrades,
            jwt: None,
        };

        let json = serde_json::to_value(&sub).unwrap();
        assert_eq!(json["type"], "unsubscribe");
        assert_eq!(json["channel"], "market_trades");
        assert!(json.get("jwt").is_none());
    }

    #[rstest]
    fn test_serialize_channel_level_subscription_omits_product_ids() {
        let sub = CoinbaseWsSubscription {
            msg_type: CoinbaseWsAction::Subscribe,
            product_ids: vec![],
            channel: CoinbaseWsChannel::Heartbeats,
            jwt: None,
        };

        let json = serde_json::to_value(&sub).unwrap();
        assert_eq!(json["type"], "subscribe");
        assert_eq!(json["channel"], "heartbeats");
        assert!(json.get("product_ids").is_none());
        assert!(json.get("jwt").is_none());
    }

    #[rstest]
    fn test_ws_event_type_values() {
        let snapshot: WsEventType = serde_json::from_str("\"snapshot\"").unwrap();
        assert_eq!(snapshot, WsEventType::Snapshot);

        let update: WsEventType = serde_json::from_str("\"update\"").unwrap();
        assert_eq!(update, WsEventType::Update);
    }

    #[rstest]
    fn test_ws_book_side_values() {
        let bid: WsBookSide = serde_json::from_str("\"bid\"").unwrap();
        assert_eq!(bid, WsBookSide::Bid);

        let offer: WsBookSide = serde_json::from_str("\"offer\"").unwrap();
        assert_eq!(offer, WsBookSide::Offer);
    }
}
