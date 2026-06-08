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

//! Component-level micro-benches that decompose the canonical pipeline numbers
//! into their constituent costs (`decode_only`, `parse_only`, atom-level
//! Decimal/UUID/TradeId construction, dispatch-state churn).
//!
//! `decode_only` is the raw-bytes -> typed-message cost (frame envelope decode
//! plus the channel `from_value` decode). `parse_only` is the typed-message ->
//! Nautilus-domain cost. The two sum to the matching `data.rs` inbound number.
//! `parse_only/order_report` and `parse_only/fill_report` decompose the inbound
//! execution-report path that `dispatch` runs end-to-end in `exec.rs`.
//!
//! Use these when a `data.rs` or `exec.rs` bench regresses and you need to
//! localise where the time went.

mod common;

use std::{hint::black_box, str::FromStr};

use common::{PRICE_PRECISION, SIZE_PRECISION, account_id, fixtures, subscription_frame};
use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_derive::{
    common::consts::DERIVE_VENUE,
    http::{
        models::{DeriveOrder, DeriveTrade},
        parse::{parse_derive_order_to_report, parse_derive_trade_to_fill_report},
    },
    websocket::{
        dispatch::{OrderIdentity, WsDispatchState},
        messages::{DeriveOrderbookMsg, DeriveTickerMsg, DeriveTradesMsg, DeriveWsFrame},
        parse::{
            parse_orderbook_deltas, parse_orderbook_msg, parse_ticker_msg, parse_ticker_quote,
            parse_trade_tick, parse_trades_msg,
        },
    },
};
use nautilus_model::{
    enums::{OrderSide, OrderType},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, Symbol, TradeId, VenueOrderId},
    types::{Currency, Price},
};
use rust_decimal::Decimal;

fn orderbook_frame() -> String {
    subscription_frame(fixtures::ORDERBOOK_CHANNEL, fixtures::ORDERBOOK)
}

fn ticker_frame() -> String {
    subscription_frame(fixtures::TICKER_PERP_CHANNEL, fixtures::TICKER_PERP)
}

fn trades_frame() -> String {
    subscription_frame(fixtures::TRADES_CHANNEL, &format!("[{}]", fixtures::TRADE))
}

fn orderbook_msg() -> DeriveOrderbookMsg {
    let frame = orderbook_frame();
    let DeriveWsFrame::Subscription(payload) = DeriveWsFrame::parse(&frame).unwrap() else {
        unreachable!()
    };
    parse_orderbook_msg(&payload).unwrap()
}

fn trades_msg() -> DeriveTradesMsg {
    let frame = trades_frame();
    let DeriveWsFrame::Subscription(payload) = DeriveWsFrame::parse(&frame).unwrap() else {
        unreachable!()
    };
    parse_trades_msg(&payload).unwrap()
}

fn ticker_msg() -> DeriveTickerMsg {
    let frame = ticker_frame();
    let DeriveWsFrame::Subscription(payload) = DeriveWsFrame::parse(&frame).unwrap() else {
        unreachable!()
    };
    parse_ticker_msg(&payload).unwrap()
}

// Decode-only: raw bytes -> typed message

fn bench_decode_orderbook(c: &mut Criterion) {
    let frame = orderbook_frame();
    let mut group = c.benchmark_group("decode_only");
    group.bench_function("orderbook", |b| {
        b.iter(|| {
            let DeriveWsFrame::Subscription(payload) =
                DeriveWsFrame::parse(black_box(&frame)).unwrap()
            else {
                unreachable!()
            };
            let msg = parse_orderbook_msg(&payload).unwrap();
            black_box(msg);
        });
    });
    group.finish();
}

fn bench_decode_ticker(c: &mut Criterion) {
    let frame = ticker_frame();
    let mut group = c.benchmark_group("decode_only");
    group.bench_function("ticker", |b| {
        b.iter(|| {
            let DeriveWsFrame::Subscription(payload) =
                DeriveWsFrame::parse(black_box(&frame)).unwrap()
            else {
                unreachable!()
            };
            let msg = parse_ticker_msg(&payload).unwrap();
            black_box(msg);
        });
    });
    group.finish();
}

// Parse-only: typed message -> Nautilus domain

fn bench_parse_orderbook_deltas(c: &mut Criterion) {
    let msg = orderbook_msg();
    let mut group = c.benchmark_group("parse_only");
    group.bench_function("orderbook_deltas", |b| {
        b.iter(|| {
            let deltas = parse_orderbook_deltas(
                black_box(&msg),
                PRICE_PRECISION,
                SIZE_PRECISION,
                UnixNanos::default(),
            )
            .unwrap();
            black_box(deltas);
        });
    });
    group.finish();
}

fn bench_parse_trade(c: &mut Criterion) {
    let msg = trades_msg();
    let mut group = c.benchmark_group("parse_only");
    group.bench_function("trade", |b| {
        b.iter(|| {
            let tick = parse_trade_tick(
                black_box(&msg.trades[0]),
                PRICE_PRECISION,
                SIZE_PRECISION,
                UnixNanos::default(),
            )
            .unwrap();
            black_box(tick);
        });
    });
    group.finish();
}

