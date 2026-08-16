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

//! Parsing utilities for the Polymarket adapter.

pub use nautilus_core::serialization::{
    deserialize_decimal_from_str, deserialize_optional_decimal_from_str, serialize_decimal_as_str,
    serialize_optional_decimal_as_str,
};
use nautilus_model::identifiers::TradeId;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
use serde_json::{Number, value::RawValue};

use crate::common::enums::PolymarketOrderSide;

/// Deserializes a decimal directly from its JSON number token without an `f64` conversion.
pub fn deserialize_decimal_from_json_number<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Box::<RawValue>::deserialize(deserializer)?;
    Decimal::from_str_exact(raw.get()).map_err(D::Error::custom)
}

/// Deserializes an optional decimal directly from its JSON number token.
pub fn deserialize_optional_decimal_from_json_number<'de, D>(
    deserializer: D,
) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Box<RawValue>>::deserialize(deserializer)?
        .map(|raw| Decimal::from_str_exact(raw.get()).map_err(D::Error::custom))
        .transpose()
}

/// Serializes a decimal as an exact JSON number token.
pub fn serialize_decimal_as_json_number<S>(
    value: &Decimal,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let raw = RawValue::from_string(value.to_string()).map_err(serde::ser::Error::custom)?;
    raw.serialize(serializer)
}

/// Serializes an optional decimal as an exact JSON number token or `null`.
pub fn serialize_optional_decimal_as_json_number<S>(
    value: &Option<Decimal>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(value) => serialize_decimal_as_json_number(value, serializer),
        None => serializer.serialize_none(),
    }
}

/// Deserializes a Polymarket game ID as an opaque identifier.
///
/// The Gamma API returns the field in several shapes: an integer on
/// `GammaEvent`, a numeric string on most `GammaMarket` records, and a
/// composite `<uuid>:<away>:<home>` string on some sports markets. The value
/// identifies a venue-side fixture and is never used for arithmetic, so it is
/// kept verbatim rather than parsed into a number. Both `null` and `-1` (or
/// `"-1"`) are the "no game" sentinel and map to `None`.
pub fn deserialize_optional_polymarket_game_id<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Str(String),
        Num(Number),
    }

    let game_id = match Option::<Raw>::deserialize(deserializer)? {
        None => return Ok(None),
        Some(Raw::Str(value)) => value,
        Some(Raw::Num(value)) => value.to_string(),
    };

    if game_id.is_empty() || game_id == "-1" {
        return Ok(None);
    }

    Ok(Some(game_id))
}

// FNV-1a 64-bit constants (see http://www.isthe.com/chongo/tech/comp/fnv/).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

