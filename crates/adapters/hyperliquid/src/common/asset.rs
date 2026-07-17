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

//! Unified asset representation for all Hyperliquid market assets.

use std::fmt::{self, Display};

use ahash::AHashMap;
use nautilus_core::correctness::{
    CorrectnessResult, CorrectnessResultExt, FAILED, check_predicate_false,
};
use nautilus_model::identifiers::{InstrumentId, Symbol};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display as StrumDisplay, EnumIter, EnumString};
use ustr::Ustr;

use super::consts::HYPERLIQUID_VENUE;

const HIP_1_SPOT_BASE: u32 = 10_000;
const HIP_3_BUILDER_PERP_BASE: u32 = 100_000;
const HIP_4_OUTCOME_BASE: u32 = 100_000_000;

// ─── HyperliquidProductType ─────────────────────────────────────────────────

/// Hyperliquid product type.
#[derive(
    Copy,
    Clone,
    Debug,
    StrumDisplay,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.adapters.hyperliquid")
)]
#[serde(rename_all = "UPPERCASE")]
#[strum(serialize_all = "UPPERCASE")]
pub enum HyperliquidProductType {
    /// Perpetual futures.
    Perp,
    /// Spot markets.
    Spot,
    /// HIP-3 builder-deployed perpetual.
    BuilderPerp,
    /// HIP-4 binary outcome side tokens.
    Outcome,
    /// Vault LP tokens.
    Vault,
}

impl HyperliquidProductType {
    /// Extract product type from an instrument symbol.
    ///
    /// Accepts both Nautilus instrument symbols (`{BASE}-USD-PERP`,
    /// `{BASE}-{QUOTE}-SPOT`, `{N}-{YES|NO}-OUTCOME`) and venue wire coin
    /// names (`#<encoding>` / `+<encoding>` for HIP-4 outcomes).
    ///
    /// # Errors
    ///
    /// Returns error if symbol doesn't match any expected format.
    pub fn from_instrument_symbol(symbol: &str) -> anyhow::Result<Self> {
        if symbol.ends_with("-PERP") {
            if symbol.contains(':') {
                Ok(Self::BuilderPerp)
            } else {
                Ok(Self::Perp)
            }
        } else if symbol.ends_with("-SPOT") {
            if symbol.starts_with("vntls:") {
                Ok(Self::Vault)
            } else {
                Ok(Self::Spot)
            }
        } else if symbol.ends_with("-OUTCOME") || is_outcome_wire_symbol(symbol) {
            Ok(Self::Outcome)
        } else {
            anyhow::bail!("Invalid Hyperliquid symbol format: {symbol}")
        }
    }

    /// Deterministic detection from unambiguous coin prefixes.
    ///
    /// Returns `None` for plain names (ambiguous — caller must supply context).
    #[must_use]
    pub fn try_coin(coin: &str) -> Option<Self> {
        if coin.starts_with('#') || coin.starts_with('+') {
            Some(Self::Outcome)
        } else if coin.starts_with("vntls:") {
            Some(Self::Vault)
        } else if coin.starts_with('@') {
            Some(Self::Spot)
        } else if coin.contains(':') {
            Some(Self::BuilderPerp)
        } else {
            None
        }
    }
}

/// Checks whether `symbol` is an outcome wire format (`#N` or `+N`).
fn is_outcome_wire_symbol(symbol: &str) -> bool {
    let Some(rest) = symbol
        .strip_prefix('#')
        .or_else(|| symbol.strip_prefix('+'))
    else {
        return false;
    };
    !rest.is_empty() && rest.parse::<u32>().is_ok()
}

// ─── HyperliquidAssetId ─────────────────────────────────────────────────────

/// Sentinel asset ID for vault LP tokens which have no wire index.
pub const VAULT_ASSET_ID: u32 = u32::MAX;

