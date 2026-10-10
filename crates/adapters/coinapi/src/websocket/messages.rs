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

//! Raw CoinAPI WebSocket V1 frames, decoded directly from JSON bytes to preserve decimal tokens.

use jiff::Timestamp;
use nautilus_core::serialization::decimal;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::error::Error;
use crate::http::models::BookLevel;

/// A market data stream type accepted in `subscribe_data_type`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    strum::AsRefStr,
    strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DataType {
    Book,
    Book5,
    Book20,
    Book50,
    BookL3,
    Quote,
    Trade,
    Ohlcv,
}

impl DataType {
    /// Returns whether this is an order book type.
    ///
    /// A session scope can list only one book type; CoinAPI rejects a second with a terminal
    /// `error` frame.
    #[must_use]
    pub fn is_book(self) -> bool {
        matches!(
            self,
            Self::Book | Self::Book5 | Self::Book20 | Self::Book50 | Self::BookL3
        )
    }
}

/// A data type, plus its period for OHLCV.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Channel {
    pub data_type: DataType,
    /// Required for and only valid with [`DataType::Ohlcv`], for example `1MIN`.
    pub period_id: Option<Ustr>,
}

impl Channel {
    pub(super) fn validate(&self) -> Result<(), Error> {
        match (self.data_type, self.period_id) {
            (DataType::Ohlcv, Some(period))
                if !period.is_empty() && period.bytes().all(|b| b.is_ascii_alphanumeric()) =>
            {
                Ok(())
            }
            (DataType::Ohlcv, _) => Err(Error::Validation(
                "OHLCV requires an alphanumeric period_id".into(),
            )),
            (_, Some(_)) => Err(Error::Validation(
                "period_id is only valid for OHLCV".into(),
            )),
            (_, None) => Ok(()),
        }
    }
}

/// One symbol on one channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Subscription {
    pub channel: Channel,
    /// Full CoinAPI `symbol_id`, matched exactly.
    pub symbol_id: Ustr,
}

impl Subscription {
    pub(super) fn validate(&self) -> Result<(), Error> {
        self.channel.validate()?;

        if self.symbol_id.is_empty()
            || !self
                .symbol_id
                .bytes()
                .all(|b| (33..=126).contains(&b) && b != b'$')
        {
            return Err(Error::Validation(
                "symbol_id must contain visible ASCII characters other than '$'".into(),
            ));
        }

        Ok(())
    }
}

/// A change to the session scope, applied in order with others that share one `hello`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeChange {
    Subscribe(Subscription),
    Unsubscribe(Subscription),
}

impl ScopeChange {
    /// Returns the pair the change adds or removes.
    #[must_use]
    pub fn subscription(&self) -> &Subscription {
        match self {
            Self::Subscribe(subscription) | Self::Unsubscribe(subscription) => subscription,
        }
    }
}

/// A `hello` request that replaces the whole session scope.
///
/// CoinAPI combines every listed data type, symbol, and period, so the scope can include
/// pairs nobody subscribed to. Authentication uses the `X-CoinAPI-Key` upgrade header, so this
/// frame carries no credential.
#[derive(Debug, Serialize)]
pub(super) struct Hello {
    r#type: &'static str,
    heartbeat: bool,
    subscribe_data_type: Vec<DataType>,
    subscribe_filter_symbol_id: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subscribe_filter_period_id: Vec<Ustr>,
}

impl Hello {
    /// Builds the smallest exact-symbol scope covering every subscription, in sorted order.
    pub(super) fn new(subscriptions: &[Subscription]) -> Self {
        let mut data_types: Vec<DataType> =
            subscriptions.iter().map(|s| s.channel.data_type).collect();
        data_types.sort_unstable_by(|a, b| a.as_ref().cmp(b.as_ref()));
        data_types.dedup();

        let mut symbols: Vec<&str> = subscriptions.iter().map(|s| s.symbol_id.as_str()).collect();
        symbols.sort_unstable();
        symbols.dedup();

        let mut periods: Vec<Ustr> = subscriptions
            .iter()
            .filter_map(|s| s.channel.period_id)
            .collect();
        periods.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));
        periods.dedup();

        Self {
            r#type: "hello",
            heartbeat: true,
            subscribe_data_type: data_types,
            subscribe_filter_symbol_id: symbols.iter().map(|s| format!("{s}$")).collect(),
            subscribe_filter_period_id: periods,
        }
    }
}

