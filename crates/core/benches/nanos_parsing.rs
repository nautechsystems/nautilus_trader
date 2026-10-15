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

//! Benchmarks for integer timestamp parsing and existing fallback routes.
//! Fixtures and semantic checks are outside timing.

#[path = "nanos_parsing/corpus.rs"]
mod corpus;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use nautilus_core::UnixNanos;

fn bench_nanos(c: &mut Criterion) {
    for input in ["1707578323456789123", "2024-02-10T15:18:43.456789123Z"] {
        assert_eq!(
            input.parse::<UnixNanos>().unwrap(),
            UnixNanos::from(1_707_578_323_456_789_123)
        );
        c.bench_with_input(
            BenchmarkId::new("core_nanos/parse", input),
            &input,
            |b, input| {
                b.iter(|| black_box(black_box(input).parse::<UnixNanos>().unwrap()));
            },
        );
    }
}

criterion_group!(
    benches,
    bench_nanos,
    corpus::bench_integer_corpus,
    corpus::bench_fallback_controls
);
criterion_main!(benches);
