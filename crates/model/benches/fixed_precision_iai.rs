// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use iai::black_box;
use nautilus_model::types::fixed::{f64_to_fixed_i64, f64_to_fixed_i128};

fn bench_fixed_i64_precision() -> i64 {
    f64_to_fixed_i64(black_box(-0.000_000_001), black_box(9))
}

fn bench_fixed_i128_precision() -> i128 {
    f64_to_fixed_i128(black_box(-0.000_000_001), black_box(9))
}

// i64 operations
fn bench_i64_add() -> i64 {
    let a = black_box(1_000_000_000); // 1 billion
    let b = black_box(2_000_000_000); // 2 billion
    a + b
}

fn bench_i64_sub() -> i64 {
    let a = black_box(2_000_000_000);
    let b = black_box(1_000_000_000);
    a - b
}

fn bench_i64_mul() -> i64 {
    let a = black_box(1_000_000);
    let b = black_box(1_000);
    a * b
}

fn bench_i64_div() -> i64 {
    let a = black_box(1_000_000_000);
    let b = black_box(1_000);
    a / b
}

// i128 operations
fn bench_i128_add() -> i128 {
    let a = black_box(1_000_000_000_i128);
    let b = black_box(2_000_000_000_i128);
    a + b
}

fn bench_i128_sub() -> i128 {
    let a = black_box(2_000_000_000_i128);
    let b = black_box(1_000_000_000_i128);
    a - b
}

fn bench_i128_mul() -> i128 {
    let a = black_box(1_000_000_i128);
    let b = black_box(1_000_i128);
    a * b
}

fn bench_i128_div() -> i128 {
    let a = black_box(1_000_000_000_i128);
    let b = black_box(1_000_i128);
    a / b
}

iai::main!(
    bench_i64_add,
    bench_i64_sub,
    bench_i64_mul,
    bench_i64_div,
    bench_i128_add,
    bench_i128_sub,
    bench_i128_mul,
    bench_i128_div,
    bench_fixed_i64_precision,
    bench_fixed_i128_precision,
);
