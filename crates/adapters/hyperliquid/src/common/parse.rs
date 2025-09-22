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

use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serializer};

/// Serializes decimal as string (lossless, no scientific notation).
pub fn serialize_decimal_as_str<S>(decimal: &Decimal, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&decimal.normalize().to_string())
}

/// Deserializes decimal from string only (reject numbers to avoid precision loss).
pub fn deserialize_decimal_from_str<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Decimal::from_str(&s).map_err(serde::de::Error::custom)
}

/// Serialize optional decimal as string
pub fn serialize_optional_decimal_as_str<S>(
    decimal: &Option<Decimal>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match decimal {
        Some(d) => serializer.serialize_str(&d.normalize().to_string()),
        None => serializer.serialize_none(),
    }
}

/// Deserialize optional decimal from string
pub fn deserialize_optional_decimal_from_str<'de, D>(
    deserializer: D,
) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) => {
            let decimal = Decimal::from_str(&s).map_err(serde::de::Error::custom)?;
            Ok(Some(decimal))
        }
        None => Ok(None),
    }
}

////////////////////////////////////////////////////////////////////////////////
// Normalization and Validation Functions
////////////////////////////////////////////////////////////////////////////////

/// Round price down to the nearest valid tick size
#[inline]
pub fn round_down_to_tick(price: Decimal, tick_size: Decimal) -> Decimal {
    if tick_size.is_zero() {
        return price;
    }
    (price / tick_size).floor() * tick_size
}

/// Round quantity down to the nearest valid step size
#[inline]
pub fn round_down_to_step(qty: Decimal, step_size: Decimal) -> Decimal {
    if step_size.is_zero() {
        return qty;
    }
    (qty / step_size).floor() * step_size
}

/// Ensure the notional value meets minimum requirements
#[inline]
pub fn ensure_min_notional(
    price: Decimal,
    qty: Decimal,
    min_notional: Decimal,
) -> Result<(), String> {
    let notional = price * qty;
    if notional < min_notional {
        Err(format!(
            "Notional value {} is less than minimum required {}",
            notional, min_notional
        ))
    } else {
        Ok(())
    }
}

/// Normalize price to the specified number of decimal places
pub fn normalize_price(price: Decimal, decimals: u8) -> Decimal {
    let scale = Decimal::from(10_u64.pow(decimals as u32));
    (price * scale).floor() / scale
}

/// Normalize quantity to the specified number of decimal places
pub fn normalize_quantity(qty: Decimal, decimals: u8) -> Decimal {
    let scale = Decimal::from(10_u64.pow(decimals as u32));
    (qty * scale).floor() / scale
}

/// Complete normalization for an order including price, quantity, and notional validation
pub fn normalize_order(
    price: Decimal,
    qty: Decimal,
    tick_size: Decimal,
    step_size: Decimal,
    min_notional: Decimal,
    price_decimals: u8,
    size_decimals: u8,
) -> Result<(Decimal, Decimal), String> {
    // Normalize to decimal places first
    let normalized_price = normalize_price(price, price_decimals);
    let normalized_qty = normalize_quantity(qty, size_decimals);

    // Round down to tick/step sizes
    let final_price = round_down_to_tick(normalized_price, tick_size);
    let final_qty = round_down_to_step(normalized_qty, step_size);

    // Validate minimum notional
    ensure_min_notional(final_price, final_qty, min_notional)?;

    Ok((final_price, final_qty))
}

////////////////////////////////////////////////////////////////////////////////
// Risk Management and Validation
////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone)]
pub struct RiskLimits {
    pub max_leverage: Decimal,
    pub min_qty: Decimal,
    pub min_notional: Decimal,
    pub hedge_mode: bool, // if false = one-way
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_leverage: Decimal::from(100), // 100x default max leverage
            min_qty: Decimal::new(1, 4),      // 0.0001 using exact decimal constructor
            min_notional: Decimal::from(10),  // $10 minimum
            hedge_mode: true,                 // Allow hedge positions by default
        }
    }
}

