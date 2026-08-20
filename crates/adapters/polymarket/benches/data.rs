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

use ahash::AHashMap;
use common::{fixtures, instrument_cache, instrument_precisions, yes_instrument};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::OrderBookDeltas, enums::LiquiditySide, identifiers::InstrumentId,
    instruments::Instrument, types::Currency,
};
use nautilus_polymarket::{
    common::enums::PolymarketOrderSide,
    execution::parse::{build_maker_fill_report, parse_fill_report, parse_order_status_report},
    http::models::{PolymarketOpenOrder, PolymarketTradeReport},
    websocket::{
        messages::{MarketWsMessage, PolymarketQuote, PolymarketQuotes},
        parse::{
            parse_book_deltas, parse_book_snapshot, parse_quote_from_price_change,
            parse_quote_from_snapshot, parse_timestamp_ms, parse_trade_tick,
        },
    },
};
use rust_decimal_macros::dec;
use ustr::Ustr;

#[derive(Clone, Copy)]
struct BookDeltaMeta {
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
}

fn price_change_batch() -> (PolymarketQuotes, AHashMap<Ustr, BookDeltaMeta>) {
    let asset_a = Ustr::from("0xTOKEN-A");
    let asset_b = Ustr::from("0xTOKEN-B");
    let instrument_a = InstrumentId::from("A.POLYMARKET");
    let instrument_b = InstrumentId::from("B.POLYMARKET");
    let price_changes = [
        (asset_a, "0.007", PolymarketOrderSide::Buy, "20"),
        (asset_b, "0.997", PolymarketOrderSide::Buy, "20"),
        (asset_a, "0.005", PolymarketOrderSide::Sell, "0"),
        (asset_b, "0.995", PolymarketOrderSide::Sell, "0"),
        (asset_a, "0.009", PolymarketOrderSide::Sell, "30"),
        (asset_b, "0.999", PolymarketOrderSide::Sell, "30"),
    ]
    .into_iter()
    .map(|(asset_id, price, side, size)| PolymarketQuote {
        asset_id,
        price: price.to_string(),
        side,
        size: size.to_string(),
        hash: String::new(),
        best_bid: None,
        best_ask: None,
    })
    .collect();
    let metadata = AHashMap::from_iter([
        (
            asset_a,
            BookDeltaMeta {
                instrument_id: instrument_a,
                price_precision: 3,
                size_precision: 2,
            },
        ),
        (
            asset_b,
            BookDeltaMeta {
                instrument_id: instrument_b,
                price_precision: 3,
                size_precision: 2,
            },
        ),
    ]);

    (
        PolymarketQuotes {
            market: Ustr::from("0xMARKET"),
            price_changes,
            timestamp: "1700000003000".to_string(),
        },
        metadata,
    )
}

fn dispatch_book_deltas(
    quotes: &PolymarketQuotes,
    metadata: &AHashMap<Ustr, BookDeltaMeta>,
    ts_init: UnixNanos,
) -> Vec<OrderBookDeltas> {
    let ts_event = parse_timestamp_ms(&quotes.timestamp).unwrap();
    let mut resolved = Vec::with_capacity(quotes.price_changes.len());
    let mut groups: Vec<(BookDeltaMeta, Vec<&PolymarketQuote>)> = Vec::new();
    let mut group_indices = AHashMap::with_capacity(quotes.price_changes.len());

    for change in &quotes.price_changes {
        let meta = *metadata.get(&change.asset_id).unwrap();
        let group_index = match group_indices.get(&meta.instrument_id) {
            Some(index) => *index,
            None => {
                let index = groups.len();
                groups.push((meta, Vec::new()));
                group_indices.insert(meta.instrument_id, index);
                index
            }
        };
        groups[group_index].1.push(change);
        resolved.push((group_index, meta, change));
    }

    let mut batches = Vec::with_capacity(groups.len());
    for (group_index, meta, _change) in resolved {
        let changes = std::mem::take(&mut groups[group_index].1);
        if changes.is_empty() {
            continue;
        }

        let parsed = parse_book_deltas(
            &changes,
            meta.instrument_id,
            meta.price_precision,
            meta.size_precision,
            ts_event,
            ts_init,
        )
        .into_iter()
        .filter_map(Result::ok)
        .collect();
        batches.push(OrderBookDeltas::new(meta.instrument_id, parsed));
    }

    batches
}

