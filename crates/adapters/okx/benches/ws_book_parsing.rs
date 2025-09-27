// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::identifiers::InstrumentId;
use nautilus_okx::websocket::{messages::OKXWebSocketEvent, parse::parse_book_msg_vec};
use serde_json::from_str;

const BOOK_SNAPSHOT: &str = include_str!("../test_data/ws_books_snapshot.json");
const BOOK_UPDATE: &str = include_str!("../test_data/ws_books_update.json");

fn bench_book_snapshot(c: &mut Criterion) {
    c.bench_function("parse_book_snapshot", |b| {
        b.iter_batched(
            || from_str::<OKXWebSocketEvent>(BOOK_SNAPSHOT).expect("snapshot event"),
            |event| match event {
                OKXWebSocketEvent::BookData { data, action, .. } => {
                    let instrument_id = InstrumentId::from("BTC-USDT.OKX");
                    let payload = parse_book_msg_vec(
                        data,
                        &instrument_id,
                        2,
                        1,
                        action,
                        UnixNanos::default(),
                    )
                    .expect("snapshot parsing");
                    black_box(payload);
                }
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_book_update(c: &mut Criterion) {
    c.bench_function("parse_book_update", |b| {
        b.iter_batched(
            || from_str::<OKXWebSocketEvent>(BOOK_UPDATE).expect("update event"),
            |event| match event {
                OKXWebSocketEvent::BookData { data, action, .. } => {
                    let instrument_id = InstrumentId::from("BTC-USDT.OKX");
                    let payload = parse_book_msg_vec(
                        data,
                        &instrument_id,
                        2,
                        1,
                        action,
                        UnixNanos::default(),
                    )
                    .expect("update parsing");
                    black_box(payload);
                }
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        );
    });
}

fn benches(c: &mut Criterion) {
    bench_book_snapshot(c);
    bench_book_update(c);
}

criterion_group!(okx_books, benches);
criterion_main!(okx_books);
