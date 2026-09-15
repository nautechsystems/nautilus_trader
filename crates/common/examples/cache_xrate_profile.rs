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

use std::{hint::black_box, time::Instant};

use nautilus_common::cache::Cache;
use nautilus_model::{enums::PriceType, identifiers::Venue, types::Currency};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[path = "../benches/cache/xrate_workload.rs"]
mod xrate_workload;

use xrate_workload::{FX_BASES, add_bar_types, add_instruments, make_quote};

const SAMPLES: usize = 30;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        6,
        "Usage: cache_xrate_profile quotes|bars|mixed|scan pairs bar_steps venues iterations"
    );
    let mode = args[1].as_str();
    assert!(matches!(mode, "quotes" | "bars" | "mixed" | "scan"));
    let pairs: usize = args[2].parse().unwrap();
    let steps: u64 = args[3].parse().unwrap();
    let venues: usize = args[4].parse().unwrap();
    let iterations: usize = args[5].parse().unwrap();
    assert!((2..=FX_BASES.len()).contains(&pairs));
    assert!(steps > 0 && venues > 0 && iterations > 0);

    let before_currencies = ustr::total_allocated();
    initialize_currencies();
    let before_setup = ustr::total_allocated();
    let cache = setup(mode, pairs, steps, venues);
    let venue = Venue::from("SIM0");
    let after_setup = ustr::total_allocated();
    let first = first_lookup(&cache, venue);
    let after_first = ustr::total_allocated();
    let expected = Some(dec!(0.80005));
    assert_eq!(first, expected);

    let mut samples_ns = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();

        for _ in 0..iterations {
            assert_eq!(steady_lookup(&cache, venue), expected);
        }
        samples_ns.push(started.elapsed().as_nanos());
    }
    let after_steady = ustr::total_allocated();
    println!(
        "{}",
        serde_json::json!({
            "lookups": SAMPLES * iterations,
            "iterations_per_sample": iterations,
            "samples_ns": samples_ns,
            "interning_currency_bytes": before_setup - before_currencies,
            "interning_setup_bytes": after_setup - before_setup,
            "interning_first_bytes": after_first - after_setup,
            "interning_steady_bytes": after_steady - after_first,
        })
    );
}

#[inline(never)]
fn initialize_currencies() {
    for code in FX_BASES {
        black_box(Currency::from(code));
    }
    black_box(Currency::USD());
}

#[inline(never)]
fn setup(mode: &str, pair_count: usize, steps: u64, venue_count: usize) -> Cache {
    let mut cache = Cache::default();

    for index in 0..venue_count {
        let venue = Venue::from(format!("SIM{index}").as_str());
        let pairs = add_instruments(&mut cache, venue, pair_count);
        for (index, pair) in pairs.iter().enumerate() {
            if mode != "quotes" && (mode != "scan" || index == 0) {
                add_bar_types(&mut cache, pair.id, steps);
            }

            if mode == "quotes"
                || (mode == "mixed" && index % 2 == 0)
                || (mode == "scan" && index == 0)
            {
                cache.add_quote(make_quote(pair.id)).unwrap();
            }
        }
    }
    cache
}

// Separate stack frames keep setup and the first lookup out of steady-state profiles
#[inline(never)]
fn first_lookup(cache: &Cache, venue: Venue) -> Option<Decimal> {
    let rate = steady_lookup(cache, venue);
    assert_eq!(rate, Some(dec!(0.80005)));
    rate
}

#[inline(never)]
fn steady_lookup(cache: &Cache, venue: Venue) -> Option<Decimal> {
    black_box(
        cache
            .try_get_xrate(
                black_box(venue),
                Currency::AUD(),
                Currency::USD(),
                PriceType::Mid,
            )
            .unwrap(),
    )
}
