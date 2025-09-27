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
use nautilus_okx::websocket::{messages::OKXWebSocketEvent, parse::parse_trade_msg_vec};
use serde_json::from_str;

const TRADES: &str = include_str!("../test_data/ws_trades.json");

fn bench_trades(c: &mut Criterion) {
    c.bench_function("parse_trades", |b| {
        b.iter_batched(
            || from_str::<OKXWebSocketEvent>(TRADES).expect("trades event"),
            |event| match event {
                OKXWebSocketEvent::Data { data, .. } => {
                    let instrument_id = InstrumentId::from("BTC-USDT.OKX");
                    let parsed =
                        parse_trade_msg_vec(data, &instrument_id, 1, 8, UnixNanos::default())
                            .expect("trade parsing");
                    black_box(parsed);
                }
                _ => unreachable!(),
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(okx_trades, bench_trades);
criterion_main!(okx_trades);
