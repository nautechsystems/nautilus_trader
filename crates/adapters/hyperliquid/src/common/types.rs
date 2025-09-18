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

use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};

/// Represents an asset ID for Hyperliquid.
///
/// Asset IDs follow Hyperliquid's convention:
/// - Perps: raw index into meta.universe
/// - Spot: 10000 + index in spotMeta.universe
/// - Builder perps: 100000 + dex_index * 10000 + meta_index
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HyperliquidAssetId(pub u32);

impl HyperliquidAssetId {
    /// Creates a perpetual asset ID from raw index.
    pub fn perp(index: u32) -> Self {
        Self(index)
    }

    /// Creates a spot asset ID (10000 + index).
    pub fn spot(index: u32) -> Self {
        Self(10_000 + index)
    }

    /// Creates a builder perpetual asset ID.
    pub fn builder_perp(dex_index: u32, meta_index: u32) -> Self {
        Self(100_000 + dex_index * 10_000 + meta_index)
    }

    /// Checks if this is a spot asset (>= 10000).
    pub fn is_spot(self) -> bool {
        self.0 >= 10_000 && self.0 < 100_000
    }

    /// Checks if this is a builder perp (>= 100000).
    pub fn is_builder_perp(self) -> bool {
        self.0 >= 100_000
    }

    /// Gets the base index for spot assets (asset_id - 10000).
    /// For perps, returns the raw index.
    pub fn base_index(self) -> u32 {
        if self.is_spot() {
            self.0 - 10_000
        } else if self.is_builder_perp() {
            // For builder perps, return the meta_index
            (self.0 - 100_000) % 10_000
        } else {
            self.0
        }
    }

    /// Gets the raw asset ID value.
    pub fn to_raw(self) -> u32 {
        self.0
    }
}

impl Display for HyperliquidAssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_asset_id_perp() {
        let asset_id = HyperliquidAssetId::perp(7);
        assert_eq!(asset_id.to_raw(), 7);
        assert!(!asset_id.is_spot());
        assert!(!asset_id.is_builder_perp());
        assert_eq!(asset_id.base_index(), 7);
    }

    #[rstest]
    fn test_asset_id_spot() {
        let asset_id = HyperliquidAssetId::spot(7);
        assert_eq!(asset_id.to_raw(), 10_007);
        assert!(asset_id.is_spot());
        assert!(!asset_id.is_builder_perp());
        assert_eq!(asset_id.base_index(), 7);
    }

    #[rstest]
    fn test_asset_id_builder_perp() {
        let asset_id = HyperliquidAssetId::builder_perp(1, 7);
        assert_eq!(asset_id.to_raw(), 110_007);
        assert!(!asset_id.is_spot());
        assert!(asset_id.is_builder_perp());
        assert_eq!(asset_id.base_index(), 7);
    }
}
