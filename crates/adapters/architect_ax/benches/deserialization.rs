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

//! Benchmarks for Ax WebSocket message deserialization.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_architect_ax::websocket::{
    messages::{AxMdBookL1, AxMdBookL2, AxMdBookL3, AxMdTrade},
    parse::parse_md_message,
};

const MD_BOOK_L1: &str = include_str!("../test_data/ws_md_book_l1_captured.json");
const MD_BOOK_L2: &str = include_str!("../test_data/ws_md_book_l2_captured.json");
const MD_BOOK_L3: &str = include_str!("../test_data/ws_md_book_l3_captured.json");
const MD_TRADE: &str = include_str!("../test_data/ws_md_trade_captured.json");
const MD_TICKER: &str = include_str!("../test_data/ws_md_ticker_captured.json");
const MD_HEARTBEAT: &str = include_str!("../test_data/ws_md_heartbeat_captured.json");

fn bench_md_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("ax_md_parse");

    group.bench_function("book_l1", |b| {
        b.iter(|| parse_md_message(black_box(MD_BOOK_L1)).unwrap());
    });

    group.bench_function("book_l2", |b| {
        b.iter(|| parse_md_message(black_box(MD_BOOK_L2)).unwrap());
    });

    group.bench_function("book_l3", |b| {
        b.iter(|| parse_md_message(black_box(MD_BOOK_L3)).unwrap());
    });

    group.bench_function("trade", |b| {
        b.iter(|| parse_md_message(black_box(MD_TRADE)).unwrap());
    });

    group.bench_function("ticker", |b| {
        b.iter(|| parse_md_message(black_box(MD_TICKER)).unwrap());
    });

    group.bench_function("heartbeat", |b| {
        b.iter(|| parse_md_message(black_box(MD_HEARTBEAT)).unwrap());
    });

    group.finish();
}

fn bench_direct_struct_deser(c: &mut Criterion) {
    let mut group = c.benchmark_group("ax_direct_struct_deser");

    group.bench_function("book_l1", |b| {
        b.iter(|| serde_json::from_str::<AxMdBookL1>(black_box(MD_BOOK_L1)).unwrap());
    });

    group.bench_function("book_l2", |b| {
        b.iter(|| serde_json::from_str::<AxMdBookL2>(black_box(MD_BOOK_L2)).unwrap());
    });

    group.bench_function("book_l3", |b| {
        b.iter(|| serde_json::from_str::<AxMdBookL3>(black_box(MD_BOOK_L3)).unwrap());
    });

    group.bench_function("trade", |b| {
        b.iter(|| serde_json::from_str::<AxMdTrade>(black_box(MD_TRADE)).unwrap());
    });

    group.finish();
}

criterion_group!(benches, bench_md_parse, bench_direct_struct_deser);
criterion_main!(benches);
