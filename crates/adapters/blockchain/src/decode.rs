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

use alloy::primitives::U256;
use nautilus_model::types::{
    fixed::FIXED_PRECISION,
    price::{PRICE_RAW_MAX, Price, PriceRaw},
    quantity::{QUANTITY_RAW_MAX, Quantity, QuantityRaw},
};

/// Convert a `U256` amount to [`Quantity`].
///
/// - If `decimals == 18`, the value represents wei and uses the dedicated lossless
///   `Quantity::from_wei` constructor.
/// - Other precisions use checked integer scaling and clamp `decimals` to
///   [`FIXED_PRECISION`]. Discarded source digits are rounded half to even.
///
/// # Errors
///
/// Returns an error if scaling overflows or the result exceeds [`QUANTITY_RAW_MAX`].
pub fn u256_to_quantity(amount: U256, decimals: u8) -> anyhow::Result<Quantity> {
    if decimals == 18 {
        check_raw_range(
            amount,
            U256::from(QUANTITY_RAW_MAX),
            "Quantity",
            "QUANTITY_RAW_MAX",
        )?;
        return Ok(Quantity::from_wei(amount));
    }

    let precision = decimals.min(FIXED_PRECISION);
    let raw = scale_u256_to_raw(
        amount,
        decimals,
        FIXED_PRECISION,
        U256::from(QUANTITY_RAW_MAX),
        "Quantity",
        "QUANTITY_RAW_MAX",
    )?;
    let raw = QuantityRaw::try_from(raw)
        .map_err(|e| anyhow::anyhow!("Failed to convert Quantity raw value: {e}"))?;
    Ok(Quantity::from_raw_checked(raw, precision)?)
}

/// Convert a `U256` amount to [`Price`].
///
/// - If `decimals == 18`, the value represents wei and uses the dedicated lossless
///   `Price::from_wei` constructor.
/// - Other precisions use checked integer scaling and clamp `decimals` to
///   [`FIXED_PRECISION`]. Discarded source digits are rounded half to even.
///
/// # Errors
///
/// Returns an error if scaling overflows or the result exceeds [`PRICE_RAW_MAX`].
pub fn u256_to_price(amount: U256, decimals: u8) -> anyhow::Result<Price> {
    if decimals == 18 {
        check_raw_range(amount, U256::from(PRICE_RAW_MAX), "Price", "PRICE_RAW_MAX")?;
        return Ok(Price::from_wei(amount));
    }

    let precision = decimals.min(FIXED_PRECISION);
    let raw = scale_u256_to_raw(
        amount,
        decimals,
        FIXED_PRECISION,
        U256::from(PRICE_RAW_MAX),
        "Price",
        "PRICE_RAW_MAX",
    )?;
    let raw = PriceRaw::try_from(raw)
        .map_err(|e| anyhow::anyhow!("Failed to convert Price raw value: {e}"))?;
    Ok(Price::from_raw_checked(raw, precision)?)
}

fn scale_u256_to_raw(
    amount: U256,
    decimals: u8,
    fixed_precision: u8,
    raw_max: U256,
    type_name: &str,
    raw_max_name: &str,
) -> anyhow::Result<U256> {
    let raw = if decimals < fixed_precision {
        let scale = U256::from(10)
            .checked_pow(U256::from(fixed_precision - decimals))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Scale 10^{} exceeds U256 while converting {type_name}",
                    fixed_precision - decimals
                )
            })?;
        amount.checked_mul(scale).ok_or_else(|| {
            anyhow::anyhow!(
                "{type_name} amount {amount} overflows U256 while scaling from {decimals} to {fixed_precision} decimals"
            )
        })?
    } else if decimals > fixed_precision {
        round_u256_half_even(amount, decimals - fixed_precision, type_name)?
    } else {
        amount
    };

    check_raw_range(raw, raw_max, type_name, raw_max_name)?;
    Ok(raw)
}

fn check_raw_range(
    raw: U256,
    raw_max: U256,
    type_name: &str,
    raw_max_name: &str,
) -> anyhow::Result<()> {
    if raw > raw_max {
        anyhow::bail!("{type_name} raw value {raw} exceeds {raw_max_name}={raw_max}");
    }

    Ok(())
}

