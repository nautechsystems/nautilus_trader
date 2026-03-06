use alloy::primitives::U256;
use nautilus_model::types::{fixed::FIXED_PRECISION, price::Price, quantity::Quantity};

use crate::math::convert_u256_to_f64;

/// Convert a `U256` amount to [`Quantity`].
///
/// - If `decimals == 18` the value represents wei and we leverage the dedicated
///   `Price::from_wei` constructor for loss-less conversion.
/// - For other precisions we fall back to a floating-point conversion identical
///   to the pre-existing path in `convert_u256_to_f64` and then construct a
///   `Quantity` with the smaller `decimals` (clamped to `FIXED_PRECISION`).
///
/// # Errors
///
/// Returns an error when the helper must fall back to the floating-point path
/// (i.e. `decimals != 18`) and the provided `amount` cannot be converted to an
/// `f64` (see `convert_u256_to_f64`).
pub fn u256_to_quantity(amount: U256, decimals: u8) -> anyhow::Result<Quantity> {
    if decimals == 18 {
        return Ok(Quantity::from_wei(amount));
    }

    let value = convert_u256_to_f64(amount, decimals)?;
    let precision = decimals.min(FIXED_PRECISION);
    Ok(Quantity::new(value, precision))
}

/// Convert a `U256` amount to [`Price`].
///
/// - If `decimals == 18` the value represents wei and we leverage the dedicated
///   `Quantity::from_wei` constructor for loss-less conversion.
/// - For other precisions we fall back to a floating-point conversion identical
///   to the pre-existing path in `convert_u256_to_f64` and then construct a
///   `Quantity` with the smaller `decimals` (clamped to `FIXED_PRECISION`).
///
/// # Errors
///
/// Returns an error when the helper must fall back to the floating-point path
/// (i.e. `decimals != 18`) and the provided `amount` cannot be converted to an
/// `f64` (see `convert_u256_to_f64`).
pub fn u256_to_price(amount: U256, decimals: u8) -> anyhow::Result<Price> {
    if decimals == 18 {
        return Ok(Price::from_wei(amount));
    }

    let value = convert_u256_to_f64(amount, decimals)?;
    let precision = decimals.min(FIXED_PRECISION);
    Ok(Price::new(value, precision))
}

#[cfg(test)]
mod tests {
    use alloy::primitives::U256;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_quantity_from_wei() {
        let wei = U256::from(1_000_000_000_000_000_000u128); // 1 * 10^18
        let q = u256_to_quantity(wei, 18).unwrap();
        assert_eq!(q.precision, 18);
        assert_eq!(q.as_wei(), wei);
    }

    #[rstest]
    fn test_quantity_from_small_decimals() {
        let raw = U256::from(1_500_000u128); // 1.5 with 6 decimals
        let q = u256_to_quantity(raw, 6).unwrap();
        assert_eq!(q.precision, 6.min(FIXED_PRECISION));
        assert_eq!(q.to_string(), "1.500000");
    }

    #[rstest]
    fn test_price_from_wei() {
        let wei = U256::from(2_000_000_000_000_000_000u128); // 2 ETH
        let p = u256_to_price(wei, 18).unwrap();
        assert_eq!(p.precision, 18);
        assert_eq!(p.as_wei(), wei);
    }

    #[rstest]
    fn test_price_precision_clamp() {
        let value = U256::from(10_000_000_000u128); // 10 with 9 decimals
        // Request unrealistic 20-dec precision → should clamp to FIXED_PRECISION (16 or 9)
        let p = u256_to_price(value, 20).unwrap();
        assert_eq!(p.precision, FIXED_PRECISION);
    }
}
