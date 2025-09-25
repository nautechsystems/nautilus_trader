// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Helpers for converting Hyperliquid instrument definitions into Nautilus instruments.

use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::{
    currencies::CURRENCY_MAP,
    enums::CurrencyType,
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};

use crate::{
    common::consts::HYPERLIQUID_VENUE,
    http::parse::{HyperliquidInstrumentDef, HyperliquidMarketType},
};

fn get_currency(code: &str) -> Currency {
    CURRENCY_MAP
        .lock()
        .expect("Failed to acquire CURRENCY_MAP lock")
        .get(code)
        .copied()
        .unwrap_or_else(|| Currency::new(code, 8, 0, code, CurrencyType::Crypto))
}

/// Converts a single Hyperliquid instrument definition into a Nautilus `InstrumentAny`.
///
/// Returns `None` if the conversion fails (e.g., unsupported market type).
#[must_use]
pub fn create_instrument_from_def(def: &HyperliquidInstrumentDef) -> Option<InstrumentAny> {
    let clock = get_atomic_clock_realtime();
    let ts_event = clock.get_time_ns();
    let ts_init = ts_event;

    let symbol = Symbol::new(&def.symbol);
    let venue = *HYPERLIQUID_VENUE;
    let instrument_id = InstrumentId::new(symbol, venue);

    let raw_symbol = Symbol::new(&def.symbol);
    let base_currency = get_currency(&def.base);
    let quote_currency = get_currency(&def.quote);
    let price_increment = Price::from(&def.tick_size.to_string());
    let size_increment = Quantity::from(&def.lot_size.to_string());

    match def.market_type {
        HyperliquidMarketType::Spot => Some(InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            raw_symbol,
            base_currency,
            quote_currency,
            def.price_decimals as u8,
            def.size_decimals as u8,
            price_increment,
            size_increment,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            ts_event,
            ts_init,
        ))),
        HyperliquidMarketType::Perp => {
            let settlement_currency = get_currency("USDC");

            Some(InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
                instrument_id,
                raw_symbol,
                base_currency,
                quote_currency,
                settlement_currency,
                false,
                def.price_decimals as u8,
                def.size_decimals as u8,
                price_increment,
                size_increment,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                ts_event,
                ts_init,
            )))
        }
    }
}

/// Convert a collection of Hyperliquid instrument definitions into Nautilus instruments,
/// discarding any definitions that fail to convert.
#[must_use]
pub fn instruments_from_defs(defs: &[HyperliquidInstrumentDef]) -> Vec<InstrumentAny> {
    defs.iter().filter_map(create_instrument_from_def).collect()
}

/// Convert owned definitions into Nautilus instruments, consuming the input vector.
#[must_use]
pub fn instruments_from_defs_owned(defs: Vec<HyperliquidInstrumentDef>) -> Vec<InstrumentAny> {
    defs.into_iter()
        .filter_map(|def| create_instrument_from_def(&def))
        .collect()
}
