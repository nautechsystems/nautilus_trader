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
//! Decimal/UUID/event construction, dispatch state churn).
//!
//! Use these when a `data.rs` or `exec.rs` bench regresses and you need to
//! localise where the time went, or when evaluating a structural change
//! (e.g. swapping the JSON tokenizer) and want to confirm the gain landed in
//! the layer it was supposed to.
//!
//! Same canonical surface every adapter should ship; pair this with `data.rs`,
//! `exec.rs`, and `signing.rs`.

mod common;

use std::{hint::black_box, str::FromStr};

use common::{btc_perp, fixtures};
use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_hyperliquid::websocket::{
    dispatch::{OrderIdentity, WsDispatchState},
    messages::{HyperliquidWsMessage, WsBookData, WsTradeData},
    parse::{parse_ws_order_book_deltas, parse_ws_trade_tick},
};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::{OrderAccepted, OrderFilled},
    identifiers::{ClientOrderId, StrategyId, TradeId, VenueOrderId},
    instruments::Instrument,
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;

// ----- decode-only (whichever JSON tokenizer is active via features) --------

fn bench_decode_trade(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_only");
    group.bench_function("trade", |b| {
        b.iter(|| {
            let msg: HyperliquidWsMessage =
                serde_json::from_str(black_box(fixtures::TRADE)).unwrap();
            black_box(msg);
        });
    });
    group.finish();
}

fn bench_decode_book(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_only");
    group.bench_function("book", |b| {
        b.iter(|| {
            let msg: HyperliquidWsMessage =
                serde_json::from_str(black_box(fixtures::BOOK_L2)).unwrap();
            black_box(msg);
        });
    });
    group.finish();
}

// ----- parse-only (skip JSON; start from pre-decoded typed input) -----------

fn bench_parse_trade(c: &mut Criterion) {
    let instrument = btc_perp();
    let msg: HyperliquidWsMessage = serde_json::from_str(fixtures::TRADE).unwrap();
    let trade: WsTradeData = match msg {
        HyperliquidWsMessage::Trades { data } => data.into_iter().next().unwrap(),
        _ => unreachable!(),
    };

    let mut group = c.benchmark_group("parse_only");
    group.bench_function("trade", |b| {
        b.iter(|| {
            let tick =
                parse_ws_trade_tick(black_box(&trade), &instrument, UnixNanos::default()).unwrap();
            black_box(tick);
        });
    });
    group.finish();
}

fn bench_parse_book_deltas(c: &mut Criterion) {
    let instrument = btc_perp();
    let msg: HyperliquidWsMessage = serde_json::from_str(fixtures::BOOK_L2).unwrap();
    let book: WsBookData = match msg {
        HyperliquidWsMessage::L2Book { data } => data,
        _ => unreachable!(),
    };

    let mut group = c.benchmark_group("parse_only");
    group.bench_function("book_deltas", |b| {
        b.iter(|| {
            let deltas =
                parse_ws_order_book_deltas(black_box(&book), &instrument, UnixNanos::default())
                    .unwrap();
            black_box(deltas);
        });
    });
    group.finish();
}

// ----- atomic costs: Decimal / Price / Quantity construction ---------------

fn bench_decimal_from_str(c: &mut Criterion) {
    let s = "98450.5";
    c.bench_function("atom/decimal_from_str", |b| {
        b.iter(|| {
            let d = Decimal::from_str(black_box(s)).unwrap();
            black_box(d);
        });
    });
}

fn bench_price_from_decimal_dp(c: &mut Criterion) {
    let d = Decimal::from_str("98450.5").unwrap();
    c.bench_function("atom/price_from_decimal_dp", |b| {
        b.iter(|| {
            let p = Price::from_decimal_dp(black_box(d), 2).unwrap();
            black_box(p);
        });
    });
}

fn bench_price_from_str_combined(c: &mut Criterion) {
    let s = "98450.5";
    c.bench_function("atom/price_combined", |b| {
        b.iter(|| {
            let d = Decimal::from_str(black_box(s)).unwrap();
            let p = Price::from_decimal_dp(d, 2).unwrap();
            black_box(p);
        });
    });
}