/// Numeric asset identifier on the Hyperliquid order wire.
///
/// Encodes the asset kind in its range:
/// - Perps: `0..10_000`
/// - Spot: `10_000 + pair_index`
/// - HIP-3 builder perps: `100_000 + dex_index * 10_000 + meta_index`
/// - HIP-4 outcomes: `100_000_000 + 10 * outcome + side`
/// - Vault LP tokens: [`VAULT_ASSET_ID`]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HyperliquidProductId(pub u32);

impl HyperliquidProductId {
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

    /// Checks if this is a perp asset (`< 10_000`).
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
    pub fn is_outcome(self) -> bool {
        self.0 >= HIP_4_OUTCOME_BASE && (self.0 - HIP_4_OUTCOME_BASE) % 10 <= 1
    }

    /// Gets the base index for the asset.
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

    /// Outcome coin form: `"#970"`.
    #[must_use]
    pub fn to_outcome_coin(self) -> Option<String> {
        self.outcome_encoding().map(|e| format!("#{e}"))
    }

    /// Outcome token form: `"+970"`.
    #[must_use]
    pub fn to_outcome_token(self) -> Option<String> {
        self.outcome_encoding().map(|e| format!("+{e}"))
    }

    /// Outcome → Nautilus `InstrumentId` (`"97-YES-OUTCOME.HYPERLIQUID"`).
    #[must_use]
    pub fn to_outcome_instrument_id(self) -> Option<InstrumentId> {
        let encoding = self.outcome_encoding()?;
        let outcome_index = encoding / 10;
        let side = (encoding % 10) as u8;
        let label = if side == 0 { "YES" } else { "NO" };
        let symbol = format!("{outcome_index}-{label}-OUTCOME");
        Some(InstrumentId::new(
            nautilus_model::identifiers::Symbol::new(&symbol),
            *HYPERLIQUID_VENUE,
        ))
    }

    /// Parse from outcome wire format (`"#970"` or `"+970"`).
    ///
    /// Returns `None` if not a valid `#`/`+` encoded outcome.
    #[must_use]
    pub fn from_outcome_wire(coin: &str) -> Option<Self> {
        let rest = coin.strip_prefix('#').or_else(|| coin.strip_prefix('+'))?;
        if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let encoding = rest.parse::<u32>().ok()?;
        Self::from_outcome_encoding(encoding)
    }

    /// Parse from Nautilus outcome symbol (`"97-YES-OUTCOME"` or `"97-NO-OUTCOME"`).
    ///
    /// Returns `None` if the symbol doesn't match the outcome pattern.
    #[must_use]
    pub fn from_outcome_symbol(symbol: &str) -> Option<Self> {
        let rest = symbol.strip_suffix("-OUTCOME")?;
        let (index_str, side_str) = rest.rsplit_once('-')?;
        let outcome_index = index_str.parse::<u32>().ok()?;
        let side: u8 = match side_str {
            "YES" => 0,
            "NO" => 1,
            _ => return None,
        };
        let encoding = outcome_index
            .checked_mul(10)?
            .checked_add(u32::from(side))?;
        Self::from_outcome_encoding(encoding)
    }

    /// Parse from either Nautilus symbol or wire format.
    ///
    /// Tries `from_outcome_symbol` first, then `from_outcome_wire`.
    #[must_use]
    pub fn from_outcome_instrument_id(instrument_id: InstrumentId) -> Option<Self> {
        let symbol = instrument_id.symbol.as_str();
        Self::from_outcome_symbol(symbol).or_else(|| Self::from_outcome_wire(symbol))
    }
}

impl Display for HyperliquidProductId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The market type of a Hyperliquid asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HyperliquidProduct {
    /// Standard perpetual future
    Perp,
    /// Spot token
    Spot {
        /// The `@{pair_index}` used in spot fill messages.
        pair_index: u32,
        /// Quote currency
        quote: Ustr,
    },
    /// HIP-3 builder-deployed perpetual
    BuilderPerp {
        /// The builder dex index
        dex_index: u32,
    },
    /// HIP-4 binary outcome side token
    Outcome {
        /// The outcome market number.
        outcome_index: u32,
        /// `0` or `1` — the side of the binary outcome.
        side: u8,
        /// Side label (e.g. "YES", "NO", or custom from market spec).
        side_label: Ustr,
    },
    /// Vault LP token (asset ID [`VAULT_ASSET_ID`]).
    Vault,
}

