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
use nautilus_common::throttler::RateLimit;
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModelAny};
use nautilus_model::{
    data::{Data, OrderBookDelta},
    enums::{AccountType, BookType, OmsType},
    identifiers::{InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    types::{Currency, Money, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_risk::engine::config::RiskEngineConfig;
use nautilus_testkit::common::{itch_aapl_equity, load_itch_aapl_deltas};
use nautilus_trading::examples::strategies::{GridMarketMaker, GridMarketMakerConfig};
use rstest::rstest;
use tempfile::TempDir;

// Subsample for CI (covers initial snapshot + active trading)
const CI_DELTA_LIMIT: usize = 10_000;

fn create_engine(instrument: &InstrumentAny) -> BacktestEngine {
    // Use an unrestricted throttle rate so the grid MM can place orders freely
    // without hitting the default 100/sec limit on high-frequency ITCH data.
    let unlimited = RateLimit::new(1_000_000, 1_000_000_000);
    let config = BacktestEngineConfig {
        risk_engine: Some(RiskEngineConfig {
            max_order_submit: unlimited.clone(),
            max_order_modify: unlimited,
            ..Default::default()
        }),
        ..Default::default()
    };
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
            None,
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
    let quotes = OrderBook::deltas_to_quotes(BookType::L3_MBO, &deltas);
    let data: Vec<Data> = quotes.into_iter().map(Data::Quote).collect();
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
    let quotes = OrderBook::deltas_to_quotes(BookType::L3_MBO, &loaded_deltas);
    let data: Vec<Data> = quotes.into_iter().map(Data::Quote).collect();
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
    let all_quotes: Vec<Data> = OrderBook::deltas_to_quotes(BookType::L3_MBO, &deltas)
        .into_iter()
        .map(Data::Quote)
        .collect();
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
