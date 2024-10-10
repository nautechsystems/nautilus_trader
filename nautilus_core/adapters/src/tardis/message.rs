// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use serde::{Deserialize, Serialize};

/// Represents the type of Tardis WebSocket message.
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    BookChange,
    BookSnapshot,
    Trade,
    Unknown(String),
}

/// Represents a single level in the order book (bid or ask).
#[derive(Debug, Deserialize, Serialize)]
pub struct OrderBookLevel {
    /// The price at this level.
    pub price: f64,
    /// The amount at this level.
    pub amount: f64,
}

/// Represents a Tardis WebSocket message for book changes.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub struct BookChangeMessage {
    /// The type of message (tagged union).
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    /// The instrument symbol as provided by the exchange.
    pub symbol: String,
    /// The exchange ID.
    pub exchange: String,
    /// Indicates whether this is an initial order book snapshot.
    pub is_snapshot: bool,
    /// Updated bids, with price and amount levels.
    pub bids: Vec<OrderBookLevel>,
    /// Updated asks, with price and amount levels.
    pub asks: Vec<OrderBookLevel>,
    /// The order book update timestamp provided by the exchange (ISO 8601 format).
    pub timestamp: String,
    /// The local timestamp when the message was received (ISO 8601 format).
    pub local_timestamp: String,
}

/// Represents a Tardis WebSocket message for book snapshots.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub struct BookSnapshotMessage {
    /// The type of message (tagged union).
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    /// The instrument symbol as provided by the exchange.
    pub symbol: String,
    /// The exchange ID.
    pub exchange: String,
    /// The name of the snapshot, e.g., `book_snapshot_{depth}_{interval}{time_unit}`.
    pub name: String,
    /// The requested number of levels (top bids/asks).
    pub depth: u32,
    /// The requested snapshot interval in milliseconds.
    pub interval: u32,
    /// The top bids price-amount levels.
    pub bids: Vec<OrderBookLevel>,
    /// The top asks price-amount levels.
    pub asks: Vec<OrderBookLevel>,
    /// The snapshot timestamp based on the last book change message processed timestamp.
    pub timestamp: String,
    /// The local timestamp when the message was received.
    pub local_timestamp: String,
}

/// Represents a Tardis WebSocket message for trades.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub struct TradeMessage {
    /// The type of message (tagged union).
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    /// The instrument symbol as provided by the exchange.
    pub symbol: String,
    /// The exchange ID.
    pub exchange: String,
    /// The trade ID provided by the exchange (optional).
    pub id: Option<String>,
    /// The trade price as provided by the exchange.
    pub price: f64,
    /// The trade amount as provided by the exchange.
    pub amount: f64,
    /// The liquidity taker side (aggressor) for the trade.
    pub side: String,
    /// The trade timestamp provided by the exchange (ISO 8601 format).
    pub timestamp: String,
    /// The local timestamp when the message was received (ISO 8601 format).
    pub local_timestamp: String,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_parse_book_change_message() {
        let json_data = r#"
        {
          "type": "book_change",
          "symbol": "XBTUSD",
          "exchange": "bitmex",
          "isSnapshot": false,
          "bids": [],
          "asks": [
            {
              "price": 7985,
              "amount": 283318
            }
          ],
          "timestamp": "2019-10-23T11:29:53.469Z",
          "localTimestamp": "2019-10-23T11:29:53.469Z"
        }
        "#;

        let message: BookChangeMessage =
            serde_json::from_str(json_data).expect("Failed to parse JSON");

        assert_eq!(message.msg_type, MessageType::BookChange);
        assert_eq!(message.symbol, "XBTUSD");
        assert_eq!(message.exchange, "bitmex");
        assert_eq!(message.is_snapshot, false);
        assert!(message.bids.is_empty());
        assert_eq!(message.asks.len(), 1);
        assert_eq!(message.asks[0].price, 7985.0);
        assert_eq!(message.asks[0].amount, 283318.0);
        assert_eq!(message.timestamp, "2019-10-23T11:29:53.469Z");
        assert_eq!(message.local_timestamp, "2019-10-23T11:29:53.469Z");
    }

    #[rstest]
    fn test_parse_book_snapshot_message() {
        let json_data = r#"
        {
          "type": "book_snapshot",
          "symbol": "XBTUSD",
          "exchange": "bitmex",
          "name": "book_snapshot_2_50ms",
          "depth": 2,
          "interval": 50,
          "bids": [
            {
              "price": 7633.5,
              "amount": 1906067
            },
            {
              "price": 7633,
              "amount": 65319
            }
          ],
          "asks": [
            {
              "price": 7634,
              "amount": 1467849
            },
            {
              "price": 7634.5,
              "amount": 67939
            }
          ],
          "timestamp": "2019-10-25T13:39:46.950Z",
          "localTimestamp": "2019-10-25T13:39:46.961Z"
        }
        "#;

        let message: BookSnapshotMessage =
            serde_json::from_str(json_data).expect("Failed to parse JSON");

        assert_eq!(message.msg_type, MessageType::BookSnapshot);
        assert_eq!(message.symbol, "XBTUSD");
        assert_eq!(message.exchange, "bitmex");
        assert_eq!(message.name, "book_snapshot_2_50ms");
        assert_eq!(message.depth, 2);
        assert_eq!(message.interval, 50);
        assert_eq!(message.bids.len(), 2);
        assert_eq!(message.asks.len(), 2);
        assert_eq!(message.bids[0].price, 7633.5);
        assert_eq!(message.bids[0].amount, 1906067.0);
        assert_eq!(message.asks[0].price, 7634.0);
        assert_eq!(message.asks[0].amount, 1467849.0);
        assert_eq!(message.timestamp, "2019-10-25T13:39:46.950Z");
        assert_eq!(message.local_timestamp, "2019-10-25T13:39:46.961Z");
    }

    #[rstest]
    fn test_parse_trade_message() {
        let json_data = r#"
        {
          "type": "trade",
          "symbol": "XBTUSD",
          "exchange": "bitmex",
          "id": "282a0445-0e3a-abeb-f403-11003204ea1b",
          "price": 7996,
          "amount": 50,
          "side": "sell",
          "timestamp": "2019-10-23T10:32:49.669Z",
          "localTimestamp": "2019-10-23T10:32:49.740Z"
        }
        "#;

        let message: TradeMessage = serde_json::from_str(json_data).expect("Failed to parse JSON");

        assert_eq!(message.msg_type, MessageType::Trade);
        assert_eq!(message.symbol, "XBTUSD");
        assert_eq!(message.exchange, "bitmex");
        assert_eq!(
            message.id,
            Some("282a0445-0e3a-abeb-f403-11003204ea1b".to_string())
        );
        assert_eq!(message.price, 7996.0);
        assert_eq!(message.amount, 50.0);
        assert_eq!(message.side, "sell");
        assert_eq!(message.timestamp, "2019-10-23T10:32:49.669Z");
        assert_eq!(message.local_timestamp, "2019-10-23T10:32:49.740Z");
    }
}
