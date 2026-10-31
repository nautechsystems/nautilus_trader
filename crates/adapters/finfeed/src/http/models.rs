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

//! Raw responses decoded directly from JSON bytes to preserve decimal tokens.

use std::collections::HashMap;

use jiff::Timestamp;
use nautilus_core::serialization::decimal;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

/// Exchange identity and metadata, retaining identifiers verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Exchange {
    /// Provider exchange identifier, retained verbatim.
    pub exchange_id: Option<String>,
    /// Provider description of the exchange or institution.
    pub market_name_institution_description: Option<String>,
    /// Provider-supplied website URL.
    pub website: Option<String>,
}

/// Provider lifecycle state, without settlement inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketStatus {
    /// Provider labels the market open.
    Open,
    /// Provider labels the market closed; this is not a settlement event.
    Closed,
    /// Provider labels the market resolved; no payout is inferred.
    Resolved,
    /// Provider labels the market suspended.
    Suspended,
    /// Unrecognized provider lifecycle label.
    #[serde(other)]
    Unknown,
}

/// Normalized outcome classification, without native instrument inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutcomeType {
    /// Binary outcome contract.
    Binary,
    /// Multiple-choice outcome contract.
    MultipleChoice,
    /// Numeric outcome contract.
    Numeric,
    /// Unrecognized provider outcome classification.
    #[serde(other)]
    Other,
}

/// Provider trade-side label, without aggressor-side inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum TradeSide {
    /// Provider buy-side label; does not identify the aggressor.
    Buy,
    /// Provider sell-side label; does not identify the aggressor.
    Sell,
    /// Unrecognized provider trade-side label.
    #[serde(other)]
    Other,
}

/// Outcome metadata with source values retained as raw JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct Market {
    /// Provider outcome identifier, retained verbatim.
    pub market_id: Option<String>,
    /// Provider market title.
    pub title: Option<String>,
    /// Provider market description.
    pub description: Option<String>,
    /// Provider outcome label, such as `Yes` or `No`.
    pub outcome_name: Option<String>,
    /// Provider-reported outcome price, decoded without floating-point conversion.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price: Decimal,
    /// Provider lifecycle label; unknown labels decode as `Unknown`.
    pub status: MarketStatus,
    /// Nested exchange metadata, when supplied.
    pub exchange: Option<Exchange>,
    /// Provider exchange identifier for this outcome.
    pub exchange_id: Option<String>,
    /// Provider contract classification, independent of native mapping support.
    pub outcome_type: OutcomeType,
    /// Provider market mechanism label.
    pub mechanism: Option<String>,
    /// Venue-specific metadata retained as raw JSON values.
    pub source_specific_data: Option<HashMap<String, Box<RawValue>>>,
}

/// Cursor listing envelope; a null or omitted next cursor establishes termination.
#[derive(Debug, Clone, Deserialize)]
pub struct MarketsPage {
    /// Outcome definitions for this page; absent or null data remains `None`.
    pub data: Option<Vec<Market>>,
    /// Continuation cursor; `None` ends traversal.
    pub next_cursor: Option<String>,
}

/// A provider trade with an optional opaque identifier.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Trade {
    /// Opaque provider trade ID; native conversion requires a nonempty ID.
    pub id: Option<String>,
    /// Provider outcome identifier.
    pub market_id: Option<String>,
    /// Trade price, decoded without floating-point conversion.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price: Decimal,
    /// Trade quantity, decoded without floating-point conversion.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub quantity: Decimal,
    /// Sole provider timestamp, used for both native event and initialization times.
    pub timestamp: Timestamp,
    /// Provider outcome label, when supplied.
    pub outcome: Option<String>,
    /// Provider side label; does not identify the aggressor.
    pub side: TradeSide,
}

/// Quote prices, sizes, and both provider timestamps.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Quote {
    /// Ask price, decoded exactly.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub ask: Decimal,
    /// Bid price, decoded exactly.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub bid: Decimal,
    /// Size available at the ask, decoded exactly.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub ask_volume: Decimal,
    /// Size available at the bid, decoded exactly.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub bid_volume: Decimal,
    /// Provider entry timestamp, used as native `ts_event`.
    pub entry_time: Timestamp,
    /// Provider receive timestamp, used as native `ts_init`.
    pub recv_time: Timestamp,
}

