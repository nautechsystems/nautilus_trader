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

//! Exact JSON-text serialization for Kraken financial values.
//!
//! `serde_json::Value` cannot retain arbitrary decimal tokens without its workspace-wide
//! `arbitrary_precision` feature. Kraken wire models therefore deserialize directly from JSON text
//! or readers and serialize directly to JSON text.

use std::{borrow::Cow, str::FromStr};

use ahash::AHashMap;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize, de::Error as _, ser::Error as _};
use serde_json::value::RawValue;

#[derive(Clone, Copy)]
struct JsonDecimal(Decimal);

impl<'de> Deserialize<'de> for JsonDecimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        parse_raw_decimal(raw.get())
            .map(Self)
            .map_err(D::Error::custom)
    }
}

impl Serialize for JsonDecimal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        RawValue::from_string(self.0.to_string())
            .map_err(S::Error::custom)?
            .serialize(serializer)
    }
}

fn parse_raw_decimal(raw: &str) -> Result<Decimal, String> {
    let value = if raw.starts_with('"') {
        Cow::Owned(serde_json::from_str::<String>(raw).map_err(|e| e.to_string())?)
    } else {
        Cow::Borrowed(raw)
    };

    Decimal::from_str(&value)
        .or_else(|_| Decimal::from_scientific(&value))
        .map_err(|e| e.to_string())
}

pub(crate) mod decimal {
    use super::*;

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        JsonDecimal::deserialize(deserializer).map(|value| value.0)
    }

    pub(crate) fn serialize<S>(value: &Decimal, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        JsonDecimal(*value).serialize(serializer)
    }
}

pub(crate) mod optional_decimal {
    use super::*;

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        if raw.get() == "null" {
            return Ok(None);
        }

        parse_raw_decimal(raw.get())
            .map(Some)
            .map_err(D::Error::custom)
    }

    pub(crate) fn serialize<S>(value: &Option<Decimal>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match value {
            Some(value) => JsonDecimal(*value).serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
}

pub(crate) mod decimal_map {
    use serde::ser::SerializeMap;

    use super::*;

    pub(crate) fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<AHashMap<String, Decimal>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        AHashMap::<String, JsonDecimal>::deserialize(deserializer).map(|values| {
            values
                .into_iter()
                .map(|(key, value)| (key, value.0))
                .collect()
        })
    }

    pub(crate) fn serialize<S>(
        values: &AHashMap<String, Decimal>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(values.len()))?;
        for (key, value) in values {
            map.serialize_entry(key, &JsonDecimal(*value))?;
        }
        map.end()
    }
}

pub(crate) mod decimal_pairs {
    use super::*;

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<(i32, Decimal)>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<(i32, JsonDecimal)>::deserialize(deserializer).map(|values| {
            values
                .into_iter()
                .map(|(threshold, value)| (threshold, value.0))
                .collect()
        })
    }

    pub(crate) fn serialize<S>(values: &[(i32, Decimal)], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        values
            .iter()
            .map(|(threshold, value)| (*threshold, JsonDecimal(*value)))
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
}

pub(crate) fn deserialize_decimal_pair<'de, D>(
    deserializer: D,
) -> Result<(Decimal, Decimal), D::Error>
where
    D: serde::Deserializer<'de>,
{
    <(JsonDecimal, JsonDecimal)>::deserialize(deserializer)
        .map(|(first, second)| (first.0, second.0))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct WireDecimals {
        #[serde(with = "decimal")]
        required: Decimal,
        #[serde(with = "optional_decimal")]
        optional: Option<Decimal>,
        #[serde(with = "decimal_map")]
        balances: AHashMap<String, Decimal>,
        #[serde(with = "decimal_pairs")]
        fees: Vec<(i32, Decimal)>,
    }

    #[rstest]
    fn test_json_decimals_preserve_precision() {
        let json = include_str!("../../test_data/decimal_exact.json").trim();
        let values: WireDecimals = serde_json::from_str(json).unwrap();
        let values_from_reader: WireDecimals = serde_json::from_reader(json.as_bytes()).unwrap();

        assert_eq!(values.required, dec!(0.1234567890123456789012345678));
        assert_eq!(values.optional, Some(dec!(123456789.123456789)));
        assert_eq!(values.balances["USD"], dec!(987654321.987654321));
        assert_eq!(
            values.fees,
            vec![(1000, dec!(0.1234567890123456789012345678))]
        );
        assert_eq!(values_from_reader, values);
        assert_eq!(serde_json::to_string(&values).unwrap(), json);
    }
}
