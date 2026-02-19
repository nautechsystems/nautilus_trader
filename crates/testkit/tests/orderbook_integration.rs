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

//! Integration tests for OrderBook using real market data.

use indexmap::IndexMap;
use nautilus_model::{
    enums::{BookAction, BookType},
    identifiers::InstrumentId,
    orderbook::{OrderBook, analysis::book_check_integrity},
};
use nautilus_testkit::common::{load_itch_aapl_deltas, load_tardis_deribit_deltas};
use rstest::rstest;
use rust_decimal_macros::dec;

// Subsample size for routine CI (first ~100K deltas covers initial snapshot + trading)
const CI_DELTA_LIMIT: usize = 100_000;

#[rstest]
fn test_apply_tardis_deribit_deltas_full_replay() {
    let deltas = load_tardis_deribit_deltas(Some(CI_DELTA_LIMIT));
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");

    // Validate dataset preconditions
    assert_eq!(deltas[0].action, BookAction::Clear);
    assert_eq!(deltas[0].instrument_id, instrument_id);
    let mut last_ts = deltas[0].ts_event;
    for delta in &deltas {
        assert!(
            delta.ts_event >= last_ts,
            "Timestamps not monotonic: {} < {}",
            delta.ts_event,
            last_ts,
        );
        last_ts = delta.ts_event;
    }

    // Replay through order book
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    for delta in &deltas {
        book.apply_delta(delta).unwrap();
    }

    book_check_integrity(&book).unwrap();

    assert_eq!(book.instrument_id, instrument_id);
    assert_eq!(book.spread().unwrap(), 0.5);
    assert_eq!(book.midpoint().unwrap(), 6424.75);
    assert_eq!(book.bids(None).count(), 1157);
    assert_eq!(book.asks(None).count(), 956);
    assert_eq!(book.update_count, 100_000);
    assert_eq!(book.sequence, 0);
    assert_eq!(book.ts_last.as_u64(), 1_585_699_686_323_000_000);

    assert_eq!(
        book.bids_as_map(Some(5)),
        IndexMap::from([
            (dec!(6424.5), dec!(4030)),
            (dec!(6423.5), dec!(20)),
            (dec!(6423.0), dec!(2800)),
            (dec!(6422.5), dec!(390)),
            (dec!(6422.0), dec!(15730)),
        ]),
    );
    assert_eq!(
        book.asks_as_map(Some(5)),
        IndexMap::from([
            (dec!(6425.0), dec!(84750)),
            (dec!(6425.5), dec!(27740)),
            (dec!(6426.0), dec!(1440)),
            (dec!(6426.5), dec!(12980)),
            (dec!(6427.0), dec!(20800)),
        ]),
    );

    println!("{}", book.pprint(5, None));
}

const ITCH_CI_DELTA_LIMIT: usize = 100_000;

#[rstest]
fn test_apply_itch_aapl_deltas_full_replay() {
    let deltas = load_itch_aapl_deltas(Some(ITCH_CI_DELTA_LIMIT));
    let instrument_id = InstrumentId::from("AAPL.XNAS");

    // Validate dataset preconditions
    assert_eq!(deltas[0].instrument_id, instrument_id);
    let mut last_ts = deltas[0].ts_event;
    for delta in &deltas {
        assert!(
            delta.ts_event >= last_ts,
            "Timestamps not monotonic: {} < {}",
            delta.ts_event,
            last_ts,
        );
        last_ts = delta.ts_event;
    }

    // Replay through L3 order book
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
    for delta in &deltas {
        book.apply_delta(delta).unwrap();
    }

    book_check_integrity(&book).unwrap();

    assert_eq!(book.instrument_id, instrument_id);
    assert_eq!(book.midpoint().unwrap(), 162.075);
    assert_eq!(book.bids(None).count(), 2708);
    assert_eq!(book.asks(None).count(), 2659);
    assert_eq!(book.update_count, 100_000);
    assert_eq!(book.sequence, 100_000);
    assert_eq!(book.ts_last.as_u64(), 1_548_858_802_938_981_784);

    assert_eq!(
        book.bids_as_map(Some(5)),
        IndexMap::from([
            (dec!(162.0500), dec!(600)),
            (dec!(162.0400), dec!(600)),
            (dec!(162.0300), dec!(561)),
            (dec!(162.0200), dec!(581)),
            (dec!(162.0100), dec!(530)),
        ]),
    );
    assert_eq!(
        book.asks_as_map(Some(5)),
        IndexMap::from([
            (dec!(162.1000), dec!(164)),
            (dec!(162.1100), dec!(600)),
            (dec!(162.1200), dec!(600)),
            (dec!(162.1300), dec!(712)),
            (dec!(162.1400), dec!(130)),
        ]),
    );

    println!("{}", book.pprint(5, None));
}
