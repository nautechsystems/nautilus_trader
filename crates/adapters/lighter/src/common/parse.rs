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

//! Cross-cutting parsing helpers shared between HTTP and WebSocket layers.
//!
//! Lighter's wire payloads encode prices and order sizes as integer multiples
//! of a per-market tick. The number of decimal places (`price_decimals` and
//! `size_decimals`) is published once per market in the `orderBookDetails`
//! REST response. The helpers here turn those mantissa/precision pairs into
//! Nautilus [`Price`] and [`Quantity`] without a floating-point round-trip.

use std::str::FromStr;

use nautilus_core::{
    UnixNanos,
    datetime::{NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND},
};
use nautilus_model::types::{Price, Quantity, fixed::FIXED_PRECISION};
use rust_decimal::Decimal;

/// Maximum decimal places that fit into Nautilus [`Price`] / [`Quantity`].
pub const MAX_DECIMALS: u8 = FIXED_PRECISION;

/// Converts a Lighter price tick count into a Nautilus [`Price`].
///
/// Lighter encodes limit and trigger prices as `u32` multiples of `10^-decimals`
/// quote-asset units. The conversion uses pure integer arithmetic via
/// [`Price::from_mantissa_exponent`].
///
/// # Errors
///
/// Returns an error if `decimals` exceeds [`MAX_DECIMALS`].
pub fn parse_price_from_ticks(ticks: u32, decimals: u8) -> anyhow::Result<Price> {
    anyhow::ensure!(
        decimals <= MAX_DECIMALS,
        "price decimals {decimals} exceeds maximum {MAX_DECIMALS}",
    );
    let exponent = -(decimals as i8);
    Ok(Price::from_mantissa_exponent(
        i64::from(ticks),
        exponent,
        decimals,
    ))
}

/// Converts a Lighter base-amount tick count into a Nautilus [`Quantity`].
///
/// Order sizes on the wire are signed `i64` multiples of `10^-decimals`
/// base-asset units. Nautilus [`Quantity`] is non-negative, so a negative
/// `ticks` value is rejected: callers (e.g. position parsers) extract the
/// sign separately before invoking this helper.
///
/// Conversion routes through [`Decimal`] and [`Quantity::from_decimal_dp`]
/// so out-of-range tick counts return an error rather than panicking inside
/// the unchecked mantissa-exponent constructor.
///
/// # Errors
///
/// Returns an error if `decimals` exceeds [`MAX_DECIMALS`], if `ticks` is
/// negative, or if the resulting value exceeds the [`Quantity`] range.
pub fn parse_quantity_from_ticks(ticks: i64, decimals: u8) -> anyhow::Result<Quantity> {
    anyhow::ensure!(
        decimals <= MAX_DECIMALS,
        "size decimals {decimals} exceeds maximum {MAX_DECIMALS}",
    );
    anyhow::ensure!(ticks >= 0, "negative tick count {ticks} for Quantity");
    let decimal = Decimal::new(ticks, u32::from(decimals));
    Quantity::from_decimal_dp(decimal, decimals).map_err(|e| {
        anyhow::anyhow!("Quantity overflow for ticks={ticks}, decimals={decimals}: {e}")
    })
}

/// Converts a decimal string into a Nautilus [`Price`] at the requested precision.
///
/// # Errors
///
/// Returns an error if the string is not a decimal, if `precision` exceeds
/// [`MAX_DECIMALS`], or if the resulting value is out of range.
pub fn parse_price(value: &str, precision: u8) -> anyhow::Result<Price> {
    anyhow::ensure!(
        precision <= MAX_DECIMALS,
        "price precision {precision} exceeds maximum {MAX_DECIMALS}",
    );
    let decimal =
        Decimal::from_str(value).map_err(|e| anyhow::anyhow!("invalid price `{value}`: {e}"))?;
    Price::from_decimal_dp(decimal, precision)
        .map_err(|e| anyhow::anyhow!("invalid price `{value}` at precision {precision}: {e}"))
}

/// Converts a decimal string into a non-negative Nautilus [`Quantity`].
///
/// Zero is allowed because Lighter sends zero-size book levels to delete
/// existing orders.
///
/// # Errors
///
/// Returns an error if the string is not a decimal, if `precision` exceeds
/// [`MAX_DECIMALS`], if the value is negative, or if the resulting quantity
/// is out of range.
pub fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    anyhow::ensure!(
        precision <= MAX_DECIMALS,
        "size precision {precision} exceeds maximum {MAX_DECIMALS}",
    );
    let decimal =
        Decimal::from_str(value).map_err(|e| anyhow::anyhow!("invalid quantity `{value}`: {e}"))?;
    anyhow::ensure!(decimal.is_sign_positive(), "negative quantity `{value}`");
    Quantity::from_decimal_dp(decimal, precision)
        .map_err(|e| anyhow::anyhow!("invalid quantity `{value}` at precision {precision}: {e}"))
}

