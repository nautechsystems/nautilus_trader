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

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_tardis::machine::message::TradeMsg;

const TRADE: &[u8] = include_bytes!("../test_data/trade.json");

fn bench_messages(c: &mut Criterion) {
    let mut group = c.benchmark_group("ingest_parse");
    group.throughput(Throughput::Elements(1));
    group.bench_function("trade", |b| {
        b.iter(|| {
            let message = serde_json::from_slice::<TradeMsg>(black_box(TRADE)).unwrap();
            black_box(message);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_messages);
criterion_main!(benches);
