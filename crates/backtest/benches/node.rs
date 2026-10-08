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

//! Benchmarks for [`BacktestNode`] catalog replay of order book deltas.
//!
//! The benchmark writes one synthetic L2 delta catalog that both cases share. Each case times a
//! one-shot `BacktestNode::run`, which covers the catalog query, decoding, delta batching, and
//! engine replay on an `L2_MBP` venue. Node construction and instrument loading happen before
//! timing, and disposal happens after it.
//!
//! Cases:
//! - `batched`: `batch_deltas` enabled, so each event replays as one `OrderBookDeltas`.
//! - `individual`: `batch_deltas` disabled, so each delta replays on its own.
//!
//! Run with `cargo bench -p nautilus-backtest --features streaming --bench node`.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_backtest::{
    config::{BacktestDataConfig, BacktestEngineConfig, BacktestRunConfig, BacktestVenueConfig},
    node::BacktestNode,
};
use nautilus_common::logging::logger::LoggerConfig;
use nautilus_core::UnixNanos;
use nautilus_execution::models::fee::{FeeModelAny, MakerTakerFeeModel};
use nautilus_model::{
    data::{BookOrder, NautilusDataType, OrderBookDelta},
    enums::{AccountType, BookAction, BookType, OmsType, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Price, Quantity},
};
use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
use tempfile::TempDir;
use ustr::Ustr;

const EVENT_COUNT: usize = 5_000;
const LEVELS_PER_SIDE: usize = 10;
const DELTAS_PER_EVENT: usize = 2 * LEVELS_PER_SIDE;
const BASE_TS_NS: u64 = 1_735_689_600_000_000_000;
const EVENT_INTERVAL_NS: u64 = 1_000_000;

fn bench_book_delta_replay(c: &mut Criterion) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (_temp_dir, catalog_path) = create_catalog(&instrument);
    let delta_count = EVENT_COUNT * DELTAS_PER_EVENT;

    let mut group = c.benchmark_group("backtest_node/book_delta_replay");
    group.sample_size(10);
    group.throughput(Throughput::Elements(delta_count as u64));

    for (case, batch_deltas, expected_iterations) in [
        ("batched", true, EVENT_COUNT),
        ("individual", false, delta_count),
    ] {
        let config = run_config(&catalog_path, instrument.id(), batch_deltas);
        group.bench_function(BenchmarkId::from_parameter(case), |b| {
            b.iter_custom(|iters| run_iterations(iters, &config, expected_iterations));
        });
    }

    group.finish();
}

fn run_iterations(iters: u64, config: &BacktestRunConfig, expected_iterations: usize) -> Duration {
    let mut elapsed = Duration::ZERO;

    for _ in 0..iters {
        let mut node =
            BacktestNode::new(vec![config.clone()]).expect("node config should be valid");
        node.build().expect("node should build");

        let started = Instant::now();
        let results = node.run().expect("backtest run should succeed");
        elapsed += started.elapsed();

        black_box(&results);
        assert_eq!(results[0].iterations, expected_iterations);
        node.dispose();
    }

    elapsed
}

// Writes an opening event that adds every level, then events that update every level, each
// event closing with `F_LAST`
fn create_catalog(instrument: &InstrumentAny) -> (TempDir, String) {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let catalog_path = temp_dir.path().to_str().unwrap().to_string();
    let catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
    let instrument_id = instrument.id();
    let mut deltas = Vec::with_capacity(EVENT_COUNT * DELTAS_PER_EVENT);

    for event in 0..EVENT_COUNT {
        let action = if event == 0 {
            BookAction::Add
        } else {
            BookAction::Update
        };

        let ts = UnixNanos::from(BASE_TS_NS + event as u64 * EVENT_INTERVAL_NS);

        for index in 0..DELTAS_PER_EVENT {
            let flags = if index == DELTAS_PER_EVENT - 1 {
                RecordFlag::F_LAST as u8
            } else {
                0
            };

            let sequence = (event * DELTAS_PER_EVENT + index) as u64 + 1;
            deltas.push(OrderBookDelta::new(
                instrument_id,
                action,
                level_order(index, event),
                flags,
                sequence,
                ts,
                ts,
            ));
        }
    }

    catalog
        .write_instruments(vec![instrument.clone()])
        .expect("instrument should be written");
    catalog
        .write_to_parquet(&deltas, None, None, None)
        .expect("deltas should be written");

    (temp_dir, catalog_path)
}

// Bids sit at 999.99 down to 999.90 and asks at 1000.01 up to 1000.10, with sizes cycling per event
fn level_order(index: usize, event: usize) -> BookOrder {
    let level = index % LEVELS_PER_SIDE + 1;

    let (side, price) = if index < LEVELS_PER_SIDE {
        (OrderSide::Buy, format!("999.{:02}", 100 - level))
    } else {
        (OrderSide::Sell, format!("1000.{level:02}"))
    };

    let size = format!("{}.000", 1 + (event + index) % 10);

    BookOrder::new(
        side,
        Price::from(price.as_str()),
        Quantity::from(size.as_str()),
        0,
    )
}

fn run_config(
    catalog_path: &str,
    instrument_id: InstrumentId,
    batch_deltas: bool,
) -> BacktestRunConfig {
    let venue = BacktestVenueConfig::builder()
        .name(Ustr::from("BINANCE"))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L2_MBP)
        .starting_balances(vec!["1_000_000 USDT".to_string()])
        .fee_model(FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()))
        .build()
        .expect("venue config should be valid");
    let data = BacktestDataConfig::builder()
        .data_type(NautilusDataType::OrderBookDelta)
        .catalog_path(catalog_path.to_string())
        .instrument_id(instrument_id)
        .batch_deltas(batch_deltas)
        .build()
        .expect("data config should be valid");

    let engine = BacktestEngineConfig {
        logging: LoggerConfig::from_spec("bypass_logging")
            .expect("benchmark logger config should be valid"),
        bypass_logging: true,
        run_analysis: false,
        ..Default::default()
    };

    BacktestRunConfig::builder()
        .venues(vec![venue])
        .data(vec![data])
        .engine(engine)
        .raise_exception(true)
        .dispose_on_completion(false)
        .build()
        .expect("run config should be valid")
}

criterion_group!(benches, bench_book_delta_replay);
criterion_main!(benches);