impl From<&HyperliquidProduct> for HyperliquidProductType {
    fn from(product: &HyperliquidProduct) -> Self {
        match product {
            HyperliquidProduct::Perp => Self::Perp,
            HyperliquidProduct::BuilderPerp { .. } => Self::BuilderPerp,
            HyperliquidProduct::Spot { .. } => Self::Spot,
            HyperliquidProduct::Vault => Self::Vault,
            HyperliquidProduct::Outcome { .. } => Self::Outcome,
        }
    }
}

/// Unified representation of any asset on Hyperliquid.
///
/// Connects the numeric wire index and the coin string used in API responses
/// so callers never need to maintain parallel lookup maps.
///
/// ```text
/// Context        │ Perp │ Spot   │ Builder perp │ Outcome         │ Vault
/// ───────────────┼──────┼────────┼──────────────┼───────────────--┼───────────
/// Clearinghouse  │ BTC  │ HYPE   │ xyz:TSLA     │ #970          │ —
/// Spot balances  │ —    │ HYPE   │ —            │ #970 / +970   │ vntls:X
/// Spot fills     │ —    │ @7     │ —            │ —               │ —
/// Trades WS      │ BTC  │ HYPE   │ xyz:TSLA     │ —               │ —
/// Place Order    │ 0    │ 10_007 │ 110_001      │ 100_000_970     │ —
/// ```
///
/// Vault LP tokens (no wire index): represent shares in Hyperliquid vaults
///
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HyperliquidAsset {
    /// Numeric index used on the order wire. [`VAULT_ASSET_ID`] for vault tokens.
    id: HyperliquidProductId,
    /// Coin string as it appears in most API responses (e.g. BTC, #970, xyz:TSLA).
    coin: Ustr,
    /// The asset product with product-specific metadata.
    product: HyperliquidProduct,
}

impl HyperliquidAsset {
    /// Creates a new [`HyperliquidAsset`] with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `coin` contains wildcard characters (`*`, `?`).
    pub fn new_checked(
        id: HyperliquidProductId,
        coin: Ustr,
        product: HyperliquidProduct,
    ) -> CorrectnessResult<Self> {
        check_predicate_false(
            coin.as_str().bytes().any(|b| b == b'*' || b == b'?'),
            &format!("coin `{coin}` contains wildcard characters"),
        )?;
        Ok(Self { id, coin, product })
    }

    /// Creates a new [`HyperliquidAsset`].
    ///
    /// # Panics
    ///
    /// Panics if `coin` contains wildcard characters (`*`, `?`).
    pub fn new(id: HyperliquidProductId, coin: Ustr, product: HyperliquidProduct) -> Self {
        Self::new_checked(id, coin, product).expect_display(FAILED)
    }

    /// Standard perpetual.
    pub fn perp(asset_index: u32, coin: Ustr) -> Self {
        Self::new(
            HyperliquidProductId::perp(asset_index),
            coin,
            HyperliquidProduct::Perp,
        )
    }

    /// Spot token.
    pub fn spot(asset_index: u32, pair_index: u32, coin: Ustr, quote: Ustr) -> Self {
        Self::new(
            HyperliquidProductId::spot(asset_index),
            coin,
            HyperliquidProduct::Spot { pair_index, quote },
        )
    }

    /// HIP-3 builder perpetual.
    pub fn builder_perp(dex_index: u32, meta_index: u32, coin: Ustr) -> Self {
        Self::new(
            HyperliquidProductId::builder_perp(dex_index, meta_index),
            coin,
            HyperliquidProduct::BuilderPerp { dex_index },
        )
    }