/// An L2 book message from `book`, `book5`, `book20`, or `book50`.
///
/// Only `book` carries `is_snapshot`; the fixed-depth types always list their top levels.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Book {
    #[serde(rename = "type")]
    pub data_type: DataType,
    pub symbol_id: Ustr,
    pub sequence: u64,
    pub time_exchange: Timestamp,
    pub time_coinapi: Timestamp,
    pub is_snapshot: Option<bool>,
    pub asks: Vec<BookLevel>,
    pub bids: Vec<BookLevel>,
}

/// An order-by-order book snapshot or update.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BookL3 {
    pub symbol_id: Ustr,
    pub sequence: u64,
    pub time_exchange: Timestamp,
    pub time_coinapi: Timestamp,
    pub is_snapshot: bool,
    pub asks: Vec<BookL3Entry>,
    pub bids: Vec<BookL3Entry>,
}

/// An order entry; a missing identifier marks an aggregated L2 fallback entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BookL3Entry {
    pub id: Option<String>,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub size: Decimal,
    /// Absent in snapshots.
    pub update_type: Option<BookUpdateType>,
}

/// How an L3 update changes an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BookUpdateType {
    Add,
    Update,
    Subtract,
    Delete,
    Match,
    #[serde(other)]
    Other,
}

/// A best bid and ask update; zero sizes mean the size is unknown.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Quote {
    pub symbol_id: Ustr,
    pub sequence: u64,
    pub time_exchange: Timestamp,
    pub time_coinapi: Timestamp,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub ask_price: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub ask_size: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub bid_price: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub bid_size: Decimal,
}

/// An executed trade.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Trade {
    pub symbol_id: Ustr,
    pub sequence: u64,
    pub time_exchange: Timestamp,
    pub time_coinapi: Timestamp,
    pub uuid: String,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub size: Decimal,
    pub taker_side: Ustr,
}

/// An OHLCV period update, sent on close or while the period is still open.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ohlcv {
    pub symbol_id: Ustr,
    pub sequence: u64,
    pub period_id: Ustr,
    pub time_period_start: Timestamp,
    pub time_period_end: Timestamp,
    pub time_open: Timestamp,
    pub time_close: Timestamp,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price_open: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price_high: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price_low: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price_close: Decimal,
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub volume_traded: Decimal,
    pub trades_count: u64,
}

/// A server notice that the connection closes before `before_time`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Reconnect {
    pub within_seconds: u64,
    pub before_time: Timestamp,
}

/// A terminal venue error; the server closes the connection after sending it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VenueError {
    pub message: String,
}

/// Messages emitted by [`CoinApiWebSocketClient`](super::client::CoinApiWebSocketClient).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoinApiWsMessage {
    Book(Book),
    BookL3(BookL3),
    Quote(Quote),
    Trade(Trade),
    Ohlcv(Ohlcv),
    /// The venue rejected the session; its connection and subscriptions are retired.
    Error(String),
    /// A replacement connection replayed the subscriptions.
    ///
    /// Sequence numbers restart per connection, so consumers must reset stream state such as books.
    Reconnected,
}

impl CoinApiWsMessage {
    /// Returns the channel and symbol a data message belongs to.
    pub(super) fn subscription(&self) -> Option<Subscription> {
        let (data_type, period_id, symbol_id) = match self {
            Self::Book(book) => (book.data_type, None, book.symbol_id),
            Self::BookL3(book) => (DataType::BookL3, None, book.symbol_id),
            Self::Quote(quote) => (DataType::Quote, None, quote.symbol_id),
            Self::Trade(trade) => (DataType::Trade, None, trade.symbol_id),
            Self::Ohlcv(ohlcv) => (DataType::Ohlcv, Some(ohlcv.period_id), ohlcv.symbol_id),
            Self::Error(_) | Self::Reconnected => return None,
        };

        Some(Subscription {
            channel: Channel {
                data_type,
                period_id,
            },
            symbol_id,
        })
    }
}

/// One decoded inbound text frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Data(CoinApiWsMessage),
    Heartbeat,
    Reconnect(Reconnect),
    Error(VenueError),
    Unknown(String),
}

#[derive(Deserialize)]
struct Envelope<'a> {
    #[serde(borrow)]
    r#type: &'a str,
}

