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
//! Decimal/UUID/event construction, signing component costs).
//!
//! Use these when an `inbound_pipeline` or `exec_pipeline` bench regresses
//! and you need to localise where the time went, or when evaluating a
//! structural change (e.g. swapping the JSON tokenizer) and want to confirm
//! the gain landed in the layer it was supposed to.
//!
//! Same canonical surface every adapter should ship; pair this with
//! `data.rs`, `exec.rs`, and `signing_sign_verify.rs`.

mod common;

use std::{hint::black_box, str::FromStr};

use common::{
    CHAIN_ID, cancel_order_tx, create_order_tx, eth_perp, fixed_hashed_msg, fixed_k, fixed_sk,
    fixtures,
};
use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_lighter::{
    signing::{
        curve::Point,
        hash::{hash_to_quintic_extension, hash_two_to_quintic},
        tx::{TxInfoJson, compute_tx_hash, sign_tx},
    },
    websocket::{
        messages::LighterWsFrame,
        parse::{parse_ws_order_book_deltas, parse_ws_trade_tick},
    },
};
use nautilus_model::{
    identifiers::TradeId,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

// ----- decode-only (whichever JSON tokenizer is active via features) -------

fn bench_decode_trade(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_only");
    group.bench_function("trade", |b| {
        b.iter(|| {
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::TRADE_UPDATE)).unwrap();
            black_box(frame);
        });
    });
    group.finish();
}

fn bench_decode_book(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode_only");
    group.bench_function("book", |b| {
        b.iter(|| {
            let frame: LighterWsFrame =
                serde_json::from_str(black_box(fixtures::BOOK_UPDATE)).unwrap();
            black_box(frame);
        });
    });
    group.finish();
}

// ----- parse-only (skip JSON; start from pre-decoded typed input) ----------

