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

//! Shared parsing helpers converting Massive wire values into Nautilus domain types.

use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    types::{Price, Quantity, fixed::FIXED_PRECISION},
};
use rust_decimal::Decimal;

use crate::common::consts::MASSIVE_VENUE;

/// Minimum display precision applied to parsed US equity prices.
///
/// US equities display in cents; trades and sub-dollar quotes can carry up to
/// four decimals, which the natural-scale parsing below preserves exactly.
pub const MIN_PRICE_PRECISION: u8 = 2;

/// Returns the Nautilus instrument ID for a Massive ticker symbol.
#[must_use]
pub fn instrument_id_from_ticker(ticker: &str) -> InstrumentId {
    InstrumentId::new(Symbol::new(ticker), *MASSIVE_VENUE)
}

/// Returns the display precision for a decimal price: its natural scale,
/// floored at [`MIN_PRICE_PRECISION`] so values keep exact cents display.
///
/// # Errors
///
/// Returns an error if the decimal scale exceeds [`FIXED_PRECISION`].
pub fn price_precision(value: Decimal) -> anyhow::Result<u8> {
    let scale = value.scale() as u8;
    anyhow::ensure!(
        scale <= FIXED_PRECISION,
        "Price scale {scale} exceeds FIXED_PRECISION {FIXED_PRECISION}: {value}"
    );
    Ok(scale.max(MIN_PRICE_PRECISION))
}

/// Converts a decimal price to a [`Price`] preserving the exact wire value.
///
/// `precision` must be at least the decimal's scale so no rounding occurs.
///
/// # Errors
///
/// Returns an error if the value cannot be represented at the precision.
pub fn parse_price(value: Decimal, precision: u8) -> anyhow::Result<Price> {
    Price::from_decimal_dp(value, precision)
        .map_err(|e| anyhow::anyhow!("Failed to parse price {value}: {e}"))
}

/// Returns the shared display precision for a pair of decimal prices.
///
/// # Errors
///
/// Returns an error if either decimal scale exceeds [`FIXED_PRECISION`].
pub fn shared_price_precision(a: Decimal, b: Decimal) -> anyhow::Result<u8> {
    Ok(price_precision(a)?.max(price_precision(b)?))
}

/// Converts a decimal size to a [`Quantity`] preserving the exact wire value.
///
/// # Errors
///
/// Returns an error if the value cannot be represented exactly.
pub fn parse_quantity(value: Decimal) -> anyhow::Result<Quantity> {
    let scale = value.scale() as u8;
    anyhow::ensure!(
        scale <= FIXED_PRECISION,
        "Quantity scale {scale} exceeds FIXED_PRECISION {FIXED_PRECISION}: {value}"
    );
    Quantity::from_decimal_dp(value, scale)
        .map_err(|e| anyhow::anyhow!("Failed to parse quantity {value}: {e}"))
}

/// Converts a Unix millisecond timestamp to [`UnixNanos`].
///
/// # Errors
///
/// Returns an error if the timestamp is negative or overflows.
pub fn unix_nanos_from_millis(millis: i64) -> anyhow::Result<UnixNanos> {
    anyhow::ensure!(
        millis >= 0,
        "Timestamp millis must be non-negative: {millis}"
    );
    let nanos = (millis as u64)
        .checked_mul(1_000_000)
        .ok_or_else(|| anyhow::anyhow!("Timestamp millis overflow: {millis}"))?;
    Ok(UnixNanos::from(nanos))
}

/// Converts a Unix nanosecond timestamp to [`UnixNanos`].
///
/// # Errors
///
/// Returns an error if the timestamp is negative.
pub fn unix_nanos_from_nanos(nanos: i64) -> anyhow::Result<UnixNanos> {
    anyhow::ensure!(nanos >= 0, "Timestamp nanos must be non-negative: {nanos}");
    Ok(UnixNanos::from(nanos as u64))
}

/// Parses an RFC 3339 timestamp string to [`UnixNanos`].
///
/// # Errors
///
/// Returns an error if the string is not a valid RFC 3339 timestamp.
pub fn unix_nanos_from_rfc3339(value: &str) -> anyhow::Result<UnixNanos> {
    let ts: jiff::Timestamp = value
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid RFC 3339 timestamp '{value}': {e}"))?;
    let nanos = ts.as_nanosecond();
    anyhow::ensure!(nanos >= 0, "Timestamp before Unix epoch: {value}");
    Ok(UnixNanos::from(nanos as u64))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_instrument_id_from_ticker() {
        let id = instrument_id_from_ticker("AAPL");
        assert_eq!(id.to_string(), "AAPL.MASSIVE");
    }

    #[rstest]
    fn test_instrument_id_with_dotted_ticker() {
        let id = instrument_id_from_ticker("BRK.A");
        assert_eq!(id.symbol.as_str(), "BRK.A");
        assert_eq!(id.venue.as_str(), "MASSIVE");
    }

    #[rstest]
    fn test_price_precision_floors_at_two() {
        assert_eq!(price_precision(dec!(311)).unwrap(), 2);
        assert_eq!(price_precision(dec!(311.9)).unwrap(), 2);
        assert_eq!(price_precision(dec!(311.91)).unwrap(), 2);
        assert_eq!(price_precision(dec!(309.4037)).unwrap(), 4);
    }

    #[rstest]
    fn test_parse_price_preserves_sub_penny() {
        let price = parse_price(dec!(309.4037), 4).unwrap();
        assert_eq!(price.to_string(), "309.4037");
    }

    #[rstest]
    fn test_shared_price_precision() {
        let p = shared_price_precision(dec!(309.4), dec!(309.5525)).unwrap();
        assert_eq!(p, 4);
    }

    #[rstest]
    fn test_parse_quantity_fractional() {
        let qty = parse_quantity(dec!(0.040600)).unwrap();
        assert_eq!(qty.to_string(), "0.040600");
    }

    #[rstest]
    fn test_parse_quantity_integer() {
        let qty = parse_quantity(dec!(120)).unwrap();
        assert_eq!(qty.to_string(), "120");
        assert_eq!(qty.precision, 0);
    }

    #[rstest]
    fn test_unix_nanos_from_millis() {
        let ts = unix_nanos_from_millis(1_536_036_818_784).unwrap();
        assert_eq!(ts.as_u64(), 1_536_036_818_784_000_000);
    }

    #[rstest]
    fn test_unix_nanos_from_millis_rejects_negative() {
        assert!(unix_nanos_from_millis(-1).is_err());
    }

    #[rstest]
    fn test_unix_nanos_from_nanos() {
        let ts = unix_nanos_from_nanos(1_787_691_846_702_314_576).unwrap();
        assert_eq!(ts.as_u64(), 1_787_691_846_702_314_576);
    }

    #[rstest]
    fn test_unix_nanos_from_rfc3339() {
        let ts = unix_nanos_from_rfc3339("2026-08-25T06:10:50.192375589Z").unwrap();
        assert_eq!(ts.as_u64(), 1_787_638_250_192_375_589);
    }

    #[rstest]
    fn test_unix_nanos_from_rfc3339_rejects_invalid() {
        assert!(unix_nanos_from_rfc3339("not-a-timestamp").is_err());
    }
}