    /// HIP-4 outcome side token.
    pub fn outcome(outcome_index: u32, side: u8, coin: Ustr, side_label: Ustr) -> Self {
        Self::new(
            HyperliquidProductId::outcome(outcome_index, side),
            coin,
            HyperliquidProduct::Outcome {
                outcome_index,
                side,
                side_label,
            },
        )
    }

    /// Vault LP token.
    pub fn vault(coin: Ustr) -> Self {
        Self::new(
            HyperliquidProductId(VAULT_ASSET_ID),
            coin,
            HyperliquidProduct::Vault,
        )
    }

    /// Attempts to construct a vault asset if `coin` has the `vntls:` prefix.
    /// Returns `None` if the coin is not a vault token.
    #[must_use]
    pub fn try_vault(coin: &Ustr) -> Option<Self> {
        coin.as_str()
            .starts_with("vntls:")
            .then(|| Self::vault(*coin))
    }

    /// The numeric wire asset ID.
    #[must_use]
    pub fn id(&self) -> HyperliquidProductId {
        self.id
    }

    /// The coin string as it appears in most API responses.
    #[must_use]
    pub fn coin(&self) -> Ustr {
        self.coin
    }

    /// The asset product.
    #[must_use]
    pub fn product(&self) -> &HyperliquidProduct {
        &self.product
    }

    /// The outcome token form (+970), or `None` for non-outcome assets.
    #[must_use]
    pub fn outcome_token(&self) -> Option<String> {
        self.id
            .outcome_encoding()
            .map(|encoding| format!("+{encoding}"))
    }

    /// The outcome coin form (#970), or `None` for non-outcome assets.
    #[must_use]
    pub fn outcome_coin(&self) -> Option<String> {
        self.id
            .outcome_encoding()
            .map(|encoding| format!("#{encoding}"))
    }

    /// The spot fill coin form (@7), or `None` for non-spot assets.
    #[must_use]
    pub fn fill_coin(&self) -> Option<String> {
        match self.product {
            HyperliquidProduct::Spot { pair_index, .. } => Some(format!("@{pair_index}")),
            _ => None,
        }
    }

    /// Derives the Nautilus `InstrumentId` from this asset's properties.
    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        let symbol = match &self.product {
            HyperliquidProduct::Perp | HyperliquidProduct::BuilderPerp { .. } => {
                format!("{}-USD-PERP", self.coin)
            }
            HyperliquidProduct::Spot { quote, .. } => {
                format!("{}-{quote}-SPOT", self.coin)
            }
            HyperliquidProduct::Outcome {
                outcome_index,
                side,
                ..
            } => {
                let label = if *side == 0 { "YES" } else { "NO" };
                format!("{outcome_index}-{label}-OUTCOME")
            }
            HyperliquidProduct::Vault => {
                // "vntls:vCURSOR" → "vntls:vCURSOR-USDC-SPOT"
                format!("{}-USDC-SPOT", self.coin)
            }
        };

        InstrumentId::new(Symbol::new(&symbol), *HYPERLIQUID_VENUE)
    }

    #[must_use]
    pub fn is_perp(&self) -> bool {
        matches!(self.product, HyperliquidProduct::Perp)
    }

    #[must_use]
    pub fn is_spot(&self) -> bool {
        matches!(self.product, HyperliquidProduct::Spot { .. })
    }

    #[must_use]
    pub fn is_builder_perp(&self) -> bool {
        matches!(self.product, HyperliquidProduct::BuilderPerp { .. })
    }

    #[must_use]
    pub fn is_outcome(&self) -> bool {
        matches!(self.product, HyperliquidProduct::Outcome { .. })
    }

    #[must_use]
    pub fn is_vault(&self) -> bool {
        matches!(self.product, HyperliquidProduct::Vault)
    }
}

impl fmt::Display for HyperliquidAsset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.coin, self.id)
    }
}

