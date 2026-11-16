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

//! Shared testing utilities for OKX adapter unit tests.

use std::{fs, path::PathBuf};

use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::CryptoPerpetual,
    types::{Currency, Price, Quantity},
};

#[cfg(test)]
#[must_use]
/// Loads a JSON fixture from the adapter test data directory.
pub(crate) fn load_test_json(file_name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(file_name);

    fs::read_to_string(path).expect("Failed to read test JSON file")
}

#[cfg(test)]
#[must_use]
/// Creates the BTC-USDT perpetual stub used by OKX parser and dispatch tests.
pub(crate) fn create_stub_instrument() -> CryptoPerpetual {
    CryptoPerpetual::builder()
        .instrument_id(InstrumentId::from("BTC-USDT-SWAP.OKX"))
        .raw_symbol(Symbol::from("BTC-USDT-SWAP"))
        .base_currency(Currency::BTC())
        .quote_currency(Currency::USDT())
        .settlement_currency(Currency::USDT())
        .is_inverse(false)
        .price_precision(2)
        .size_precision(8)
        .price_increment(Price::from("0.01"))
        .size_increment(Quantity::from("0.00000001"))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}
