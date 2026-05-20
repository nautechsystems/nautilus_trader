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

//! Canonical inbound pipeline benches: raw WS / REST frame bytes -> Nautilus
//! domain type. Covers JSON decode + parse + cache lookup + Nautilus type
//! construction. No I/O, no async runtime, no channel.
//!
//! Each bench measures one message kind end-to-end. Rows are ordered from the
//! most fundamental market-data stream (book deltas / trades) through the
//! quote derivations down to the private user-channel reports.

mod common;

use std::hint::black_box;

use common::{fixtures, instrument_cache, instrument_precisions, yes_instrument};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_model::{instruments::Instrument, types::Currency};
use nautilus_polymarket::{
    execution::parse::{parse_fill_report, parse_order_status_report},
    http::models::{PolymarketOpenOrder, PolymarketTradeReport},
    websocket::{
        messages::MarketWsMessage,
        parse::{
            parse_book_deltas, parse_book_snapshot, parse_quote_from_price_change,
            parse_quote_from_snapshot, parse_timestamp_ms, parse_trade_tick,
        },
    },
};
use rust_decimal::Decimal;

fn bench_book_deltas(c: &mut Criterion) {
    let instruments = instrument_cache();
    let (px_prec, sz_prec) = instrument_precisions();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book_deltas", |b| {
        b.iter(|| {
            let msg: MarketWsMessage =
                serde_json::from_str(black_box(fixtures::MARKET_PRICE_CHANGE)).unwrap();
            let MarketWsMessage::PriceChange(quotes) = msg else {
                unreachable!()
            };
            let asset_id = quotes.price_changes[0].asset_id;
            let instrument = instruments.get(&asset_id).unwrap();
            let deltas =
                parse_book_deltas(&quotes, instrument.id(), px_prec, sz_prec, ts_init).unwrap();
            black_box(deltas);
        });
    });
    group.finish();
}

fn bench_book_snapshot(c: &mut Criterion) {
    let instruments = instrument_cache();
    let (px_prec, sz_prec) = instrument_precisions();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book_snapshot", |b| {
        b.iter(|| {
            let msg: MarketWsMessage =
                serde_json::from_str(black_box(fixtures::MARKET_BOOK)).unwrap();
            let MarketWsMessage::Book(snap) = msg else {
                unreachable!()
            };
            let instrument = instruments.get(&snap.asset_id).unwrap();
            let deltas =
                parse_book_snapshot(&snap, instrument.id(), px_prec, sz_prec, ts_init).unwrap();
            black_box(deltas);
        });
    });
    group.finish();
}

fn bench_quote_from_snapshot(c: &mut Criterion) {
    let instruments = instrument_cache();
    let (px_prec, sz_prec) = instrument_precisions();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("quote_from_snapshot", |b| {
        b.iter(|| {
            let msg: MarketWsMessage =
                serde_json::from_str(black_box(fixtures::MARKET_BOOK)).unwrap();
            let MarketWsMessage::Book(snap) = msg else {
                unreachable!()
            };
            let instrument = instruments.get(&snap.asset_id).unwrap();
            let quote =
                parse_quote_from_snapshot(&snap, instrument.id(), px_prec, sz_prec, ts_init)
                    .unwrap();
            black_box(quote);
        });
    });
    group.finish();
}

fn bench_quote_from_price_change(c: &mut Criterion) {
    let instruments = instrument_cache();
    let (px_prec, sz_prec) = instrument_precisions();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("quote_from_price_change", |b| {
        b.iter(|| {
            let msg: MarketWsMessage =
                serde_json::from_str(black_box(fixtures::MARKET_PRICE_CHANGE)).unwrap();
            let MarketWsMessage::PriceChange(quotes) = msg else {
                unreachable!()
            };
            let change = &quotes.price_changes[0];
            let instrument = instruments.get(&change.asset_id).unwrap();
            let ts_event = parse_timestamp_ms(&quotes.timestamp).unwrap();
            let quote = parse_quote_from_price_change(
                change,
                instrument.id(),
                px_prec,
                sz_prec,
                None,
                ts_event,
                ts_init,
            )
            .unwrap();
            black_box(quote);
        });
    });
    group.finish();
}

fn bench_trades(c: &mut Criterion) {
    let instruments = instrument_cache();
    let (px_prec, sz_prec) = instrument_precisions();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("trades", |b| {
        b.iter(|| {
            let msg: MarketWsMessage =
                serde_json::from_str(black_box(fixtures::MARKET_LAST_TRADE)).unwrap();
            let MarketWsMessage::LastTradePrice(trade) = msg else {
                unreachable!()
            };
            let instrument = instruments.get(&trade.asset_id).unwrap();
            let tick =
                parse_trade_tick(&trade, instrument.id(), px_prec, sz_prec, ts_init).unwrap();
            black_box(tick);
        });
    });
    group.finish();
}

fn bench_order_event(c: &mut Criterion) {
    // Polymarket has no public WS user -> OrderStatusReport entry point
    // (the conversion is private to dispatch). The REST `GET /orders` parse
    // is the canonical equivalent and exercises the same string-decimal +
    // status-resolution work that the WS path does internally.
    let instrument = yes_instrument();
    let (px_prec, sz_prec) = instrument_precisions();
    let account_id = common::account_id();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("order_event", |b| {
        b.iter(|| {
            let order: PolymarketOpenOrder =
                serde_json::from_str(black_box(fixtures::HTTP_OPEN_ORDER)).unwrap();
            let report = parse_order_status_report(
                &order,
                instrument.id(),
                account_id,
                None,
                px_prec,
                sz_prec,
                ts_init,
            );
            black_box(report);
        });
    });
    group.finish();
}

fn bench_order_fill(c: &mut Criterion) {
    // Same rationale as `order_event`: REST `GET /trades` parse stands in for
    // the (private) WS user-trade -> FillReport conversion.
    let instrument = yes_instrument();
    let (px_prec, sz_prec) = instrument_precisions();
    let account_id = common::account_id();
    let currency = Currency::pUSD();
    let taker_fee = Decimal::ZERO;
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("order_fill", |b| {
        b.iter(|| {
            let trade: PolymarketTradeReport =
                serde_json::from_str(black_box(fixtures::HTTP_TRADE_REPORT)).unwrap();
            let report = parse_fill_report(
                &trade,
                instrument.id(),
                account_id,
                None,
                px_prec,
                sz_prec,
                currency,
                taker_fee,
                ts_init,
            );
            black_box(report);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_book_deltas,
    bench_book_snapshot,
    bench_quote_from_snapshot,
    bench_quote_from_price_change,
    bench_trades,
    bench_order_event,
    bench_order_fill,
);
criterion_main!(benches);
