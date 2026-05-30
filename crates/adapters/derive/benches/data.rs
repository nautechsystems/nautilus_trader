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
//! Each bench measures one message kind end-to-end (frame decode + channel
//! decode + parse + Nautilus type construction). No I/O, no async runtime, no
//! channel. The `bars` row is the REST OHLCV path (Derive has no WS candle
//! channel); it decodes the candle record and builds a `Bar`.

mod common;

use std::hint::black_box;

use common::{PRICE_PRECISION, SIZE_PRECISION, fixtures, subscription_frame};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_derive::{
    http::models::DerivePublicCandle,
    websocket::{
        messages::DeriveWsFrame,
        parse::{
            parse_candle_record, parse_funding_rate, parse_index_price, parse_mark_price,
            parse_option_greeks, parse_orderbook_deltas, parse_orderbook_msg, parse_ticker_msg,
            parse_ticker_quote, parse_trade_tick, parse_trades_msg,
        },
    },
};
use nautilus_model::data::BarType;

fn bench_book_deltas(c: &mut Criterion) {
    let frame = subscription_frame(fixtures::ORDERBOOK_CHANNEL, fixtures::ORDERBOOK);
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book_deltas", |b| {
        b.iter(|| {
            let DeriveWsFrame::Subscription(payload) =
                DeriveWsFrame::parse(black_box(&frame)).unwrap()
            else {
                unreachable!()
            };
            let msg = parse_orderbook_msg(&payload).unwrap();
            let deltas =
                parse_orderbook_deltas(&msg, PRICE_PRECISION, SIZE_PRECISION, ts_init).unwrap();
            black_box(deltas);
        });
    });
    group.finish();
}

fn bench_quotes(c: &mut Criterion) {
    let frame = subscription_frame(fixtures::TICKER_PERP_CHANNEL, fixtures::TICKER_PERP);
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("quotes", |b| {
        b.iter(|| {
            let DeriveWsFrame::Subscription(payload) =
                DeriveWsFrame::parse(black_box(&frame)).unwrap()
            else {
                unreachable!()
            };
            let msg = parse_ticker_msg(&payload).unwrap();
            let quote = parse_ticker_quote(&msg, PRICE_PRECISION, SIZE_PRECISION, ts_init).unwrap();
            black_box(quote);
        });
    });
    group.finish();
}

fn bench_trades(c: &mut Criterion) {
    let frame = subscription_frame(fixtures::TRADES_CHANNEL, &format!("[{}]", fixtures::TRADE));
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("trades", |b| {
        b.iter(|| {
            let DeriveWsFrame::Subscription(payload) =
                DeriveWsFrame::parse(black_box(&frame)).unwrap()
            else {
                unreachable!()
            };
            let msg = parse_trades_msg(&payload).unwrap();
            let tick =
                parse_trade_tick(&msg.trades[0], PRICE_PRECISION, SIZE_PRECISION, ts_init).unwrap();
            black_box(tick);
        });
    });
    group.finish();
}

fn bench_mark_price(c: &mut Criterion) {
    let frame = subscription_frame(fixtures::TICKER_PERP_CHANNEL, fixtures::TICKER_PERP);
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("mark_price", |b| {
        b.iter(|| {
            let DeriveWsFrame::Subscription(payload) =
                DeriveWsFrame::parse(black_box(&frame)).unwrap()
            else {
                unreachable!()
            };
            let msg = parse_ticker_msg(&payload).unwrap();
            let mark = parse_mark_price(&msg, PRICE_PRECISION, ts_init).unwrap();
            black_box(mark);
        });
    });
    group.finish();
}

fn bench_index_price(c: &mut Criterion) {
    let frame = subscription_frame(fixtures::TICKER_PERP_CHANNEL, fixtures::TICKER_PERP);
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("index_price", |b| {
        b.iter(|| {
            let DeriveWsFrame::Subscription(payload) =
                DeriveWsFrame::parse(black_box(&frame)).unwrap()
            else {
                unreachable!()
            };
            let msg = parse_ticker_msg(&payload).unwrap();
            let index = parse_index_price(&msg, PRICE_PRECISION, ts_init).unwrap();
            black_box(index);
        });
    });
    group.finish();
}

fn bench_funding_rate(c: &mut Criterion) {
    let frame = subscription_frame(fixtures::TICKER_PERP_CHANNEL, fixtures::TICKER_PERP);
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("funding_rate", |b| {
        b.iter(|| {
            let DeriveWsFrame::Subscription(payload) =
                DeriveWsFrame::parse(black_box(&frame)).unwrap()
            else {
                unreachable!()
            };
            let msg = parse_ticker_msg(&payload).unwrap();
            let funding = parse_funding_rate(&msg, ts_init).unwrap();
            black_box(funding);
        });
    });
    group.finish();
}

fn bench_bars(c: &mut Criterion) {
    let bar_type = BarType::from("ETH-PERP.DERIVE-1-MINUTE-LAST-EXTERNAL");
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("bars", |b| {
        b.iter(|| {
            let records: Vec<DerivePublicCandle> =
                serde_json::from_str(black_box(fixtures::CANDLES)).unwrap();
            let bar = parse_candle_record(
                &records[0],
                bar_type,
                PRICE_PRECISION,
                SIZE_PRECISION,
                ts_init,
            )
            .unwrap();
            black_box(bar);
        });
    });
    group.finish();
}

fn bench_option_greeks(c: &mut Criterion) {
    let frame = subscription_frame(fixtures::TICKER_OPTION_CHANNEL, fixtures::TICKER_OPTION);
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("option_greeks", |b| {
        b.iter(|| {
            let DeriveWsFrame::Subscription(payload) =
                DeriveWsFrame::parse(black_box(&frame)).unwrap()
            else {
                unreachable!()
            };
            let msg = parse_ticker_msg(&payload).unwrap();
            let greeks = parse_option_greeks(&msg, ts_init).unwrap();
            black_box(greeks);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_book_deltas,
    bench_quotes,
    bench_trades,
    bench_mark_price,
    bench_index_price,
    bench_funding_rate,
    bench_bars,
    bench_option_greeks,
);
criterion_main!(benches);