impl Frame {
    /// Decodes a text frame by its `type` field.
    ///
    /// Reads the tag in a first pass and then decodes the matching struct from the original bytes:
    /// Serde tagged enums buffer numbers and lose raw decimal tokens, and CoinAPI sends `type` last.
    ///
    /// # Errors
    ///
    /// Returns an error if the frame is not JSON, has no string `type`, or a known type's payload
    /// does not decode.
    pub fn decode(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        let envelope: Envelope = serde_json::from_slice(bytes)?;
        Ok(match envelope.r#type {
            "book" | "book5" | "book20" | "book50" => {
                Self::Data(CoinApiWsMessage::Book(serde_json::from_slice(bytes)?))
            }
            "book_l3" => Self::Data(CoinApiWsMessage::BookL3(serde_json::from_slice(bytes)?)),
            "quote" => Self::Data(CoinApiWsMessage::Quote(serde_json::from_slice(bytes)?)),
            "trade" => Self::Data(CoinApiWsMessage::Trade(serde_json::from_slice(bytes)?)),
            "ohlcv" => Self::Data(CoinApiWsMessage::Ohlcv(serde_json::from_slice(bytes)?)),

            // CoinAPI spells the heartbeat type this way on the wire
            "hearbeat" => Self::Heartbeat,
            "reconnect" => Self::Reconnect(serde_json::from_slice(bytes)?),
            "error" => Self::Error(serde_json::from_slice(bytes)?),
            other => Self::Unknown(other.to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn decimal(text: &str) -> Decimal {
        Decimal::from_str_exact(text).unwrap()
    }

    fn time(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn decode(bytes: &str) -> Frame {
        Frame::decode(bytes.as_bytes()).unwrap()
    }

    fn level(price: &str, size: &str) -> BookLevel {
        BookLevel {
            id: None,
            price: decimal(price),
            size: decimal(size),
        }
    }

    #[rstest]
    fn full_book_carries_snapshot_flag() {
        assert_eq!(
            decode(include_str!("../../test_data/ws_book.json")),
            Frame::Data(CoinApiWsMessage::Book(Book {
                data_type: DataType::Book,
                symbol_id: "BITSTAMP_SPOT_BTC_USD".into(),
                sequence: 2_323_346,
                time_exchange: time("2013-09-28T22:40:50.0000000Z"),
                time_coinapi: time("2017-03-18T22:42:21.3763342Z"),
                is_snapshot: Some(true),
                asks: vec![level("456.35", "123"), level("456.36", "23")],
                bids: vec![level("456.10", "42"), level("456.09", "5")],
            }))
        );
    }

    #[rstest]
    fn fixed_depth_book_keeps_its_type_without_snapshot_flag() {
        assert_eq!(
            decode(include_str!("../../test_data/ws_book20.json")),
            Frame::Data(CoinApiWsMessage::Book(Book {
                data_type: DataType::Book20,
                symbol_id: "BITSTAMP_SPOT_BTC_USD".into(),
                sequence: 2_323_346,
                time_exchange: time("2013-09-28T22:40:50.0000000Z"),
                time_coinapi: time("2017-03-18T22:42:21.3763342Z"),
                is_snapshot: None,
                asks: vec![level("456.35", "123")],
                bids: vec![level("456.10", "42")],
            }))
        );
    }

    #[rstest]
    fn book_l3_preserves_order_identity_and_update_types() {
        let entry = |id: Option<&str>, price: &str, size: &str, update_type| BookL3Entry {
            id: id.map(Into::into),
            price: decimal(price),
            size: decimal(size),
            update_type: Some(update_type),
        };

        assert_eq!(
            decode(include_str!("../../test_data/ws_book_l3.json")),
            Frame::Data(CoinApiWsMessage::BookL3(BookL3 {
                symbol_id: "COINBASE_SPOT_BCH_USD".into(),
                sequence: 2_323_346,
                time_exchange: time("2020-08-26T12:36:04.3464706Z"),
                time_coinapi: time("2020-08-26T12:36:04.2807750Z"),
                is_snapshot: false,
                asks: vec![
                    entry(
                        Some("520269d4-c085-47ed-8825-6d787af90800"),
                        "273.01",
                        "1.81940502",
                        BookUpdateType::Add
                    ),
                    entry(
                        Some("0ac891c7-8360-462c-8d9a-d8b217d3fca2"),
                        "273.02",
                        "1.0",
                        BookUpdateType::Delete
                    ),
                ],
                bids: vec![
                    entry(
                        Some("6340ec1e-8be8-404e-b9a4-a14120ec179e"),
                        "272.94",
                        "1.81940502",
                        BookUpdateType::Update
                    ),
                    entry(None, "272.93", "4.5", BookUpdateType::Match),
                ],
            }))
        );
    }

    #[rstest]
    #[case("SUBTRACT", BookUpdateType::Subtract)]
    #[case("REPLACE", BookUpdateType::Other)]
    fn book_update_type_classifies_open_values(
        #[case] wire: &str,
        #[case] expected: BookUpdateType,
    ) {
        assert_eq!(
            serde_json::from_str::<BookUpdateType>(&format!("\"{wire}\"")).unwrap(),
            expected
        );
    }

    #[rstest]
    fn quote_preserves_every_field_and_decimal_scale() {
        let Frame::Data(CoinApiWsMessage::Quote(quote)) =
            decode(include_str!("../../test_data/ws_quote.json"))
        else {
            panic!("expected quote");
        };

        assert_eq!(
            quote,
            Quote {
                symbol_id: "BITSTAMP_SPOT_BTC_USD".into(),
                sequence: 2_323_346,
                time_exchange: time("2013-09-28T22:40:50.0000000Z"),
                time_coinapi: time("2017-03-18T22:42:21.3763342Z"),
                ask_price: decimal("770.000000000"),
                ask_size: decimal("3252"),
                bid_price: decimal("760"),
                bid_size: decimal("124"),
            }
        );
        assert_eq!(quote.ask_price.scale(), 9);
    }

    #[rstest]
    fn trade_preserves_every_field_and_decimal_scale() {
        let Frame::Data(CoinApiWsMessage::Trade(trade)) =
            decode(include_str!("../../test_data/ws_trade.json"))
        else {
            panic!("expected trade");
        };

        assert_eq!(
            trade,
            Trade {
                symbol_id: "COINBASE_SPOT_BTC_USD".into(),
                sequence: 1507,
                time_exchange: time("2026-09-25T01:29:06.0656180Z"),
                time_coinapi: time("2026-09-25T01:29:06.1438272Z"),
                uuid: "c2519dfb-79a6-40e9-a1c8-a95d94209399".into(),
                price: decimal("84520.02"),
                size: decimal("0.00000004"),
                taker_side: "SELL".into(),
            }
        );
        assert_eq!((trade.price.scale(), trade.size.scale()), (2, 8));
    }

    #[rstest]
    fn ohlcv_preserves_period_and_every_field() {
        assert_eq!(
            decode(include_str!("../../test_data/ws_ohlcv.json")),
            Frame::Data(CoinApiWsMessage::Ohlcv(Ohlcv {
                symbol_id: "COINBASE_SPOT_BTC_USD".into(),
                sequence: 520_348,
                period_id: "1SEC".into(),
                time_period_start: time("2026-09-25T01:30:01.0000000Z"),
                time_period_end: time("2026-09-25T01:30:02.0000000Z"),
                time_open: time("2026-09-25T01:30:01.0173140Z"),
                time_close: time("2026-09-25T01:30:01.7668490Z"),
                price_open: decimal("84523.69"),
                price_high: decimal("84523.69"),
                price_low: decimal("84523.68"),
                price_close: decimal("84523.69"),
                volume_traded: decimal("0.00468951"),
                trades_count: 5,
            }))
        );
    }

    #[rstest]
    #[case(include_str!("../../test_data/ws_heartbeat.json"), Frame::Heartbeat)]
    #[case(
        include_str!("../../test_data/ws_reconnect.json"),
        Frame::Reconnect(Reconnect {
            within_seconds: 10,
            before_time: "2020-08-06T19:19:09.7035429Z".parse().unwrap(),
        })
    )]
    #[case(
        include_str!("../../test_data/ws_error.json"),
        Frame::Error(VenueError {
            message: "Subscribed data type 'nonsense' is invalid. Allowed values: book5, book20, \
                      book50, book, book_l3, quote, trade, ohlcv, exrate, asset, exchange, \
                      symbol, connectivity_status."
                .into(),
        })
    )]
    #[case(r#"{"type":"heartbeat"}"#, Frame::Unknown("heartbeat".into()))]
    #[case(r#"{"asset_id":"BTC","type":"asset"}"#, Frame::Unknown("asset".into()))]
    fn control_frames_decode_by_type(#[case] bytes: &str, #[case] expected: Frame) {
        assert_eq!(decode(bytes), expected);
    }

