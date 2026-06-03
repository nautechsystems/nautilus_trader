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

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, de::Error};
use ustr::Ustr;

use crate::common::{enums::TardisExchange, parse::deserialize_uppercase};

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
    pub exchange: TardisExchange,
    /// Indicates whether this is an initial order book snapshot.
    pub is_snapshot: bool,
    /// Updated bids, with price and amount levels.
    #[serde(deserialize_with = "deserialize_book_levels")]
    pub bids: Vec<BookLevel>,
    /// Updated asks, with price and amount levels.
    #[serde(deserialize_with = "deserialize_book_levels")]
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
    pub exchange: TardisExchange,
    /// The name of the snapshot, e.g., `book_snapshot_{depth}_{interval}{time_unit}`.
    pub name: String,
    /// The requested number of levels (top bids/asks).
    pub depth: u32,
    /// The requested snapshot interval in milliseconds.
    pub interval: u32,
    /// The top bids price-amount levels.
    #[serde(deserialize_with = "deserialize_book_levels")]
    pub bids: Vec<BookLevel>,
    /// The top asks price-amount levels.
    #[serde(deserialize_with = "deserialize_book_levels")]
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
    pub exchange: TardisExchange,
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
    pub exchange: TardisExchange,
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

/// Option summary info sourced from the options instrument channel, carrying exchange-provided
/// greeks, implied volatilities, mark and underlying prices, and best bid/ask for a single option.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionSummaryMsg {
    /// The symbol as provided by the exchange.
    #[serde(deserialize_with = "deserialize_uppercase")]
    pub symbol: Ustr,
    /// The exchange ID.
    pub exchange: TardisExchange,
    /// The option type, either `put` or `call`.
    pub option_type: String,
    /// The option strike price.
    pub strike_price: f64,
    /// The option expiration date provided by the exchange.
    pub expiration_date: DateTime<Utc>,
    /// The best bid price if provided by the exchange.
    pub best_bid_price: Option<f64>,
    /// The best bid amount if provided by the exchange.
    pub best_bid_amount: Option<f64>,
    /// The best bid implied volatility if provided by the exchange.
    #[serde(rename = "bestBidIV")]
    pub best_bid_iv: Option<f64>,
    /// The best ask price if provided by the exchange.
    pub best_ask_price: Option<f64>,
    /// The best ask amount if provided by the exchange.
    pub best_ask_amount: Option<f64>,
    /// The best ask implied volatility if provided by the exchange.
    #[serde(rename = "bestAskIV")]
    pub best_ask_iv: Option<f64>,
    /// The last trade price if provided by the exchange.
    pub last_price: Option<f64>,
    /// The open interest if provided by the exchange.
    pub open_interest: Option<f64>,
    /// The mark price if provided by the exchange.
    pub mark_price: Option<f64>,
    /// The mark implied volatility if provided by the exchange.
    #[serde(rename = "markIV")]
    pub mark_iv: Option<f64>,
    /// The option delta if provided by the exchange.
    pub delta: Option<f64>,
    /// The option gamma if provided by the exchange.
    pub gamma: Option<f64>,
    /// The option vega if provided by the exchange.
    pub vega: Option<f64>,
    /// The option theta if provided by the exchange.
    pub theta: Option<f64>,
    /// The option rho if provided by the exchange.
    pub rho: Option<f64>,
    /// The underlying price if provided by the exchange.
    pub underlying_price: Option<f64>,
    /// The underlying index name.
    pub underlying_index: String,
    /// The message timestamp provided by the exchange.
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
    pub exchange: TardisExchange,
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
    pub exchange: TardisExchange,
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
    OptionSummary(OptionSummaryMsg),
    Disconnect(DisconnectMsg),
}

#[derive(Debug, Deserialize)]
struct RawBookLevel {
    price: Option<f64>,
    amount: Option<f64>,
}

