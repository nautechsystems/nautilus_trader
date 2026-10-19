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

use nautilus_common::cache::Cache;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType, QuoteTick},
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{CurrencyPair, InstrumentAny, stubs::default_fx_ccy},
    types::{Price, Quantity},
};

pub(super) const FX_BASES: [&str; 20] = [
    "AUD", "EUR", "GBP", "NZD", "CAD", "CHF", "JPY", "SGD", "HKD", "SEK", "NOK", "DKK", "ZAR",
    "MXN", "TRY", "KRW", "THB", "PLN", "HUF", "CZK",
];

pub(super) fn add_instruments(cache: &mut Cache, venue: Venue, count: usize) -> Vec<CurrencyPair> {
    FX_BASES
        .iter()
        .take(count)
        .map(|base| default_fx_ccy(Symbol::from(format!("{base}/USD").as_str()), Some(venue)))
        .inspect(|pair| {
            cache
                .add_instrument(InstrumentAny::CurrencyPair(pair.clone()))
                .unwrap();
        })
        .collect()
}

pub(super) fn make_quote(instrument_id: InstrumentId) -> QuoteTick {
    QuoteTick {
        instrument_id,
        bid_price: Price::from("0.80000"),
        ask_price: Price::from("0.80010"),
        bid_size: Quantity::from(1),
        ask_size: Quantity::from(1),
        ..Default::default()
    }
}

pub(super) fn add_bar_types(cache: &mut Cache, instrument_id: InstrumentId, count: u64) {
    for step in 1..=count {
        let bid_type = BarType::from(format!("{instrument_id}-{step}-TICK-BID-EXTERNAL").as_str());
        let ask_type = BarType::from(format!("{instrument_id}-{step}-TICK-ASK-EXTERNAL").as_str());
        cache.add_bar(make_bar(bid_type, step)).unwrap();
        cache.add_bar(make_bar(ask_type, step)).unwrap();
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