fn bench_parse_trade(c: &mut Criterion) {
    let instrument = eth_perp();
    let frame: LighterWsFrame = serde_json::from_str(fixtures::TRADE_UPDATE).unwrap();
    let trade = match frame {
        LighterWsFrame::Trade { trades, .. } => trades.into_iter().next().unwrap(),
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
    let instrument = eth_perp();
    let frame: LighterWsFrame = serde_json::from_str(fixtures::BOOK_UPDATE).unwrap();
    let (book, timestamp) = match frame {
        LighterWsFrame::OrderBook {
            order_book,
            timestamp,
            ..
        } => (order_book, timestamp),
        _ => unreachable!(),
    };

    let mut group = c.benchmark_group("parse_only");
    group.bench_function("book_deltas", |b| {
        b.iter(|| {
            let deltas = parse_ws_order_book_deltas(
                black_box(&book),
                &instrument,
                timestamp,
                false,
                UnixNanos::default(),
            )
            .unwrap();
            black_box(deltas);
        });
    });
    group.finish();
}

// ----- atomic costs: Decimal / Price / Quantity construction ---------------

fn bench_decimal_from_str(c: &mut Criterion) {
    let s = "2064.54";
    c.bench_function("atom/decimal_from_str", |b| {
        b.iter(|| {
            let d = Decimal::from_str(black_box(s)).unwrap();
            black_box(d);
        });
    });
}

fn bench_price_from_decimal_dp(c: &mut Criterion) {
    let d = Decimal::from_str("2064.54").unwrap();
    c.bench_function("atom/price_from_decimal_dp", |b| {
        b.iter(|| {
            let p = Price::from_decimal_dp(black_box(d), 2).unwrap();
            black_box(p);
        });
    });
}

fn bench_price_from_str_combined(c: &mut Criterion) {
    let s = "2064.54";
    c.bench_function("atom/price_combined", |b| {
        b.iter(|| {
            let d = Decimal::from_str(black_box(s)).unwrap();
            let p = Price::from_decimal_dp(d, 2).unwrap();
            black_box(p);
        });
    });
}

fn bench_quantity_from_decimal_dp(c: &mut Criterion) {
    let d = Decimal::from_str("0.1336").unwrap();
    c.bench_function("atom/quantity_from_decimal_dp", |b| {
        b.iter(|| {
            let q = Quantity::from_decimal_dp(black_box(d), 4).unwrap();
            black_box(q);
        });
    });
}

fn bench_trade_id_new(c: &mut Criterion) {
    let tid: u64 = 16_164_557_907;
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

// ----- signing component micros --------------------------------------------

// Pure hash cost: drives the Poseidon2 permutation over the preimage size
// each tx kind feeds in. Use to localise regressions in `exec_pipeline`
// numbers between the encode + hash stage and the signing stage.
fn bench_compute_tx_hash_create_order(c: &mut Criterion) {
    let tx = create_order_tx();
    let mut group = c.benchmark_group("signing");
    group.bench_function("compute_tx_hash_create_order", |b| {
        b.iter(|| compute_tx_hash(black_box(&tx), black_box(CHAIN_ID)));
    });
    group.finish();
}

fn bench_compute_tx_hash_cancel_order(c: &mut Criterion) {
    let tx = cancel_order_tx();
    let mut group = c.benchmark_group("signing");
    group.bench_function("compute_tx_hash_cancel_order", |b| {
        b.iter(|| compute_tx_hash(black_box(&tx), black_box(CHAIN_ID)));
    });
    group.finish();
}

fn bench_hash_to_quintic_extension(c: &mut Criterion) {
    use nautilus_lighter::signing::field::Fp;
    let elems: Vec<Fp> = (0..16).map(|i| Fp::from_u64_reduce(i as u64 + 1)).collect();
    let mut group = c.benchmark_group("signing");
    group.bench_function("hash_to_quintic_extension_16", |b| {
        b.iter(|| hash_to_quintic_extension(black_box(elems.as_slice())));
    });
    group.finish();
}

fn bench_hash_two_to_quintic(c: &mut Criterion) {
    let a = fixed_hashed_msg();
    let b_val = fixed_hashed_msg();
    let mut group = c.benchmark_group("signing");
    group.bench_function("hash_two_to_quintic", |bencher| {
        bencher.iter(|| hash_two_to_quintic(black_box(a), black_box(b_val)));
    });
    group.finish();
}

// Scalar mul on the generator is the dominant Schnorr-sign cost. Splits
// the `sign_tx` bench so a regression in the field/curve stack can be
// distinguished from a regression in the hash stack.
fn bench_mulgen_ct(c: &mut Criterion) {
    let k = fixed_k();
    let mut group = c.benchmark_group("signing");
    group.bench_function("mulgen_ct", |b| {
        b.iter(|| Point::mulgen_ct(black_box(k)));
    });
    group.finish();
}

// Only the `sign_tx` portion (hash + Schnorr sign) — pairs with the
// `exec_pipeline/submit_limit` bench which adds the wire JSON render on top.
fn bench_sign_tx_create_order(c: &mut Criterion) {
    let sk = fixed_sk();
    let k = fixed_k();
    let tx = create_order_tx();
    let mut group = c.benchmark_group("signing");
    group.bench_function("sign_tx_create_order", |b| {
        b.iter(|| sign_tx(black_box(&tx), CHAIN_ID, &sk, k));
    });
    group.finish();
}

// JSON render-only cost: confirms whether the `exec_pipeline` numbers are
// dominated by hash+sign or by the wire payload assembly.
fn bench_render_create_order_json(c: &mut Criterion) {
    let sk = fixed_sk();
    let k = fixed_k();
    let tx = create_order_tx();
    let signed = sign_tx(&tx, CHAIN_ID, &sk, k);
    let mut group = c.benchmark_group("signing");
    group.bench_function("render_create_order_json", |b| {
        b.iter(|| TxInfoJson::create_order(black_box(&tx), black_box(&signed)));
    });
    group.finish();
}

// Tx-info wire-wrap cost: the dispatch path used to `serde_json::from_str`
// the rendered tx into a `Value` before handing it to `send_tx`. The
// post-refactor path wraps in `Box<RawValue>` instead, validating the
// bytes without building an AST. This pair shows the saving directly.
fn bench_wire_wrap_create_order_value(c: &mut Criterion) {
    let sk = fixed_sk();
    let k = fixed_k();
    let tx = create_order_tx();
    let signed = sign_tx(&tx, CHAIN_ID, &sk, k);
    let json = TxInfoJson::create_order(&tx, &signed);
    let mut group = c.benchmark_group("signing");
    group.bench_function("wire_wrap_value", |b| {
        b.iter(|| {
            let v: serde_json::Value = serde_json::from_str(black_box(&json)).unwrap();
            black_box(v);
        });
    });
    group.finish();
}

fn bench_wire_wrap_create_order_raw(c: &mut Criterion) {
    let sk = fixed_sk();
    let k = fixed_k();
    let tx = create_order_tx();
    let signed = sign_tx(&tx, CHAIN_ID, &sk, k);
    let mut group = c.benchmark_group("signing");
    group.bench_function("wire_wrap_raw", |b| {
        b.iter(|| {
            // RawValue takes the source by-value, so each iteration needs a
            // fresh String. `to_string()` matches what `TxInfoJson` returns.
            let json = TxInfoJson::create_order(&tx, &signed);
            let raw = serde_json::value::RawValue::from_string(json).unwrap();
            black_box(raw);
        });
    });
    group.finish();
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
    bench_quantity_from_decimal_dp,
    bench_trade_id_new,
    bench_uuid4_new,
    bench_compute_tx_hash_create_order,
    bench_compute_tx_hash_cancel_order,
    bench_hash_to_quintic_extension,
    bench_hash_two_to_quintic,
    bench_mulgen_ct,
    bench_sign_tx_create_order,
    bench_render_create_order_json,
    bench_wire_wrap_create_order_value,
    bench_wire_wrap_create_order_raw,
);
criterion_main!(benches);