/// Bidirectional index of all known Hyperliquid assets.
///
#[derive(Debug, Default, Clone)]
pub struct HyperLiquidAssetRegistry {
    /// Canonical storage — one copy per asset.
    assets: AHashMap<HyperliquidProductId, HyperliquidAsset>,
    /// Coin → asset IDs (multiple possible for same coin across product types).
    by_coin: AHashMap<Ustr, Vec<HyperliquidProductId>>,
    /// Spot fill coin (@7) → asset ID.
    by_fill_coin: AHashMap<Ustr, HyperliquidProductId>,
    /// InstrumentId → asset ID.
    by_instrument_id: AHashMap<InstrumentId, HyperliquidProductId>,
}

impl HyperLiquidAssetRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an asset, indexing it by all known representations.
    pub fn register(&mut self, asset: HyperliquidAsset) {
        let id = asset.id();

        self.by_coin.entry(asset.coin()).or_default().push(id);

        if let Some(fill_coin) = asset.fill_coin() {
            self.by_fill_coin.insert(Ustr::from(&fill_coin), id);
        }

        // Outcome coin form (#970) is the canonical coin, already in by_coin.
        // Outcome token form (+970) is handled in resolve() by converting to #.

        self.by_instrument_id.insert(asset.instrument_id(), id);
        self.assets.insert(id, asset);
    }

    /// Looks up an asset by numeric wire ID.
    #[must_use]
    pub fn get_by_id(&self, id: HyperliquidProductId) -> Option<&HyperliquidAsset> {
        self.assets.get(&id)
    }

    /// Looks up an asset by Nautilus instrument ID.
    #[must_use]
    pub fn get_by_instrument_id(&self, instrument_id: &InstrumentId) -> Option<&HyperliquidAsset> {
        self.by_instrument_id
            .get(instrument_id)
            .and_then(|id| self.assets.get(id))
    }

    /// Returns the wire asset index for a Nautilus instrument ID.
    #[must_use]
    pub fn asset_index(&self, instrument_id: &InstrumentId) -> Option<u32> {
        self.by_instrument_id
            .get(instrument_id)
            .map(|id| id.to_raw())
    }

    /// Resolves a coin with a product type hint.
    #[must_use]
    pub fn resolve_with_product_type(
        &self,
        coin: &Ustr,
        product_type: HyperliquidProductType,
    ) -> Option<&HyperliquidAsset> {
        self.by_coin.get(coin)?.iter().find_map(|id| {
            let asset = self.assets.get(id)?;
            let matches = match product_type {
                HyperliquidProductType::Perp => {
                    matches!(asset.product(), HyperliquidProduct::Perp)
                }
                HyperliquidProductType::BuilderPerp => {
                    matches!(asset.product(), HyperliquidProduct::BuilderPerp { .. })
                }
                HyperliquidProductType::Spot => {
                    matches!(asset.product(), HyperliquidProduct::Spot { .. })
                }
                HyperliquidProductType::Vault => {
                    matches!(asset.product(), HyperliquidProduct::Vault)
                }
                HyperliquidProductType::Outcome => {
                    matches!(asset.product(), HyperliquidProduct::Outcome { .. })
                }
            };
            matches.then_some(asset)
        })
    }

    /// Number of registered assets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// Clears all entries.
    pub fn clear(&mut self) {
        self.assets.clear();
        self.by_coin.clear();
        self.by_fill_coin.clear();
        self.by_instrument_id.clear();
    }

    /// Returns an iterator over all registered assets.
    pub fn iter(&self) -> impl Iterator<Item = &HyperliquidAsset> {
        self.assets.values()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_perp_asset() {
        let asset = HyperliquidAsset::perp(0, Ustr::from("BTC"));

        assert!(asset.is_perp());
        assert!(!asset.is_spot());
        assert!(!asset.is_builder_perp());
        assert!(!asset.is_outcome());
        assert!(!asset.is_vault());
        assert_eq!(asset.coin(), Ustr::from("BTC"));
        assert_eq!(asset.id().to_raw(), 0);
        assert_eq!(asset.fill_coin(), None);
        assert_eq!(asset.outcome_token(), None);
        assert_eq!(
            asset.instrument_id().to_string(),
            "BTC-USD-PERP.HYPERLIQUID"
        );
    }

    #[rstest]
    #[should_panic(expected = "Condition failed")]
    fn test_builder_perp_rejects_wildcards() {
        let _ = HyperliquidAsset::builder_perp(1, 0, Ustr::from("dex:STREAMABCD****"));
    }

    #[rstest]
    fn test_spot_asset() {
        let asset = HyperliquidAsset::spot(7, 7, Ustr::from("HYPE"), Ustr::from("USDC"));

        assert!(asset.is_spot());
        assert_eq!(asset.fill_coin(), Some("@7".to_string()));
        assert_eq!(asset.id().to_raw(), 10_007);
        assert_eq!(
            asset.instrument_id().to_string(),
            "HYPE-USDC-SPOT.HYPERLIQUID"
        );
    }

    #[rstest]
    fn test_builder_perp_asset() {
        let asset = HyperliquidAsset::builder_perp(1, 1, Ustr::from("xyz:TSLA"));

        assert!(asset.is_builder_perp());
        assert_eq!(asset.coin(), Ustr::from("xyz:TSLA"));
        assert_eq!(asset.id().to_raw(), 110_001);
    }

    #[rstest]
    fn test_outcome_asset() {
        let asset = HyperliquidAsset::outcome(97, 0, Ustr::from("#970"), Ustr::from("YES"));

        assert!(asset.is_outcome());
        assert_eq!(asset.coin(), Ustr::from("#970"));
        assert_eq!(asset.outcome_token(), Some("+970".to_string()));
        assert_eq!(asset.id().to_raw(), 100_000_970);
        assert_eq!(
            asset.instrument_id().to_string(),
            "97-YES-OUTCOME.HYPERLIQUID"
        );
    }

    #[rstest]
    fn test_vault_asset() {
        let asset = HyperliquidAsset::vault(Ustr::from("vntls:vCURSOR"));

        assert!(asset.is_vault());
        assert_eq!(asset.id().to_raw(), VAULT_ASSET_ID);
    }

    #[rstest]
    fn test_registry_resolve_all_formats() {
        let mut registry = HyperLiquidAssetRegistry::new();

        registry.register(HyperliquidAsset::perp(0, Ustr::from("BTC")));
        registry.register(HyperliquidAsset::spot(
            7,
            7,
            Ustr::from("HYPE"),
            Ustr::from("USDC"),
        ));
        registry.register(HyperliquidAsset::builder_perp(1, 1, Ustr::from("xyz:TSLA")));
        registry.register(HyperliquidAsset::outcome(
            97,
            0,
            Ustr::from("#970"),
            Ustr::from("YES"),
        ));
        registry.register(HyperliquidAsset::vault(Ustr::from("vntls:vCURSOR")));

        assert_eq!(registry.len(), 5);

        // Lookup by product type
        assert_eq!(
            registry
                .resolve_with_product_type(&Ustr::from("BTC"), HyperliquidProductType::Perp)
                .unwrap()
                .coin(),
            Ustr::from("BTC"),
        );
        assert_eq!(
            registry
                .resolve_with_product_type(
                    &Ustr::from("xyz:TSLA"),
                    HyperliquidProductType::BuilderPerp
                )
                .unwrap()
                .coin(),
            Ustr::from("xyz:TSLA"),
        );
        assert_eq!(
            registry
                .resolve_with_product_type(&Ustr::from("#970"), HyperliquidProductType::Outcome)
                .unwrap()
                .coin(),
            Ustr::from("#970"),
        );
        assert_eq!(
            registry
                .resolve_with_product_type(&Ustr::from("HYPE"), HyperliquidProductType::Spot)
                .unwrap()
                .coin(),
            Ustr::from("HYPE"),
        );
        assert_eq!(
            registry
                .resolve_with_product_type(
                    &Ustr::from("vntls:vCURSOR"),
                    HyperliquidProductType::Vault
                )
                .unwrap()
                .coin(),
            Ustr::from("vntls:vCURSOR"),
        );

        // Unknown
        assert!(
            registry
                .resolve_with_product_type(&Ustr::from("UNKNOWN"), HyperliquidProductType::Perp)
                .is_none()
        );

        // By ID
        assert!(registry.get_by_id(HyperliquidProductId::perp(0)).is_some());
        assert!(
            registry
                .get_by_id(HyperliquidProductId(VAULT_ASSET_ID))
                .is_some()
        );

        // By InstrumentId
        let btc_id = HyperliquidAsset::perp(0, Ustr::from("BTC")).instrument_id();
        assert!(registry.get_by_instrument_id(&btc_id).is_some());
        assert_eq!(registry.asset_index(&btc_id), Some(0));
    }

    #[rstest]
    fn test_registry_coin_collision_both_registered() {
        let mut registry = HyperLiquidAssetRegistry::new();

        // Same coin "BTC" as both perp and spot — both should register fine
        registry.register(HyperliquidAsset::spot(
            42,
            42,
            Ustr::from("BTC"),
            Ustr::from("USDC"),
        ));
        registry.register(HyperliquidAsset::perp(0, Ustr::from("BTC")));
        assert_eq!(registry.len(), 2);

        // resolve_with_product_type picks the right one
        let perp = registry
            .resolve_with_product_type(&Ustr::from("BTC"), HyperliquidProductType::Perp)
            .unwrap();
        assert!(perp.is_perp());

        let spot = registry
            .resolve_with_product_type(&Ustr::from("BTC"), HyperliquidProductType::Spot)
            .unwrap();
        assert!(spot.is_spot());
    }

    #[rstest]
    #[case("BTC-USD-PERP", HyperliquidProductType::Perp)]
    #[case("xyz:TSLA-USD-PERP", HyperliquidProductType::BuilderPerp)]
    #[case("HYPE-USDC-SPOT", HyperliquidProductType::Spot)]
    #[case("vntls:vCURSOR-USDC-SPOT", HyperliquidProductType::Vault)]
    #[case("25-YES-OUTCOME", HyperliquidProductType::Outcome)]
    #[case("25-NO-OUTCOME", HyperliquidProductType::Outcome)]
    #[case("0-YES-OUTCOME", HyperliquidProductType::Outcome)]
    #[case("#10", HyperliquidProductType::Outcome)]
    #[case("+31", HyperliquidProductType::Outcome)]
    #[case("#0", HyperliquidProductType::Outcome)]
    fn test_product_type_from_symbol(
        #[case] symbol: &str,
        #[case] expected: HyperliquidProductType,
    ) {
        assert_eq!(
            HyperliquidProductType::from_instrument_symbol(symbol).unwrap(),
            expected
        );
    }

    #[rstest]
    #[case("")]
    #[case("BTC")]
    #[case("#")]
    #[case("+")]
    #[case("#abc")]
    #[case("+12.5")]
    #[case("@1")]
    #[case("#-1")]
    #[case("+-1")]
    #[case("25-YES")]
    #[case("OUTCOME")]
    #[case("25-YES-outcome")]
    fn test_product_type_from_symbol_rejects_invalid(#[case] symbol: &str) {
        assert!(HyperliquidProductType::from_instrument_symbol(symbol).is_err());
    }

    #[rstest]
    #[case("#970", Some(HyperliquidProductType::Outcome))]
    #[case("+500", Some(HyperliquidProductType::Outcome))]
    #[case("vntls:vCURSOR", Some(HyperliquidProductType::Vault))]
    #[case("@107", Some(HyperliquidProductType::Spot))]
    #[case("xyz:TSLA", Some(HyperliquidProductType::BuilderPerp))]
    #[case("BTC", None)]
    #[case("HYPE", None)]
    #[case("ETH", None)]
    fn test_product_type_from_coin(
        #[case] coin: &str,
        #[case] expected: Option<HyperliquidProductType>,
    ) {
        assert_eq!(HyperliquidProductType::try_coin(coin), expected);
    }
}
