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

//! Canonical inbound pipeline benches: raw WS frame bytes → Nautilus domain type.
//!
//! Each bench measures one message kind end-to-end (decode + parse + cache
//! lookup + Nautilus type construction). No I/O, no async runtime, no channel.

mod common;

use std::hint::black_box;

use common::{btc_perp, fixtures, instrument_cache};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::UnixNanos;
use nautilus_hyperliquid::websocket::{
    messages::{HyperliquidWsMessage, WsActiveAssetCtxData, WsUserEventData},
    parse::{
        parse_ws_asset_context, parse_ws_candle, parse_ws_fill_report, parse_ws_order_book_deltas,
        parse_ws_order_book_depth10, parse_ws_order_status_report, parse_ws_quote_tick,
        parse_ws_trade_tick,
    },
};
use nautilus_model::data::BarType;

fn bench_trades(c: &mut Criterion) {
    let instruments = instrument_cache();
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("trades", |b| {
        b.iter(|| {
            let msg: HyperliquidWsMessage =
                serde_json::from_str(black_box(fixtures::TRADE)).unwrap();
            let HyperliquidWsMessage::Trades { data } = msg else {
                unreachable!()
            };
            let trade = &data[0];
            let instrument = instruments.get(&trade.coin).unwrap();
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
            let msg: HyperliquidWsMessage =
                serde_json::from_str(black_box(fixtures::BOOK_L2)).unwrap();
            let HyperliquidWsMessage::L2Book { data } = msg else {
                unreachable!()
            };
            let instrument = instruments.get(&data.coin).unwrap();
            let deltas = parse_ws_order_book_deltas(&data, instrument, ts_init).unwrap();
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
            let msg: HyperliquidWsMessage =
                serde_json::from_str(black_box(fixtures::BOOK_L2)).unwrap();
            let HyperliquidWsMessage::L2Book { data } = msg else {
                unreachable!()
            };
            let instrument = instruments.get(&data.coin).unwrap();
            let depth = parse_ws_order_book_depth10(&data, instrument, ts_init).unwrap();
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
            let msg: HyperliquidWsMessage = serde_json::from_str(black_box(fixtures::BBO)).unwrap();
            let HyperliquidWsMessage::Bbo { data } = msg else {
                unreachable!()
            };
            let instrument = instruments.get(&data.coin).unwrap();
            let quote = parse_ws_quote_tick(&data, instrument, ts_init).unwrap();
            black_box(quote);
        });
    });
    group.finish();
}

fn bench_bars(c: &mut Criterion) {
    let instrument = btc_perp();
    let bar_type = BarType::from("BTC-USD-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL");
    let ts_init = UnixNanos::default();

    let mut group = c.benchmark_group("inbound_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("bars", |b| {
        b.iter(|| {
            let msg: HyperliquidWsMessage =
                serde_json::from_str(black_box(fixtures::CANDLE)).unwrap();
            let HyperliquidWsMessage::Candle { data } = msg else {
                unreachable!()
            };
            let bar = parse_ws_candle(&data, &instrument, &bar_type, ts_init).unwrap();
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
            let msg: HyperliquidWsMessage =
                serde_json::from_str(black_box(fixtures::ACTIVE_ASSET_CTX_PERP)).unwrap();
            let HyperliquidWsMessage::ActiveAssetCtx { data } = msg else {
                unreachable!()
            };
            let coin = match &data {
                WsActiveAssetCtxData::Perp { coin, .. }
                | WsActiveAssetCtxData::Spot { coin, .. } => *coin,
            };
            let instrument = instruments.get(&coin).unwrap();
            let (mark, _index, _funding) =
                parse_ws_asset_context(&data, instrument, ts_init).unwrap();
            black_box(mark);
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
            let msg: HyperliquidWsMessage =
                serde_json::from_str(black_box(fixtures::ACTIVE_ASSET_CTX_PERP)).unwrap();
            let HyperliquidWsMessage::ActiveAssetCtx { data } = msg else {
                unreachable!()
            };
            let coin = match &data {
                WsActiveAssetCtxData::Perp { coin, .. }
                | WsActiveAssetCtxData::Spot { coin, .. } => *coin,
            };
            let instrument = instruments.get(&coin).unwrap();
            let (_mark, index, _funding) =
                parse_ws_asset_context(&data, instrument, ts_init).unwrap();
            black_box(index);
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
            let msg: HyperliquidWsMessage =
                serde_json::from_str(black_box(fixtures::ACTIVE_ASSET_CTX_PERP)).unwrap();
            let HyperliquidWsMessage::ActiveAssetCtx { data } = msg else {
                unreachable!()
            };
            let coin = match &data {
                WsActiveAssetCtxData::Perp { coin, .. }
                | WsActiveAssetCtxData::Spot { coin, .. } => *coin,
            };
            let instrument = instruments.get(&coin).unwrap();
            let (_mark, _index, funding) =
                parse_ws_asset_context(&data, instrument, ts_init).unwrap();
            black_box(funding);
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
            let msg: HyperliquidWsMessage =
                serde_json::from_str(black_box(fixtures::USER_FILL)).unwrap();
            let fills = match msg {
                HyperliquidWsMessage::User { data } | HyperliquidWsMessage::UserEvents { data } => {
                    match data {
                        WsUserEventData::Fills { fills } => fills,
                        _ => unreachable!(),
                    }
                }
                _ => unreachable!(),
            };
            let fill = &fills[0];
            let instrument = instruments.get(&fill.coin).unwrap();
            let report = parse_ws_fill_report(fill, instrument, account_id, ts_init).unwrap();
            black_box(report);
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
            let msg: HyperliquidWsMessage =
                serde_json::from_str(black_box(fixtures::ORDER_UPDATE)).unwrap();
            let HyperliquidWsMessage::OrderUpdates { data } = msg else {
                unreachable!()
            };
            let order = &data[0];
            let instrument = instruments.get(&order.order.coin).unwrap();
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
    bench_order_fill,
    bench_order_event,
);
criterion_main!(benches);
