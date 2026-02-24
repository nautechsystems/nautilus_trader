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

//! Common type aliases for Betfair identifiers and values.

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};
use serde_json;

/// Betfair market identifier (e.g., "1.201070830").
pub type MarketId = String;

/// Betfair selection (runner) identifier.
pub type SelectionId = u64;

/// Deserializes a `SelectionId` from either a JSON number or string.
///
/// The streaming API sometimes sends selection IDs as strings (e.g. `"19248890"`)
/// rather than bare integers.
///
/// # Errors
///
/// Returns an error if the value is a string that cannot be parsed as `u64`.
pub fn deserialize_selection_id<'de, D>(deserializer: D) -> Result<SelectionId, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrU64 {
        U64(u64),
        Str(String),
    }

    match StringOrU64::deserialize(deserializer)? {
        StringOrU64::U64(v) => Ok(v),
        StringOrU64::Str(s) => s.parse().map_err(serde::de::Error::custom),
    }
}

/// Betfair bet identifier.
pub type BetId = String;

/// Betfair event identifier.
pub type EventId = String;

/// Betfair event type identifier.
pub type EventTypeId = String;

/// Betfair exchange identifier.
pub type ExchangeId = String;

/// Competition identifier.
pub type CompetitionId = String;

/// Customer order reference (max 32 characters).
pub type CustomerOrderRef = String;

/// Customer strategy reference (max 15 characters).
pub type CustomerStrategyRef = String;

/// Handicap value for Asian handicap markets.
pub type Handicap = Decimal;

/// Deserializes an `Option<String>` from either a JSON string or number.
///
/// Betfair API docs define many ID fields as strings, but the actual responses
/// sometimes send them as bare integers (e.g. `"id": 7` instead of `"id": "7"`).
///
/// # Errors
///
/// Returns an error if the underlying JSON value cannot be deserialized.
pub fn deserialize_optional_string_lenient<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.map(|v| match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }))
}

/// Deserializes an `Option<u32>` leniently from a JSON number, numeric string, or
/// empty string.
///
/// Betfair navigation responses sometimes send `"numberOfWinners": ""` for
/// handicap/total-points markets, which would fail a strict `Option<u32>` parse.
///
/// # Errors
///
/// Returns an error if the value is a non-empty string that cannot be parsed as `u32`.
pub fn deserialize_optional_u32_lenient<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Num(u32),
        Str(String),
    }

    match Option::<Value>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Value::Num(n)) => Ok(Some(n)),
        Some(Value::Str(s)) if s.is_empty() => Ok(None),
        Some(Value::Str(s)) => s.parse().map(Some).map_err(serde::de::Error::custom),
    }
}