fn deserialize_book_levels<'de, D>(deserializer: D) -> Result<Vec<BookLevel>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<RawBookLevel>::deserialize(deserializer)?
        .into_iter()
        .filter_map(|level| match (level.price, level.amount) {
            (Some(price), Some(amount)) => Some(Ok(BookLevel { price, amount })),
            (None, None) => None,
            (None, Some(_)) => Some(Err(D::Error::custom("book level missing price"))),
            (Some(_), None) => Some(Err(D::Error::custom("book level missing amount"))),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    #[rstest]
    fn test_parse_book_change_message() {
        let json_data = load_test_json("book_change.json");
        let message: BookChangeMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(message.symbol, "XBTUSD");
        assert_eq!(message.exchange, TardisExchange::Bitmex);
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
    fn test_parse_book_change_message_skips_empty_book_levels() {
        let json_data = r#"{
            "type": "book_change",
            "symbol": "XBTUSD",
            "exchange": "bitmex",
            "isSnapshot": false,
            "bids": [{"price": 7984, "amount": 100}, {}],
            "asks": [{}],
            "timestamp": "2019-10-23T11:29:53.469Z",
            "localTimestamp": "2019-10-23T11:29:53.469Z"
        }"#;

        let message: BookChangeMsg = serde_json::from_str(json_data).unwrap();

        assert_eq!(message.bids.len(), 1);
        assert!(message.asks.is_empty());
        assert_eq!(message.bids[0].price, 7_984.0);
        assert_eq!(message.bids[0].amount, 100.0);
    }

    #[rstest]
    #[case(r#"[{"price": 7984}]"#, "book level missing amount")]
    #[case(r#"[{"amount": 100}]"#, "book level missing price")]
    fn test_parse_book_change_message_rejects_partial_book_level(
        #[case] bids: &str,
        #[case] error_message: &str,
    ) {
        let json_data = format!(
            r#"{{
                "type": "book_change",
                "symbol": "XBTUSD",
                "exchange": "bitmex",
                "isSnapshot": false,
                "bids": {bids},
                "asks": [],
                "timestamp": "2019-10-23T11:29:53.469Z",
                "localTimestamp": "2019-10-23T11:29:53.469Z"
            }}"#
        );

        let error = serde_json::from_str::<BookChangeMsg>(&json_data).unwrap_err();

        assert!(error.to_string().contains(error_message));
    }

    #[rstest]
    fn test_parse_book_snapshot_message() {
        let json_data = load_test_json("book_snapshot.json");
        let message: BookSnapshotMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(message.symbol, "XBTUSD");
        assert_eq!(message.exchange, TardisExchange::Bitmex);
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
    fn test_parse_book_snapshot_message_skips_empty_book_levels() {
        let json_data = r#"{
            "type": "book_snapshot",
            "symbol": "ETC",
            "exchange": "hyperliquid",
            "name": "book_snapshot_20_10s",
            "depth": 20,
            "interval": 10000,
            "bids": [{"price": 20.002, "amount": 5.81}],
            "asks": [{"price": 20.003, "amount": 162.45}, {}],
            "timestamp": "2025-03-03T10:48:10.000Z",
            "localTimestamp": "2025-03-03T10:48:10.596818Z"
        }"#;

        let message: BookSnapshotMsg = serde_json::from_str(json_data).unwrap();

        assert_eq!(message.symbol, "ETC");
        assert_eq!(message.exchange, TardisExchange::Hyperliquid);
        assert_eq!(message.bids.len(), 1);
        assert_eq!(message.asks.len(), 1);
        assert_eq!(message.asks[0].price, 20.003);
        assert_eq!(message.asks[0].amount, 162.45);
    }

    #[rstest]
    fn test_parse_book_snapshot_message_rejects_partial_book_level() {
        let json_data = r#"{
            "type": "book_snapshot",
            "symbol": "ETC",
            "exchange": "hyperliquid",
            "name": "book_snapshot_20_10s",
            "depth": 20,
            "interval": 10000,
            "bids": [{"price": 20.002}],
            "asks": [],
            "timestamp": "2025-03-03T10:48:10.000Z",
            "localTimestamp": "2025-03-03T10:48:10.596818Z"
        }"#;

        let error = serde_json::from_str::<BookSnapshotMsg>(json_data).unwrap_err();

        assert!(error.to_string().contains("book level missing amount"));
    }

    #[rstest]
    fn test_parse_trade_message() {
        let json_data = load_test_json("trade.json");
        let message: TradeMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(message.symbol, "XBTUSD");
        assert_eq!(message.exchange, TardisExchange::Bitmex);
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
        assert_eq!(message.exchange, TardisExchange::Deribit);
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
    fn test_parse_option_summary_message() {
        let json_data = load_test_json("option_summary.json");
        let message: OptionSummaryMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(message.symbol, "BTC-28JUN24-70000-C");
        assert_eq!(message.exchange, TardisExchange::Deribit);
        assert_eq!(message.option_type, "call");
        assert_eq!(message.strike_price, 70_000.0);
        assert_eq!(message.best_bid_iv, Some(0.55));
        assert_eq!(message.best_ask_iv, Some(0.58));
        assert_eq!(message.mark_iv, Some(0.565));
        assert_eq!(message.delta, Some(0.25));
        assert_eq!(message.gamma, Some(0.000_02));
        assert_eq!(message.vega, Some(45.5));
        assert_eq!(message.theta, Some(-15.2));
        assert_eq!(message.rho, Some(0.05));
        assert_eq!(message.underlying_price, Some(63_500.0));
        assert_eq!(message.underlying_index, "BTC-USD");
        assert_eq!(message.open_interest, Some(150.0));
        assert_eq!(
            message.timestamp,
            DateTime::parse_from_rfc3339("2024-01-15T10:30:00.123Z").unwrap()
        );
        assert_eq!(
            message.local_timestamp,
            DateTime::parse_from_rfc3339("2024-01-15T10:30:00.234Z").unwrap()
        );
    }

    #[rstest]
    fn test_parse_bar_message() {
        let json_data = load_test_json("bar.json");
        let message: BarMsg = serde_json::from_str(&json_data).unwrap();

        assert_eq!(message.symbol, "XBTUSD");
        assert_eq!(message.exchange, TardisExchange::Bitmex);
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

        assert_eq!(message.exchange, TardisExchange::Deribit);
        assert_eq!(
            message.local_timestamp,
            DateTime::parse_from_rfc3339("2019-10-23T11:34:29.416Z").unwrap()
        );
    }
}
