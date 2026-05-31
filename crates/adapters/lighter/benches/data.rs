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

use common::{BENCH_ACCOUNT_INDEX, ETH_MARKET_INDEX, eth_perp, fixtures, instrument_cache};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_lighter::{
    common::enums::LighterCandleResolution,
    websocket::{
        messages::{LighterMarketStatsPayload, LighterWsFrame},
        parse::{
            parse_ws_bar, parse_ws_fill_report, parse_ws_funding_rate_update,
            parse_ws_index_price_update, parse_ws_mark_price_update, parse_ws_order_book_deltas,
            parse_ws_order_book_depth10, parse_ws_order_status_report, parse_ws_quote_tick,
            parse_ws_trade_tick,
        },
    },
};

fn bench_trades(c: &mut Criterion) {
    let instruments = instrument_cache();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("trades", |b| {
        b.iter(|| {
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::TRADE_UPDATE)).unwrap();
            let LighterWsFrame::Trade { trades, .. } = frame else {
                unreachable!()
            };
            let trade = &trades[0];
            let instrument = instruments.get(&trade.market_id).unwrap();
            let tick = parse_ws_trade_tick(trade, instrument, ts_init).unwrap();
            black_box(tick);
        });
    });
    group.finish();
}

fn bench_book_deltas(c: &mut Criterion) {
    let instruments = instrument_cache();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("book_deltas", |b| {
        b.iter(|| {
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::BOOK_UPDATE)).unwrap();
            let LighterWsFrame::OrderBook {
                order_book,
                timestamp,
                ..
            } = frame
            else {
                unreachable!()
            };
            let instrument = instruments.get(&ETH_MARKET_INDEX).unwrap();
            let deltas =
                parse_ws_order_book_deltas(&order_book, instrument, timestamp, false, ts_init)
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
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::BOOK_SNAPSHOT)).unwrap();
            let LighterWsFrame::OrderBookSnapshot {
                order_book,
                timestamp,
                ..
            } = frame
            else {
                unreachable!()
            };
            let instrument = instruments.get(&ETH_MARKET_INDEX).unwrap();
            let depth =
                parse_ws_order_book_depth10(&order_book, instrument, timestamp, ts_init).unwrap();
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
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::TICKER_UPDATE)).unwrap();
            let LighterWsFrame::Ticker {
                ticker, timestamp, ..
            } = frame
            else {
                unreachable!()
            };
            let instrument = instruments.get(&ETH_MARKET_INDEX).unwrap();
            let quote = parse_ws_quote_tick(&ticker, instrument, timestamp, ts_init)
                .unwrap()
                .unwrap();
            black_box(quote);
        });
    });
    group.finish();
}

fn bench_bars(c: &mut Criterion) {
    let instrument = eth_perp();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("bars", |b| {
        b.iter(|| {
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::CANDLE_UPDATE)).unwrap();
            let LighterWsFrame::Candle { candles, .. } = frame else {
                unreachable!()
            };
            let bar = parse_ws_bar(
                &instrument,
                &candles[0],
                LighterCandleResolution::OneMinute,
                ts_init,
            )
            .unwrap();
            black_box(bar);
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
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::MARKET_STATS_SINGLE)).unwrap();
            let LighterWsFrame::MarketStats {
                market_stats,
                timestamp,
                ..
            } = frame
            else {
                unreachable!()
            };
            let stats = match market_stats {
                LighterMarketStatsPayload::One(s) => s,
                _ => unreachable!(),
            };
            let instrument = instruments.get(&stats.market_id).unwrap();
            let update =
                parse_ws_mark_price_update(&stats, instrument, timestamp, ts_init).unwrap();
            black_box(update);
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
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::MARKET_STATS_SINGLE)).unwrap();
            let LighterWsFrame::MarketStats {
                market_stats,
                timestamp,
                ..
            } = frame
            else {
                unreachable!()
            };
            let stats = match market_stats {
                LighterMarketStatsPayload::One(s) => s,
                _ => unreachable!(),
            };
            let instrument = instruments.get(&stats.market_id).unwrap();
            let update =
                parse_ws_index_price_update(&stats, instrument, timestamp, ts_init).unwrap();
            black_box(update);
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
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::MARKET_STATS_SINGLE)).unwrap();
            let LighterWsFrame::MarketStats {
                market_stats,
                timestamp,
                ..
            } = frame
            else {
                unreachable!()
            };
            let stats = match market_stats {
                LighterMarketStatsPayload::One(s) => s,
                _ => unreachable!(),
            };
            let instrument = instruments.get(&stats.market_id).unwrap();
            let update =
                parse_ws_funding_rate_update(&stats, instrument, timestamp, ts_init).unwrap();
            black_box(update);
        });
    });
    group.finish();
}

fn bench_fill_report(c: &mut Criterion) {
    let instruments = instrument_cache();
    let account_id = common::account_id();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("fill_report", |b| {
        b.iter(|| {
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::ACCOUNT_ALL_TRADES_UPDATE)).unwrap();
            let LighterWsFrame::AccountAllTrades { trades, .. } = frame else {
                unreachable!()
            };
            // The account_all_trades payload is keyed by market id; this
            // bench measures one market entry, matching the per-frame work
            // the handler does in the consumption loop.
            let market_trades = trades.values().next().unwrap();
            let trade = &market_trades[0];
            let instrument = instruments.get(&trade.market_id).unwrap();
            let report =
                parse_ws_fill_report(trade, BENCH_ACCOUNT_INDEX, instrument, account_id, ts_init)
                    .unwrap()
                    .unwrap();
            black_box(report);
        });
    });
    group.finish();
}

fn bench_order_status(c: &mut Criterion) {
    let instruments = instrument_cache();
    let account_id = common::account_id();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("order_status", |b| {
        b.iter(|| {
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::ACCOUNT_ORDERS_UPDATE)).unwrap();
            let LighterWsFrame::AccountOrders { orders, .. } = frame else {
                unreachable!()
            };
            let market_orders = orders.values().next().unwrap();
            let order = &market_orders[0];
            let instrument = instruments.get(&order.market_index).unwrap();
            let report =
                parse_ws_order_status_report(order, instrument, account_id, ts_init).unwrap();
            black_box(report);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_trades,
    bench_book_deltas,
    bench_book_depth10,
    bench_quotes,
    bench_bars,
    bench_mark_price,
    bench_index_price,
    bench_funding_rate,
    bench_fill_report,
    bench_order_status,
);
criterion_main!(benches);
