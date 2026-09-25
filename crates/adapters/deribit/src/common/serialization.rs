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

//! Decimal readers for Deribit JSON fields that need Deribit-specific shapes or absence
//! handling around the core token readers.

use nautilus_core::serialization::{deserialize_decimal_token, deserialize_optional_decimal_token};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};

pub(crate) fn deserialize_decimal_token_or_zero<'de, D>(
    deserializer: D,
) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_decimal_token(deserializer).map(Option::unwrap_or_default)
}

pub(crate) fn deserialize_decimal_token_vec<'de, D>(
    deserializer: D,
) -> Result<Vec<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<DecimalToken>::deserialize(deserializer)
        .map(|values| values.into_iter().map(|value| value.0).collect())
}

pub(crate) fn deserialize_decimal_token_pairs<'de, D>(
    deserializer: D,
) -> Result<Vec<[Decimal; 2]>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<[DecimalToken; 2]>::deserialize(deserializer).map(|pairs| {
        pairs
            .into_iter()
            .map(|[first, second]| [first.0, second.0])
            .collect()
    })
}

#[derive(Deserialize)]
struct DecimalToken(#[serde(deserialize_with = "deserialize_decimal_token")] Decimal);

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[derive(Debug, Deserialize)]
    struct DecimalOrZero {
        #[serde(deserialize_with = "deserialize_decimal_token_or_zero")]
        value: Decimal,
    }

    #[rstest]
    #[case(r#"{"value": null}"#, "0")]
    #[case(r#"{"value": ""}"#, "0")]
    #[case(r#"{"value": "0.10"}"#, "0.10")]
    #[case(r#"{"value": 100000000.123456789}"#, "100000000.123456789")]
    fn test_deserialize_decimal_token_or_zero(#[case] json: &str, #[case] expected: &str) {
        let parsed: DecimalOrZero = serde_json::from_str(json).unwrap();

        assert_eq!(parsed.value.to_string(), expected);
    }

    #[rstest]
    fn test_deserialize_decimal_token_or_zero_rejects_invalid() {
        let result = serde_json::from_str::<DecimalOrZero>(r#"{"value": "abc"}"#);

        assert!(result.is_err());
    }
}
