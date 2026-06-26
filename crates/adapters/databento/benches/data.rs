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

//! File-backed Databento pipeline benches.
//!
//! `dbn_stream_decode` measures zstd + DBN decode into Databento typed records.
//! `historical_loader` measures the user-facing loader path from DBN fixture
//! file to Nautilus domain values. Both groups include file open and zstd setup
//! because that is the cost paid by historical-data users.

mod common;

use std::{hint::black_box, path::Path};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use databento::dbn::{self, decode::DecodeStream};
use fallible_streaming_iterator::FallibleStreamingIterator;
use nautilus_databento::{
    loader::DatabentoDataLoader,
    types::{DatabentoImbalance, DatabentoStatistics},
};
use nautilus_model::{
    data::{Bar, InstrumentStatus, OrderBookDelta, OrderBookDepth10, QuoteTick},
    identifiers::InstrumentId,
};

const LARGE_MBO_FIXTURE: &str = "tests/test_data/databento/esh4-glbx-mdp3-20231225.mbo.dbn.zst";

fn bench_dbn_stream_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("dbn_stream_decode");

    bench_stream::<dbn::MboMsg>(&mut group, "mbo", "test_data.mbo.dbn.zst", 2);
    bench_stream::<dbn::Mbp1Msg>(&mut group, "mbp1", "test_data.mbp-1.dbn.zst", 2);
    bench_stream::<dbn::Mbp10Msg>(&mut group, "mbp10", "test_data.mbp-10.dbn.zst", 2);
    bench_stream::<dbn::TradeMsg>(&mut group, "trades", "test_data.trades.dbn.zst", 2);
    bench_stream::<dbn::OhlcvMsg>(&mut group, "ohlcv_1s", "test_data.ohlcv-1s.dbn.zst", 2);
    bench_stream::<dbn::StatusMsg>(&mut group, "status", "test_data.status.dbn.zst", 4);

    group.finish();
}

