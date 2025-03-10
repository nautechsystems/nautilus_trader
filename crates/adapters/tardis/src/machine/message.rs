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

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{enums::Exchange, parse::deserialize_uppercase};

/// Represents a single level in the order book (bid or ask).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BookLevel {
    /// The price at this level.
    pub price: f64,
    /// The amount at this level.
    pub amount: f64,
}

/// Represents a Tardis WebSocket message for book changes.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookChangeMsg {
    /// The symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    /// The exchange ID.
    pub exchange: Exchange,
    /// Indicates whether this is an initial order book snapshot.
    pub is_snapshot: bool,
    /// Updated bids, with price and amount levels.
    pub bids: Vec<BookLevel>,
    /// Updated asks, with price and amount levels.
    pub asks: Vec<BookLevel>,
    /// The order book update timestamp provided by the exchange (ISO 8601 format).
    pub timestamp: DateTime<Utc>,
    /// The local timestamp when the message was received.
    pub local_timestamp: DateTime<Utc>,
}

/// Represents a Tardis WebSocket message for book snapshots.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookSnapshotMsg {
    /// The symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    /// The exchange ID.
    pub exchange: Exchange,
    /// The name of the snapshot, e.g., `book_snapshot_{depth}_{interval}{time_unit}`.
    pub name: String,
    /// The requested number of levels (top bids/asks).
    pub depth: u32,
    /// The requested snapshot interval in milliseconds.
    pub interval: u32,
    /// The top bids price-amount levels.
    pub bids: Vec<BookLevel>,
    /// The top asks price-amount levels.
    pub asks: Vec<BookLevel>,
    /// The snapshot timestamp based on the last book change message processed timestamp.
    pub timestamp: DateTime<Utc>,
    /// The local timestamp when the message was received.
    pub local_timestamp: DateTime<Utc>,
}

/// Represents a Tardis WebSocket message for trades.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub struct TradeMsg {
    /// The symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    /// The exchange ID.
    pub exchange: Exchange,
    /// The trade ID provided by the exchange (optional).
    pub id: Option<String>,
    /// The trade price as provided by the exchange.
    pub price: f64,
    /// The trade amount as provided by the exchange.
    pub amount: f64,
    /// The liquidity taker side (aggressor) for the trade.
    pub side: String,
    /// The trade timestamp provided by the exchange.
    pub timestamp: DateTime<Utc>,
    /// The local timestamp when the message was received.
    pub local_timestamp: DateTime<Utc>,
}

/// Derivative instrument ticker info sourced from real-time ticker & instrument channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivativeTickerMsg {
    /// The symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    /// The exchange ID.
    pub exchange: Exchange,
    /// The last instrument price if provided by exchange.
    pub last_price: Option<f64>,
    /// The last open interest if provided by exchange.
    pub open_interest: Option<f64>,
    /// The last funding rate if provided by exchange.
    pub funding_rate: Option<f64>,
    /// The last index price if provided by exchange.
    pub index_price: Option<f64>,
    /// The last mark price if provided by exchange.
    pub mark_price: Option<f64>,
    /// The message timestamp provided by exchange.
    pub timestamp: DateTime<Utc>,
    /// The local timestamp when the message was received.
    pub local_timestamp: DateTime<Utc>,
}

/// Trades data in aggregated form, known as OHLC, candlesticks, klines etc. Not only most common
/// time based aggregation is supported, but volume and tick count based as well. Bars are computed
/// from tick-by-tick raw trade data, if in given interval no trades happened, there is no bar produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BarMsg {
    /// The symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    /// The exchange ID.
    pub exchange: Exchange,
    /// name with format `trade_bar`_{interval}
    pub name: String,
    /// The requested trade bar interval.
    pub interval: u64,
    /// The open price.
    pub open: f64,
    /// The high price.
    pub high: f64,
    /// The low price.
    pub low: f64,
    /// The close price.
    pub close: f64,
    /// The total volume traded in given interval.
    pub volume: f64,
    /// The buy volume traded in given interval.
    pub buy_volume: f64,
    /// The sell volume traded in given interval.
    pub sell_volume: f64,
    /// The trades count in given interval.
    pub trades: u64,
    /// The volume weighted average price.
    pub vwap: f64,
    /// The timestamp of first trade for given bar.
    pub open_timestamp: DateTime<Utc>,
    /// The timestamp of last trade for given bar.
    pub close_timestamp: DateTime<Utc>,
    /// The end of interval period timestamp.
    pub timestamp: DateTime<Utc>,
    /// The message arrival timestamp that triggered given bar computation.
    pub local_timestamp: DateTime<Utc>,
}

/// Message that marks events when real-time WebSocket connection that was used to collect the
/// historical data got disconnected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectMsg {
    /// The exchange ID.
    pub exchange: Exchange,
    /// The message arrival timestamp that triggered given bar computation (ISO 8601 format).
    pub local_timestamp: DateTime<Utc>,
}

