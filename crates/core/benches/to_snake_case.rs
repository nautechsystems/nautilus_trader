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
use nautilus_core::string::conversions::to_snake_case;

fn bench_pascal_case(c: &mut Criterion) {
    c.bench_function("to_snake_case (PascalCase)", |b| {
        b.iter(|| to_snake_case(black_box("OrderBookDelta")));
    });
}

fn bench_camel_case(c: &mut Criterion) {
    c.bench_function("to_snake_case (camelCase)", |b| {
        b.iter(|| to_snake_case(black_box("camelCase")));
    });
}

fn bench_acronyms(c: &mut Criterion) {
    c.bench_function("to_snake_case (acronyms)", |b| {
        b.iter(|| to_snake_case(black_box("getHTTPResponse")));
    });
}

fn bench_already_snake(c: &mut Criterion) {
    c.bench_function("to_snake_case (already snake)", |b| {
        b.iter(|| to_snake_case(black_box("already_snake_case")));
    });
}

fn bench_type_path(c: &mut Criterion) {
    c.bench_function("to_snake_case (type path)", |b| {
        b.iter(|| to_snake_case(black_box("nautilus_model::data::bar::Bar"))); // nautilus-import-ok
    });
}

fn bench_screaming_snake(c: &mut Criterion) {
    c.bench_function("to_snake_case (SCREAMING_SNAKE)", |b| {
        b.iter(|| to_snake_case(black_box("SHOUTY_SNAKE_CASE")));
    });
}

fn bench_mixed_digits(c: &mut Criterion) {
    c.bench_function("to_snake_case (mixed digits)", |b| {
        b.iter(|| to_snake_case(black_box("ABC123Def456")));
    });
}

fn bench_long_type_path(c: &mut Criterion) {
    // nautilus-import-ok
    c.bench_function("to_snake_case (long type path)", |b| {
        b.iter(|| {
            to_snake_case(black_box(
                "std::vec::Vec<nautilus_model::data::order_book_delta::OrderBookDelta>",
            ))
        });
    });
}

fn bench_short_single_word(c: &mut Criterion) {
    c.bench_function("to_snake_case (short single)", |b| {
        b.iter(|| to_snake_case(black_box("Bar")));
    });
}

fn bench_repeated_same_input(c: &mut Criterion) {
    let input = "OrderBookDelta";
    c.bench_function("to_snake_case (repeated same)", |b| {
        b.iter(|| {
            let _ = to_snake_case(black_box(input));
            let _ = to_snake_case(black_box(input));
            let _ = to_snake_case(black_box(input));
            to_snake_case(black_box(input))
        });
    });
}

fn bench_unicode(c: &mut Criterion) {
    c.bench_function("to_snake_case (unicode)", |b| {
        b.iter(|| to_snake_case(black_box("CafeLatt\u{00e9}Order")));
    });
}

criterion_group!(
    benches,
    bench_pascal_case,
    bench_camel_case,
    bench_acronyms,
    bench_already_snake,
    bench_type_path,
    bench_screaming_snake,
    bench_mixed_digits,
    bench_long_type_path,
    bench_short_single_word,
    bench_repeated_same_input,
    bench_unicode,
);
criterion_main!(benches);
