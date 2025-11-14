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

//! Parsing utilities that convert dYdX payloads into Nautilus domain models.

use std::str::FromStr;

use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::consts::DYDX_VENUE;

/// Returns a currency from the internal map or creates a new crypto currency.
///
/// If the code is empty, logs a warning with context and returns USDC as fallback.
/// Uses [`Currency::get_or_create_crypto`] to handle unknown currency codes,
/// which automatically registers newly listed dYdX assets.
fn get_currency_with_context(code: &str, context: Option<&str>) -> Currency {
    let trimmed = code.trim();
    let ctx = context.unwrap_or("unknown");

    if trimmed.is_empty() {
        tracing::warn!("Empty currency code for context {ctx}, defaulting to USDC as fallback");
        return Currency::USDC();
    }

    Currency::get_or_create_crypto(trimmed)
}

/// Returns a currency from the given code.
///
/// Uses [`Currency::get_or_create_crypto`] to handle unknown currency codes.
#[must_use]
pub fn get_currency(code: &str) -> Currency {
    get_currency_with_context(code, None)
}

/// Parses a dYdX instrument ID from a ticker string.
///
/// dYdX v4 only lists perpetual markets, with tickers in the format
/// "BASE-QUOTE" (e.g., "BTC-USD"). Nautilus standardizes perpetual
/// instrument symbols by appending the product suffix "-PERP".
///
/// This function converts a dYdX ticker into a Nautilus `InstrumentId`
/// by appending "-PERP" to the symbol and using the dYdX venue.
///
#[must_use]
pub fn parse_instrument_id<S: AsRef<str>>(ticker: S) -> InstrumentId {
    let mut base = ticker.as_ref().trim().to_uppercase();
    // Ensure we don't double-append when given a symbol already suffixed.
    if !base.ends_with("-PERP") {
        base.push_str("-PERP");
    }
    let symbol = Ustr::from(base.as_str());
    InstrumentId::new(Symbol::from_ustr_unchecked(symbol), *DYDX_VENUE)
}

/// Parses a decimal string into a [`Price`].
///
/// # Errors
///
/// Returns an error if the string cannot be parsed into a valid price.
pub fn parse_price(value: &str, field_name: &str) -> anyhow::Result<Price> {
    Price::from_str(value).map_err(|e| {
        anyhow::anyhow!("Failed to parse '{field_name}' value '{value}' into Price: {e}")
    })
}

/// Parses a decimal string into a [`Quantity`].
///
/// # Errors
///
/// Returns an error if the string cannot be parsed into a valid quantity.
pub fn parse_quantity(value: &str, field_name: &str) -> anyhow::Result<Quantity> {
    Quantity::from_str(value).map_err(|e| {
        anyhow::anyhow!("Failed to parse '{field_name}' value '{value}' into Quantity: {e}")
    })
}

/// Parses a decimal string into a [`Decimal`].
///
/// # Errors
///
/// Returns an error if the string cannot be parsed into a valid decimal.
pub fn parse_decimal(value: &str, field_name: &str) -> anyhow::Result<Decimal> {
    Decimal::from_str(value).map_err(|e| {
        anyhow::anyhow!("Failed to parse '{field_name}' value '{value}' into Decimal: {e}")
    })
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_get_currency() {
        let btc = get_currency("BTC");
        assert_eq!(btc.code.as_str(), "BTC");

        let usdc = get_currency("USDC");
        assert_eq!(usdc.code.as_str(), "USDC");
    }

    #[rstest]
    fn test_parse_instrument_id() {
        let instrument_id = parse_instrument_id("BTC-USD");
        assert_eq!(instrument_id.symbol.as_str(), "BTC-USD-PERP");
        assert_eq!(instrument_id.venue, *DYDX_VENUE);
    }

    #[rstest]
    fn test_parse_price() {
        let price = parse_price("0.01", "test_price").unwrap();
        assert_eq!(price.to_string(), "0.01");

        let err = parse_price("invalid", "invalid_price");
        assert!(err.is_err());
    }

    #[rstest]
    fn test_parse_quantity() {
        let qty = parse_quantity("1.5", "test_qty").unwrap();
        assert_eq!(qty.to_string(), "1.5");
    }

    #[rstest]
    fn test_parse_decimal() {
        let decimal = parse_decimal("0.001", "test_decimal").unwrap();
        assert_eq!(decimal.to_string(), "0.001");
    }
}
