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
use nautilus_common::cache::Cache;
use nautilus_model::{enums::PriceType, identifiers::Venue, types::Currency};
use rust_decimal_macros::dec;

mod xrate_workload;

use xrate_workload::{FX_BASES, add_bar_types, add_instruments, make_quote};

fn bench_get_xrate(c: &mut Criterion) {
    let venue = Venue::from("SIM");

    // Baseline: every instrument has quotes, so the bars map is never scanned
    let mut cache = Cache::default();
    let pairs = add_instruments(&mut cache, venue, FX_BASES.len());
    for pair in &pairs {
        cache.add_quote(make_quote(pair.id)).unwrap();
    }

    c.bench_function("Cache get_xrate quotes", |b| {
        b.iter(|| {
            let _ = cache.get_xrate(
                black_box(venue),
                black_box(Currency::AUD()),
                black_box(Currency::USD()),
                black_box(PriceType::Mid),
            );
        });
    });

    // Bars fallback with no quotes: one instrument holds all bar types (bid + ask
    // per step, so the map holds twice the step count)
    for bar_type_count in [10_u64, 100, 500] {
        let mut cache = Cache::default();
        let pairs = add_instruments(&mut cache, venue, FX_BASES.len());
        add_bar_types(&mut cache, pairs[0].id, bar_type_count);

        c.bench_function(
            format!(
                "Cache get_xrate bars fallback ({} bar types)",
                bar_type_count * 2
            )
            .as_str(),
            |b| {
                b.iter(|| {
                    let _ = cache.get_xrate(
                        black_box(venue),
                        black_box(Currency::AUD()),
                        black_box(Currency::USD()),
                        black_box(PriceType::Mid),
                    );
                });
            },
        );
    }

    // Scan-and-miss: only the queried pair has quotes; unrelated bar types force a
    // full bars-map scan for every other instrument
    let mut cache = Cache::default();
    let pairs = add_instruments(&mut cache, venue, FX_BASES.len());
    cache.add_quote(make_quote(pairs[0].id)).unwrap();
    add_bar_types(&mut cache, pairs[0].id, 200);

    c.bench_function("Cache get_xrate scan-and-miss (400 bar types)", |b| {
        b.iter(|| {
            let _ = cache.get_xrate(
                black_box(venue),
                black_box(Currency::AUD()),
                black_box(Currency::USD()),
                black_box(PriceType::Mid),
            );
        });
    });
}

fn bench_get_xrate_mixed(c: &mut Criterion) {
    let mut group = c.benchmark_group("Cache try_get_xrate mixed");

    for pair_count in [5, FX_BASES.len()] {
        for bar_steps in [1, 25] {
            for venue_count in [1, 4] {
                let mut cache = Cache::default();
                let venue = Venue::from("SIM0");

                for venue_index in 0..venue_count {
                    let current_venue = Venue::from(format!("SIM{venue_index}").as_str());
                    let pairs = add_instruments(&mut cache, current_venue, pair_count);
                    for (index, pair) in pairs.iter().enumerate() {
                        add_bar_types(&mut cache, pair.id, bar_steps);
                        if index % 2 == 0 {
                            cache.add_quote(make_quote(pair.id)).unwrap();
                        }
                    }
                }

                // AUD uses its quote while EUR falls back to bars, both directions
                // are checked before timing so a missing fallback cannot look faster.
                for (currency, bid, ask) in [
                    (Currency::AUD(), dec!(0.80000), dec!(0.80010)),
                    (Currency::EUR(), dec!(0.80005), dec!(0.80005)),
                ] {
                    for (price_type, forward, reverse) in [
                        (PriceType::Bid, bid, dec!(1) / ask),
                        (PriceType::Ask, ask, dec!(1) / bid),
                        (PriceType::Mid, (bid + ask) / dec!(2), dec!(2) / (bid + ask)),
                    ] {
                        assert_eq!(
                            cache
                                .try_get_xrate(venue, currency, Currency::USD(), price_type)
                                .unwrap(),
                            Some(forward),
                        );
                        assert_eq!(
                            cache
                                .try_get_xrate(venue, Currency::USD(), currency, price_type)
                                .unwrap(),
                            Some(reverse),
                        );
                    }
                }

                let id = format!("{pair_count} pairs/{bar_steps} steps/{venue_count} venues");
                group.bench_with_input(BenchmarkId::new("EUR-USD", id), &cache, |b, cache| {
                    b.iter(|| {
                        black_box(cache.try_get_xrate(
                            black_box(venue),
                            black_box(Currency::EUR()),
                            black_box(Currency::USD()),
                            black_box(PriceType::Mid),
                        ))
                    });
                });
            }
        }
    }

    group.finish();
}

criterion_group!(benches, bench_get_xrate, bench_get_xrate_mixed);
criterion_main!(benches);
