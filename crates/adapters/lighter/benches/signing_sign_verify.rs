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

//! Published signing baseline.
//!
//! Measures the user-visible critical path: `PrivateKey::sign`,
//! `PublicKey::verify`, `compute_tx_hash` for the two trading-hot tx kinds,
//! `sign_tx` (hash + sign), public-key derivation, and `build_auth_token_at`.
//!
//! Quote these IDs in `BENCHMARKS.md`. `micros.rs` repeats a few of the same
//! calls next to decode and JSON render so a pipeline regression can be
//! localised; do not treat those duplicates as a second baseline.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
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
    let mut group = c.benchmark_group("signing");
    group.throughput(Throughput::Elements(1));
    group.bench_function("PrivateKey::sign", |b| {
        b.iter(|| black_box(sk.sign(black_box(msg), black_box(k))));
    });
    group.finish();
}

fn bench_verify(c: &mut Criterion) {
    let pk = fixed_pk();
    let msg = fixed_hashed_msg();
    let sig = fixed_signature();
    let mut group = c.benchmark_group("signing");
    group.throughput(Throughput::Elements(1));
    group.bench_function("PublicKey::verify", |b| {
        b.iter(|| black_box(pk.verify(black_box(msg), black_box(&sig))));
    });
    group.finish();
}

fn bench_public_key(c: &mut Criterion) {
    let sk = fixed_sk();
    let mut group = c.benchmark_group("signing");
    group.throughput(Throughput::Elements(1));
    group.bench_function("PrivateKey::public_key", |b| {
        b.iter(|| black_box(black_box(&sk).public_key()));
    });
    group.finish();
}

fn bench_compute_tx_hash_create_order(c: &mut Criterion) {
    let tx = create_order_tx();
    let mut group = c.benchmark_group("signing");
    group.throughput(Throughput::Elements(1));
    group.bench_function("compute_tx_hash (CreateOrder)", |b| {
        b.iter(|| black_box(compute_tx_hash(black_box(&tx), black_box(CHAIN_ID))));
    });
    group.finish();
}

fn bench_compute_tx_hash_cancel_order(c: &mut Criterion) {
    let tx = cancel_order_tx();
    let mut group = c.benchmark_group("signing");
    group.throughput(Throughput::Elements(1));
    group.bench_function("compute_tx_hash (CancelOrder)", |b| {
        b.iter(|| black_box(compute_tx_hash(black_box(&tx), black_box(CHAIN_ID))));
    });
    group.finish();
}

fn bench_sign_tx_create_order(c: &mut Criterion) {
    let tx = create_order_tx();
    let sk = fixed_sk();
    let k = fixed_k();
    let mut group = c.benchmark_group("signing");
    group.throughput(Throughput::Elements(1));
    group.bench_function("sign_tx (CreateOrder)", |b| {
        b.iter(|| {
            black_box(sign_tx(
                black_box(&tx),
                black_box(CHAIN_ID),
                black_box(&sk),
                black_box(k),
            ))
        });
    });
    group.finish();
}

fn bench_sign_tx_cancel_order(c: &mut Criterion) {
    let tx = cancel_order_tx();
    let sk = fixed_sk();
    let k = fixed_k();
    let mut group = c.benchmark_group("signing");
    group.throughput(Throughput::Elements(1));
    group.bench_function("sign_tx (CancelOrder)", |b| {
        b.iter(|| {
            black_box(sign_tx(
                black_box(&tx),
                black_box(CHAIN_ID),
                black_box(&sk),
                black_box(k),
            ))
        });
    });
    group.finish();
}

fn bench_build_auth_token(c: &mut Criterion) {
    let sk = fixed_sk();
    let k = fixed_k();
    let now = 1_700_000_000;
    let deadline = now + 600;
    let mut group = c.benchmark_group("signing");
    group.throughput(Throughput::Elements(1));
    group.bench_function("build_auth_token_at", |b| {
        b.iter(|| {
            black_box(
                build_auth_token_at(
                    black_box(now),
                    black_box(deadline),
                    black_box(12345),
                    black_box(5),
                    black_box(&sk),
                    black_box(k),
                )
                .expect("auth token must build"),
            )
        });
    });
    group.finish();
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
