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
}
