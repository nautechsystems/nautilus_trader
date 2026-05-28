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

//! Shared signing utilities.

use std::time::{SystemTime, UNIX_EPOCH};

use alloy_primitives::{Address, B256, I256, U256};
use rust_decimal::Decimal;
use thiserror::Error;

use crate::common::consts::DECIMAL_SCALE;

/// Errors raised by [`parse_address_const`] / [`parse_b256_const`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HexConstError {
    /// The constant still holds the `<paste_from_docs.derive.xyz_*>` placeholder.
    #[error(
        "{name} is a placeholder; replace with the value from Protocol Constants at https://docs.derive.xyz before signing"
    )]
    Placeholder {
        /// Constant name surfaced in the error message.
        name: &'static str,
    },
    /// The constant is not a valid 0x-prefixed hex string of the expected length.
    #[error("{name} is not valid {kind} hex: {message}")]
    InvalidHex {
        /// Constant name.
        name: &'static str,
        /// Expected encoding label.
        kind: &'static str,
        /// Underlying parse error.
        message: String,
    },
}

/// Returns the current UNIX time in milliseconds.
///
/// # Errors
///
/// Returns [`SystemTimeError`](std::time::SystemTimeError) if the system clock
/// is before the UNIX epoch.
pub fn utc_now_ms() -> Result<u64, std::time::SystemTimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
}

/// Scales a [`Decimal`] amount to a 1e18 fixed-point [`I256`].
///
/// Mirrors `derive_action_signing/utils.py::decimal_to_big_int`. Negative
/// amounts are supported because the venue uses signed integers for fields
/// like `limit_price` and `amount` (sells are encoded as positive amounts but
/// other action variants use signed magnitudes).
///
/// # Errors
///
/// Returns [`HexConstError`] is not actually used here; we return a plain
/// `&'static str` describing overflow when the scaled value exceeds the
/// signed-256-bit range.
pub fn decimal_to_scaled_i256(value: Decimal) -> Result<I256, &'static str> {
    let scaled = value
        .checked_mul(Decimal::from(DECIMAL_SCALE))
        .ok_or("decimal scaling overflow before truncation")?;
    let truncated = scaled.trunc();
    let mantissa_str = truncated.to_string();
    I256::from_dec_str(&mantissa_str).map_err(|_| "scaled decimal exceeds signed 256-bit range")
}

/// Scales a [`Decimal`] amount to a 1e18 fixed-point [`U256`].
///
/// Used for unsigned fields like `max_fee` where negative amounts are not
/// meaningful and the venue rejects negative encodings.
///
/// # Errors
///
/// Returns an error string if the value is negative or exceeds the unsigned
/// 256-bit range after scaling.
pub fn decimal_to_scaled_u256(value: Decimal) -> Result<U256, &'static str> {
    if value.is_sign_negative() {
        return Err("unsigned scaled decimal must be non-negative");
    }
    let scaled = value
        .checked_mul(Decimal::from(DECIMAL_SCALE))
        .ok_or("decimal scaling overflow before truncation")?;
    let truncated = scaled.trunc();
    let mantissa_str = truncated.to_string();
    U256::from_str_radix(&mantissa_str, 10)
        .map_err(|_| "scaled decimal exceeds unsigned 256-bit range")
}

/// Parses a `0x`-prefixed 20-byte address constant, surfacing a clear error
/// if the placeholder marker `<paste_` is still present.
///
/// # Errors
///
/// Returns [`HexConstError::Placeholder`] if `value` still contains the
/// `<paste_` marker, or [`HexConstError::InvalidHex`] if it is not valid
/// 20-byte hex.
pub fn parse_address_const(value: &str, name: &'static str) -> Result<Address, HexConstError> {
    if value.contains("<paste_") {
        return Err(HexConstError::Placeholder { name });
    }
    value
        .parse::<Address>()
        .map_err(|e| HexConstError::InvalidHex {
            name,
            kind: "20-byte address",
            message: e.to_string(),
        })
}