fn bench_historical_loader(c: &mut Criterion) {
    let loader = common::loader();
    let instrument_id = common::instrument_id();

    let mbo_path = common::data_path("test_data.mbo.dbn.zst");
    let mbp1_path = common::data_path("test_data.mbp-1.dbn.zst");
    let mbp10_path = common::data_path("test_data.mbp-10.dbn.zst");
    let bbo_path = common::data_path("test_data.bbo-1s.dbn.zst");
    let cmbp_path = common::data_path("test_data.cmbp-1.dbn.zst");
    let cbbo_path = common::data_path("test_data.cbbo-1s.dbn.zst");
    let tbbo_path = common::data_path("test_data.tbbo.dbn.zst");
    let trades_path = common::data_path("test_data.trades.dbn.zst");
    let bars_path = common::data_path("test_data.ohlcv-1s.dbn.zst");
    let status_path = common::data_path("test_data.status.dbn.zst");
    let imbalance_path = common::data_path("test_data.imbalance.dbn.zst");
    let statistics_path = common::data_path("test_data.statistics.dbn.zst");

    let mut group = c.benchmark_group("historical_loader");

    group.throughput(Throughput::Elements(2));
    group.bench_function("mbo_deltas", |b| {
        b.iter(|| {
            let items: Vec<OrderBookDelta> = loader
                .load_order_book_deltas(black_box(&mbo_path), Some(instrument_id), None)
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(2));
    group.bench_function("mbp1_quotes", |b| {
        b.iter(|| {
            let items: Vec<QuoteTick> = loader
                .load_quotes(black_box(&mbp1_path), Some(instrument_id), None)
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(2));
    group.bench_function("mbp10_depth", |b| {
        b.iter(|| {
            let items: Vec<OrderBookDepth10> = loader
                .load_order_book_depth10(black_box(&mbp10_path), Some(instrument_id), None)
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(4));
    group.bench_function("bbo_quotes", |b| {
        b.iter(|| {
            let items: Vec<QuoteTick> = loader
                .load_bbo_quotes(black_box(&bbo_path), Some(instrument_id), None)
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(2));
    group.bench_function("cmbp_quotes", |b| {
        b.iter(|| {
            let items: Vec<QuoteTick> = loader
                .load_cmbp_quotes(black_box(&cmbp_path), Some(instrument_id), None)
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(2));
    group.bench_function("cbbo_quotes", |b| {
        b.iter(|| {
            let items: Vec<QuoteTick> = loader
                .load_cbbo_quotes(black_box(&cbbo_path), Some(instrument_id), None)
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(2));
    group.bench_function("tbbo_trades", |b| {
        b.iter(|| {
            let items = loader
                .load_tbbo_trades(black_box(&tbbo_path), Some(instrument_id), None)
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(2));
    group.bench_function("trades", |b| {
        b.iter(|| {
            let items = loader
                .load_trades(black_box(&trades_path), Some(instrument_id), None)
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(2));
    group.bench_function("bars", |b| {
        b.iter(|| {
            let items: Vec<Bar> = loader
                .load_bars(black_box(&bars_path), Some(instrument_id), None, None)
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(4));
    group.bench_function("status", |b| {
        b.iter(|| {
            let items: Vec<InstrumentStatus> = loader
                .load_status_records::<dbn::StatusMsg>(black_box(&status_path), Some(instrument_id))
                .unwrap()
                .collect::<anyhow::Result<Vec<_>>>()
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(2));
    group.bench_function("imbalance", |b| {
        b.iter(|| {
            let items: Vec<DatabentoImbalance> = loader
                .read_imbalance_records::<dbn::ImbalanceMsg>(
                    black_box(&imbalance_path),
                    Some(instrument_id),
                    None,
                )
                .unwrap()
                .collect::<anyhow::Result<Vec<_>>>()
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(2));
    group.bench_function("statistics", |b| {
        b.iter(|| {
            let items: Vec<DatabentoStatistics> = loader
                .read_statistics_records::<dbn::StatMsg>(
                    black_box(&statistics_path),
                    Some(instrument_id),
                    None,
                )
                .unwrap()
                .collect::<anyhow::Result<Vec<_>>>()
                .unwrap();
            black_box(items);
        });
    });

    group.finish();
}

fn bench_large_mbo(c: &mut Criterion) {
    let loader = common::loader();
    let instrument_id = common::large_mbo_instrument_id();
    let path = common::repository_path(LARGE_MBO_FIXTURE);
    let raw_records = common::record_count::<dbn::MboMsg>(&path);
    let delta_count = count_order_book_deltas(&loader, &path, instrument_id);

    let mut group = c.benchmark_group("large_mbo");

    bench_stream_path::<dbn::MboMsg>(&mut group, "dbn_stream_decode", &path, raw_records);

    group.throughput(Throughput::Elements(delta_count));
    group.bench_function("loader_collect", |b| {
        b.iter(|| {
            let items: Vec<OrderBookDelta> = loader
                .load_order_book_deltas(
                    black_box(&path),
                    Some(instrument_id),
                    Some(common::PRICE_PRECISION),
                )
                .unwrap();
            black_box(items);
        });
    });

    group.throughput(Throughput::Elements(delta_count));
    group.bench_function("loader_stream_count", |b| {
        b.iter(|| {
            let count = loader
                .read_order_book_deltas(
                    black_box(&path),
                    Some(instrument_id),
                    Some(common::PRICE_PRECISION),
                )
                .unwrap()
                .map(|result| result.map(|_| 1_u64))
                .sum::<anyhow::Result<u64>>()
                .unwrap();
            black_box(count);
        });
    });

    group.finish();
}

fn bench_stream<T>(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    filename: &str,
    records: u64,
) where
    T: dbn::Record + dbn::HasRType + 'static,
{
    let path = common::data_path(filename);
    bench_stream_path::<T>(group, name, &path, records);
}

fn bench_stream_path<T>(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    path: &Path,
    records: u64,
) where
    T: dbn::Record + dbn::HasRType + 'static,
{
    group.throughput(Throughput::Elements(records));
    group.bench_function(name, |b| {
        b.iter(|| {
            let decoder =
                databento::dbn::decode::dbn::Decoder::from_zstd_file(black_box(&path)).unwrap();
            let mut stream = decoder.decode_stream::<T>();
            let mut count = 0usize;

            loop {
                stream.advance().unwrap();
                let Some(record) = stream.get() else {
                    break;
                };
                count += 1;
                black_box(record);
            }
            black_box(count);
        });
    });
}

fn count_order_book_deltas(
    loader: &DatabentoDataLoader,
    path: &Path,
    instrument_id: InstrumentId,
) -> u64 {
    loader
        .read_order_book_deltas(path, Some(instrument_id), Some(common::PRICE_PRECISION))
        .unwrap()
        .map(|result| result.map(|_| 1_u64))
        .sum::<anyhow::Result<u64>>()
        .unwrap()
}

criterion_group!(
    benches,
    bench_dbn_stream_decode,
    bench_historical_loader,
    bench_large_mbo
);
criterion_main!(benches);
