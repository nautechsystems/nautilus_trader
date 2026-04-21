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
    deserialize_decimal_from_str, deserialize_optional_decimal_from_str,
    deserialize_optional_string_to_u64, serialize_decimal_as_str,
    serialize_optional_decimal_as_str,
};
use nautilus_model::identifiers::TradeId;
use ustr::Ustr;

use crate::common::enums::PolymarketOrderSide;

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
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;

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
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    TradeId::new(Ustr::from(&format!("{h:016x}")))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

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
