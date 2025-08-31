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
}
