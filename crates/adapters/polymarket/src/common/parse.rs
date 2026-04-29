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
use serde::{Deserialize, Deserializer, de::Error};

use crate::common::enums::PolymarketOrderSide;

/// Deserializes a Polymarket game ID. The Gamma API returns the field in two
/// shapes (string on `GammaMarket`, integer on `GammaEvent`) and uses both
/// `null` and `-1` (or `"-1"`) as the "no game" sentinel for non-sport
/// markets. Either sentinel is mapped to `None`; valid values must be
/// non-negative.
pub fn deserialize_optional_polymarket_game_id<'de, D>(
    deserializer: D,
) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Str(String),
        Int(i64),
    }

    let raw: Option<Raw> = Option::deserialize(deserializer)?;
    match raw {
        None => Ok(None),
        Some(Raw::Str(s)) if s.is_empty() || s == "-1" => Ok(None),
        Some(Raw::Str(s)) => s.parse::<u64>().map(Some).map_err(D::Error::custom),
        Some(Raw::Int(-1)) => Ok(None),
        Some(Raw::Int(i)) if i < 0 => Err(D::Error::custom(format!(
            "negative game_id {i}: only -1 is recognized as the no-game sentinel"
        ))),
        Some(Raw::Int(i)) => Ok(Some(i as u64)),
    }
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
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize)]
    struct GameIdHolder {
        #[serde(default, deserialize_with = "deserialize_optional_polymarket_game_id")]
        game_id: Option<u64>,
    }

    #[rstest]
    #[case::null(r#"{"game_id": null}"#, None)]
    #[case::missing("{}", None)]
    #[case::empty_string(r#"{"game_id": ""}"#, None)]
    #[case::int_neg_one(r#"{"game_id": -1}"#, None)]
    #[case::str_neg_one(r#"{"game_id": "-1"}"#, None)]
    #[case::int_zero(r#"{"game_id": 0}"#, Some(0))]
    #[case::str_zero(r#"{"game_id": "0"}"#, Some(0))]
    #[case::int_value(r#"{"game_id": 1427074}"#, Some(1_427_074))]
    #[case::str_value(r#"{"game_id": "1427074"}"#, Some(1_427_074))]
    fn test_deserialize_optional_polymarket_game_id(
        #[case] payload: &str,
        #[case] expected: Option<u64>,
    ) {
        let holder: GameIdHolder = serde_json::from_str(payload).unwrap();
        assert_eq!(holder.game_id, expected);
    }

    #[rstest]
    fn test_deserialize_optional_polymarket_game_id_rejects_garbage_string() {
        let err = serde_json::from_str::<GameIdHolder>(r#"{"game_id": "not-a-number"}"#);
        assert!(err.is_err());
    }

    #[rstest]
    fn test_deserialize_optional_polymarket_game_id_rejects_negative_other_than_minus_one() {
        // Only -1 is the documented no-game sentinel; other negatives must
        // surface as errors so unexpected wire shapes do not collapse to
        // "no game" silently.
        let err = serde_json::from_str::<GameIdHolder>(r#"{"game_id": -2}"#).unwrap_err();
        assert!(err.to_string().contains("only -1"));
    }

    #[rstest]
    fn test_deserialize_optional_polymarket_game_id_rejects_negative_string_other_than_minus_one() {
        // Mirrors the integer behaviour: only "-1" is a sentinel; "-2" must
        // bubble up as a parse error rather than silent None.
        let err = serde_json::from_str::<GameIdHolder>(r#"{"game_id": "-2"}"#);
        assert!(err.is_err());
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
