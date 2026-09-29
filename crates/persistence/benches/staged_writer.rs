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

use std::{
    sync::{Arc, atomic::AtomicU64},
    time::Instant,
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Data, QuoteTick},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use nautilus_persistence::{
    backend::default_writer_factories,
    writer::{
        factory::{WriterBackendType, WriterConnectConfig, create_writer},
        feather::WriterClock,
    },
};
use tempfile::TempDir;

fn bench_staged_writer(c: &mut Criterion) {
    let mut group = c.benchmark_group("staged_writer");
    group.throughput(Throughput::Elements(1));
    group.bench_function("single_quote_round_trip", |b| {
        b.iter_custom(|iterations| {
            let temp = TempDir::new().expect("temporary catalog");
            let session = temp.path().join("backtest").join("benchmark");
            let config = WriterConnectConfig::new(session.to_str().unwrap(), None);
            let registry = default_writer_factories();
            let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));
            let mut writer =
                create_writer(&WriterBackendType::Feather, &config, clock, &registry).unwrap();
            let quote = quote();

            let start = Instant::now();

            for _ in 0..iterations {
                writer.write_data(Data::Quote(quote)).unwrap();
            }
            let elapsed = start.elapsed();

            writer.close().unwrap();
            elapsed
        });
    });
    group.finish();
}

fn quote() -> QuoteTick {
    QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("0.66"),
        Price::from("0.67"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    )
}

criterion_group!(benches, bench_staged_writer);
criterion_main!(benches);
