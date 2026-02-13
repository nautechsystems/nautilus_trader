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

use criterion::{BenchmarkId, Criterion, criterion_group};
use nautilus_model::data::greeks::{
    black_scholes_greeks, imply_vol_and_greeks, refine_vol_and_greeks,
};

fn bench_black_scholes_greeks_moneyness(c: &mut Criterion) {
    let r = 0.05;
    let cost_of_carry = 0.05;
    let vol = 0.2;
    let k = 100.0;
    let t = 1.0;
    let multiplier = 1.0;

    // Test different moneyness levels and call/put - all should have similar timings
    // since the same code path is executed regardless of moneyness or option type
    let mut group = c.benchmark_group("black_scholes_greeks");
    for (spot, moneyness_label) in [(90.0, "otm"), (100.0, "atm"), (110.0, "itm")] {
        for (is_call, option_type) in [(true, "call"), (false, "put")] {
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{moneyness_label}_{option_type}")),
                &(spot, is_call),
                |b, &(s, is_call)| {
                    b.iter(|| {
                        black_box(black_scholes_greeks(
                            black_box(s),
                            black_box(r),
                            black_box(cost_of_carry),
                            black_box(vol),
                            black_box(is_call),
                            black_box(k),
                            black_box(t),
                            black_box(multiplier),
                        ))
                    });
                },
            );
        }
    }
    group.finish();
}

fn bench_imply_vol_and_greeks_moneyness(c: &mut Criterion) {
    let r = 0.05;
    let cost_of_carry = 0.05;
    let vol = 0.2;
    let k = 100.0;
    let t = 1.0;
    let multiplier = 1.0;

    let mut group = c.benchmark_group("imply_vol_and_greeks");
    for (spot, moneyness_label) in [(90.0, "otm"), (100.0, "atm"), (110.0, "itm")] {
        for (is_call, option_type) in [(true, "call"), (false, "put")] {
            // Calculate theoretical price for this scenario
            let theoretical_price =
                black_scholes_greeks(spot, r, cost_of_carry, vol, is_call, k, t, multiplier).price;
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{moneyness_label}_{option_type}")),
                &(spot, is_call, theoretical_price),
                |b, &(s, is_call, price)| {
                    b.iter(|| {
                        black_box(imply_vol_and_greeks(
                            black_box(s),
                            black_box(r),
                            black_box(cost_of_carry),
                            black_box(is_call),
                            black_box(k),
                            black_box(t),
                            black_box(price),
                            black_box(multiplier),
                        ))
                    });
                },
            );
        }
    }
    group.finish();
}

fn bench_refine_vol_and_greeks_moneyness(c: &mut Criterion) {
    let r = 0.05;
    let cost_of_carry = 0.05;
    let initial_vol = 0.2;
    let k = 100.0;
    let t = 1.0;
    let multiplier = 1.0;

    let mut group = c.benchmark_group("refine_vol_and_greeks");
    for (spot, moneyness_label) in [(90.0, "otm"), (100.0, "atm"), (110.0, "itm")] {
        for (is_call, option_type) in [(true, "call"), (false, "put")] {
            // Calculate target price for this scenario
            let target_price = black_scholes_greeks(
                spot,
                r,
                cost_of_carry,
                initial_vol,
                is_call,
                k,
                t,
                multiplier,
            )
            .price;
            // Use a slightly different initial guess (10% off)
            let initial_guess = if is_call {
                initial_vol * 1.1
            } else {
                initial_vol * 0.9
            };
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{moneyness_label}_{option_type}")),
                &(spot, is_call, target_price, initial_guess),
                |b, &(s, is_call, price, guess)| {
                    b.iter(|| {
                        black_box(refine_vol_and_greeks(
                            black_box(s),
                            black_box(r),
                            black_box(cost_of_carry),
                            black_box(is_call),
                            black_box(k),
                            black_box(t),
                            black_box(price),
                            black_box(guess),
                            black_box(multiplier),
                        ))
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_black_scholes_greeks_moneyness,
    bench_imply_vol_and_greeks_moneyness,
    bench_refine_vol_and_greeks_moneyness,
);
criterion::criterion_main!(benches);
