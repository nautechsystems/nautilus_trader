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

//! Canonical inbound pipeline benches: raw WS frame bytes -> Nautilus domain type.
//!
//! Each bench measures one message kind end-to-end (decode + parse + cache
//! lookup + Nautilus type construction). No I/O, no async runtime, no channel.

mod common;

use std::hint::black_box;

use ahash::AHashMap;
use common::{fixtures, instrument_cache};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, Data, bar::BAR_SPEC_1_DAY_LAST},
    instruments::Instrument,
};
use nautilus_okx::websocket::{
    messages::{OKXOrderMsg, OKXWsFrame},
    parse::{
        parse_book_msg_vec, parse_book10_msg_vec, parse_candle_msg_vec, parse_funding_rate_msg_vec,
        parse_index_price_msg_vec, parse_mark_price_msg_vec, parse_order_msg_vec,
        parse_quote_msg_vec, parse_trade_msg_vec,
    },
};
use ustr::Ustr;

fn bench_book_deltas(c: &mut Criterion) {
    let instruments = instrument_cache();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book_deltas", |b| {
        b.iter(|| {
            let frame: OKXWsFrame = serde_json::from_str(black_box(fixtures::BOOK_UPDATE)).unwrap();
            let OKXWsFrame::BookData { arg, action, data } = frame else {
                unreachable!()
            };
            let inst_id = arg.inst_id.unwrap();
            let instrument = instruments.get(&inst_id).unwrap();
            let deltas = parse_book_msg_vec(
                data,
                &instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
                action,
                ts_init,
            )
            .unwrap();
            black_box(deltas);
        });
    });
    group.finish();
}

fn bench_book_depth10(c: &mut Criterion) {
    let instruments = instrument_cache();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book_depth10", |b| {
        b.iter(|| {
            let frame: OKXWsFrame =
                serde_json::from_str(black_box(fixtures::BOOK_SNAPSHOT)).unwrap();
            let OKXWsFrame::BookData { arg, data, .. } = frame else {
                unreachable!()
            };
            let inst_id = arg.inst_id.unwrap();
            let instrument = instruments.get(&inst_id).unwrap();
            let depth = parse_book10_msg_vec(
                data,
                &instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
            )
            .unwrap();
            black_box(depth);
        });
    });
    group.finish();
}

fn bench_quotes(c: &mut Criterion) {
    let instruments = instrument_cache();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("quotes", |b| {
        b.iter(|| {
            let frame: OKXWsFrame = serde_json::from_str(black_box(fixtures::BBO_TBT)).unwrap();
            let OKXWsFrame::Data { arg, data } = frame else {
                unreachable!()
            };
            let inst_id = arg.inst_id.unwrap();
            let instrument = instruments.get(&inst_id).unwrap();
            let quotes = parse_quote_msg_vec(
                data,
                &instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
            )
            .unwrap();
            black_box(quotes);
        });
    });
    group.finish();
}

fn bench_trades(c: &mut Criterion) {
    let instruments = instrument_cache();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("trades", |b| {
        b.iter(|| {
            let frame: OKXWsFrame = serde_json::from_str(black_box(fixtures::TRADE)).unwrap();
            let OKXWsFrame::Data { arg, data } = frame else {
                unreachable!()
            };
            let inst_id = arg.inst_id.unwrap();
            let instrument = instruments.get(&inst_id).unwrap();
            let trades = parse_trade_msg_vec(
                data,
                &instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
            )
            .unwrap();
            black_box(trades);
        });
    });
    group.finish();
}

fn bench_mark_price(c: &mut Criterion) {
    let instruments = instrument_cache();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("mark_price", |b| {
        b.iter(|| {
            let frame: OKXWsFrame = serde_json::from_str(black_box(fixtures::MARK_PRICE)).unwrap();
            let OKXWsFrame::Data { arg, data } = frame else {
                unreachable!()
            };
            let inst_id = arg.inst_id.unwrap();
            let instrument = instruments.get(&inst_id).unwrap();
            let updates = parse_mark_price_msg_vec(
                data,
                &instrument.id(),
                instrument.price_precision(),
                ts_init,
            )
            .unwrap();
            black_box(updates);
        });
    });
    group.finish();
}