/// Converts a [`Decimal`] into a Nautilus [`Price`] at the requested precision.
///
/// Use this when the wire value has already been deserialized as a [`Decimal`]
/// (the standard pattern for model fields tagged with `deserialize_decimal`).
///
/// # Errors
///
/// Returns an error if `precision` exceeds [`MAX_DECIMALS`] or if the value
/// is out of [`Price`] range.
pub fn price_from_decimal(value: Decimal, precision: u8) -> anyhow::Result<Price> {
    anyhow::ensure!(
        precision <= MAX_DECIMALS,
        "price precision {precision} exceeds maximum {MAX_DECIMALS}",
    );
    Price::from_decimal_dp(value, precision)
        .map_err(|e| anyhow::anyhow!("invalid price `{value}` at precision {precision}: {e}"))
}

/// Converts a [`Decimal`] into a non-negative Nautilus [`Quantity`] at the
/// requested precision.
///
/// Zero is allowed because Lighter sends zero-size book levels to delete
/// existing orders.
///
/// # Errors
///
/// Returns an error if `precision` exceeds [`MAX_DECIMALS`], if the value is
/// negative, or if the resulting quantity is out of range.
pub fn quantity_from_decimal(value: Decimal, precision: u8) -> anyhow::Result<Quantity> {
    anyhow::ensure!(
        precision <= MAX_DECIMALS,
        "size precision {precision} exceeds maximum {MAX_DECIMALS}",
    );
    anyhow::ensure!(value.is_sign_positive(), "negative quantity `{value}`");
    Quantity::from_decimal_dp(value, precision)
        .map_err(|e| anyhow::anyhow!("invalid quantity `{value}` at precision {precision}: {e}"))
}

/// Converts a Unix millisecond timestamp into [`UnixNanos`].
///
/// # Errors
///
/// Returns an error if `millis * 1_000_000` would overflow `u64`. Realistic
/// venue timestamps are nowhere near this bound; the check rejects malformed
/// payloads instead of silently wrapping in release builds.
pub fn parse_millis_to_nanos(millis: u64) -> anyhow::Result<UnixNanos> {
    let nanos = millis
        .checked_mul(NANOSECONDS_IN_MILLISECOND)
        .ok_or_else(|| {
            anyhow::anyhow!("millisecond timestamp {millis} overflows when scaled to nanoseconds")
        })?;
    Ok(UnixNanos::from(nanos))
}

/// Converts a Unix microsecond timestamp into [`UnixNanos`].
///
/// # Errors
///
/// Returns an error if `micros * 1_000` would overflow `u64`.
pub fn parse_micros_to_nanos(micros: u64) -> anyhow::Result<UnixNanos> {
    let nanos = micros.checked_mul(1_000).ok_or_else(|| {
        anyhow::anyhow!("microsecond timestamp {micros} overflows when scaled to nanoseconds")
    })?;
    Ok(UnixNanos::from(nanos))
}

/// Converts a Unix second timestamp into [`UnixNanos`].
///
/// # Errors
///
/// Returns an error if `secs * 1_000_000_000` would overflow `u64`.
pub fn parse_secs_to_nanos(secs: u64) -> anyhow::Result<UnixNanos> {
    let nanos = secs.checked_mul(NANOSECONDS_IN_SECOND).ok_or_else(|| {
        anyhow::anyhow!("second timestamp {secs} overflows when scaled to nanoseconds")
    })?;
    Ok(UnixNanos::from(nanos))
}