/// Latest activity; absent or null trade and quote fields are valid.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ActivityCurrent {
    /// Latest trade; absent or null values remain `None`.
    pub trade: Option<Trade>,
    /// Latest quote; absent or null values remain `None`.
    pub quote: Option<Quote>,
}

/// Bounded latest activity; list limits apply independently.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ActivityBatch {
    /// Trade list, independently bounded by the activity query limit.
    pub trades: Option<Vec<Trade>>,
    /// Quote list, independently bounded by the activity query limit.
    pub quotes: Option<Vec<Quote>>,
}

/// A snapshot price level with exact size.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BookLevel {
    /// Price of the aggregated level, decoded exactly.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price: Decimal,
    /// Size of the aggregated level, decoded exactly.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub size: Decimal,
}

/// Current snapshot; absent sides stay distinct from empty sides.
/// No incremental continuity or book recovery is implied.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Book {
    /// Provider exchange identifier.
    pub exchange_id: Option<String>,
    /// Provider outcome identifier.
    pub market_id: Option<String>,
    /// Exchange timestamp, used as native `ts_event`.
    pub time_exchange: Timestamp,
    /// Provider receive timestamp, used as native `ts_init`.
    pub time_coinapi: Timestamp,
    /// Ask levels; `None` remains distinct from an empty side.
    pub asks: Option<Vec<BookLevel>>,
    /// Bid levels; `None` remains distinct from an empty side.
    pub bids: Option<Vec<BookLevel>>,
}

/// A raw historical book event, retaining provider identity and numeric action codes.
///
/// Numeric action meanings and absolute versus delta sizes are unverified. Response order is
/// retained by the client; these events do not establish a complete snapshot or replay boundary.
/// The processing clock stays absent when the provider omits it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BookEvent {
    /// Provider symbol identifier, retained verbatim.
    pub symbol_id: String,
    /// Exchange event timestamp.
    pub time_exchange: Timestamp,
    /// Optional provider processing timestamp; omissions remain `None`.
    pub time_coinapi: Option<Timestamp>,
    /// Raw event price, decoded exactly.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub price: Decimal,
    /// Raw event size; absolute or relative meaning depends on the action.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub size: Decimal,
    /// Whether the event applies to the ask side.
    pub is_sell_ask: bool,
    /// Raw numeric action code, interpreted only during native L2 replay.
    pub update_type: i64,
    /// Provider event identifier, retained verbatim.
    pub id: String,
}

/// A provider period definition.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OhlcvPeriod {
    /// Provider identifier accepted by DB-backed OHLCV queries.
    pub period_id: Option<String>,
    /// Fixed period length in seconds.
    pub length_seconds: i32,
    /// Calendar period length in months.
    pub length_months: i32,
    /// Number of units in the provider period definition.
    pub unit_count: Option<i32>,
    /// Provider unit label.
    pub unit_name: Option<String>,
    /// Provider display label for the period.
    pub display_name: Option<String>,
}