/// A Tardis Machine Server message type.
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum WsMessage {
    BookChange(BookChangeMsg),
    BookSnapshot(BookSnapshotMsg),
    Trade(TradeMsg),
    TradeBar(BarMsg),
    DerivativeTicker(DerivativeTickerMsg),
    Disconnect(DisconnectMsg),
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::tests::load_test_json;

    #[rstest]
    fn test_parse_book_change_message() {
        let json_data = load_test_json("book_change.json");
        let message: BookChangeMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(message.symbol, "XBTUSD");
        assert_eq!(message.exchange, Exchange::Bitmex);
        assert!(!message.is_snapshot);
        assert!(message.bids.is_empty());
        assert_eq!(message.asks.len(), 1);
        assert_eq!(message.asks[0].price, 7_985.0);
        assert_eq!(message.asks[0].amount, 283_318.0);
        assert_eq!(
            message.timestamp,
            DateTime::parse_from_rfc3339("2019-10-23T11:29:53.469Z").unwrap()
        );
        assert_eq!(
            message.local_timestamp,
            DateTime::parse_from_rfc3339("2019-10-23T11:29:53.469Z").unwrap()
        );
    }

    #[rstest]
    fn test_parse_book_snapshot_message() {
        let json_data = load_test_json("book_snapshot.json");
        let message: BookSnapshotMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(message.symbol, "XBTUSD");
        assert_eq!(message.exchange, Exchange::Bitmex);
        assert_eq!(message.name, "book_snapshot_2_50ms");
        assert_eq!(message.depth, 2);
        assert_eq!(message.interval, 50);
        assert_eq!(message.bids.len(), 2);
        assert_eq!(message.asks.len(), 2);
        assert_eq!(message.bids[0].price, 7_633.5);
        assert_eq!(message.bids[0].amount, 1_906_067.0);
        assert_eq!(message.asks[0].price, 7_634.0);
        assert_eq!(message.asks[0].amount, 1_467_849.0);
        assert_eq!(
            message.timestamp,
            DateTime::parse_from_rfc3339("2019-10-25T13:39:46.950Z").unwrap(),
        );
        assert_eq!(
            message.local_timestamp,
            DateTime::parse_from_rfc3339("2019-10-25T13:39:46.961Z").unwrap()
        );
    }

    #[rstest]
    fn test_parse_trade_message() {
        let json_data = load_test_json("trade.json");
        let message: TradeMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(message.symbol, "XBTUSD");
        assert_eq!(message.exchange, Exchange::Bitmex);
        assert_eq!(
            message.id,
            Some("282a0445-0e3a-abeb-f403-11003204ea1b".to_string())
        );
        assert_eq!(message.price, 7_996.0);
        assert_eq!(message.amount, 50.0);
        assert_eq!(message.side, "sell");
        assert_eq!(
            message.timestamp,
            DateTime::parse_from_rfc3339("2019-10-23T10:32:49.669Z").unwrap()
        );
        assert_eq!(
            message.local_timestamp,
            DateTime::parse_from_rfc3339("2019-10-23T10:32:49.740Z").unwrap()
        );
    }

    #[rstest]
    fn test_parse_derivative_ticker_message() {
        let json_data = load_test_json("derivative_ticker.json");
        let message: DerivativeTickerMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(message.symbol, "BTC-PERPETUAL");
        assert_eq!(message.exchange, Exchange::Deribit);
        assert_eq!(message.last_price, Some(7_987.5));
        assert_eq!(message.open_interest, Some(84_129_491.0));
        assert_eq!(message.funding_rate, Some(-0.00001568));
        assert_eq!(message.index_price, Some(7_989.28));
        assert_eq!(message.mark_price, Some(7_987.56));
        assert_eq!(
            message.timestamp,
            DateTime::parse_from_rfc3339("2019-10-23T11:34:29.302Z").unwrap()
        );
        assert_eq!(
            message.local_timestamp,
            DateTime::parse_from_rfc3339("2019-10-23T11:34:29.416Z").unwrap()
        );
    }

    #[rstest]
    fn test_parse_bar_message() {
        let json_data = load_test_json("bar.json");
        let message: BarMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(message.symbol, "XBTUSD");
        assert_eq!(message.exchange, Exchange::Bitmex);
        assert_eq!(message.name, "trade_bar_10000ms");
        assert_eq!(message.interval, 10_000);
        assert_eq!(message.open, 7_623.5);
        assert_eq!(message.high, 7_623.5);
        assert_eq!(message.low, 7_623.0);
        assert_eq!(message.close, 7_623.5);
        assert_eq!(message.volume, 37_034.0);
        assert_eq!(message.buy_volume, 24_244.0);
        assert_eq!(message.sell_volume, 12_790.0);
        assert_eq!(message.trades, 9);
        assert_eq!(message.vwap, 7_623.327320840309);
        assert_eq!(
            message.open_timestamp,
            DateTime::parse_from_rfc3339("2019-10-25T13:11:31.574Z").unwrap()
        );
        assert_eq!(
            message.close_timestamp,
            DateTime::parse_from_rfc3339("2019-10-25T13:11:39.212Z").unwrap()
        );
        assert_eq!(
            message.local_timestamp,
            DateTime::parse_from_rfc3339("2019-10-25T13:11:40.369Z").unwrap()
        );
        assert_eq!(
            message.timestamp,
            DateTime::parse_from_rfc3339("2019-10-25T13:11:40.000Z").unwrap()
        );
    }

    #[rstest]
    fn test_parse_disconnect_message() {
        let json_data = load_test_json("disconnect.json");
        let message: DisconnectMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(message.exchange, Exchange::Deribit);
        assert_eq!(
            message.local_timestamp,
            DateTime::parse_from_rfc3339("2019-10-23T11:34:29.416Z").unwrap()
        );
    }
}
