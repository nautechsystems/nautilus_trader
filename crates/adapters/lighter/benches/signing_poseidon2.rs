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

//! Poseidon2 permutation and sponge benches.
//!
//! Sweeps `hash_no_pad` over input lengths `[1, RATE, 2*RATE, 3*RATE]` so a
//! regression in either the absorption loop or the permutation surfaces in a
//! distinct curve. `permute` is benched in isolation.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_lighter::signing::{
    field::Fp,
    hash::{RATE, WIDTH, hash_no_pad, hash_to_quintic_extension, hash_two_to_quintic, permute},
};

mod common;
use common::fixed_hashed_msg;

fn make_state() -> [Fp; WIDTH] {
    let mut state = [Fp::ZERO; WIDTH];
    for (i, slot) in state.iter_mut().enumerate() {
        *slot = Fp::from_u64_reduce(0x9E37_79B9_7F4A_7C15u64.wrapping_mul(i as u64 + 1));
    }
    state
}

fn make_inputs(len: usize) -> Vec<Fp> {
    (0..len)
        .map(|i| Fp::from_u64_reduce(0xBB67_AE85_84CA_A73Bu64.wrapping_mul(i as u64 + 1)))
        .collect()
}

fn bench_permute(c: &mut Criterion) {
    let state = make_state();
    c.bench_function("Poseidon2::permute", |b| {
        b.iter_batched(
            || state,
            |mut s| {
                permute(black_box(&mut s));
                s
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_hash_no_pad(c: &mut Criterion) {
    let mut group = c.benchmark_group("Poseidon2::hash_no_pad");

    for &len in &[1usize, RATE, 2 * RATE, 3 * RATE] {
        let input = make_inputs(len);
        group.bench_function(format!("len={len}"), |b| {
            b.iter(|| hash_no_pad(black_box(&input)));
        });
    }
    group.finish();
}

fn bench_hash_to_quintic_extension(c: &mut Criterion) {
    let mut group = c.benchmark_group("Poseidon2::hash_to_quintic_extension");

    for &len in &[5usize, 10, 16] {
        let input = make_inputs(len);
        group.bench_function(format!("len={len}"), |b| {
            b.iter(|| hash_to_quintic_extension(black_box(&input)));
        });
    }
    group.finish();
}

fn bench_hash_two_to_quintic(c: &mut Criterion) {
    let a = fixed_hashed_msg();
    let b_ = fixed_hashed_msg();
    c.bench_function("Poseidon2::hash_two_to_quintic", |b| {
        b.iter(|| hash_two_to_quintic(black_box(a), black_box(b_)));
    });
}

criterion_group!(
    benches,
    bench_permute,
    bench_hash_no_pad,
    bench_hash_to_quintic_extension,
    bench_hash_two_to_quintic,
);
criterion_main!(benches);