fn bench_trade_id_new(c: &mut Criterion) {
    let tid: u64 = 987_654_321;
    c.bench_function("atom/trade_id_new", |b| {
        b.iter(|| {
            let id = TradeId::new(black_box(tid).to_string());
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

// ----- dispatch breakdown ----------------------------------------------------

fn ident() -> OrderIdentity {
    OrderIdentity {
        strategy_id: StrategyId::from("S-BENCH"),
        instrument_id: btc_perp().id(),
        order_side: OrderSide::Buy,
        order_type: OrderType::Limit,
        quantity: Quantity::from("0.001"),
        price: Some(Price::from("92572.0")),
    }
}

fn bench_state_construct(c: &mut Criterion) {
    let cid = ClientOrderId::from("O-BENCH-S");
    let voi = VenueOrderId::from("430481837807");
    c.bench_function("atom/state_construct_primed", |b| {
        b.iter(|| {
            let state = WsDispatchState::new();
            state.register_identity(cid, ident());
            state.record_venue_order_id(cid, voi);
            state.insert_accepted(cid);
            black_box(state);
        });
    });
}

fn bench_state_drop(c: &mut Criterion) {
    // Construct outside the iter loop so we time only drop
    let cid = ClientOrderId::from("O-BENCH-S");
    let voi = VenueOrderId::from("430481837807");
    c.bench_function("atom/state_drop_primed", |b| {
        b.iter_with_setup(
            || {
                let state = WsDispatchState::new();
                state.register_identity(cid, ident());
                state.record_venue_order_id(cid, voi);
                state.insert_accepted(cid);
                state
            },
            |state| {
                drop(black_box(state));
            },
        );
    });
}

fn bench_event_filled_construct(c: &mut Criterion) {
    use nautilus_model::identifiers::{AccountId, TraderId};
    let trader_id = TraderId::from("BENCH-001");
    let strategy = StrategyId::from("S-BENCH");
    let cid = ClientOrderId::from("O-BENCH-F");
    let voi = VenueOrderId::from("430481837807");
    let acct = AccountId::from("HYPERLIQUID-001");
    let trade_id = TradeId::new("TRADE-1");
    let qty = Quantity::from("0.001");
    let px = Price::from("92572.0");
    let commission = Money::from("0.05 USDC");

    c.bench_function("atom/event_filled_construct", |b| {
        b.iter(|| {
            let filled = OrderFilled::new(
                trader_id,
                strategy,
                btc_perp().id(),
                cid,
                voi,
                acct,
                trade_id,
                OrderSide::Buy,
                OrderType::Limit,
                qty,
                px,
                commission.currency,
                LiquiditySide::Taker,
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
                false,
                None,
                Some(commission),
            );
            black_box(filled);
        });
    });
}

// Measures dispatch with state reused across iterations: state lives forever
// in production, so per-iter construct + drop in the canonical dispatch bench
// is a measurement artifact, not a real cost. The second+ calls hit the
// `filled_orders` dedup early return.
fn bench_dispatch_reused_fill(c: &mut Criterion) {
    use nautilus_hyperliquid::websocket::dispatch::dispatch_order_fill;
    use nautilus_model::reports::FillReport;

    let (emitter, mut _rx) = common::bench_emitter();
    let cid = ClientOrderId::from("O-BENCH-R");
    let voi = VenueOrderId::from("430481837807");
    let state = WsDispatchState::new();
    state.register_identity(cid, ident());
    state.record_venue_order_id(cid, voi);
    state.insert_accepted(cid);

    let report = FillReport::new(
        common::account_id(),
        btc_perp().id(),
        voi,
        TradeId::new("TRADE-1"),
        OrderSide::Buy,
        Quantity::from("0.001"),
        Price::from("92572.0"),
        Money::from("0.05 USDC"),
        LiquiditySide::Taker,
        Some(cid),
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
        Some(UUID4::new()),
    );

    c.bench_function("atom/dispatch_fill_reused", |b| {
        b.iter(|| {
            let outcome =
                dispatch_order_fill(black_box(&report), &state, &emitter, UnixNanos::default());
            black_box(outcome);
        });
    });
    // keep rx alive
    while _rx.try_recv().is_ok() {}
}

fn bench_event_accepted_construct(c: &mut Criterion) {
    use nautilus_model::identifiers::{AccountId, TraderId};
    let trader_id = TraderId::from("BENCH-001");
    let strategy = StrategyId::from("S-BENCH");
    let cid = ClientOrderId::from("O-BENCH-A");
    let voi = VenueOrderId::from("430481837807");
    let acct = AccountId::from("HYPERLIQUID-001");

    c.bench_function("atom/event_accepted_construct", |b| {
        b.iter(|| {
            let accepted = OrderAccepted::new(
                trader_id,
                strategy,
                btc_perp().id(),
                cid,
                voi,
                acct,
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
                false,
            );
            black_box(accepted);
        });
    });
}

criterion_group!(
    benches,
    bench_decode_trade,
    bench_decode_book,
    bench_parse_trade,
    bench_parse_book_deltas,
    bench_decimal_from_str,
    bench_price_from_decimal_dp,
    bench_price_from_str_combined,
    bench_trade_id_new,
    bench_uuid4_new,
    bench_state_construct,
    bench_state_drop,
    bench_event_filled_construct,
    bench_dispatch_reused_fill,
    bench_event_accepted_construct,
);
criterion_main!(benches);
