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

//! Grid market maker acceptance tests using AAPL ITCH L3 data.
//!
//! Requires the `high-precision` feature because the ITCH parquet data
//! uses 128-bit fixed-point encoding.

#![cfg(feature = "high-precision")]

use ahash::AHashMap;
use nautilus_backtest::{config::BacktestEngineConfig, engine::BacktestEngine};
use nautilus_core::UnixNanos;
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModelAny};
use nautilus_model::{
    data::{Data, OrderBookDelta, QuoteTick},
    enums::{AccountType, BookType, OmsType},
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{Equity, Instrument, InstrumentAny},
    orderbook::OrderBook,
    types::{Currency, Money, Price, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_testkit::common::load_itch_aapl_deltas;
use nautilus_trading::examples::strategies::{GridMarketMaker, GridMarketMakerConfig};
use rstest::rstest;
use tempfile::TempDir;
use ustr::Ustr;

// Subsample for CI (covers initial snapshot + active trading)
const CI_DELTA_LIMIT: usize = 100_000;

// ITCH data uses price_precision=4, size_precision=0
fn itch_aapl_equity() -> InstrumentAny {
    InstrumentAny::Equity(Equity::new(
        InstrumentId::from("AAPL.XNAS"),
        Symbol::from("AAPL"),
        Some(Ustr::from("US0378331005")),
        Currency::from("USD"),
        4,
        Price::from("0.0001"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ))
}

/// Replays deltas through an L3 book and emits a quote tick whenever
/// the best bid or ask price changes (synthetic top-of-book feed).
fn deltas_to_quotes(deltas: &[OrderBookDelta]) -> Vec<Data> {
    let instrument_id = deltas[0].instrument_id;
    let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
    let mut quotes = Vec::new();
    let mut last_bid: Option<Price> = None;
    let mut last_ask: Option<Price> = None;

    for delta in deltas {
        book.apply_delta(delta).unwrap();
        let bid = book.best_bid_price();
        let ask = book.best_ask_price();

        if let (Some(b), Some(a)) = (bid, ask)
            && (bid != last_bid || ask != last_ask)
        {
            last_bid = bid;
            last_ask = ask;
            quotes.push(Data::Quote(QuoteTick::new(
                instrument_id,
                b,
                a,
                Quantity::from("1"),
                Quantity::from("1"),
                delta.ts_event,
                delta.ts_init,
            )));
        }
    }
    quotes
}

fn create_engine(instrument: &InstrumentAny) -> BacktestEngine {
    let config = BacktestEngineConfig::default();
    let mut engine = BacktestEngine::new(config).unwrap();
    engine
        .add_venue(
            Venue::from("XNAS"),
            OmsType::Netting,
            AccountType::Margin,
            BookType::L1_MBP,
            vec![Money::from("1_000_000 USD")],
            Some(Currency::from("USD")),
            None,
            AHashMap::new(),
            vec![],
            FillModelAny::default(),
            FeeModelAny::default(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    engine.add_instrument(instrument.clone()).unwrap();
    engine
}

fn create_strategy(instrument_id: InstrumentId) -> GridMarketMaker {
    let config = GridMarketMakerConfig::new(instrument_id, Quantity::from("100"))
        .with_trade_size(Quantity::from("100"))
        .with_num_levels(3)
        .with_grid_step_bps(10)
        .with_skew_factor(0.01)
        .with_requote_threshold_bps(5);
    GridMarketMaker::new(config)
}

#[rstest]
fn test_grid_mm_itch_direct_load() {
    let deltas = load_itch_aapl_deltas(Some(CI_DELTA_LIMIT));
    let data = deltas_to_quotes(&deltas);
    let num_quotes = data.len();
    let instrument = itch_aapl_equity();
    let instrument_id = instrument.id();

    let mut engine = create_engine(&instrument);
    engine.add_strategy(create_strategy(instrument_id)).unwrap();
    engine.add_data(data, None, true, true);

    engine.run(None, None, None, false).unwrap();

    let result = engine.get_result();
    assert_eq!(result.iterations, num_quotes);
    assert!(
        result.total_orders > 0,
        "Expected grid MM to place orders, was 0"
    );
}

#[rstest]
fn test_grid_mm_itch_catalog_load() {
    let deltas = load_itch_aapl_deltas(Some(CI_DELTA_LIMIT));
    let instrument = itch_aapl_equity();
    let instrument_id = instrument.id();

    // Write deltas to a temp catalog then query back
    let temp_dir = TempDir::new().unwrap();
    let catalog = ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);
    catalog
        .write_to_parquet(deltas.clone(), None, None, None)
        .unwrap();
    catalog.write_instruments(vec![instrument.clone()]).unwrap();

    let mut catalog =
        ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);
    let loaded_deltas: Vec<OrderBookDelta> = catalog
        .query_typed_data(
            Some(vec![instrument_id.to_string()]),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();

    assert_eq!(loaded_deltas.len(), deltas.len());

    // Run backtest with catalog-loaded data
    let data = deltas_to_quotes(&loaded_deltas);
    let num_quotes = data.len();
    let mut engine = create_engine(&instrument);
    engine.add_strategy(create_strategy(instrument_id)).unwrap();
    engine.add_data(data, None, true, true);

    engine.run(None, None, None, false).unwrap();

    let result = engine.get_result();
    assert_eq!(result.iterations, num_quotes);
    assert!(
        result.total_orders > 0,
        "Expected grid MM to place orders, was 0"
    );
}

#[rstest]
fn test_grid_mm_itch_streaming() {
    let deltas = load_itch_aapl_deltas(Some(CI_DELTA_LIMIT));
    let instrument = itch_aapl_equity();
    let instrument_id = instrument.id();

    // Generate quotes from the full delta set, then split for streaming
    let all_quotes = deltas_to_quotes(&deltas);
    let midpoint = all_quotes.len() / 2;
    let batch1 = all_quotes[..midpoint].to_vec();
    let batch2 = all_quotes[midpoint..].to_vec();

    // Streaming: two batches
    let mut engine = create_engine(&instrument);
    engine.add_strategy(create_strategy(instrument_id)).unwrap();

    engine.add_data(batch1, None, true, true);
    engine.run(None, None, None, true).unwrap();

    engine.clear_data();
    engine.add_data(batch2, None, true, true);
    engine.run(None, None, None, false).unwrap();

    let streaming_result = engine.get_result();
    assert_eq!(streaming_result.iterations, all_quotes.len());
    assert!(
        streaming_result.total_orders > 0,
        "Expected grid MM to place orders, was 0"
    );
}
