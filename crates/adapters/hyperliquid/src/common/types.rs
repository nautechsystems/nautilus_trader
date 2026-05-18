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

use std::fmt::Display;

use serde::{Deserialize, Serialize};

/// Represents an asset ID for Hyperliquid.
///
/// Asset IDs follow Hyperliquid's convention:
/// - Perps: raw index into meta.universe (`0..10_000`)
/// - Spot: `10_000 + index` in spotMeta.universe (`10_000..100_000`)
/// - Builder perps: `100_000 + dex_index * 10_000 + meta_index` (`100_000..100_000_000`)
/// - Outcomes (HIP-4): `100_000_000 + 10 * outcome + side` where side is `0` or `1`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HyperliquidAssetId(pub u32);

const HIP_1_SPOT_BASE: u32 = 10_000;
const HIP_3_BUILDER_PERP_BASE: u32 = 100_000;
const HIP_4_OUTCOME_BASE: u32 = 100_000_000;

impl HyperliquidAssetId {
    /// Creates a perpetual asset ID from raw index.
    pub fn perp(index: u32) -> Self {
        Self(index)
    }

    /// Creates a spot asset ID (`10_000 + index`).
    pub fn spot(index: u32) -> Self {
        Self(HIP_1_SPOT_BASE + index)
    }

    /// Creates a builder perpetual asset ID.
    pub fn builder_perp(dex_index: u32, meta_index: u32) -> Self {
        Self(HIP_3_BUILDER_PERP_BASE + dex_index * 10_000 + meta_index)
    }

    /// Creates an outcome (HIP-4) asset ID from `outcome` and `side`.
    ///
    /// Encoding: `100_000_000 + 10 * outcome + side`. Only sides `0` and `1`
    /// are valid for binary outcomes.
    ///
    /// # Panics
    ///
    /// Panics if `side` is not `0` or `1`.
    pub fn outcome(outcome: u32, side: u8) -> Self {
        assert!(side <= 1, "outcome side must be 0 or 1, received {side}");
        Self(HIP_4_OUTCOME_BASE + 10 * outcome + u32::from(side))
    }

    /// Creates an outcome (HIP-4) asset ID from an encoded `10 * outcome + side` value.
    pub fn from_outcome_encoding(encoding: u32) -> Option<Self> {
        let raw = HIP_4_OUTCOME_BASE.checked_add(encoding)?;
        let asset_id = Self(raw);
        asset_id.is_outcome().then_some(asset_id)
    }

    /// Checks if this is a perp asset (raw index, `< 10_000`).
    pub fn is_perp(self) -> bool {
        self.0 < HIP_1_SPOT_BASE
    }

    /// Checks if this is a spot asset (`10_000..100_000`).
    pub fn is_spot(self) -> bool {
        self.0 >= HIP_1_SPOT_BASE && self.0 < HIP_3_BUILDER_PERP_BASE
    }

    /// Checks if this is a builder perp (`100_000..100_000_000`).
    pub fn is_builder_perp(self) -> bool {
        self.0 >= HIP_3_BUILDER_PERP_BASE && self.0 < HIP_4_OUTCOME_BASE
    }

    /// Checks if this is a valid outcome (HIP-4) asset.
    ///
    /// Requires the id to be in the outcome range and have a valid side
    /// digit (`0` or `1`). Ids in the range with side digits `2..=9` are
    /// not valid HIP-4 outcomes per the protocol.
    pub fn is_outcome(self) -> bool {
        self.0 >= HIP_4_OUTCOME_BASE && (self.0 - HIP_4_OUTCOME_BASE) % 10 <= 1
    }

    /// Gets the base index for the asset.
    ///
    /// - Perp: raw index.
    /// - Spot: `asset_id - 10_000`.
    /// - Builder perp: meta index within the dex.
    /// - Outcome: encoding `10 * outcome + side`.
    pub fn base_index(self) -> u32 {
        if self.is_outcome() {
            self.0 - HIP_4_OUTCOME_BASE
        } else if self.is_builder_perp() {
            (self.0 - HIP_3_BUILDER_PERP_BASE) % 10_000
        } else if self.is_spot() {
            self.0 - HIP_1_SPOT_BASE
        } else {
            self.0
        }
    }

    /// Returns the outcome number for an outcome asset, otherwise `None`.
    pub fn outcome_index(self) -> Option<u32> {
        self.outcome_encoding().map(|encoding| encoding / 10)
    }

    /// Returns the outcome side (`0` or `1`) for an outcome asset, otherwise `None`.
    pub fn outcome_side(self) -> Option<u8> {
        self.outcome_encoding()
            .map(|encoding| (encoding % 10) as u8)
    }

    /// Returns the outcome encoding (`10 * outcome + side`) for an outcome asset.
    pub fn outcome_encoding(self) -> Option<u32> {
        self.is_outcome().then(|| self.0 - HIP_4_OUTCOME_BASE)
    }

    /// Gets the raw asset ID value.
    pub fn to_raw(self) -> u32 {
        self.0
    }
}