fn bench_parse_ticker_quote(c: &mut Criterion) {
    let msg = ticker_msg();
    let mut group = c.benchmark_group("parse_only");
    group.bench_function("ticker_quote", |b| {
        b.iter(|| {
            let quote = parse_ticker_quote(
                black_box(&msg),
                PRICE_PRECISION,
                SIZE_PRECISION,
                UnixNanos::default(),
            )
            .unwrap();
            black_box(quote);
        });
    });
    group.finish();
}

fn bench_parse_order_report(c: &mut Criterion) {
    let order: DeriveOrder = serde_json::from_str(fixtures::ORDER).unwrap();
    let account_id = account_id();
    let mut group = c.benchmark_group("parse_only");
    group.bench_function("order_report", |b| {
        b.iter(|| {
            let report =
                parse_derive_order_to_report(black_box(&order), account_id, UnixNanos::default())
                    .unwrap();
            black_box(report);
        });
    });
    group.finish();
}

fn bench_parse_fill_report(c: &mut Criterion) {
    let trade: DeriveTrade = serde_json::from_str(fixtures::TRADE_PRIVATE).unwrap();
    let account_id = account_id();
    let fee_currency = Currency::USDC();
    let mut group = c.benchmark_group("parse_only");
    group.bench_function("fill_report", |b| {
        b.iter(|| {
            let report = parse_derive_trade_to_fill_report(
                black_box(&trade),
                account_id,
                fee_currency,
                UnixNanos::default(),
            )
            .unwrap();
            black_box(report);
        });
    });
    group.finish();
}

// Atomic costs

fn bench_decimal_from_str(c: &mut Criterion) {
    let s = "3500.5";
    c.bench_function("atom/decimal_from_str", |b| {
        b.iter(|| {
            let d = Decimal::from_str(black_box(s)).unwrap();
            black_box(d);
        });
    });
}

fn bench_price_from_decimal_dp(c: &mut Criterion) {
    let d = Decimal::from_str("3500.5").unwrap();
    c.bench_function("atom/price_from_decimal_dp", |b| {
        b.iter(|| {
            let p = Price::from_decimal_dp(black_box(d), PRICE_PRECISION).unwrap();
            black_box(p);
        });
    });
}

fn bench_price_combined(c: &mut Criterion) {
    let s = "3500.5";
    c.bench_function("atom/price_combined", |b| {
        b.iter(|| {
            let d = Decimal::from_str(black_box(s)).unwrap();
            let p = Price::from_decimal_dp(d, PRICE_PRECISION).unwrap();
            black_box(p);
        });
    });
}

fn bench_trade_id_new(c: &mut Criterion) {
    let s = "trade-xyz";
    c.bench_function("atom/trade_id_new", |b| {
        b.iter(|| {
            let id = TradeId::new(black_box(s));
            black_box(id);
        });
    });
}

fn bench_uuid4_new(c: &mut Criterion) {
    c.bench_function("atom/uuid4_new", |b| {
        b.iter(|| {
            let u = UUID4::new();
            black_box(u);
        });
    });
}

// Dispatch-state churn

fn ident() -> OrderIdentity {
    OrderIdentity {
        instrument_id: InstrumentId::new(Symbol::new("ETH-PERP"), *DERIVE_VENUE),
        strategy_id: StrategyId::from("S-BENCH"),
        order_side: OrderSide::Buy,
        order_type: OrderType::Limit,
    }
}

fn bench_state_construct(c: &mut Criterion) {
    let cid = ClientOrderId::from("O-BENCH-S");
    let voi = VenueOrderId::from("order-abc");
    c.bench_function("atom/state_construct_primed", |b| {
        b.iter(|| {
            let state = WsDispatchState::new();
            state.register_identity(cid, ident());
            state.record_venue_order_id(cid, voi);
            state.mark_accepted(cid);
            black_box(state);
        });
    });
}

fn bench_state_drop(c: &mut Criterion) {
    let cid = ClientOrderId::from("O-BENCH-S");
    let voi = VenueOrderId::from("order-abc");
    c.bench_function("atom/state_drop_primed", |b| {
        b.iter_with_setup(
            || {
                let state = WsDispatchState::new();
                state.register_identity(cid, ident());
                state.record_venue_order_id(cid, voi);
                state.mark_accepted(cid);
                state
            },
            |state| {
                drop(black_box(state));
            },
        );
    });
}

// Trade dedup hit on a reused state: the first insert primes the cache, every
// measured iteration takes the already-seen early return.
fn bench_dedup_trade_hit(c: &mut Criterion) {
    let trade_id = TradeId::new("trade-xyz");
    let state = WsDispatchState::new();
    state.check_and_insert_trade(trade_id);
    c.bench_function("atom/dedup_trade_hit", |b| {
        b.iter(|| {
            let seen = state.check_and_insert_trade(black_box(trade_id));
            black_box(seen);
        });
    });
}

criterion_group!(
    benches,
    bench_decode_orderbook,
    bench_decode_ticker,
    bench_parse_orderbook_deltas,
    bench_parse_trade,
    bench_parse_ticker_quote,
    bench_parse_order_report,
    bench_parse_fill_report,
    bench_decimal_from_str,
    bench_price_from_decimal_dp,
    bench_price_combined,
    bench_trade_id_new,
    bench_uuid4_new,
    bench_state_construct,
    bench_state_drop,
    bench_dedup_trade_hit,
);
criterion_main!(benches);
