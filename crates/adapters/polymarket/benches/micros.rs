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
//! Decimal/Price/Quantity/TradeId/UUID construction).
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

use common::{
    YES_TOKEN_ID, account_id, fixtures, instrument_precisions, yes_instrument, yes_instrument_id,
};
use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType},
    events::{OrderAccepted, OrderFilled},
    identifiers::{ClientOrderId, StrategyId, TradeId, VenueOrderId},
    instruments::Instrument,
    types::{Currency, Money, Price, Quantity},
};
use nautilus_polymarket::{
    common::{enums::PolymarketOrderSide, parse::determine_trade_id},
    websocket::{
        messages::{MarketWsMessage, PolymarketBookSnapshot, PolymarketTrade},
        parse::{parse_book_snapshot, parse_trade_tick},
    },
};
use rust_decimal::Decimal;

fn bench_decode_trade(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_only");
    group.bench_function("trade", |b| {
        b.iter(|| {
            let msg: MarketWsMessage =
                serde_json::from_str(black_box(fixtures::MARKET_LAST_TRADE)).unwrap();
            black_box(msg);
        });
    });
    group.finish();
}

fn bench_decode_book(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_only");
    group.bench_function("book", |b| {
        b.iter(|| {
            let msg: MarketWsMessage =
                serde_json::from_str(black_box(fixtures::MARKET_BOOK)).unwrap();
            black_box(msg);
        });
    });
    group.finish();
}

fn bench_parse_trade(c: &mut Criterion) {
    let (px_prec, sz_prec) = instrument_precisions();
    let id = yes_instrument_id();
    let msg: MarketWsMessage = serde_json::from_str(fixtures::MARKET_LAST_TRADE).unwrap();
    let trade: PolymarketTrade = match msg {
        MarketWsMessage::LastTradePrice(t) => t,
        _ => unreachable!(),
    };

    let mut group = c.benchmark_group("parse_only");
    group.bench_function("trade", |b| {
        b.iter(|| {
            let tick = parse_trade_tick(
                black_box(&trade),
                id,
                px_prec,
                sz_prec,
                UnixNanos::default(),
            )
            .unwrap();
            black_box(tick);
        });
    });
    group.finish();
}

fn bench_parse_book_snapshot(c: &mut Criterion) {
    let (px_prec, sz_prec) = instrument_precisions();
    let id = yes_instrument_id();
    let msg: MarketWsMessage = serde_json::from_str(fixtures::MARKET_BOOK).unwrap();
    let snap: PolymarketBookSnapshot = match msg {
        MarketWsMessage::Book(s) => s,
        _ => unreachable!(),
    };

    let mut group = c.benchmark_group("parse_only");
    group.bench_function("book_snapshot", |b| {
        b.iter(|| {
            let deltas =
                parse_book_snapshot(black_box(&snap), id, px_prec, sz_prec, UnixNanos::default())
                    .unwrap();
            black_box(deltas);
        });
    });
    group.finish();
}

fn bench_decimal_from_str(c: &mut Criterion) {
    let s = "0.5000";
    c.bench_function("atom/decimal_from_str", |b| {
        b.iter(|| {
            let d = Decimal::from_str(black_box(s)).unwrap();
            black_box(d);
        });
    });
}

// Precision 2 matches the bench instrument (tick 0.01); paired with a
// scale-4 input like "0.5000" so the conversion exercises the rounding
// branch the WS book parser hits in production.
fn bench_price_from_decimal_dp(c: &mut Criterion) {
    let d = Decimal::from_str("0.5000").unwrap();
    c.bench_function("atom/price_from_decimal_dp", |b| {
        b.iter(|| {
            let p = Price::from_decimal_dp(black_box(d), 2).unwrap();
            black_box(p);
        });
    });
}

fn bench_quantity_from_decimal_dp(c: &mut Criterion) {
    let d = Decimal::from_str("100.0").unwrap();
    c.bench_function("atom/quantity_from_decimal_dp", |b| {
        b.iter(|| {
            let q = Quantity::from_decimal_dp(black_box(d), 6).unwrap();
            black_box(q);
        });
    });
}

fn bench_price_combined(c: &mut Criterion) {
    // Full string -> Price path used by `parse_price` in production.
    let s = "0.5000";
    c.bench_function("atom/price_combined", |b| {
        b.iter(|| {
            let d = Decimal::from_str(black_box(s)).unwrap();
            let p = Price::from_decimal_dp(d, 2).unwrap();
            black_box(p);
        });
    });
}

fn bench_trade_id_determine(c: &mut Criterion) {
    let asset_id = YES_TOKEN_ID;
    let side = PolymarketOrderSide::Buy;
    let price = "0.51";
    let size = "25.0";
    let ts = "1703875202000";
    c.bench_function("atom/trade_id_determine", |b| {
        b.iter(|| {
            let id = determine_trade_id(
                black_box(asset_id),
                black_box(side),
                black_box(price),
                black_box(size),
                black_box(ts),
            );
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

fn bench_event_filled_construct(c: &mut Criterion) {
    use nautilus_model::identifiers::TraderId;
    let trader_id = TraderId::from("BENCH-001");
    let strategy = StrategyId::from("S-BENCH");
    let cid = ClientOrderId::from("O-BENCH-F");
    let voi =
        VenueOrderId::from("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12");
    let acct = account_id();
    let trade_id = TradeId::new("TRADE-1");
    let qty = Quantity::from("25");
    let px = Price::from("0.50");
    let commission = Money::new(0.0, Currency::pUSD());
    let instrument_id = yes_instrument().id();

    c.bench_function("atom/event_filled_construct", |b| {
        b.iter(|| {
            let filled = OrderFilled::new(
                trader_id,
                strategy,
                instrument_id,
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

fn bench_event_accepted_construct(c: &mut Criterion) {
    use nautilus_model::identifiers::TraderId;
    let trader_id = TraderId::from("BENCH-001");
    let strategy = StrategyId::from("S-BENCH");
    let cid = ClientOrderId::from("O-BENCH-A");
    let voi =
        VenueOrderId::from("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12");
    let acct = account_id();
    let instrument_id = yes_instrument().id();

    c.bench_function("atom/event_accepted_construct", |b| {
        b.iter(|| {
            let accepted = OrderAccepted::new(
                trader_id,
                strategy,
                instrument_id,
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
    bench_parse_book_snapshot,
    bench_decimal_from_str,
    bench_price_from_decimal_dp,
    bench_quantity_from_decimal_dp,
    bench_price_combined,
    bench_trade_id_determine,
    bench_uuid4_new,
    bench_event_filled_construct,
    bench_event_accepted_construct,
);
criterion_main!(benches);