fn bench_price_change_dispatch(c: &mut Criterion) {
    let (quotes, metadata) = price_change_batch();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(quotes.price_changes.len() as u64));
    group.bench_function("price_change_interleaved", |b| {
        b.iter(|| {
            black_box(dispatch_book_deltas(
                black_box(&quotes),
                black_box(&metadata),
                ts_init,
            ));
        });
    });
    group.finish();
}

fn bench_book_deltas(c: &mut Criterion) {
    let instruments = instrument_cache();
    let (px_prec, sz_prec) = instrument_precisions();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book_deltas", |b| {
        b.iter(|| {
            let msg = MarketWsMessage::parse(black_box(fixtures::MARKET_PRICE_CHANGE)).unwrap();
            let MarketWsMessage::PriceChange(quotes) = msg else {
                unreachable!()
            };
            let asset_id = quotes.price_changes[0].asset_id;
            let instrument = instruments.get(&asset_id).unwrap();
            let changes = [&quotes.price_changes[0]];
            let ts_event = parse_timestamp_ms(&quotes.timestamp).unwrap();
            let parsed = parse_book_deltas(
                &changes,
                instrument.id(),
                px_prec,
                sz_prec,
                ts_event,
                ts_init,
            )
            .into_iter()
            .collect::<anyhow::Result<Vec<_>>>()
            .unwrap();
            let deltas = OrderBookDeltas::new(instrument.id(), parsed);
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
            let msg = MarketWsMessage::parse(black_box(fixtures::MARKET_BOOK)).unwrap();
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
            let msg = MarketWsMessage::parse(black_box(fixtures::MARKET_BOOK)).unwrap();
            let MarketWsMessage::Book(snap) = msg else {
                unreachable!()
            };
            let instrument = instruments.get(&snap.asset_id).unwrap();
            let quote = parse_quote_from_snapshot(
                &snap,
                instrument.id(),
                px_prec,
                sz_prec,
                instrument.price_increment(),
                true,
                ts_init,
            )
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
            let msg = MarketWsMessage::parse(black_box(fixtures::MARKET_PRICE_CHANGE)).unwrap();
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
                instrument.price_increment(),
                true,
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
            let msg = MarketWsMessage::parse(black_box(fixtures::MARKET_LAST_TRADE)).unwrap();
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
    let taker_fee = dec!(0.03);
    let fee_exponent = 2.0;
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
                fee_exponent,
                ts_init,
            )
            .expect("benchmark fixture commission is representable");
            black_box(report);
        });
    });
    group.finish();
}

fn bench_order_fill_maker(c: &mut Criterion) {
    let instrument = yes_instrument();
    let (px_prec, sz_prec) = instrument_precisions();
    let account_id = common::account_id();
    let currency = Currency::pUSD();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("order_fill_maker", |b| {
        b.iter(|| {
            let trade: PolymarketTradeReport =
                serde_json::from_str(black_box(fixtures::HTTP_TRADE_REPORT)).unwrap();
            let reports: Vec<_> = trade
                .maker_orders
                .iter()
                .map(|order| {
                    build_maker_fill_report(
                        order,
                        &trade.id,
                        trade.trader_side,
                        trade.side,
                        trade.asset_id.as_str(),
                        account_id,
                        instrument.id(),
                        px_prec,
                        sz_prec,
                        currency,
                        LiquiditySide::Maker,
                        ts_init,
                        ts_init,
                    )
                    .expect("benchmark fixture commission is representable")
                })
                .collect();
            black_box(reports);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_price_change_dispatch,
    bench_book_deltas,
    bench_book_snapshot,
    bench_quote_from_snapshot,
    bench_quote_from_price_change,
    bench_trades,
    bench_order_event,
    bench_order_fill,
    bench_order_fill_maker,
);
criterion_main!(benches);
