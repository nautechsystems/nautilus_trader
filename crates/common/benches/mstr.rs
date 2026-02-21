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
use nautilus_common::msgbus::{MStr, Pattern, Topic};
use ustr::Ustr;

const TOPIC_STR: &str = "data.quotes.BINANCE.ETHUSDT";

fn bench_mstr_from_str(c: &mut Criterion) {
    let mut group = c.benchmark_group("MStr::from_str");

    group.bench_function("Topic", |b| {
        b.iter(|| {
            let topic: MStr<Topic> = black_box(TOPIC_STR).into();
            black_box(topic)
        });
    });

    group.bench_function("Pattern", |b| {
        b.iter(|| {
            let pattern: MStr<Pattern> = black_box(TOPIC_STR).into();
            black_box(pattern)
        });
    });

    group.finish();
}

fn bench_mstr_from_ustr(c: &mut Criterion) {
    let mut group = c.benchmark_group("MStr::from_ustr");

    let ustr = Ustr::from(TOPIC_STR);

    group.bench_function("Topic", |b| {
        b.iter(|| {
            let topic: MStr<Topic> = black_box(ustr).into();
            black_box(topic)
        });
    });

    group.finish();
}

fn bench_mstr_as_bytes(c: &mut Criterion) {
    let mut group = c.benchmark_group("MStr::as_bytes");

    let topic: MStr<Topic> = TOPIC_STR.into();
    let pattern: MStr<Pattern> = TOPIC_STR.into();

    group.bench_function("Topic", |b| {
        b.iter(|| {
            let bytes = topic.as_bytes();
            black_box(bytes.len())
        });
    });

    group.bench_function("Pattern", |b| {
        b.iter(|| {
            let bytes = pattern.as_bytes();
            black_box(bytes.len())
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_mstr_from_str,
    bench_mstr_from_ustr,
    bench_mstr_as_bytes
);
criterion_main!(benches);
