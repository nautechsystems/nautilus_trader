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

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_model::data::black_scholes::{compute_greeks, compute_iv_and_greeks};

fn bench_compute_iv_and_greeks_f32(c: &mut Criterion) {
    let mkt_price = 10.45058f32;
    let s = 100.0f32;
    let k = 100.0f32;
    let t = 1.0f32;
    let r = 0.05f32;
    let b = 0.05f32;
    let is_call = true;
    let initial_guess = 0.2f32;

    c.bench_function("compute_iv_and_greeks_f32", |b_bench| {
        b_bench.iter(|| {
            black_box(compute_iv_and_greeks::<f32>(
                black_box(mkt_price),
                black_box(s),
                black_box(k),
                black_box(t),
                black_box(r),
                black_box(b),
                black_box(is_call),
                black_box(initial_guess),
            ))
        });
    });
}

fn bench_compute_greeks_f32(c: &mut Criterion) {
    let s = 100.0f32;
    let k = 100.0f32;
    let t = 1.0f32;
    let r = 0.05f32;
    let b = 0.05f32;
    let vol = 0.2f32;
    let is_call = true;

    c.bench_function("compute_greeks_f32", |b_bench| {
        b_bench.iter(|| {
            black_box(compute_greeks::<f32>(
                black_box(s),
                black_box(k),
                black_box(t),
                black_box(r),
                black_box(b),
                black_box(vol),
                black_box(is_call),
            ))
        });
    });
}

criterion_group!(
    benches,
    bench_compute_iv_and_greeks_f32,
    bench_compute_greeks_f32,
);
criterion_main!(benches);
