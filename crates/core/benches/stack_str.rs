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
use nautilus_core::StackStr;

fn bench_stackstr_new_short(c: &mut Criterion) {
    c.bench_function("StackStr::new (short)", |b| {
        b.iter(|| StackStr::new(black_box("BINANCE")));
    });
}

fn bench_stackstr_new_medium(c: &mut Criterion) {
    c.bench_function("StackStr::new (medium)", |b| {
        b.iter(|| StackStr::new(black_box("O-20231215-001-001")));
    });
}

fn bench_stackstr_new_max(c: &mut Criterion) {
    let max_str = "x".repeat(36);
    c.bench_function("StackStr::new (max 36)", |b| {
        b.iter(|| StackStr::new(black_box(&max_str)));
    });
}

fn bench_stackstr_eq_same(c: &mut Criterion) {
    let a = StackStr::new("O-20231215-001-001");
    let b = StackStr::new("O-20231215-001-001");
    c.bench_function("StackStr::eq (same)", |b_iter| {
        b_iter.iter(|| black_box(&a) == black_box(&b));
    });
}

fn bench_stackstr_eq_different(c: &mut Criterion) {
    let a = StackStr::new("O-20231215-001-001");
    let b = StackStr::new("O-20231215-001-002");
    c.bench_function("StackStr::eq (different)", |b_iter| {
        b_iter.iter(|| black_box(&a) == black_box(&b));
    });
}

fn bench_stackstr_hash(c: &mut Criterion) {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let s = StackStr::new("O-20231215-001-001");
    c.bench_function("StackStr::hash", |b| {
        b.iter(|| {
            let mut hasher = DefaultHasher::new();
            black_box(&s).hash(&mut hasher);
            hasher.finish()
        });
    });
}

fn bench_stackstr_as_str(c: &mut Criterion) {
    let s = StackStr::new("O-20231215-001-001");
    c.bench_function("StackStr::as_str", |b| {
        b.iter(|| black_box(&s).as_str());
    });
}

fn bench_stackstr_clone(c: &mut Criterion) {
    let s = StackStr::new("O-20231215-001-001");
    c.bench_function("StackStr::clone (Copy)", |b| {
        b.iter(|| black_box(s));
    });
}

fn bench_stackstr_from_bytes(c: &mut Criterion) {
    let bytes = b"O-20231215-001-001";
    c.bench_function("StackStr::from_bytes", |b| {
        b.iter(|| StackStr::from_bytes(black_box(bytes)));
    });
}

fn bench_stackstr_cmp(c: &mut Criterion) {
    let a = StackStr::new("AAA-001");
    let b = StackStr::new("ZZZ-999");
    c.bench_function("StackStr::cmp", |b_iter| {
        b_iter.iter(|| black_box(&a).cmp(black_box(&b)));
    });
}

fn bench_stackstr_to_string(c: &mut Criterion) {
    let s = StackStr::new("O-20231215-001-001");
    c.bench_function("StackStr::to_string", |b| {
        b.iter(|| black_box(&s).to_string());
    });
}

criterion_group!(
    benches,
    bench_stackstr_new_short,
    bench_stackstr_new_medium,
    bench_stackstr_new_max,
    bench_stackstr_eq_same,
    bench_stackstr_eq_different,
    bench_stackstr_hash,
    bench_stackstr_as_str,
    bench_stackstr_clone,
    bench_stackstr_from_bytes,
    bench_stackstr_cmp,
    bench_stackstr_to_string,
);
criterion_main!(benches);
