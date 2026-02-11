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

//! Integration tests for OrderBook using real Tardis Deribit market data.

use std::{fs::File, path::PathBuf, sync::OnceLock};

use nautilus_model::{
    data::OrderBookDelta,
    enums::{BookAction, BookType},
    identifiers::InstrumentId,
    orderbook::{OrderBook, analysis::book_check_integrity},
    types::{Price, Quantity},
};
use nautilus_serialization::arrow::DecodeFromRecordBatch;
use nautilus_testkit::common::ensure_tardis_deribit_deltas_parquet;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rstest::rstest;

// Nextest runs tests within a binary in parallel; serialize the download
static PARQUET_PATH: OnceLock<PathBuf> = OnceLock::new();

fn load_deltas_from_parquet(limit: Option<usize>) -> Vec<OrderBookDelta> {
    let filepath = PARQUET_PATH.get_or_init(ensure_tardis_deribit_deltas_parquet);
    let file = File::open(filepath).unwrap();
    let mut builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
    let metadata = builder.schema().metadata().clone();

    if let Some(limit) = limit {
        builder = builder.with_limit(limit);
    }
    let reader = builder.build().unwrap();

    let mut deltas = Vec::new();
    for batch_result in reader {
        let batch = batch_result.unwrap();
        let batch_deltas = OrderBookDelta::decode_batch(&metadata, batch).unwrap();
        deltas.extend(batch_deltas);
    }
    deltas
}

/// Subsample size for routine CI (first ~100K deltas covers initial snapshot + trading)
const CI_DELTA_LIMIT: usize = 100_000;

#[rstest]
#[ignore]
fn test_apply_tardis_deribit_deltas_full_replay() {
    let deltas = load_deltas_from_parquet(Some(CI_DELTA_LIMIT));
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    for delta in &deltas {
        book.apply_delta(delta).unwrap();
    }

    book_check_integrity(&book).unwrap();

    assert_eq!(book.instrument_id, instrument_id);
    assert_eq!(book.best_bid_price().unwrap(), Price::from("6424.5"));
    assert_eq!(book.best_ask_price().unwrap(), Price::from("6425.0"));
    assert_eq!(book.best_bid_size().unwrap(), Quantity::from("4030"));
    assert_eq!(book.best_ask_size().unwrap(), Quantity::from("84750"));
    assert_eq!(book.spread().unwrap(), 0.5);
    assert_eq!(book.midpoint().unwrap(), 6424.75);
    assert_eq!(book.bids(None).count(), 1157);
    assert_eq!(book.asks(None).count(), 956);
    assert_eq!(book.update_count, 100_000);
    assert_eq!(book.sequence, 0);
    assert_eq!(book.ts_last.as_u64(), 1_585_699_686_323_000_000);

    println!("{}", book.pprint(5, None));
}

#[rstest]
#[ignore]
fn test_tardis_deribit_snapshot_boundaries() {
    let deltas = load_deltas_from_parquet(Some(CI_DELTA_LIMIT));

    let mut clear_count = 0;
    let mut last_ts = deltas[0].ts_event;

    for delta in &deltas {
        if delta.action == BookAction::Clear {
            clear_count += 1;
        }
        assert!(
            delta.ts_event >= last_ts,
            "Timestamps not monotonic: {} < {}",
            delta.ts_event,
            last_ts,
        );
        last_ts = delta.ts_event;
    }

    assert!(clear_count > 0, "Expected at least one CLEAR delta");
    println!("CLEAR deltas: {clear_count}");
}

#[rstest]
#[ignore]
fn test_tardis_deribit_spot_checks() {
    let deltas = load_deltas_from_parquet(Some(CI_DELTA_LIMIT));

    assert!(
        deltas.len() >= 100_000,
        "Expected >=100K deltas, found {}",
        deltas.len(),
    );

    assert_eq!(deltas[0].action, BookAction::Clear);
    assert_eq!(
        deltas[0].instrument_id,
        InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
    );

    println!("Total deltas: {}", deltas.len());
}
