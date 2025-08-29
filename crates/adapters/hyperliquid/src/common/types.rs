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

//! Type definitions for Hyperliquid trading.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{fmt, ops::Deref, str::FromStr};

/// Represents a price with lossless decimal precision.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Price(
    #[serde(
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub Decimal,
);

impl Price {
    pub fn new(value: Decimal) -> Self {
        Self(value)
    }
}

impl fmt::Debug for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Price({})", self.0)
    }
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for Price {
    type Target = Decimal;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Decimal> for Price {
    fn from(value: Decimal) -> Self {
        Self(value)
    }
}

impl FromStr for Price {
    type Err = rust_decimal::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Decimal::from_str(s)?))
    }
}

/// Represents a quantity with lossless decimal precision.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Qty(
    #[serde(
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub Decimal,
);

impl Qty {
    /// Creates a new quantity.
    pub fn new(value: Decimal) -> Self {
        Self(value)
    }
}

impl fmt::Debug for Qty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Qty({})", self.0)
    }
}

impl fmt::Display for Qty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for Qty {
    type Target = Decimal;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Decimal> for Qty {
    fn from(value: Decimal) -> Self {
        Self(value)
    }
}

impl FromStr for Qty {
    type Err = rust_decimal::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Decimal::from_str(s)?))
    }
}

/// Represents a USD amount with lossless decimal precision.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Usd(
    #[serde(
        serialize_with = "crate::common::parse::serialize_decimal_as_str",
        deserialize_with = "crate::common::parse::deserialize_decimal_from_str"
    )]
    pub Decimal,
);

impl Usd {
    /// Creates a new USD amount.
    pub fn new(value: Decimal) -> Self {
        Self(value)
    }
}

impl fmt::Debug for Usd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Usd({})", self.0)
    }
}

impl fmt::Display for Usd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for Usd {
    type Target = Decimal;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Decimal> for Usd {
    fn from(value: Decimal) -> Self {
        Self(value)
    }
}

impl FromStr for Usd {
    type Err = rust_decimal::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Decimal::from_str(s)?))
    }
}

/// Represents an asset ID for Hyperliquid.
///
/// Asset IDs follow Hyperliquid's convention:
/// - Perps: raw index into meta.universe
/// - Spot: 10000 + index in spotMeta.universe
/// - Builder perps: 100000 + dex_index * 10000 + meta_index
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId(pub u32);

impl AssetId {
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

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn test_price_roundtrip() {
        let decimal = Decimal::from_str("12345.678901234567890123").unwrap();
        let price = Price::new(decimal);
        let json = serde_json::to_string(&price).unwrap();
        let parsed: Price = serde_json::from_str(&json).unwrap();
        assert_eq!(price, parsed);
        assert_eq!(json, "\"12345.678901234567890123\"");
    }

    #[test]
    fn test_qty_roundtrip() {
        let decimal = Decimal::from_str("0.00000001").unwrap();
        let qty = Qty::new(decimal);
        let json = serde_json::to_string(&qty).unwrap();
        let parsed: Qty = serde_json::from_str(&json).unwrap();
        assert_eq!(qty, parsed);
    }

    #[test]
    fn test_usd_roundtrip() {
        let decimal = Decimal::from_str("1000.50").unwrap();
        let usd = Usd::new(decimal);
        let json = serde_json::to_string(&usd).unwrap();
        let parsed: Usd = serde_json::from_str(&json).unwrap();
        assert_eq!(usd, parsed);
    }

    #[test]
    fn test_asset_id_perp() {
        let asset_id = AssetId::perp(7);
        assert_eq!(asset_id.to_raw(), 7);
        assert!(!asset_id.is_spot());
        assert!(!asset_id.is_builder_perp());
        assert_eq!(asset_id.base_index(), 7);
    }

    #[test]
    fn test_asset_id_spot() {
        let asset_id = AssetId::spot(7);
        assert_eq!(asset_id.to_raw(), 10_007);
        assert!(asset_id.is_spot());
        assert!(!asset_id.is_builder_perp());
        assert_eq!(asset_id.base_index(), 7);
    }

    #[test]
    fn test_asset_id_builder_perp() {
        let asset_id = AssetId::builder_perp(1, 7);
        assert_eq!(asset_id.to_raw(), 110_007);
        assert!(!asset_id.is_spot());
        assert!(asset_id.is_builder_perp());
        assert_eq!(asset_id.base_index(), 7);
    }
}
