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

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use nautilus_common::msgbus::matching::{is_matching, is_matching_backtracking};

const TOPIC: &str = "data.quotes.BINANCE.ETHUSDT";
const TOPIC_BYTES: &[u8] = b"data.quotes.BINANCE.ETHUSDT";

struct PatternCase {
    name: &'static str,
    pattern: &'static str,
    pattern_bytes: &'static [u8],
}

const PATTERNS: &[PatternCase] = &[
    PatternCase {
        name: "exact",
        pattern: "data.quotes.BINANCE.ETHUSDT",
        pattern_bytes: b"data.quotes.BINANCE.ETHUSDT",
    },
    PatternCase {
        name: "star_end",
        pattern: "data.quotes.BINANCE.*",
        pattern_bytes: b"data.quotes.BINANCE.*",
    },
    PatternCase {
        name: "star_middle",
        pattern: "data.*.BINANCE.ETHUSDT",
        pattern_bytes: b"data.*.BINANCE.ETHUSDT",
    },
    PatternCase {
        name: "multi_star",
        pattern: "data.*.BINANCE.*",
        pattern_bytes: b"data.*.BINANCE.*",
    },
    PatternCase {
        name: "question",
        pattern: "data.quotes.BINANCE.ETHUS?T",
        pattern_bytes: b"data.quotes.BINANCE.ETHUS?T",
    },
    PatternCase {
        name: "multi_question",
        pattern: "data.quotes.BINANCE.ETH????",
        pattern_bytes: b"data.quotes.BINANCE.ETH????",
    },
    PatternCase {
        name: "mixed",
        pattern: "data.*.BINANCE.ETH*",
        pattern_bytes: b"data.*.BINANCE.ETH*",
    },
    PatternCase {
        name: "no_match",
        pattern: "order.*.BYBIT.*",
        pattern_bytes: b"order.*.BYBIT.*",
    },
];

fn bench_is_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("is_matching");

    for case in PATTERNS {
        group.bench_with_input(BenchmarkId::new("bytes", case.name), &case, |b, case| {
            b.iter(|| black_box(is_matching(TOPIC_BYTES, case.pattern_bytes)));
        });
    }

    group.finish();
}

fn bench_is_matching_backtracking(c: &mut Criterion) {
    let mut group = c.benchmark_group("is_matching_backtracking");

    for case in PATTERNS {
        group.bench_with_input(BenchmarkId::new("mstr", case.name), &case, |b, case| {
            b.iter(|| black_box(is_matching_backtracking(TOPIC.into(), case.pattern.into())));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_is_matching, bench_is_matching_backtracking);
criterion_main!(benches);