/// Derives a deterministic [`TradeId`] for a Polymarket market data trade.
///
/// Polymarket does not publish a trade ID with `last_trade_price` events, so
/// one is derived from the trade's identifying fields. FNV-1a is stable across
/// architectures and crate versions, and the 0x1f delimiter prevents
/// variable-length fields from colliding (e.g. `"0.12"` + `"34"` vs `"0.1"` +
/// `"234"`).
#[must_use]
pub fn determine_trade_id(
    asset_id: &str,
    side: PolymarketOrderSide,
    price: &str,
    size: &str,
    timestamp: &str,
) -> TradeId {
    let side_byte: &[u8] = match side {
        PolymarketOrderSide::Buy => b"B",
        PolymarketOrderSide::Sell => b"S",
    };
    let mut h: u64 = FNV_OFFSET_BASIS;

    for bytes in [
        asset_id.as_bytes(),
        b"\x1f",
        side_byte,
        b"\x1f",
        price.as_bytes(),
        b"\x1f",
        size.as_bytes(),
        b"\x1f",
        timestamp.as_bytes(),
    ] {
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(FNV_PRIME);
        }
    }
    TradeId::new(format!("{h:016x}"))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, Deserialize)]
    struct GameIdHolder {
        #[serde(default, deserialize_with = "deserialize_optional_polymarket_game_id")]
        game_id: Option<String>,
    }

    #[derive(Debug, Deserialize, Serialize)]
    struct JsonDecimalHolder {
        #[serde(
            deserialize_with = "deserialize_decimal_from_json_number",
            serialize_with = "serialize_decimal_as_json_number"
        )]
        value: Decimal,
        #[serde(
            default,
            deserialize_with = "deserialize_optional_decimal_from_json_number",
            serialize_with = "serialize_optional_decimal_as_json_number"
        )]
        optional: Option<Decimal>,
    }

    #[rstest]
    fn test_json_decimal_number_preserves_precision() {
        let json =
            r#"{"value":0.1234567890123456789012345678,"optional":123456789.1234567890123456789}"#;
        let holder: JsonDecimalHolder = serde_json::from_str(json).unwrap();

        assert_eq!(
            holder.value,
            Decimal::from_str_exact("0.1234567890123456789012345678").unwrap()
        );
        assert_eq!(
            holder.optional,
            Some(Decimal::from_str_exact("123456789.1234567890123456789").unwrap())
        );
        assert_eq!(serde_json::to_string(&holder).unwrap(), json);
    }

    #[rstest]
    fn test_optional_json_decimal_number_accepts_null_and_missing() {
        let null: JsonDecimalHolder =
            serde_json::from_str(r#"{"value":1,"optional":null}"#).unwrap();
        let missing: JsonDecimalHolder = serde_json::from_str(r#"{"value":1}"#).unwrap();

        assert_eq!(null.value, Decimal::ONE);
        assert!(null.optional.is_none());
        assert_eq!(missing.value, Decimal::ONE);
        assert!(missing.optional.is_none());
    }

    #[rstest]
    #[case::null(r#"{"game_id": null}"#, None)]
    #[case::missing("{}", None)]
    #[case::empty_string(r#"{"game_id": ""}"#, None)]
    #[case::int_neg_one(r#"{"game_id": -1}"#, None)]
    #[case::str_neg_one(r#"{"game_id": "-1"}"#, None)]
    #[case::int_zero(r#"{"game_id": 0}"#, Some("0"))]
    #[case::str_zero(r#"{"game_id": "0"}"#, Some("0"))]
    #[case::int_value(r#"{"game_id": 1427074}"#, Some("1427074"))]
    #[case::str_value(r#"{"game_id": "1427074"}"#, Some("1427074"))]
    // Some sports markets carry a composite `<uuid>:<away>:<home>` game ID.
    #[case::composite(
        r#"{"game_id": "dd80aae9-52f9-4c7b-a1cf-7b4ab63cd281:STL:TEX"}"#,
        Some("dd80aae9-52f9-4c7b-a1cf-7b4ab63cd281:STL:TEX")
    )]
    #[case::composite_rematch(
        r#"{"game_id": "dd80aae9-52f9-4c7b-a1cf-7b4ab63cd281:DAL:LA:m2"}"#,
        Some("dd80aae9-52f9-4c7b-a1cf-7b4ab63cd281:DAL:LA:m2")
    )]
    // Only -1 is the no-game sentinel, so other negatives stay verbatim
    // rather than collapsing to "no game".
    #[case::int_neg_other(r#"{"game_id": -2}"#, Some("-2"))]
    #[case::str_neg_other(r#"{"game_id": "-2"}"#, Some("-2"))]
    // A numeric ID beyond `i64` must not fail the record it arrived on.
    #[case::int_beyond_i64(r#"{"game_id": 18446744073709551615}"#, Some("18446744073709551615"))]
    fn test_deserialize_optional_polymarket_game_id(
        #[case] payload: &str,
        #[case] expected: Option<&str>,
    ) {
        let holder: GameIdHolder = serde_json::from_str(payload).unwrap();
        assert_eq!(holder.game_id.as_deref(), expected);
    }

    #[rstest]
    fn test_determine_trade_id_is_deterministic() {
        let id1 = determine_trade_id("asset-1", PolymarketOrderSide::Buy, "0.5", "10", "1700000");
        let id2 = determine_trade_id("asset-1", PolymarketOrderSide::Buy, "0.5", "10", "1700000");
        assert_eq!(id1, id2);
    }

    #[rstest]
    fn test_determine_trade_id_differentiates_sides() {
        let buy = determine_trade_id("asset-1", PolymarketOrderSide::Buy, "0.5", "10", "1700000");
        let sell = determine_trade_id("asset-1", PolymarketOrderSide::Sell, "0.5", "10", "1700000");
        assert_ne!(buy, sell);
    }

    #[rstest]
    fn test_determine_trade_id_field_delimiter_prevents_collision() {
        // "0.12" + "34" would collide with "0.1" + "234" if fields were concatenated
        let a = determine_trade_id("asset-1", PolymarketOrderSide::Buy, "0.12", "34", "1700000");
        let b = determine_trade_id("asset-1", PolymarketOrderSide::Buy, "0.1", "234", "1700000");
        assert_ne!(a, b);
    }

    #[rstest]
    fn test_determine_trade_id_format() {
        let id = determine_trade_id("asset-1", PolymarketOrderSide::Buy, "0.5", "10", "1700000");
        let s = id.to_string();
        assert_eq!(s.len(), 16);
        // Pin lowercase hex so downstream consumers can rely on the format
        assert!(
            s.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
    }
}
