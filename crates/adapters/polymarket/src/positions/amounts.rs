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

//! Exact pUSD amount conversion for Conditional Token operations.

use alloy_primitives::U256;
use rust_decimal::Decimal;

use crate::{
    common::consts::USDC_DECIMALS,
    http::error::{Error, Result},
};

const PUSD_SCALE: u32 = USDC_DECIMALS;

/// Converts an exact pUSD amount into six-decimal base units.
///
/// # Errors
///
/// Returns an error if `amount` is not positive or is not exactly representable
/// at six decimal places.
pub fn pusd_to_base_units(amount: Decimal) -> Result<U256> {
    if amount.is_sign_negative() || amount.is_zero() {
        return Err(Error::bad_request(format!(
            "pUSD amount must be positive, was {amount}"
        )));
    }

    let scale = Decimal::from(10u32.pow(PUSD_SCALE));

    let scaled = amount.checked_mul(scale).ok_or_else(|| {
        Error::bad_request(format!(
            "pUSD amount overflowed six-decimal conversion, was {amount}"
        ))
    })?;

    let normalized = scaled.normalize();
    if normalized.scale() != 0 {
        return Err(Error::bad_request(format!(
            "pUSD amount must be exactly representable at {PUSD_SCALE} decimal places, was {amount}"
        )));
    }

    let mantissa = normalized.mantissa();
    u128::try_from(mantissa)
        .map(U256::from)
        .map_err(|_| Error::bad_request(format!("pUSD amount overflowed base units, was {amount}")))
}

/// Converts six-decimal pUSD base units back to a decimal amount.
///
/// # Errors
///
/// Returns an error if `base_units` cannot be represented as a decimal at six
/// decimal places.
pub fn base_units_to_pusd(base_units: U256) -> Result<Decimal> {
    let mut mantissa = base_units;
    let mut scale = PUSD_SCALE;
    let ten = U256::from(10u8);
    while scale > 0 && mantissa % ten == U256::ZERO {
        mantissa /= ten;
        scale -= 1;
    }

    let mantissa = i128::try_from(mantissa).map_err(|_| {
        Error::bad_request(format!(
            "pUSD base units overflowed decimal conversion, was {base_units}"
        ))
    })?;

    Decimal::try_from_i128_with_scale(mantissa, scale).map_err(|_| {
        Error::bad_request(format!(
            "pUSD base units overflowed decimal conversion, was {base_units}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    #[case(dec!(1), 1_000_000u64)]
    #[case(dec!(1.5), 1_500_000u64)]
    #[case(dec!(0.000001), 1u64)]
    #[case(dec!(1.000000), 1_000_000u64)]
    fn test_pusd_to_base_units_exact(#[case] amount: Decimal, #[case] expected: u64) {
        assert_eq!(pusd_to_base_units(amount).unwrap(), U256::from(expected));
    }

    #[rstest]
    #[case(dec!(0))]
    #[case(dec!(-1))]
    #[case(dec!(-0.000001))]
    fn test_pusd_to_base_units_rejects_non_positive(#[case] amount: Decimal) {
        let err = pusd_to_base_units(amount).unwrap_err();
        assert!(err.to_string().contains("pUSD amount must be positive"));
        assert!(err.to_string().contains(&format!("was {amount}")));
    }

    #[rstest]
    fn test_pusd_to_base_units_rejects_excess_precision() {
        let amount = dec!(1.0000001);
        let err = pusd_to_base_units(amount).unwrap_err();
        assert!(
            err.to_string()
                .contains("exactly representable at 6 decimal places")
        );
        assert!(err.to_string().contains("was 1.0000001"));
    }

    #[rstest]
    #[case(U256::from(1u128 << 96))]
    #[case(U256::from(u128::MAX))]
    #[case(U256::MAX)]
    fn test_base_units_to_pusd_rejects_overflow(#[case] amount: U256) {
        assert!(base_units_to_pusd(amount).is_err());
    }

    #[rstest]
    fn test_base_units_to_pusd_decimal_boundaries() {
        let max = U256::from((1u128 << 96) - 1);
        assert_eq!(
            base_units_to_pusd(max).unwrap(),
            dec!(79228162514264337593543.950335)
        );
        assert_eq!(
            base_units_to_pusd(max * U256::from(1_000_000u64)).unwrap(),
            Decimal::MAX
        );
        assert_eq!(base_units_to_pusd(U256::ZERO).unwrap(), Decimal::ZERO);
    }

    #[rstest]
    fn test_base_units_to_pusd_round_trip() {
        let amount = dec!(12.345678);
        let base_units = pusd_to_base_units(amount).unwrap();
        assert_eq!(base_units_to_pusd(base_units).unwrap(), amount);
    }
}