fn round_u256_half_even(amount: U256, excess: u8, type_name: &str) -> anyhow::Result<U256> {
    let Some(divisor) = U256::from(10).checked_pow(U256::from(excess)) else {
        // The divisor exceeds U256::MAX, so every U256 amount is below half a retained unit
        return Ok(U256::ZERO);
    };
    let quotient = amount / divisor;
    let remainder = amount % divisor;
    let half = divisor / U256::from(2);

    if remainder > half || (remainder == half && quotient.bit(0)) {
        quotient.checked_add(U256::from(1)).ok_or_else(|| {
            anyhow::anyhow!("{type_name} raw value overflows U256 while rounding half to even")
        })
    } else {
        Ok(quotient)
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::U256;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::zero(U256::ZERO, 6, 0, 6)]
    #[case::one(U256::from(1), 6, 10_000_000_000, 6)]
    #[case::above_f64_integer_limit(
        U256::from(9_007_199_254_740_993_u64),
        6,
        90_071_992_547_409_930_000_000_000,
        6
    )]
    #[case::precision_zero(U256::from(37), 0, 370_000_000_000_000_000, 0)]
    #[case::half_even_down(U256::from(25), 17, 2, FIXED_PRECISION)]
    #[case::half_even_up(U256::from(35), 17, 4, FIXED_PRECISION)]
    #[case::below_retained_unit(U256::MAX, u8::MAX, 0, FIXED_PRECISION)]
    fn test_u256_conversions_store_exact_raw_values(
        #[case] amount: U256,
        #[case] decimals: u8,
        #[case] expected_raw: u128,
        #[case] expected_precision: u8,
    ) {
        let quantity = u256_to_quantity(amount, decimals).unwrap();
        let price = u256_to_price(amount, decimals).unwrap();

        assert_eq!(quantity.raw, expected_raw);
        assert_eq!(quantity.precision, expected_precision);
        assert_eq!(price.raw, expected_raw.cast_signed());
        assert_eq!(price.precision, expected_precision);
    }

    #[rstest]
    fn test_u256_conversions_preserve_wei_raw_values() {
        let amount = U256::from(9_007_199_254_740_993_u64);

        let quantity = u256_to_quantity(amount, 18).unwrap();
        let price = u256_to_price(amount, 18).unwrap();

        assert_eq!(quantity.raw, 9_007_199_254_740_993);
        assert_eq!(quantity.precision, 18);
        assert_eq!(price.raw, 9_007_199_254_740_993);
        assert_eq!(price.precision, 18);
    }

    #[rstest]
    fn test_u256_conversions_accept_domain_raw_maximums() {
        let quantity = u256_to_quantity(U256::from(QUANTITY_RAW_MAX), FIXED_PRECISION).unwrap();
        let price = u256_to_price(U256::from(PRICE_RAW_MAX), FIXED_PRECISION).unwrap();

        assert_eq!(quantity.raw, QUANTITY_RAW_MAX);
        assert_eq!(quantity.precision, FIXED_PRECISION);
        assert_eq!(price.raw, PRICE_RAW_MAX);
        assert_eq!(price.precision, FIXED_PRECISION);
    }

    #[rstest]
    fn test_u256_conversions_reject_domain_raw_overflow() {
        let quantity_raw = U256::from(QUANTITY_RAW_MAX) + U256::from(1);
        let price_raw = U256::from(PRICE_RAW_MAX) + U256::from(1);

        let quantity_error = u256_to_quantity(quantity_raw, FIXED_PRECISION).unwrap_err();
        let price_error = u256_to_price(price_raw, FIXED_PRECISION).unwrap_err();

        assert_eq!(
            quantity_error.to_string(),
            format!(
                "Quantity raw value {quantity_raw} exceeds QUANTITY_RAW_MAX={QUANTITY_RAW_MAX}"
            )
        );
        assert_eq!(
            price_error.to_string(),
            format!("Price raw value {price_raw} exceeds PRICE_RAW_MAX={PRICE_RAW_MAX}")
        );
    }

    #[rstest]
    fn test_u256_conversions_reject_scaling_overflow() {
        let quantity_error = u256_to_quantity(U256::MAX, 0).unwrap_err();
        let price_error = u256_to_price(U256::MAX, 0).unwrap_err();

        assert_eq!(
            quantity_error.to_string(),
            format!(
                "Quantity amount {} overflows U256 while scaling from 0 to {FIXED_PRECISION} decimals",
                U256::MAX
            )
        );
        assert_eq!(
            price_error.to_string(),
            format!(
                "Price amount {} overflows U256 while scaling from 0 to {FIXED_PRECISION} decimals",
                U256::MAX
            )
        );
    }

    #[rstest]
    fn test_u256_conversions_reject_wei_overflow() {
        let quantity_raw = U256::from(QUANTITY_RAW_MAX) + U256::from(1);
        let price_raw = U256::from(PRICE_RAW_MAX) + U256::from(1);

        let quantity_error = u256_to_quantity(quantity_raw, 18).unwrap_err();
        let price_error = u256_to_price(price_raw, 18).unwrap_err();

        assert_eq!(
            quantity_error.to_string(),
            format!(
                "Quantity raw value {quantity_raw} exceeds QUANTITY_RAW_MAX={QUANTITY_RAW_MAX}"
            )
        );
        assert_eq!(
            price_error.to_string(),
            format!("Price raw value {price_raw} exceeds PRICE_RAW_MAX={PRICE_RAW_MAX}")
        );
    }

    #[rstest]
    #[case::standard(9, 9_007_199_254_740_993_000_u128)]
    #[case::high(16, 90_071_992_547_409_930_000_000_000_u128)]
    fn test_scaling_at_fixed_precision(#[case] fixed_precision: u8, #[case] expected_raw: u128) {
        let amount = U256::from(9_007_199_254_740_993_u64);

        let raw =
            scale_u256_to_raw(amount, 6, fixed_precision, U256::MAX, "Domain", "RAW_MAX").unwrap();

        assert_eq!(raw, U256::from(expected_raw));
    }
}
