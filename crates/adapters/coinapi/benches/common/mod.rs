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

//! Shared instruments and fixtures for CoinAPI criterion benches.
//!
//! Fixtures are captures from `test_data/` loaded via `include_str!`, so bench inputs track the
//! parser test corpus.
//!
//! Each bench compiles this module separately and uses a subset of it, so the module allows dead
//! code rather than warning per bench.

#![allow(dead_code)]

use nautilus_coinapi::{
    book::sync::{BookFrame, BookSyncTracker},
    websocket::messages::{Book, Channel, DataType, Subscription},
};
use nautilus_common::live::dst::time::Instant;
use nautilus_core::{AtomicMap, UnixNanos};
use nautilus_live::book::snapshot::SnapshotGate;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{CurrencyPair, Instrument, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use ustr::Ustr;

pub(crate) const BITSTAMP_BTC_USD: &str = "BITSTAMP_SPOT_BTC_USD";
pub(crate) const COINBASE_BTC_USD: &str = "COINBASE_SPOT_BTC_USD";
pub(crate) const COINBASE_BCH_USD: &str = "COINBASE_SPOT_BCH_USD";

/// Instruments keyed by CoinAPI `symbol_id`, the lookup the data client makes for every message.
#[must_use]
pub(crate) fn instruments() -> AtomicMap<Ustr, InstrumentAny> {
    let instruments = AtomicMap::new();
    for (symbol_id, instrument_id, base) in [
        (BITSTAMP_BTC_USD, "BTC-USD.BITSTAMP", "BTC"),
        (COINBASE_BTC_USD, "BTC-USD.COINBASE", "BTC"),
        (COINBASE_BCH_USD, "BCH-USD.COINBASE", "BCH"),
    ] {
        instruments.insert(Ustr::from(symbol_id), currency_pair(instrument_id, base));
    }

    instruments
}

fn currency_pair(instrument_id: &str, base: &str) -> InstrumentAny {
    let instrument_id = InstrumentId::from(instrument_id);
    InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(instrument_id)
            .raw_symbol(instrument_id.symbol)
            .base_currency(Currency::get_or_create_crypto(base))
            .quote_currency(Currency::USD())
            .price_precision(2)
            .size_precision(8)
            .price_increment(Price::from("0.01"))
            .size_increment(Quantity::from("0.00000001"))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
}

/// Returns a tracker whose book for `instrument` accepted a snapshot at `snapshot.sequence()`.
#[must_use]
pub(crate) fn synced_tracker(
    instrument: &InstrumentAny,
    symbol_id: &str,
    data_type: DataType,
    snapshot: BookFrame<'_>,
) -> BookSyncTracker {
    let now = Instant::now();
    let tracker = BookSyncTracker::default();

    let subscription = Subscription {
        channel: Channel {
            data_type,
            period_id: None,
        },
        symbol_id: Ustr::from(symbol_id),
    };

    tracker.record_subscription(instrument.id(), subscription, now, SnapshotGate::default());
    let (_, deltas) = tracker.apply(instrument.id(), snapshot, instrument, now);
    assert!(deltas.is_some(), "bench snapshot must be accepted");
    tracker
}

/// Decodes a book fixture as a snapshot one sequence before the fixture's own.
#[must_use]
pub(crate) fn levels_snapshot(update: &str) -> Book {
    let mut book: Book = serde_json::from_str(update).unwrap();
    book.sequence -= 1;
    book.is_snapshot = Some(true);
    book
}

pub(crate) mod fixtures {
    //! Captures from `test_data/`, kept in sync with parser tests.

    pub(crate) const TRADE: &str = include_str!("../../test_data/ws_trade.json");
    pub(crate) const QUOTE: &str = include_str!("../../test_data/ws_quote.json");
    pub(crate) const OHLCV: &str = include_str!("../../test_data/ws_ohlcv.json");
    pub(crate) const BOOK20: &str = include_str!("../../test_data/ws_book20.json");

    /// The captured `book` message is a snapshot; the bench streams it as the next update.
    #[must_use]
    pub(crate) fn book_update() -> String {
        let update = include_str!("../../test_data/ws_book.json")
            .replace(r#""is_snapshot": true"#, r#""is_snapshot": false"#);
        assert!(update.contains(r#""is_snapshot": false"#));
        update
    }

    /// Resting orders the L3 update changes, at the sequence before it.
    ///
    /// Constructed inline because `ws_book_l3.json` captures only an update.
    pub(crate) const BOOK_L3_SNAPSHOT: &str = r#"{
        "type": "book_l3",
        "symbol_id": "COINBASE_SPOT_BCH_USD",
        "sequence": 2323345,
        "time_exchange": "2020-08-26T12:36:04.3464706Z",
        "time_coinapi": "2020-08-26T12:36:04.2807750Z",
        "is_snapshot": true,
        "asks": [
            {"id": "0ac891c7-8360-462c-8d9a-d8b217d3fca2", "price": 273.02, "size": 1.0},
            {"id": "b2b1a3e0-5d1c-4f0a-9d7e-2f8e6c4a1b3d", "price": 273.05, "size": 2.5}
        ],
        "bids": [
            {"id": "6340ec1e-8be8-404e-b9a4-a14120ec179e", "price": 272.94, "size": 3.0},
            {"id": "d4c3b2a1-9e8f-4a7b-8c6d-5e4f3a2b1c0d", "price": 272.93, "size": 4.5}
        ]
    }"#;

    /// The `ws_book_l3.json` update with an identifier and a partial size on its `MATCH` entry.
    ///
    /// The capture's `MATCH` has no `id`, which the order index rejects as an aggregated-level
    /// source. Its size here leaves part of the resting order, so the `MATCH` emits an `Update`
    /// rather than the `Delete` the captured size would.
    pub(crate) const BOOK_L3_UPDATE: &str = r#"{
        "type": "book_l3",
        "symbol_id": "COINBASE_SPOT_BCH_USD",
        "sequence": 2323346,
        "time_exchange": "2020-08-26T12:36:04.3464706Z",
        "time_coinapi": "2020-08-26T12:36:04.2807750Z",
        "is_snapshot": false,
        "asks": [
            {
                "id": "520269d4-c085-47ed-8825-6d787af90800",
                "price": 273.01,
                "size": 1.81940502,
                "update_type": "ADD"
            },
            {
                "id": "0ac891c7-8360-462c-8d9a-d8b217d3fca2",
                "price": 273.02,
                "size": 1.0,
                "update_type": "DELETE"
            }
        ],
        "bids": [
            {
                "id": "6340ec1e-8be8-404e-b9a4-a14120ec179e",
                "price": 272.94,
                "size": 1.81940502,
                "update_type": "UPDATE"
            },
            {
                "id": "d4c3b2a1-9e8f-4a7b-8c6d-5e4f3a2b1c0d",
                "price": 272.93,
                "size": 0.5,
                "update_type": "MATCH"
            }
        ]
    }"#;
}