/// A bar with nullable prices, exact aggregate volume, and an optional raw market identifier.
///
/// Exchange-wide history supplies `market_id_exchange`; per-market responses can omit it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ohlcv {
    /// Raw outcome identifier; exchange-wide history can include unknown or missing IDs.
    pub market_id_exchange: Option<String>,
    /// Period start timestamp.
    pub time_period_start: Timestamp,
    /// Period end timestamp, used for both native bar timestamps.
    pub time_period_end: Timestamp,
    /// Provider timestamp of the opening observation, when supplied.
    pub time_open: Option<Timestamp>,
    /// Provider timestamp of the closing observation, when supplied.
    pub time_close: Option<Timestamp>,
    /// Opening price; absent or null values remain `None`.
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub price_open: Option<Decimal>,
    /// Highest price; absent or null values remain `None`.
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub price_high: Option<Decimal>,
    /// Lowest price; absent or null values remain `None`.
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub price_low: Option<Decimal>,
    /// Closing price; absent or null values remain `None`.
    #[serde(default, deserialize_with = "decimal::deserialize_optional_json")]
    pub price_close: Option<Decimal>,
    /// Provider aggregate volume, decoded without floating-point conversion.
    #[serde(deserialize_with = "decimal::deserialize_json")]
    pub volume_traded: Decimal,
    /// Provider-reported trade count.
    pub trades_count: i64,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const QUOTE: &str = include_str!("../../test_data/exact-quote.json");
    const TRADE: &str = include_str!("../../test_data/exact-trade.json");
    const BAR: &str = include_str!("../../test_data/exact-bar.json");

    #[rstest]
    #[case("Open", MarketStatus::Open)]
    #[case("Closed", MarketStatus::Closed)]
    #[case("Resolved", MarketStatus::Resolved)]
    #[case("Suspended", MarketStatus::Suspended)]
    #[case("FutureStatus", MarketStatus::Unknown)]
    #[case("Unknown", MarketStatus::Unknown)]
    #[case("", MarketStatus::Unknown)]
    fn market_status_labels_preserve_known_values_and_classify_unknowns(
        #[case] label: &str,
        #[case] expected: MarketStatus,
    ) {
        let encoded = serde_json::to_string(label).unwrap();
        assert_eq!(
            serde_json::from_str::<MarketStatus>(&encoded).unwrap(),
            expected
        );
    }

    #[rstest]
    #[case("Binary", OutcomeType::Binary)]
    #[case("MultipleChoice", OutcomeType::MultipleChoice)]
    #[case("Numeric", OutcomeType::Numeric)]
    #[case("FutureOutcome", OutcomeType::Other)]
    #[case("Other", OutcomeType::Other)]
    #[case("", OutcomeType::Other)]
    fn outcome_type_labels_preserve_known_values_and_classify_unknowns(
        #[case] label: &str,
        #[case] expected: OutcomeType,
    ) {
        let encoded = serde_json::to_string(label).unwrap();
        assert_eq!(
            serde_json::from_str::<OutcomeType>(&encoded).unwrap(),
            expected
        );
    }

    #[rstest]
    #[case("Buy", TradeSide::Buy)]
    #[case("Sell", TradeSide::Sell)]
    #[case("FutureSide", TradeSide::Other)]
    #[case("Other", TradeSide::Other)]
    #[case("", TradeSide::Other)]
    fn trade_side_labels_preserve_known_values_and_classify_unknowns(
        #[case] label: &str,
        #[case] expected: TradeSide,
    ) {
        let encoded = serde_json::to_string(label).unwrap();
        assert_eq!(
            serde_json::from_str::<TradeSide>(&encoded).unwrap(),
            expected
        );
    }

    #[rstest]
    fn exact_numbers_identifiers_and_timestamps_preserve_every_field() {
        assert_eq!(
            serde_json::from_str::<Quote>(QUOTE).unwrap(),
            Quote {
                ask: "0.1234567890123456789012345678".parse().unwrap(),
                bid: "0.9876543210987654321098765432".parse().unwrap(),
                ask_volume: "9007199254740993".parse().unwrap(),
                bid_volume: "18446744073709551617".parse().unwrap(),
                entry_time: "2026-10-11T00:00:00.0000001Z".parse().unwrap(),
                recv_time: "2026-10-11T00:00:00.0000003Z".parse().unwrap()
            }
        );
        assert_eq!(
            serde_json::from_str::<Trade>(TRADE).unwrap(),
            Trade {
                id: Some("opaque/ID?+=%".into()),
                market_id: Some("outcome/Y?+=%".into()),
                price: "0.1234567890123456789012345678".parse().unwrap(),
                quantity: "9007199254740993.125".parse().unwrap(),
                timestamp: "2026-10-11T00:00:00.0000001Z".parse().unwrap(),
                outcome: Some("MixedCase".into()),
                side: TradeSide::Buy
            }
        );
        assert_eq!(
            serde_json::from_str::<Ohlcv>(BAR).unwrap(),
            Ohlcv {
                market_id_exchange: None,
                time_period_start: "2026-10-11T00:00:00.0000001Z".parse().unwrap(),
                time_period_end: "2026-10-11T00:01:00.0000002Z".parse().unwrap(),
                time_open: Some("2026-10-11T00:00:02.0000003Z".parse().unwrap()),
                time_close: Some("2026-10-11T00:00:58.0000004Z".parse().unwrap()),
                price_open: Some("0.1".parse().unwrap()),
                price_high: Some("0.9".parse().unwrap()),
                price_low: Some("0.01".parse().unwrap()),
                price_close: Some("0.5".parse().unwrap()),
                volume_traded: "9007199254740993.125".parse().unwrap(),
                trades_count: 9223372036854775807
            }
        );
    }

    #[rstest]
    #[case(include_str!("../../test_data/overflow-quote.json"))]
    #[case(include_str!("../../test_data/underflow-quote.json"))]
    #[case(include_str!("../../test_data/null-quote.json"))]
    #[case(include_str!("../../test_data/empty-quote.json"))]
    #[case(include_str!("../../test_data/object-quote.json"))]
    #[case(include_str!("../../test_data/boolean-quote.json"))]
    #[case(include_str!("../../test_data/malformed.txt"))]
    fn invalid_decimal_shapes_and_unrepresentable_values_fail(#[case] payload: &str) {
        assert!(serde_json::from_str::<Quote>(payload).is_err());
    }

    #[rstest]
    fn every_discrete_field_rejects_overflow() {
        assert!(
            serde_json::from_str::<Ohlcv>(include_str!("../../test_data/overflow-bar.json"))
                .is_err()
        );
        let quote = serde_json::from_str::<serde_json::Value>(QUOTE).unwrap();
        for field in ["ask", "bid", "ask_volume", "bid_volume"] {
            let mut value = quote.clone();
            value[field] = "79228162514264337593543950336".into();
            assert!(
                serde_json::from_str::<Quote>(&value.to_string()).is_err(),
                "{field}"
            );
        }

        for field in [
            "price_open",
            "price_high",
            "price_low",
            "price_close",
            "volume_traded",
            "trades_count",
        ] {
            let mut value = serde_json::from_str::<serde_json::Value>(BAR).unwrap();
            value[field] = "79228162514264337593543950336".into();
            assert!(
                serde_json::from_str::<Ohlcv>(&value.to_string()).is_err(),
                "{field}"
            );
        }

        for field in ["price", "quantity"] {
            let mut value = serde_json::from_str::<serde_json::Value>(TRADE).unwrap();
            value[field] = "1e-29".into();
            assert!(
                serde_json::from_str::<Trade>(&value.to_string()).is_err(),
                "{field}"
            );
        }
    }

    #[rstest]
    fn fixtures_across_venues_and_optional_activity_decode() {
        for (payload, count, terminal) in [
            (
                include_str!("../../test_data/polymarket-listings.json"),
                2,
                false,
            ),
            (
                include_str!("../../test_data/kalshi-listings.json"),
                2,
                false,
            ),
            (
                include_str!("../../test_data/myriad-listings.json"),
                1,
                false,
            ),
            (
                include_str!("../../test_data/manifold-listings.json"),
                1,
                false,
            ),
            (
                include_str!("../../test_data/hyperliquidoutcome-listings.json"),
                1,
                false,
            ),
            (
                include_str!("../../test_data/pascal-listings.json"),
                1,
                false,
            ),
            (
                include_str!("../../test_data/geminipm-listings.json"),
                1,
                false,
            ),
            (
                include_str!("../../test_data/cryptocompm-listings.json"),
                1,
                false,
            ),
            (
                include_str!("../../test_data/forecastex-listings.json"),
                1,
                false,
            ),
        ] {
            let page: MarketsPage = serde_json::from_str(payload).unwrap();
            assert_eq!(page.data.unwrap().len(), count);
            assert_eq!(page.next_cursor.is_none(), terminal);
        }

        let current: ActivityCurrent =
            serde_json::from_str(include_str!("../../test_data/kalshi-current.json")).unwrap();
        assert_eq!(current.quote, None);
        assert_eq!(current.trade.unwrap().side, TradeSide::Sell);

        for payload in [
            include_str!("../../test_data/missing-cursor.json"),
            include_str!("../../test_data/terminal-listings.json"),
        ] {
            let terminal: MarketsPage = serde_json::from_str(payload).unwrap();
            assert_eq!(terminal.data.unwrap().len(), 0);
            assert_eq!(terminal.next_cursor, None);
        }
    }

    #[rstest]
    fn metadata_and_book_fields_preserve_exact_values() {
        let market: Market =
            serde_json::from_str(include_str!("../../test_data/exact-market.json")).unwrap();
        assert_eq!(market.market_id.as_deref(), Some("Opaque/Outcome?+=%"));
        assert_eq!(market.title.as_deref(), Some("distinct title"));
        assert_eq!(market.description.as_deref(), Some("distinct description"));
        assert_eq!(market.outcome_name, None);
        assert_eq!(
            market.price,
            "0.1234567890123456789012345678".parse().unwrap()
        );
        assert_eq!(market.status, MarketStatus::Suspended);
        assert_eq!(
            market.exchange,
            Some(Exchange {
                exchange_id: Some("MYRIAD".into()),
                market_name_institution_description: Some("distinct exchange".into()),
                website: Some("https://example.com".into())
            })
        );
        assert_eq!(market.exchange_id.as_deref(), Some("MYRIAD"));
        assert_eq!(market.outcome_type, OutcomeType::MultipleChoice);
        assert_eq!(market.mechanism.as_deref(), Some("CPMM"));
        let source = market.source_specific_data.unwrap();
        assert_eq!(source.len(), 2);
        assert_eq!(source["precise"].get(), "0.9876543210987654321098765432");
        let nested: std::collections::HashMap<String, String> =
            serde_json::from_str(source["nested"].get()).unwrap();
        assert_eq!(
            nested,
            std::collections::HashMap::from([("token".into(), "opaque/Token?+=%".into())])
        );
        let book: Book =
            serde_json::from_str(include_str!("../../test_data/polymarket-book.json")).unwrap();
        assert_eq!(book.exchange_id.as_deref(), Some("POLYMARKET"));
        assert_eq!(book.market_id.as_deref(), Some("TEST-OUTCOME_NO"));
        assert_eq!(
            book.time_exchange,
            "2026-10-12T00:00:02.0000003Z".parse().unwrap()
        );
        assert_eq!(
            book.time_coinapi,
            "2026-10-12T00:00:02.0000003Z".parse().unwrap()
        );
        let mut invalid =
            include_str!("../../test_data/polymarket-book.json").replace("0.6,", "1e-29,");
        assert!(serde_json::from_str::<Book>(&invalid).is_err());
        invalid = include_str!("../../test_data/polymarket-book.json")
            .replace("41.25", "79228162514264337593543950336");
        assert!(serde_json::from_str::<Book>(&invalid).is_err());
    }

    #[rstest]
    fn nullable_book_sides_do_not_become_empty_snapshots() {
        assert_eq!(
            serde_json::from_str::<Book>(include_str!("../../test_data/null-book.json")).unwrap(),
            Book {
                exchange_id: Some("KALSHI".into()),
                market_id: Some("opaque/NO?+=%".into()),
                time_exchange: "2026-10-11T00:00:00.0000001Z".parse().unwrap(),
                time_coinapi: "2026-10-11T00:00:00.0000003Z".parse().unwrap(),
                asks: None,
                bids: None
            }
        );
    }

    #[derive(serde::Deserialize)]
    struct NullableIdentifiers {
        exchange: Exchange,
        market: Market,
        trade: Trade,
        book: Book,
        period: OhlcvPeriod,
    }

    #[rstest]
    fn nullable_identifiers_preserve_absence_and_period_fields() {
        let data: NullableIdentifiers =
            serde_json::from_str(include_str!("../../test_data/nullable-identifiers.json"))
                .unwrap();
        assert_eq!(data.exchange.exchange_id, None);
        assert_eq!(data.market.market_id, None);
        assert_eq!(data.market.exchange_id, None);
        assert_eq!(data.trade.market_id, None);
        assert_eq!(data.trade.id, None);
        assert_eq!(data.book.market_id, None);
        assert_eq!(data.book.exchange_id, None);
        assert_eq!(
            data.period,
            OhlcvPeriod {
                period_id: None,
                length_seconds: 59,
                length_months: 17,
                unit_count: Some(13),
                unit_name: Some("distinct unit".into()),
                display_name: Some("distinct display".into())
            }
        );
    }
}
