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

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    enums::CurrencyType,
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use tracing::warn;
use ustr::Ustr;

use crate::common::constants::LIGHTER_VENUE;

use super::models::LighterOrderBook;

/// Normalized instrument definition produced from Lighter `orderBooks`.
#[derive(Debug, Clone)]
pub struct LighterInstrumentDef {
    /// The computed instrument identifier.
    pub instrument_id: InstrumentId,
    /// Raw symbol used by the venue (often required for subscriptions).
    pub raw_symbol: Symbol,
    /// Market identifier used for REST/WS subscriptions.
    pub market_index: u32,
    /// Venue symbol (e.g., "BTC").
    pub venue_symbol: Ustr,
    /// Base asset code (e.g., "BTC").
    pub base: Ustr,
    /// Quote asset code (e.g., "USD").
    pub quote: Ustr,
    /// Price decimal precision.
    pub price_decimals: u32,
    /// Size decimal precision.
    pub size_decimals: u32,
    /// Price tick size.
    pub tick_size: Decimal,
    /// Size increment (lot size).
    pub lot_size: Decimal,
    /// Minimum order size.
    pub min_base_amount: Decimal,
    /// Whether market is active/tradable.
    pub active: bool,
    /// Raw upstream entry for debugging/telemetry.
    pub raw: String,
}

#[derive(Default, Debug, Clone)]
pub struct ParseReport {
    pub skipped: usize,
    pub errors: HashMap<u32, String>,
}

/// Parse instrument definitions from the `orderBooks` response.
///
/// # Errors
/// Returns an error if required fields are missing or invalid.
pub fn parse_instrument_defs(
    books: &[LighterOrderBook],
) -> Result<(Vec<LighterInstrumentDef>, ParseReport)> {
    let mut defs = Vec::with_capacity(books.len());
    let mut report = ParseReport::default();

    for book in books {
        match parse_single_def(book) {
            Ok(def) => defs.push(def),
            Err(e) => {
                report.skipped += 1;
                report.errors.insert(book.market_index, e.to_string());
            }
        }
    }

    Ok((defs, report))
}

fn parse_single_def(book: &LighterOrderBook) -> Result<LighterInstrumentDef> {
    let price_decimals = book
        .supported_price_decimals
        .ok_or_else(|| anyhow!("missing supported_price_decimals"))?;
    let size_decimals = book
        .supported_size_decimals
        .ok_or_else(|| anyhow!("missing supported_size_decimals"))?;

    let tick_size = book.tick_size.unwrap_or_else(|| pow10_neg(price_decimals));
    let lot_size = book.lot_size.unwrap_or_else(|| pow10_neg(size_decimals));
    let min_base_amount = book
        .min_base_amount
        .unwrap_or_else(|| pow10_neg(size_decimals));

    let base = book
        .base_token
        .as_deref()
        .or(book.symbol.as_deref())
        .ok_or_else(|| anyhow!("missing base token/symbol"))?;
    let quote = book.quote_token.as_deref().unwrap_or("USD");
    let venue_symbol = book.symbol.as_deref().unwrap_or(base);
    let symbol = Symbol::new(format!("{base}-{quote}-PERP"));
    let instrument_id = InstrumentId::new(symbol, *LIGHTER_VENUE);
    let raw_symbol = Symbol::new(venue_symbol);

    Ok(LighterInstrumentDef {
        instrument_id,
        raw_symbol,
        market_index: book.market_index,
        venue_symbol: venue_symbol.into(),
        base: base.into(),
        quote: quote.into(),
        price_decimals,
        size_decimals,
        tick_size,
        lot_size,
        min_base_amount,
        active: book.active.unwrap_or(true),
        raw: serde_json::to_string(book).unwrap_or_default(),
    })
}

