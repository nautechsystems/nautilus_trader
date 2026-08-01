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
use nautilus_common::cache::Cache;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType, QuoteTick},
    enums::PriceType,
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{CurrencyPair, InstrumentAny, stubs::default_fx_ccy},
    types::{Currency, Price, Quantity},
};

const FX_BASES: [&str; 20] = [
    "AUD", "EUR", "GBP", "NZD", "CAD", "CHF", "JPY", "SGD", "HKD", "SEK", "NOK", "DKK", "ZAR",
    "MXN", "TRY", "KRW", "THB", "PLN", "HUF", "CZK",
];

fn add_instruments(cache: &mut Cache, venue: Venue) -> Vec<CurrencyPair> {
    FX_BASES
        .iter()
        .map(|base| default_fx_ccy(Symbol::from(format!("{base}/USD").as_str()), Some(venue)))
        .map(|pair| {
            cache
                .add_instrument(InstrumentAny::CurrencyPair(pair.clone()))
                .unwrap();
            pair
        })
        .collect()
}

fn make_quote(instrument_id: InstrumentId) -> QuoteTick {
    QuoteTick {
        instrument_id,
        bid_price: Price::from("0.80000"),
        ask_price: Price::from("0.80010"),
        bid_size: Quantity::from(1),
        ask_size: Quantity::from(1),
        ..Default::default()
    }
}

fn make_bar(bar_type: BarType, ts_init: u64) -> Bar {
    Bar::new(
        bar_type,
        Price::from("0.80000"),
        Price::from("0.80010"),
        Price::from("0.79990"),
        Price::from("0.80005"),
        Quantity::from(100_000),
        UnixNanos::from(ts_init),
        UnixNanos::from(ts_init),
    )
}

fn add_bar_types(cache: &mut Cache, instrument_id: InstrumentId, count: u64) {
    for step in 1..=count {
        let bid_type = BarType::from(format!("{instrument_id}-{step}-TICK-BID-EXTERNAL").as_str());
        let ask_type = BarType::from(format!("{instrument_id}-{step}-TICK-ASK-EXTERNAL").as_str());
        cache.add_bar(make_bar(bid_type, step)).unwrap();
        cache.add_bar(make_bar(ask_type, step)).unwrap();
    }
}

fn bench_get_xrate(c: &mut Criterion) {
    let venue = Venue::from("SIM");

    // Baseline: every instrument has quotes, so the bars map is never scanned
    let mut cache = Cache::default();
    let pairs = add_instruments(&mut cache, venue);
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
        let pairs = add_instruments(&mut cache, venue);
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
    let pairs = add_instruments(&mut cache, venue);
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

criterion_group!(benches, bench_get_xrate);
criterion_main!(benches);
