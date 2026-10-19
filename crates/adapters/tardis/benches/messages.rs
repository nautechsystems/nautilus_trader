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
use nautilus_tardis::machine::message::{BookSnapshotMsg, TradeMsg};

const TRADE: &[u8] = include_bytes!("../test_data/trade.json");
const BOOK_DEPTH: usize = 50;

fn bench_messages(c: &mut Criterion) {
    let book_snapshot = book_snapshot_json(BOOK_DEPTH);

    let mut group = c.benchmark_group("ingest_parse");
    group.throughput(Throughput::Elements(1));
    group.bench_function("trade", |b| {
        b.iter(|| {
            let message = serde_json::from_slice::<TradeMsg>(black_box(TRADE)).unwrap();
            black_box(message);
        });
    });

    group.bench_function(format!("book_snapshot_{BOOK_DEPTH}"), |b| {
        b.iter(|| {
            let message =
                serde_json::from_slice::<BookSnapshotMsg>(black_box(book_snapshot.as_bytes()))
                    .unwrap();
            black_box(message);
        });
    });

    group.finish();
}

fn book_snapshot_json(depth: usize) -> String {
    let levels = |price_cents: fn(usize) -> usize| {
        (0..depth)
            .map(|i| {
                let price_cents = price_cents(i);
                let amount_millis = 1_000 + 37 * i;
                format!(
                    r#"{{"price":{}.{:02},"amount":{}.{:03}}}"#,
                    price_cents / 100,
                    price_cents % 100,
                    amount_millis / 1_000,
                    amount_millis % 1_000,
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    };

    format!(
        r#"{{"type":"book_snapshot","symbol":"BTC-PERPETUAL","exchange":"deribit","name":"book_snapshot_{depth}_100ms","depth":{depth},"interval":100,"bids":[{}],"asks":[{}],"timestamp":"2024-09-01T00:00:00.100Z","localTimestamp":"2024-09-01T00:00:00.102Z"}}"#,
        levels(|i| 5_900_050 - 50 * i),
        levels(|i| 5_900_100 + 50 * i),
    )
}

criterion_group!(benches, bench_messages);
criterion_main!(benches);