impl RiskLimits {
    pub fn new(
        max_leverage: Decimal,
        min_qty: Decimal,
        min_notional: Decimal,
        hedge_mode: bool,
    ) -> Self {
        Self {
            max_leverage,
            min_qty,
            min_notional,
            hedge_mode,
        }
    }

    /// Create conservative risk limits
    ///
    /// # Panics
    ///
    /// This function will panic if the decimal values cannot be created from f64,
    /// which should not happen under normal circumstances.
    pub fn conservative() -> Self {
        Self {
            max_leverage: Decimal::from(10), // 10x max leverage
            min_qty: Decimal::new(1, 3),     // 0.001
            min_notional: Decimal::from(50), // $50 minimum
            hedge_mode: false,               // One-way only
        }
    }

    /// Create aggressive risk limits
    ///
    /// # Panics
    ///
    /// This function will panic if the decimal values cannot be created from f64,
    /// which should not happen under normal circumstances.
    pub fn aggressive() -> Self {
        Self {
            max_leverage: Decimal::from(200), // 200x max leverage
            min_qty: Decimal::new(1, 5),      // 0.00001
            min_notional: Decimal::from(1),   // $1 minimum
            hedge_mode: true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RiskViolation {
    #[error("Leverage {leverage} exceeds maximum allowed {max_leverage}")]
    ExcessiveLeverage {
        leverage: Decimal,
        max_leverage: Decimal,
    },
    #[error("Quantity {qty} is below minimum required {min_qty}")]
    InsufficientQuantity { qty: Decimal, min_qty: Decimal },
    #[error("Notional value {notional} is below minimum required {min_notional}")]
    InsufficientNotional {
        notional: Decimal,
        min_notional: Decimal,
    },
    #[error("Reduce-only order would increase position exposure")]
    ReduceOnlyViolation,
    #[error("Position mode violation: hedge mode is disabled")]
    PositionModeViolation,
}

/// Validate a limit order against risk parameters
pub fn validate_limit_order(
    price: Decimal,
    qty: Decimal,
    leverage: Decimal,
    current_position_qty: Decimal, // signed: positive = long, negative = short
    reduce_only: bool,
    limits: &RiskLimits,
) -> Result<(), RiskViolation> {
    // Check leverage limits
    if leverage > limits.max_leverage {
        return Err(RiskViolation::ExcessiveLeverage {
            leverage,
            max_leverage: limits.max_leverage,
        });
    }

    // Check minimum quantity
    let abs_qty = qty.abs();
    if abs_qty < limits.min_qty {
        return Err(RiskViolation::InsufficientQuantity {
            qty: abs_qty,
            min_qty: limits.min_qty,
        });
    }

    // Check minimum notional
    let notional = price * abs_qty;
    if notional < limits.min_notional {
        return Err(RiskViolation::InsufficientNotional {
            notional,
            min_notional: limits.min_notional,
        });
    }

    // Check reduce-only constraint
    if reduce_only {
        validate_reduce_only(qty, current_position_qty)?;
    }

    // Check position mode constraints
    if !limits.hedge_mode {
        validate_one_way_mode(qty, current_position_qty)?;
    }

    Ok(())
}

/// Validate that an order is truly reduce-only
pub fn validate_reduce_only(
    order_qty: Decimal,
    current_position_qty: Decimal,
) -> Result<(), RiskViolation> {
    // For reduce-only orders:
    // - If currently long (pos > 0), order must be sell (qty < 0) and not exceed position
    // - If currently short (pos < 0), order must be buy (qty > 0) and not exceed position abs
    // - If currently flat (pos = 0), reduce-only orders are not allowed

    if current_position_qty.is_zero() {
        return Err(RiskViolation::ReduceOnlyViolation);
    }

    if current_position_qty.is_sign_positive() {
        // Currently long, must sell and not exceed position
        if order_qty.is_sign_positive() || order_qty.abs() > current_position_qty {
            return Err(RiskViolation::ReduceOnlyViolation);
        }
    } else {
        // Currently short, must buy and not exceed position abs
        if order_qty.is_sign_negative() || order_qty > current_position_qty.abs() {
            return Err(RiskViolation::ReduceOnlyViolation);
        }
    }

    Ok(())
}

/// Validate one-way position mode constraints
pub fn validate_one_way_mode(
    order_qty: Decimal,
    current_position_qty: Decimal,
) -> Result<(), RiskViolation> {
    // In one-way mode, you cannot have both long and short positions
    // This means orders that would flip the position sign require flattening first

    if current_position_qty.is_zero() {
        return Ok(()); // Any order is fine when flat
    }

    let new_position_qty = current_position_qty + order_qty;

    // Check if the order would flip the position sign
    if current_position_qty.is_sign_positive() && new_position_qty.is_sign_negative() {
        return Err(RiskViolation::PositionModeViolation);
    }

    if current_position_qty.is_sign_negative() && new_position_qty.is_sign_positive() {
        return Err(RiskViolation::PositionModeViolation);
    }

    Ok(())
}

/// Calculate effective leverage for a given position and account balance
pub fn calculate_leverage(position_notional: Decimal, account_balance: Decimal) -> Decimal {
    if account_balance.is_zero() {
        return Decimal::MAX; // Infinite leverage
    }
    position_notional.abs() / account_balance
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Serialize, Deserialize)]
    struct TestStruct {
        #[serde(
            serialize_with = "serialize_decimal_as_str",
            deserialize_with = "deserialize_decimal_from_str"
        )]
        value: Decimal,
        #[serde(
            serialize_with = "serialize_optional_decimal_as_str",
            deserialize_with = "deserialize_optional_decimal_from_str"
        )]
        optional_value: Option<Decimal>,
    }

    #[rstest]
    fn test_decimal_serialization_roundtrip() {
        let original = TestStruct {
            value: Decimal::from_str("123.456789012345678901234567890").unwrap(),
            optional_value: Some(Decimal::from_str("0.000000001").unwrap()),
        };

        let json = serde_json::to_string(&original).unwrap();
        println!("Serialized: {}", json);

        // Check that it's serialized as strings (rust_decimal may normalize precision)
        assert!(json.contains("\"123.45678901234567890123456789\""));
        assert!(json.contains("\"0.000000001\""));

        let deserialized: TestStruct = serde_json::from_str(&json).unwrap();
        assert_eq!(original.value, deserialized.value);
        assert_eq!(original.optional_value, deserialized.optional_value);
    }

    #[rstest]
    fn test_decimal_precision_preservation() {
        let test_cases = [
            "0",
            "1",
            "0.1",
            "0.01",
            "0.001",
            "123.456789012345678901234567890",
            "999999999999999999.999999999999999999",
        ];

        for case in test_cases {
            let decimal = Decimal::from_str(case).unwrap();
            let test_struct = TestStruct {
                value: decimal,
                optional_value: Some(decimal),
            };

            let json = serde_json::to_string(&test_struct).unwrap();
            let parsed: TestStruct = serde_json::from_str(&json).unwrap();

            assert_eq!(decimal, parsed.value, "Failed for case: {}", case);
            assert_eq!(
                Some(decimal),
                parsed.optional_value,
                "Failed for case: {}",
                case
            );
        }
    }

    #[rstest]
    fn test_optional_none_handling() {
        let test_struct = TestStruct {
            value: Decimal::from_str("42.0").unwrap(),
            optional_value: None,
        };

        let json = serde_json::to_string(&test_struct).unwrap();
        assert!(json.contains("null"));

        let parsed: TestStruct = serde_json::from_str(&json).unwrap();
        assert_eq!(test_struct.value, parsed.value);
        assert_eq!(None, parsed.optional_value);
    }

    #[rstest]
    fn test_round_down_to_tick() {
        use rust_decimal_macros::dec;

        assert_eq!(round_down_to_tick(dec!(100.07), dec!(0.05)), dec!(100.05));
        assert_eq!(round_down_to_tick(dec!(100.03), dec!(0.05)), dec!(100.00));
        assert_eq!(round_down_to_tick(dec!(100.05), dec!(0.05)), dec!(100.05));

        // Edge case: zero tick size
        assert_eq!(round_down_to_tick(dec!(100.07), dec!(0)), dec!(100.07));
    }

    #[rstest]
    fn test_round_down_to_step() {
        use rust_decimal_macros::dec;

        assert_eq!(
            round_down_to_step(dec!(0.12349), dec!(0.0001)),
            dec!(0.1234)
        );
        assert_eq!(round_down_to_step(dec!(1.5555), dec!(0.1)), dec!(1.5));
        assert_eq!(round_down_to_step(dec!(1.0001), dec!(0.0001)), dec!(1.0001));

        // Edge case: zero step size
        assert_eq!(round_down_to_step(dec!(0.12349), dec!(0)), dec!(0.12349));
    }

    #[rstest]
    fn test_min_notional_validation() {
        use rust_decimal_macros::dec;

        // Should pass
        assert!(ensure_min_notional(dec!(100), dec!(0.1), dec!(10)).is_ok());
        assert!(ensure_min_notional(dec!(100), dec!(0.11), dec!(10)).is_ok());

        // Should fail
        assert!(ensure_min_notional(dec!(100), dec!(0.05), dec!(10)).is_err());
        assert!(ensure_min_notional(dec!(1), dec!(5), dec!(10)).is_err());

        // Edge case: exactly at minimum
        assert!(ensure_min_notional(dec!(100), dec!(0.1), dec!(10)).is_ok());
    }

    #[rstest]
    fn test_normalize_price() {
        use rust_decimal_macros::dec;

        assert_eq!(normalize_price(dec!(100.12345), 2), dec!(100.12));
        assert_eq!(normalize_price(dec!(100.19999), 2), dec!(100.19));
        assert_eq!(normalize_price(dec!(100.999), 0), dec!(100));
        assert_eq!(normalize_price(dec!(100.12345), 4), dec!(100.1234));
    }

    #[rstest]
    fn test_normalize_quantity() {
        use rust_decimal_macros::dec;

        assert_eq!(normalize_quantity(dec!(1.12345), 3), dec!(1.123));
        assert_eq!(normalize_quantity(dec!(1.99999), 3), dec!(1.999));
        assert_eq!(normalize_quantity(dec!(1.999), 0), dec!(1));
        assert_eq!(normalize_quantity(dec!(1.12345), 5), dec!(1.12345));
    }

    #[rstest]
    fn test_normalize_order_complete() {
        use rust_decimal_macros::dec;

        let result = normalize_order(
            dec!(100.12345), // price
            dec!(0.123456),  // qty
            dec!(0.01),      // tick_size
            dec!(0.0001),    // step_size
            dec!(10),        // min_notional
            2,               // price_decimals
            4,               // size_decimals
        );

        assert!(result.is_ok());
        let (price, qty) = result.unwrap();
        assert_eq!(price, dec!(100.12)); // normalized and rounded down
        assert_eq!(qty, dec!(0.1234)); // normalized and rounded down
    }

    #[rstest]
    fn test_normalize_order_min_notional_fail() {
        use rust_decimal_macros::dec;

        let result = normalize_order(
            dec!(100.12345), // price
            dec!(0.05),      // qty (too small for min notional)
            dec!(0.01),      // tick_size
            dec!(0.0001),    // step_size
            dec!(10),        // min_notional
            2,               // price_decimals
            4,               // size_decimals
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Notional value"));
    }

    #[rstest]
    fn test_edge_cases() {
        use rust_decimal_macros::dec;

        // Test with very small numbers
        assert_eq!(
            round_down_to_tick(dec!(0.000001), dec!(0.000001)),
            dec!(0.000001)
        );

        // Test with large numbers
        assert_eq!(round_down_to_tick(dec!(999999.99), dec!(1.0)), dec!(999999));

        // Test rounding edge case
        assert_eq!(
            round_down_to_tick(dec!(100.009999), dec!(0.01)),
            dec!(100.00)
        );
    }

    #[rstest]
    fn test_default_risk_limits() {
        use rust_decimal_macros::dec;

        let limits = RiskLimits::default();
        assert_eq!(limits.max_leverage, dec!(100));
        assert_eq!(limits.min_qty, dec!(0.0001));
        assert_eq!(limits.min_notional, dec!(10));
        assert!(limits.hedge_mode);
    }

    #[rstest]
    fn test_risk_limits_presets() {
        use rust_decimal_macros::dec;

        let conservative = RiskLimits::conservative();
        assert_eq!(conservative.max_leverage, dec!(10));
        assert_eq!(conservative.min_notional, dec!(50));
        assert!(!conservative.hedge_mode);

        let aggressive = RiskLimits::aggressive();
        assert_eq!(aggressive.max_leverage, dec!(200));
        assert_eq!(aggressive.min_notional, dec!(1));
        assert!(aggressive.hedge_mode);
    }

    #[rstest]
    fn test_leverage_validation() {
        use rust_decimal_macros::dec;

        let limits = RiskLimits::default();

        // Should pass
        let result = validate_limit_order(
            dec!(100), // price
            dec!(1),   // qty
            dec!(50),  // leverage (within limit)
            dec!(0),   // current position
            false,     // reduce_only
            &limits,
        );
        assert!(result.is_ok());

        // Should fail - excessive leverage
        let result = validate_limit_order(
            dec!(100),
            dec!(1),
            dec!(150), // leverage exceeds 100x limit
            dec!(0),
            false,
            &limits,
        );
        assert!(matches!(
            result,
            Err(RiskViolation::ExcessiveLeverage { .. })
        ));
    }

    #[rstest]
    fn test_quantity_validation() {
        use rust_decimal_macros::dec;

        let limits = RiskLimits::default();

        // Should fail - quantity too small
        let result = validate_limit_order(
            dec!(100),
            dec!(0.00005), // Below minimum
            dec!(10),
            dec!(0),
            false,
            &limits,
        );
        assert!(matches!(
            result,
            Err(RiskViolation::InsufficientQuantity { .. })
        ));
    }

    #[rstest]
    fn test_notional_validation() {
        use rust_decimal_macros::dec;

        let limits = RiskLimits::default();

        // Should fail - notional too small
        let result = validate_limit_order(
            dec!(1), // Low price
            dec!(1), // Results in $1 notional, below $10 minimum
            dec!(10),
            dec!(0),
            false,
            &limits,
        );
        assert!(matches!(
            result,
            Err(RiskViolation::InsufficientNotional { .. })
        ));
    }

    #[rstest]
    fn test_reduce_only_validation() {
        use rust_decimal_macros::dec;

        let limits = RiskLimits::default();

        // Currently long 2 units, selling 1 unit (reduce-only) - should pass
        let result = validate_limit_order(
            dec!(100),
            dec!(-1), // Sell order
            dec!(10),
            dec!(2), // Currently long 2 units
            true,    // reduce_only
            &limits,
        );
        assert!(result.is_ok());

        // Currently long 2 units, buying more (reduce-only) - should fail
        let result = validate_limit_order(
            dec!(100),
            dec!(1), // Buy order
            dec!(10),
            dec!(2), // Currently long 2 units
            true,    // reduce_only
            &limits,
        );
        assert!(matches!(result, Err(RiskViolation::ReduceOnlyViolation)));

        // Currently flat, any reduce-only order should fail
        let result = validate_limit_order(
            dec!(100),
            dec!(1),
            dec!(10),
            dec!(0), // Flat position
            true,    // reduce_only
            &limits,
        );
        assert!(matches!(result, Err(RiskViolation::ReduceOnlyViolation)));
    }

    #[rstest]
    fn test_reduce_only_validation_short_position() {
        use rust_decimal_macros::dec;

        let limits = RiskLimits::default();

        // Currently short 2 units, buying 1 unit (reduce-only) - should pass
        let result = validate_limit_order(
            dec!(100),
            dec!(1), // Buy order
            dec!(10),
            dec!(-2), // Currently short 2 units
            true,     // reduce_only
            &limits,
        );
        assert!(result.is_ok());

        // Currently short 2 units, selling more (reduce-only) - should fail
        let result = validate_limit_order(
            dec!(100),
            dec!(-1), // Sell order
            dec!(10),
            dec!(-2), // Currently short 2 units
            true,     // reduce_only
            &limits,
        );
        assert!(matches!(result, Err(RiskViolation::ReduceOnlyViolation)));

        // Currently short 2 units, buying more than position (reduce-only) - should fail
        let result = validate_limit_order(
            dec!(100),
            dec!(3), // Buy more than short position
            dec!(10),
            dec!(-2), // Currently short 2 units
            true,     // reduce_only
            &limits,
        );
        assert!(matches!(result, Err(RiskViolation::ReduceOnlyViolation)));
    }

    #[rstest]
    fn test_one_way_mode_validation() {
        use rust_decimal_macros::dec;

        let limits = RiskLimits {
            hedge_mode: false, // Enable one-way mode
            ..Default::default()
        };

        // Currently long, selling to flat - should pass
        let result = validate_limit_order(
            dec!(100),
            dec!(-2), // Sell all
            dec!(10),
            dec!(2), // Currently long 2 units
            false,
            &limits,
        );
        assert!(result.is_ok());

        // Currently long, selling beyond flat (flipping to short) - should fail
        let result = validate_limit_order(
            dec!(100),
            dec!(-3), // Sell more than position
            dec!(10),
            dec!(2), // Currently long 2 units
            false,
            &limits,
        );
        assert!(matches!(result, Err(RiskViolation::PositionModeViolation)));

        // Currently short, buying beyond flat (flipping to long) - should fail
        let result = validate_limit_order(
            dec!(100),
            dec!(3), // Buy more than short position
            dec!(10),
            dec!(-2), // Currently short 2 units
            false,
            &limits,
        );
        assert!(matches!(result, Err(RiskViolation::PositionModeViolation)));

        // Currently flat, any order should pass
        let result = validate_limit_order(
            dec!(100),
            dec!(1),
            dec!(10),
            dec!(0), // Flat position
            false,
            &limits,
        );
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_leverage_calculation() {
        use rust_decimal_macros::dec;

        assert_eq!(calculate_leverage(dec!(1000), dec!(100)), dec!(10));

        assert_eq!(calculate_leverage(dec!(500), dec!(100)), dec!(5));

        // Test with negative position notional (should use absolute value)
        assert_eq!(calculate_leverage(dec!(-1000), dec!(100)), dec!(10));

        // Test with zero balance
        assert_eq!(calculate_leverage(dec!(1000), dec!(0)), Decimal::MAX);

        // Test with zero position
        assert_eq!(calculate_leverage(dec!(0), dec!(100)), dec!(0));
    }

    #[rstest]
    fn test_risk_limits_custom() {
        use rust_decimal_macros::dec;

        let custom_limits = RiskLimits::new(
            dec!(50),   // max_leverage
            dec!(0.01), // min_qty
            dec!(25),   // min_notional
            false,      // hedge_mode
        );

        assert_eq!(custom_limits.max_leverage, dec!(50));
        assert_eq!(custom_limits.min_qty, dec!(0.01));
        assert_eq!(custom_limits.min_notional, dec!(25));
        assert!(!custom_limits.hedge_mode);
    }

    #[rstest]
    fn test_combined_risk_validation() {
        use rust_decimal_macros::dec;

        let limits = RiskLimits::conservative(); // 10x leverage, $50 min notional, one-way mode

        // Valid order that passes all checks
        let result = validate_limit_order(
            dec!(100), // price
            dec!(1),   // qty (notional = $100, above $50 min)
            dec!(5),   // leverage (below 10x limit)
            dec!(0),   // flat position
            false,     // not reduce_only
            &limits,
        );
        assert!(result.is_ok());

        // Order that fails multiple checks
        let result = validate_limit_order(
            dec!(1),   // low price
            dec!(0.1), // small qty (notional = $0.10, below $50 min)
            dec!(20),  // high leverage (above 10x limit)
            dec!(0),
            false,
            &limits,
        );
        // Should fail on leverage first (checked before notional)
        assert!(matches!(
            result,
            Err(RiskViolation::ExcessiveLeverage { .. })
        ));
    }
}