    #[rstest]
    #[case(include_str!("../../test_data/malformed.txt"))]
    #[case(r#"{"symbol_id":"TEST"}"#)]
    #[case(r#"{"type":"trade","symbol_id":"TEST"}"#)]
    #[case(r#"{"type":"error"}"#)]
    fn malformed_frames_fail_decoding(#[case] bytes: &str) {
        assert!(Frame::decode(bytes.as_bytes()).is_err());
    }

    fn sub(data_type: DataType, period_id: Option<&str>, symbol_id: &str) -> Subscription {
        Subscription {
            channel: Channel {
                data_type,
                period_id: period_id.map(Ustr::from),
            },
            symbol_id: symbol_id.into(),
        }
    }

    #[rstest]
    #[case(
        vec![sub(DataType::Trade, None, "B"), sub(DataType::Trade, None, "A")],
        r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["trade"],"subscribe_filter_symbol_id":["A$","B$"]}"#
    )]
    #[case(
        vec![sub(DataType::Trade, None, "A"), sub(DataType::BookL3, None, "B"), sub(DataType::Book20, None, "A")],
        r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["book20","book_l3","trade"],"subscribe_filter_symbol_id":["A$","B$"]}"#
    )]
    #[case(
        vec![sub(DataType::Ohlcv, Some("1SEC"), "A"), sub(DataType::Ohlcv, Some("1MIN"), "A"), sub(DataType::Quote, None, "A")],
        r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["ohlcv","quote"],"subscribe_filter_symbol_id":["A$"],"subscribe_filter_period_id":["1MIN","1SEC"]}"#
    )]
    fn hello_serializes_sorted_union_scope(
        #[case] subscriptions: Vec<Subscription>,
        #[case] expected: &str,
    ) {
        assert_eq!(
            serde_json::to_string(&Hello::new(&subscriptions)).unwrap(),
            expected
        );
    }

    #[rstest]
    #[case(DataType::Book, "book")]
    #[case(DataType::Book5, "book5")]
    #[case(DataType::Book20, "book20")]
    #[case(DataType::Book50, "book50")]
    #[case(DataType::BookL3, "book_l3")]
    #[case(DataType::Quote, "quote")]
    #[case(DataType::Trade, "trade")]
    #[case(DataType::Ohlcv, "ohlcv")]
    fn data_type_topic_name_matches_wire_name(#[case] data_type: DataType, #[case] wire: &str) {
        assert_eq!(data_type.is_book(), wire.starts_with("book"));
        assert_eq!(data_type.as_ref(), wire);
        assert_eq!(wire.parse::<DataType>().unwrap(), data_type);
        assert_eq!(
            serde_json::to_string(&data_type).unwrap(),
            format!("\"{wire}\"")
        );
    }

    #[rstest]
    #[case(DataType::Trade, None, "COINBASE_SPOT_BTC_USD", true)]
    #[case(DataType::Ohlcv, Some("1MIN"), "COINBASE_SPOT_BTC_USD", true)]
    #[case(DataType::Ohlcv, None, "COINBASE_SPOT_BTC_USD", false)]
    #[case(DataType::Ohlcv, Some(""), "COINBASE_SPOT_BTC_USD", false)]
    #[case(DataType::Ohlcv, Some("1:MIN"), "COINBASE_SPOT_BTC_USD", false)]
    #[case(DataType::Trade, Some("1MIN"), "COINBASE_SPOT_BTC_USD", false)]
    #[case(DataType::Trade, None, "", false)]
    #[case(DataType::Trade, None, "COINBASE_SPOT_BTC_USD$", false)]
    #[case(DataType::Trade, None, "COINBASE SPOT", false)]
    fn subscription_validation(
        #[case] data_type: DataType,
        #[case] period_id: Option<&str>,
        #[case] symbol_id: &str,
        #[case] valid: bool,
    ) {
        let subscription = Subscription {
            channel: Channel {
                data_type,
                period_id: period_id.map(Ustr::from),
            },
            symbol_id: symbol_id.into(),
        };

        assert_eq!(subscription.validate().is_ok(), valid);
    }
}