/// Convert parsed instrument definitions into Nautilus instruments.
///
/// # Errors
/// Returns an error if any conversion fails (e.g., invalid precision).
pub fn instruments_from_defs(
    defs: &[LighterInstrumentDef],
    ts_init: UnixNanos,
) -> Result<Vec<InstrumentAny>> {
    let mut instruments = Vec::with_capacity(defs.len());
    let ts_event = ts_init;

    for def in defs {
        if !def.active {
            warn!(
                market_index = def.market_index,
                symbol = %def.venue_symbol,
                "Skipping inactive Lighter market",
            );
            continue;
        }

        instruments.push(create_instrument_from_def(def, ts_event, ts_init)?);
    }

    Ok(instruments)
}

fn create_instrument_from_def(
    def: &LighterInstrumentDef,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Result<InstrumentAny> {
    let base_currency = get_currency(&def.base);
    let quote_currency = get_currency(&def.quote);
    let settlement_currency = quote_currency;

    let price_increment = Price::from(def.tick_size.to_string());
    let size_increment = Quantity::from(def.lot_size.to_string());
    let min_quantity = Quantity::from(def.min_base_amount.to_string());

    let price_precision: u8 = def
        .price_decimals
        .to_u8()
        .ok_or_else(|| anyhow!("price_decimals too large"))?;
    let size_precision: u8 = def
        .size_decimals
        .to_u8()
        .ok_or_else(|| anyhow!("size_decimals too large"))?;

    Ok(InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        def.instrument_id,
        def.raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        false,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None,
        Some(size_increment),
        None,
        Some(min_quantity),
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

fn pow10_neg(decimals: u32) -> Decimal {
    if decimals == 0 {
        return Decimal::ONE;
    }

    Decimal::from_i128_with_scale(1, decimals)
}

fn get_currency(code: &str) -> Currency {
    Currency::try_from_str(code).unwrap_or_else(|| {
        let currency = Currency::new(code, 8, 0, code, CurrencyType::Crypto);
        if let Err(e) = Currency::register(currency, false) {
            warn!(%code, %e, "Failed to register currency");
        }
        currency
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use nautilus_core::nanos::UnixNanos;
    use nautilus_core::time::get_atomic_clock_realtime;

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/test_data/lighter/http/orderbooks.json")
    }

    #[test]
    fn parse_orderbooks_fixture() {
        let data = std::fs::read_to_string(fixture_path()).unwrap();
        let resp: super::super::models::OrderBooksResponse =
            serde_json::from_str(&data).expect("failed to parse fixture");
        let books = resp.into_books();
        let (defs, report) = parse_instrument_defs(&books).expect("parse failed");

        assert!(
            report.errors.is_empty(),
            "unexpected parse errors: {report:?}"
        );
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].market_index, 1);
        assert_eq!(defs[0].price_decimals, 1);
        assert_eq!(defs[0].size_decimals, 4);
        assert_eq!(defs[0].tick_size, Decimal::from_i128_with_scale(1, 1));
        assert_eq!(defs[0].lot_size, Decimal::from_i128_with_scale(1, 4));
    }

    #[test]
    fn build_instruments_from_defs() {
        let data = std::fs::read_to_string(fixture_path()).unwrap();
        let resp: super::super::models::OrderBooksResponse =
            serde_json::from_str(&data).expect("failed to parse fixture");
        let books = resp.into_books();
        let (defs, report) = parse_instrument_defs(&books).expect("parse failed");
        assert!(report.errors.is_empty());

        let ts_init: UnixNanos = get_atomic_clock_realtime().get_time_ns();
        let instruments = instruments_from_defs(&defs, ts_init).expect("conversion failed");

        assert_eq!(instruments.len(), 1, "inactive market should be skipped");
        let instrument = match &instruments[0] {
            InstrumentAny::CryptoPerpetual(cp) => cp,
            _ => panic!("expected crypto perpetual"),
        };
        assert_eq!(instrument.id.symbol.to_string(), "BTC-USD-PERP");
        assert_eq!(instrument.price_precision, 1);
        assert_eq!(instrument.size_precision, 4);
    }
}