/// Parses a `0x`-prefixed 32-byte hash constant, surfacing a clear error if
/// the placeholder marker `<paste_` is still present.
///
/// # Errors
///
/// Returns [`HexConstError::Placeholder`] if `value` still contains the
/// `<paste_` marker, or [`HexConstError::InvalidHex`] if it is not valid
/// 32-byte hex.
pub fn parse_b256_const(value: &str, name: &'static str) -> Result<B256, HexConstError> {
    if value.contains("<paste_") {
        return Err(HexConstError::Placeholder { name });
    }
    value
        .parse::<B256>()
        .map_err(|e| HexConstError::InvalidHex {
            name,
            kind: "32-byte hash",
            message: e.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_decimal_to_scaled_i256_one_unit() {
        let scaled = decimal_to_scaled_i256(dec!(1)).unwrap();
        assert_eq!(scaled, I256::try_from(DECIMAL_SCALE).unwrap());
    }

    #[rstest]
    fn test_decimal_to_scaled_i256_handles_fractional() {
        let scaled = decimal_to_scaled_i256(dec!(0.5)).unwrap();
        let expected = I256::try_from(DECIMAL_SCALE / 2).unwrap();
        assert_eq!(scaled, expected);
    }

    #[rstest]
    fn test_decimal_to_scaled_i256_handles_negative() {
        let scaled = decimal_to_scaled_i256(dec!(-2)).unwrap();
        let expected = I256::try_from(DECIMAL_SCALE).unwrap() * I256::try_from(-2).unwrap();
        assert_eq!(scaled, expected);
    }

    #[rstest]
    fn test_decimal_to_scaled_u256_one_unit() {
        let scaled = decimal_to_scaled_u256(dec!(1)).unwrap();
        assert_eq!(scaled, U256::from(DECIMAL_SCALE));
    }

    #[rstest]
    fn test_decimal_to_scaled_u256_rejects_negative() {
        let err = decimal_to_scaled_u256(dec!(-1)).expect_err("must reject negative");
        assert!(err.contains("non-negative"));
    }

    #[rstest]
    fn test_parse_address_const_rejects_placeholder() {
        let err = parse_address_const(
            "0x<paste_from_docs.derive.xyz_protocol_constants>",
            "TRADE_MODULE_ADDRESS_MAINNET",
        )
        .expect_err("must reject placeholder");
        assert_eq!(
            err,
            HexConstError::Placeholder {
                name: "TRADE_MODULE_ADDRESS_MAINNET"
            }
        );
    }

    #[rstest]
    fn test_parse_address_const_accepts_valid_hex() {
        let addr =
            parse_address_const("0x0000000000000000000000000000000000001234", "TEST").unwrap();
        assert_eq!(
            format!("{addr:?}"),
            "0x0000000000000000000000000000000000001234"
        );
    }

    #[rstest]
    fn test_parse_b256_const_rejects_placeholder() {
        let err = parse_b256_const(
            "0x<paste_from_docs.derive.xyz_protocol_constants>",
            "ACTION_TYPEHASH",
        )
        .expect_err("must reject placeholder");
        assert_eq!(
            err,
            HexConstError::Placeholder {
                name: "ACTION_TYPEHASH"
            }
        );
    }

    #[rstest]
    fn test_parse_b256_const_accepts_valid_hex() {
        let value = "0x000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        let hash = parse_b256_const(value, "TEST").unwrap();
        assert_eq!(hash.0[0], 0x00);
        assert_eq!(hash.0[31], 0x1f);
    }

    #[rstest]
    fn test_utc_now_ms_returns_thirteen_digit_value() {
        let now = utc_now_ms().unwrap();
        // ~Jan 2026 is past 1.7e12 ms; well into 13-digit territory.
        assert!(now > 1_700_000_000_000, "ms timestamp too small: {now}");
    }
}