/// Converts a signed Unix millisecond timestamp into [`UnixNanos`].
///
/// Negative inputs are mapped to `Ok(None)` so callers can model fields
/// where the wire uses `-1` (or any negative sentinel) as "absent". Fields
/// that overload `0` with a separate meaning (e.g. `0` for IOC on
/// `OrderInfo::order_expiry`) must apply that interpretation at the call
/// site; this helper treats `0` as a literal Unix epoch timestamp.
///
/// # Errors
///
/// Returns an error if a non-negative `millis` overflows when scaled to
/// nanoseconds.
pub fn parse_optional_millis_to_nanos(millis: i64) -> anyhow::Result<Option<UnixNanos>> {
    if millis < 0 {
        Ok(None)
    } else {
        parse_millis_to_nanos(millis as u64).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn parse_price_zero_decimals_is_integer() {
        let price = parse_price_from_ticks(42, 0).unwrap();
        assert_eq!(price.precision, 0);
        assert_eq!(price.to_string(), "42");
    }

    #[rstest]
    fn parse_price_with_decimals_inserts_decimal_point() {
        // 405_000 ticks at 2 decimals = $4_050.00
        let price = parse_price_from_ticks(405_000, 2).unwrap();
        assert_eq!(price.precision, 2);
        assert_eq!(price.to_string(), "4050.00");
    }

    #[rstest]
    fn parse_price_at_max_decimals() {
        let price = parse_price_from_ticks(1, MAX_DECIMALS).unwrap();
        assert_eq!(price.precision, MAX_DECIMALS);
    }

    #[rstest]
    fn parse_price_rejects_decimals_above_max() {
        let err = parse_price_from_ticks(1, MAX_DECIMALS + 1).unwrap_err();
        assert!(err.to_string().contains("exceeds maximum"));
    }

    #[rstest]
    fn parse_quantity_with_decimals_inserts_decimal_point() {
        // 1_000 ticks at 3 decimals = 1.000
        let qty = parse_quantity_from_ticks(1_000, 3).unwrap();
        assert_eq!(qty.precision, 3);
        assert_eq!(qty.to_string(), "1.000");
    }

    #[rstest]
    fn parse_quantity_rejects_negative_ticks() {
        let err = parse_quantity_from_ticks(-1, 2).unwrap_err();
        assert!(err.to_string().contains("negative tick count"));
    }

    #[rstest]
    fn parse_quantity_rejects_oversized_ticks() {
        // i64::MAX at 0 decimals exceeds the Nautilus Quantity range.
        let err = parse_quantity_from_ticks(i64::MAX, 0).unwrap_err();
        assert!(err.to_string().contains("Quantity overflow"));
    }

    #[rstest]
    fn parse_quantity_zero_is_valid() {
        let qty = parse_quantity_from_ticks(0, 4).unwrap();
        assert_eq!(qty.as_f64(), 0.0);
        assert_eq!(qty.precision, 4);
    }

    #[rstest]
    fn parse_price_from_decimal_string() {
        let price = parse_price("2352.73", 2).unwrap();
        assert_eq!(price.to_string(), "2352.73");
        assert_eq!(price.precision, 2);
    }

    #[rstest]
    fn parse_price_rejects_invalid_decimal() {
        let err = parse_price("not-a-price", 2).unwrap_err();
        assert!(err.to_string().contains("invalid price"));
    }

    #[rstest]
    fn parse_quantity_from_decimal_string() {
        let quantity = parse_quantity("0.1336", 4).unwrap();
        assert_eq!(quantity.to_string(), "0.1336");
        assert_eq!(quantity.precision, 4);
    }

    #[rstest]
    fn parse_quantity_rejects_negative_decimal_string() {
        let err = parse_quantity("-0.1", 4).unwrap_err();
        assert!(err.to_string().contains("negative quantity"));
    }

    #[rstest]
    fn parse_millis_to_nanos_scales_correctly() {
        assert_eq!(parse_millis_to_nanos(0).unwrap(), UnixNanos::from(0));
        assert_eq!(
            parse_millis_to_nanos(1).unwrap(),
            UnixNanos::from(1_000_000),
        );
        assert_eq!(
            parse_millis_to_nanos(1_700_000_000_000).unwrap(),
            UnixNanos::from(1_700_000_000_000_000_000),
        );
    }

    #[rstest]
    fn parse_millis_to_nanos_rejects_overflow() {
        let err = parse_millis_to_nanos(u64::MAX).unwrap_err();
        assert!(err.to_string().contains("overflows"));
    }

    #[rstest]
    fn parse_micros_to_nanos_scales_correctly() {
        assert_eq!(parse_micros_to_nanos(0).unwrap(), UnixNanos::from(0));
        assert_eq!(parse_micros_to_nanos(1).unwrap(), UnixNanos::from(1_000));
        assert_eq!(
            parse_micros_to_nanos(1_700_000_000_000_000).unwrap(),
            UnixNanos::from(1_700_000_000_000_000_000),
        );
    }

    #[rstest]
    fn parse_micros_to_nanos_rejects_overflow() {
        let err = parse_micros_to_nanos(u64::MAX).unwrap_err();
        assert!(err.to_string().contains("overflows"));
    }

    #[rstest]
    fn parse_secs_to_nanos_scales_correctly() {
        assert_eq!(parse_secs_to_nanos(0).unwrap(), UnixNanos::from(0));
        assert_eq!(
            parse_secs_to_nanos(1).unwrap(),
            UnixNanos::from(1_000_000_000),
        );
    }

    #[rstest]
    fn parse_secs_to_nanos_rejects_overflow() {
        let err = parse_secs_to_nanos(u64::MAX).unwrap_err();
        assert!(err.to_string().contains("overflows"));
    }

    #[rstest]
    fn parse_optional_millis_returns_none_for_negative() {
        assert!(parse_optional_millis_to_nanos(-1).unwrap().is_none());
        assert!(parse_optional_millis_to_nanos(i64::MIN).unwrap().is_none());
    }

    #[rstest]
    fn parse_optional_millis_returns_some_for_non_negative() {
        assert_eq!(
            parse_optional_millis_to_nanos(0).unwrap(),
            Some(UnixNanos::from(0)),
        );
        assert_eq!(
            parse_optional_millis_to_nanos(1_500).unwrap(),
            Some(UnixNanos::from(1_500_000_000)),
        );
    }

    #[rstest]
    fn parse_optional_millis_propagates_overflow() {
        let err = parse_optional_millis_to_nanos(i64::MAX).unwrap_err();
        assert!(err.to_string().contains("overflows"));
    }
}