impl Display for HyperliquidAssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_asset_id_perp() {
        let asset_id = HyperliquidAssetId::perp(7);
        assert_eq!(asset_id.to_raw(), 7);
        assert!(asset_id.is_perp());
        assert!(!asset_id.is_spot());
        assert!(!asset_id.is_builder_perp());
        assert!(!asset_id.is_outcome());
        assert_eq!(asset_id.base_index(), 7);
    }

    #[rstest]
    fn test_asset_id_spot() {
        let asset_id = HyperliquidAssetId::spot(7);
        assert_eq!(asset_id.to_raw(), 10_007);
        assert!(!asset_id.is_perp());
        assert!(asset_id.is_spot());
        assert!(!asset_id.is_builder_perp());
        assert!(!asset_id.is_outcome());
        assert_eq!(asset_id.base_index(), 7);
    }

    #[rstest]
    fn test_asset_id_builder_perp() {
        let asset_id = HyperliquidAssetId::builder_perp(1, 7);
        assert_eq!(asset_id.to_raw(), 110_007);
        assert!(!asset_id.is_perp());
        assert!(!asset_id.is_spot());
        assert!(asset_id.is_builder_perp());
        assert!(!asset_id.is_outcome());
        assert_eq!(asset_id.base_index(), 7);
    }

    #[rstest]
    fn test_asset_id_outcome() {
        let asset_id = HyperliquidAssetId::outcome(1, 0);
        assert_eq!(asset_id.to_raw(), 100_000_010);
        assert!(!asset_id.is_perp());
        assert!(!asset_id.is_spot());
        assert!(!asset_id.is_builder_perp());
        assert!(asset_id.is_outcome());
        assert_eq!(asset_id.base_index(), 10);
        assert_eq!(asset_id.outcome_encoding(), Some(10));
        assert_eq!(asset_id.outcome_index(), Some(1));
        assert_eq!(asset_id.outcome_side(), Some(0));
    }

    #[rstest]
    fn test_asset_id_outcome_side_one() {
        let asset_id = HyperliquidAssetId::outcome(3, 1);
        assert_eq!(asset_id.to_raw(), 100_000_031);
        assert!(asset_id.is_outcome());
        assert_eq!(asset_id.outcome_encoding(), Some(31));
        assert_eq!(asset_id.outcome_index(), Some(3));
        assert_eq!(asset_id.outcome_side(), Some(1));
    }

    #[rstest]
    fn test_asset_id_from_outcome_encoding() {
        let asset_id = HyperliquidAssetId::from_outcome_encoding(10).unwrap();
        assert_eq!(asset_id.to_raw(), 100_000_010);
        assert_eq!(asset_id.outcome_index(), Some(1));
        assert_eq!(asset_id.outcome_side(), Some(0));
    }

    #[rstest]
    fn test_asset_id_from_outcome_encoding_rejects_invalid_side() {
        assert_eq!(HyperliquidAssetId::from_outcome_encoding(12), None);
    }

    #[rstest]
    fn test_asset_id_from_outcome_encoding_rejects_overflow() {
        assert_eq!(HyperliquidAssetId::from_outcome_encoding(u32::MAX), None);
    }

    #[rstest]
    #[should_panic(expected = "outcome side must be 0 or 1")]
    fn test_asset_id_outcome_invalid_side() {
        let _ = HyperliquidAssetId::outcome(0, 2);
    }

    #[rstest]
    fn test_asset_id_outcome_accessors_non_outcome() {
        let perp = HyperliquidAssetId::perp(7);
        assert_eq!(perp.outcome_index(), None);
        assert_eq!(perp.outcome_side(), None);

        let spot = HyperliquidAssetId::spot(7);
        assert_eq!(spot.outcome_index(), None);
        assert_eq!(spot.outcome_side(), None);

        let builder = HyperliquidAssetId::builder_perp(1, 7);
        assert_eq!(builder.outcome_index(), None);
        assert_eq!(builder.outcome_side(), None);
    }

    #[rstest]
    fn test_asset_id_outcome_invalid_side_digit_not_outcome() {
        // HIP-4 only defines sides 0 and 1. An id constructed via the public
        // tuple field or deserialized from JSON could carry an invalid side
        // digit (2..=9); these must not classify as a valid outcome and the
        // accessors must not return them.
        for side_digit in 2..=9u32 {
            let raw = HyperliquidAssetId(100_000_000 + side_digit);
            assert!(!raw.is_outcome(), "side digit {side_digit} must reject");
            assert_eq!(raw.outcome_index(), None);
            assert_eq!(raw.outcome_side(), None);
        }
    }

    #[rstest]
    fn test_asset_id_ranges_mutually_exclusive() {
        // Boundary check: a builder-perp id at the high end of its range
        // (just under 100_000_000) must not register as an outcome, and
        // an outcome id at the base (100_000_000) must not register as a
        // builder perp. This guards against the previous open-ended ranges.
        let high_builder = HyperliquidAssetId(99_999_999);
        assert!(high_builder.is_builder_perp());
        assert!(!high_builder.is_outcome());

        let low_outcome = HyperliquidAssetId(100_000_000);
        assert!(!low_outcome.is_builder_perp());
        assert!(low_outcome.is_outcome());
    }
}
