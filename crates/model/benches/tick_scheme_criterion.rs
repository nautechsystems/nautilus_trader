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

//! Benchmarks named rule lookup and bid/ask navigation used by instrument price rounding.
//! Scheme construction, registration, and correctness checks run outside the timed region.
//!
//! Run with `cargo bench -p nautilus-model --bench tick_scheme_criterion`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use nautilus_model::{
    instruments::{
        FixedTickScheme, TickScheme, TieredTickScheme, register_tick_scheme,
        tick_scheme_rule_from_name,
    },
    types::Price,
};

fn bench_builtin_schemes(c: &mut Criterion) {
    bench_schemes(
        c,
        "tick_scheme_builtin",
        &[
            ("crypto", "CRYPTO_0_01", 1.133, "1.13", "1.14"),
            (
                "precision_alias",
                "fixed_precision_05",
                1.130011,
                "1.13001",
                "1.13002",
            ),
            ("betfair", "BETFAIR", 3.13, "3.10", "3.15"),
        ],
    );
}

fn bench_registered_schemes(c: &mut Criterion) {
    register_tick_scheme(
        "BENCH_FIXED_TICK_SCHEME",
        TickScheme::Fixed(FixedTickScheme::new(Price::from("0.05")).unwrap()),
    )
    .unwrap();
    register_tick_scheme(
        "BENCH_TIERED_TICK_SCHEME",
        TickScheme::Tiered(
            TieredTickScheme::new(&[(0.05, 10.0, 0.05), (10.0, 100.0, 0.25)], 2, 1000).unwrap(),
        ),
    )
    .unwrap();

    bench_schemes(
        c,
        "tick_scheme_registered",
        &[
            ("fixed", "bench_fixed_tick_scheme", 1.133, "1.10", "1.15"),
            (
                "tiered",
                "bench_tiered_tick_scheme",
                10.63,
                "10.50",
                "10.75",
            ),
        ],
    );
}

fn bench_schemes(c: &mut Criterion, name: &str, cases: &[(&str, &str, f64, &str, &str)]) {
    let mut group = c.benchmark_group(name);

    for &(label, scheme_name, value, expected_bid, expected_ask) in cases {
        let rule = tick_scheme_rule_from_name(scheme_name).unwrap();
        let expected_bid = Price::from(expected_bid);
        let expected_ask = Price::from(expected_ask);
        let precision = expected_bid.precision;
        assert_eq!(
            rule.next_bid_price(value, 0, precision)
                .map(|price| (price, price.precision)),
            Some((expected_bid, expected_bid.precision))
        );
        assert_eq!(
            rule.next_ask_price(value, 0, precision)
                .map(|price| (price, price.precision)),
            Some((expected_ask, expected_ask.precision))
        );

        group.bench_function(format!("lookup/{label}"), |b| {
            b.iter(|| tick_scheme_rule_from_name(black_box(scheme_name)));
        });

        group.bench_function(format!("navigation/{label}"), |b| {
            b.iter(|| {
                (
                    black_box(rule).next_bid_price(black_box(value), 0, black_box(precision)),
                    black_box(rule).next_ask_price(black_box(value), 0, black_box(precision)),
                )
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_builtin_schemes, bench_registered_schemes);
criterion_main!(benches);
