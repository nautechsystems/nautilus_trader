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

//! End-to-end signing benches.
//!
//! Measures the user-visible critical path: `PrivateKey::sign`,
//! `PublicKey::verify`, `compute_tx_hash` for the two trading-hot tx kinds,
//! `sign_tx` (hash + sign), public-key derivation, and `build_auth_token_at`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_lighter::signing::{
    auth_token::build_auth_token_at,
    tx::{compute_tx_hash, sign_tx},
};

mod common;
use common::{
    CHAIN_ID, cancel_order_tx, create_order_tx, fixed_hashed_msg, fixed_k, fixed_pk,
    fixed_signature, fixed_sk,
};

fn bench_sign(c: &mut Criterion) {
    let sk = fixed_sk();
    let msg = fixed_hashed_msg();
    let k = fixed_k();
    c.bench_function("PrivateKey::sign", |b| {
        b.iter(|| sk.sign(black_box(msg), black_box(k)));
    });
}

fn bench_verify(c: &mut Criterion) {
    let pk = fixed_pk();
    let msg = fixed_hashed_msg();
    let sig = fixed_signature();
    c.bench_function("PublicKey::verify", |b| {
        b.iter(|| pk.verify(black_box(msg), black_box(&sig)));
    });
}

fn bench_public_key(c: &mut Criterion) {
    let sk = fixed_sk();
    c.bench_function("PrivateKey::public_key", |b| {
        b.iter(|| black_box(&sk).public_key());
    });
}

fn bench_compute_tx_hash_create_order(c: &mut Criterion) {
    let tx = create_order_tx();
    c.bench_function("compute_tx_hash (CreateOrder)", |b| {
        b.iter(|| compute_tx_hash(black_box(&tx), black_box(CHAIN_ID)));
    });
}

fn bench_compute_tx_hash_cancel_order(c: &mut Criterion) {
    let tx = cancel_order_tx();
    c.bench_function("compute_tx_hash (CancelOrder)", |b| {
        b.iter(|| compute_tx_hash(black_box(&tx), black_box(CHAIN_ID)));
    });
}

fn bench_sign_tx_create_order(c: &mut Criterion) {
    let tx = create_order_tx();
    let sk = fixed_sk();
    let k = fixed_k();
    c.bench_function("sign_tx (CreateOrder)", |b| {
        b.iter(|| {
            sign_tx(
                black_box(&tx),
                black_box(CHAIN_ID),
                black_box(&sk),
                black_box(k),
            )
        });
    });
}

fn bench_sign_tx_cancel_order(c: &mut Criterion) {
    let tx = cancel_order_tx();
    let sk = fixed_sk();
    let k = fixed_k();
    c.bench_function("sign_tx (CancelOrder)", |b| {
        b.iter(|| {
            sign_tx(
                black_box(&tx),
                black_box(CHAIN_ID),
                black_box(&sk),
                black_box(k),
            )
        });
    });
}

fn bench_build_auth_token(c: &mut Criterion) {
    let sk = fixed_sk();
    let k = fixed_k();
    let now = 1_700_000_000;
    let deadline = now + 600;
    c.bench_function("build_auth_token_at", |b| {
        b.iter(|| {
            build_auth_token_at(
                black_box(now),
                black_box(deadline),
                black_box(12345),
                black_box(5),
                black_box(&sk),
                black_box(k),
            )
            .expect("auth token must build")
        });
    });
}

criterion_group!(
    benches,
    bench_sign,
    bench_verify,
    bench_public_key,
    bench_compute_tx_hash_create_order,
    bench_compute_tx_hash_cancel_order,
    bench_sign_tx_create_order,
    bench_sign_tx_cancel_order,
    bench_build_auth_token,
);
criterion_main!(benches);