fn bench_index_price(c: &mut Criterion) {
    let instruments = instrument_cache();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("index_price", |b| {
        b.iter(|| {
            let frame: OKXWsFrame = serde_json::from_str(black_box(fixtures::INDEX_PRICE)).unwrap();
            let OKXWsFrame::Data { arg, data } = frame else {
                unreachable!()
            };
            let inst_id = arg.inst_id.unwrap();
            let instrument = instruments.get(&inst_id).unwrap();
            let updates = parse_index_price_msg_vec(
                data,
                &instrument.id(),
                instrument.price_precision(),
                ts_init,
            )
            .unwrap();
            black_box(updates);
        });
    });
    group.finish();
}

fn bench_funding_rate(c: &mut Criterion) {
    let instruments = instrument_cache();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("funding_rate", |b| {
        b.iter(|| {
            // Use a fresh cache each iter so the dedup short-circuit doesn't skip parsing.
            let mut funding_cache: AHashMap<Ustr, (Ustr, u64)> = AHashMap::new();
            let frame: OKXWsFrame =
                serde_json::from_str(black_box(fixtures::FUNDING_RATE)).unwrap();
            let OKXWsFrame::Data { arg, data } = frame else {
                unreachable!()
            };
            let inst_id = arg.inst_id.unwrap();
            let instrument = instruments.get(&inst_id).unwrap();
            let updates =
                parse_funding_rate_msg_vec(data, &instrument.id(), ts_init, &mut funding_cache)
                    .unwrap();
            black_box(updates);
        });
    });
    group.finish();
}

fn bench_bars(c: &mut Criterion) {
    let instruments = instrument_cache();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("bars", |b| {
        b.iter(|| {
            let frame: OKXWsFrame = serde_json::from_str(black_box(fixtures::CANDLE)).unwrap();
            let OKXWsFrame::Data { arg, data } = frame else {
                unreachable!()
            };
            let inst_id = arg.inst_id.unwrap();
            let instrument = instruments.get(&inst_id).unwrap();
            let bars = parse_candle_msg_vec(
                data,
                &instrument.id(),
                instrument.price_precision(),
                instrument.size_precision(),
                BAR_SPEC_1_DAY_LAST,
                ts_init,
            )
            .unwrap();
            // The candle bench surfaces a closed-bar emit; assert non-empty so the
            // workload matches the documented baseline rather than silently
            // collapsing if the fixture's `confirm` flag changes.
            debug_assert!(matches!(bars.first(), Some(Data::Bar(_))));
            let _bar: &Bar = match &bars[0] {
                Data::Bar(b) => b,
                _ => unreachable!(),
            };
            black_box(bars);
        });
    });
    group.finish();
}

fn bench_order_event(c: &mut Criterion) {
    let instruments = instrument_cache();
    let account_id = common::account_id();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("order_event", |b| {
        b.iter(|| {
            let mut fee_cache = AHashMap::new();
            let mut filled_qty_cache = AHashMap::new();
            let frame: OKXWsFrame = serde_json::from_str(black_box(fixtures::ORDER_LIVE)).unwrap();
            let OKXWsFrame::Data { data, .. } = frame else {
                unreachable!()
            };
            let msgs: Vec<OKXOrderMsg> = serde_json::from_value(data).unwrap();
            let reports = parse_order_msg_vec(
                &msgs,
                account_id,
                &instruments,
                &mut fee_cache,
                &mut filled_qty_cache,
                ts_init,
            )
            .unwrap();
            black_box(reports);
        });
    });
    group.finish();
}

fn bench_order_fill(c: &mut Criterion) {
    let instruments = instrument_cache();
    let account_id = common::account_id();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("order_fill", |b| {
        b.iter(|| {
            let mut fee_cache = AHashMap::new();
            let mut filled_qty_cache = AHashMap::new();
            let frame: OKXWsFrame = serde_json::from_str(black_box(fixtures::ORDERS)).unwrap();
            let OKXWsFrame::Data { data, .. } = frame else {
                unreachable!()
            };
            let msgs: Vec<OKXOrderMsg> = serde_json::from_value(data).unwrap();
            let reports = parse_order_msg_vec(
                &msgs,
                account_id,
                &instruments,
                &mut fee_cache,
                &mut filled_qty_cache,
                ts_init,
            )
            .unwrap();
            black_box(reports);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_book_deltas,
    bench_book_depth10,
    bench_quotes,
    bench_trades,
    bench_mark_price,
    bench_index_price,
    bench_funding_rate,
    bench_bars,
    bench_order_event,
    bench_order_fill,
);
criterion_main!(benches);
