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

//! Bidirectional mapping between Nautilus `InstrumentId` and Lighter `market_index`.
//!
//! Lighter identifies markets by a 16-bit `market_index` (perpetuals occupy
//! `0..=254`, spot markets `2048..=4094`). The mapping is populated at
//! bootstrap from `GET /api/v1/orderBookDetails` and subsequently consulted
//! on every WebSocket frame and outbound transaction.

use dashmap::DashMap;
use nautilus_model::identifiers::{InstrumentId, Symbol};
use ustr::Ustr;

use super::{consts::LIGHTER_VENUE, enums::LighterProductType};

/// Suffix applied to perpetual instrument symbols on the Nautilus side.
pub const PERP_SUFFIX: &str = "-PERP";

/// Suffix applied to spot instrument symbols on the Nautilus side.
pub const SPOT_SUFFIX: &str = "-SPOT";

/// Builds a Nautilus [`InstrumentId`] from a venue symbol and product type.
///
/// The venue symbol is upper-cased and combined with the product suffix
/// (`-PERP` or `-SPOT`) before being qualified by the Lighter venue.
#[must_use]
pub fn format_instrument_id(venue_symbol: &str, product_type: LighterProductType) -> InstrumentId {
    let suffix = product_suffix(product_type);
    let trimmed = venue_symbol.trim();
    let upper = trimmed.to_ascii_uppercase();
    let symbol = format!("{upper}{suffix}");
    InstrumentId::new(Symbol::from_str_unchecked(&symbol), *LIGHTER_VENUE)
}

/// Returns the venue-native symbol for an instrument id by stripping any
/// known product suffix. Returns the raw symbol unchanged when no suffix
/// is present.
#[must_use]
pub fn format_venue_symbol(instrument_id: &InstrumentId) -> &str {
    let s = instrument_id.symbol.as_str();
    s.strip_suffix(PERP_SUFFIX)
        .or_else(|| s.strip_suffix(SPOT_SUFFIX))
        .unwrap_or(s)
}

/// Returns the [`LighterProductType`] implied by the instrument id's suffix,
/// or `None` if the symbol carries neither suffix.
#[must_use]
pub fn product_type_from_instrument_id(instrument_id: &InstrumentId) -> Option<LighterProductType> {
    let s = instrument_id.symbol.as_str();
    if s.ends_with(PERP_SUFFIX) {
        Some(LighterProductType::Perp)
    } else if s.ends_with(SPOT_SUFFIX) {
        Some(LighterProductType::Spot)
    } else {
        None
    }
}

const fn product_suffix(product_type: LighterProductType) -> &'static str {
    match product_type {
        LighterProductType::Perp => PERP_SUFFIX,
        LighterProductType::Spot => SPOT_SUFFIX,
    }
}

fn canonical_symbol_key(venue_symbol: &str) -> Ustr {
    Ustr::from(&venue_symbol.trim().to_ascii_uppercase())
}

/// Registry mapping `market_index` to `InstrumentId` and back.
///
/// Indexed for `O(1)` lookup by all three keys the adapter switches between:
/// the venue's numeric `market_index` (used in transaction encoding and
/// WebSocket subscriptions), the Nautilus [`InstrumentId`] (used by the
/// engine), and the raw venue symbol scoped by product type (used when
/// parsing REST list responses). Designed to be shared across the HTTP and
/// WebSocket clients via `Arc`.
///
/// The registry is intended for write-once bootstrap followed by read-only
/// consumption: each individual lookup is lock-free, but a single `insert`
/// is not transactional across the three indexes. Rare write events such
/// as relists must be coordinated by the caller (e.g. quiesce consumers
/// before reinserting) to avoid concurrent readers observing partial
/// state.
#[derive(Debug, Default)]
pub struct MarketRegistry {
    by_index: DashMap<i16, InstrumentId>,
    by_id: DashMap<InstrumentId, i16>,
    by_raw_symbol: DashMap<(Ustr, LighterProductType), InstrumentId>,
}

impl MarketRegistry {
    /// Returns a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a market and returns the resulting [`InstrumentId`].
    ///
    /// Re-inserting the same `market_index` overwrites the previous mapping;
    /// callers may use this to handle venue rename or relisting events. Stale
    /// entries in the inverse indexes are evicted before the new mapping is
    /// installed so all three lookups stay consistent.
    pub fn insert(
        &self,
        market_index: i16,
        venue_symbol: &str,
        product_type: LighterProductType,
    ) -> InstrumentId {
        let instrument_id = format_instrument_id(venue_symbol, product_type);
        let canonical = canonical_symbol_key(venue_symbol);

        // Evict any prior mapping that shared this market_index but pointed
        // at a different InstrumentId.
        if let Some((_, prior_id)) = self.by_index.remove(&market_index)
            && prior_id != instrument_id
        {
            self.by_id
                .remove_if(&prior_id, |_, idx| *idx == market_index);
            if let Some(prior_pt) = product_type_from_instrument_id(&prior_id) {
                let prior_key = Ustr::from(format_venue_symbol(&prior_id));
                self.by_raw_symbol
                    .remove_if(&(prior_key, prior_pt), |_, id| *id == prior_id);
            }
        }

        // Evict any prior mapping that shared this InstrumentId but pointed
        // at a different market_index.
        if let Some((_, prior_index)) = self.by_id.remove(&instrument_id)
            && prior_index != market_index
        {
            self.by_index
                .remove_if(&prior_index, |_, id| *id == instrument_id);
        }

        self.by_index.insert(market_index, instrument_id);
        self.by_id.insert(instrument_id, market_index);
        self.by_raw_symbol
            .insert((canonical, product_type), instrument_id);
        instrument_id
    }

    /// Returns the [`InstrumentId`] for a given `market_index`.
    #[must_use]
    pub fn instrument_id(&self, market_index: i16) -> Option<InstrumentId> {
        self.by_index.get(&market_index).map(|e| *e)
    }

    /// Returns every registered `market_index`.
    ///
    /// Callers iterating across all venue markets (e.g. the mass-status
    /// reconciliation path) use this to bound the per-market REST fan-out.
    #[must_use]
    pub fn all_market_indices(&self) -> Vec<i16> {
        self.by_index.iter().map(|e| *e.key()).collect()
    }

    /// Returns the venue `market_index` for a given [`InstrumentId`].
    #[must_use]
    pub fn market_index(&self, instrument_id: &InstrumentId) -> Option<i16> {
        self.by_id.get(instrument_id).map(|e| *e)
    }

    /// Returns the [`InstrumentId`] for a raw venue symbol scoped by product.
    ///
    /// The raw symbol on its own is ambiguous when the venue lists the same
    /// asset on both perpetual and spot; the product discriminant resolves it.
    #[must_use]
    pub fn instrument_id_by_symbol(
        &self,
        venue_symbol: &str,
        product_type: LighterProductType,
    ) -> Option<InstrumentId> {
        let key = canonical_symbol_key(venue_symbol);
        self.by_raw_symbol.get(&(key, product_type)).map(|e| *e)
    }

    /// Removes all registered markets.
    pub fn clear(&self) {
        self.by_index.clear();
        self.by_id.clear();
        self.by_raw_symbol.clear();
    }

    /// Returns the number of registered markets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_index.len()
    }

    /// Returns whether the registry has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_index.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn format_instrument_id_perp_uppercases_and_suffixes() {
        let id = format_instrument_id("eth", LighterProductType::Perp);
        assert_eq!(id.symbol.as_str(), "ETH-PERP");
        assert_eq!(id.venue, *LIGHTER_VENUE);
    }

    #[rstest]
    fn format_instrument_id_spot_uppercases_and_suffixes() {
        let id = format_instrument_id("usdc", LighterProductType::Spot);
        assert_eq!(id.symbol.as_str(), "USDC-SPOT");
    }

    #[rstest]
    #[case::leading("  ETH", "ETH-PERP")]
    #[case::trailing("ETH  ", "ETH-PERP")]
    #[case::both_sides("  eth  ", "ETH-PERP")]
    #[case::tab("\tBTC\n", "BTC-PERP")]
    fn format_instrument_id_trims_whitespace(#[case] input: &str, #[case] expected_symbol: &str) {
        let id = format_instrument_id(input, LighterProductType::Perp);
        assert_eq!(id.symbol.as_str(), expected_symbol);
        assert_eq!(id.venue, *LIGHTER_VENUE);
    }

    #[rstest]
    fn format_venue_symbol_strips_known_suffixes() {
        let perp = format_instrument_id("BTC", LighterProductType::Perp);
        assert_eq!(format_venue_symbol(&perp), "BTC");
        let spot = format_instrument_id("SOL", LighterProductType::Spot);
        assert_eq!(format_venue_symbol(&spot), "SOL");
    }

    #[rstest]
    fn format_venue_symbol_returns_unsuffixed_unchanged() {
        let id = InstrumentId::new(Symbol::from_str_unchecked("ETH"), *LIGHTER_VENUE);
        assert_eq!(format_venue_symbol(&id), "ETH");
    }

    #[rstest]
    fn product_type_from_instrument_id_dispatches_on_suffix() {
        let perp = format_instrument_id("BTC", LighterProductType::Perp);
        let spot = format_instrument_id("BTC", LighterProductType::Spot);
        let none = InstrumentId::new(Symbol::from_str_unchecked("BTC"), *LIGHTER_VENUE);
        assert_eq!(
            product_type_from_instrument_id(&perp),
            Some(LighterProductType::Perp),
        );
        assert_eq!(
            product_type_from_instrument_id(&spot),
            Some(LighterProductType::Spot),
        );
        assert_eq!(product_type_from_instrument_id(&none), None);
    }

    #[rstest]
    fn registry_round_trip_by_all_keys() {
        let registry = MarketRegistry::new();
        let id = registry.insert(0, "ETH", LighterProductType::Perp);

        assert_eq!(registry.instrument_id(0), Some(id));
        assert_eq!(registry.market_index(&id), Some(0));
        assert_eq!(
            registry.instrument_id_by_symbol("ETH", LighterProductType::Perp),
            Some(id),
        );
        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
    }

    #[rstest]
    fn registry_idempotent_reinsert_is_stable() {
        let registry = MarketRegistry::new();
        let first = registry.insert(0, "ETH", LighterProductType::Perp);
        let second = registry.insert(0, "ETH", LighterProductType::Perp);

        assert_eq!(first, second);
        assert_eq!(registry.instrument_id(0), Some(first));
        assert_eq!(registry.market_index(&first), Some(0));
        assert_eq!(
            registry.instrument_id_by_symbol("ETH", LighterProductType::Perp),
            Some(first),
        );
        assert_eq!(registry.len(), 1);
    }

    #[rstest]
    fn registry_disambiguates_perp_and_spot_for_same_symbol() {
        let registry = MarketRegistry::new();
        let perp = registry.insert(1, "BTC", LighterProductType::Perp);
        let spot = registry.insert(2049, "BTC", LighterProductType::Spot);

        assert_ne!(perp, spot);
        assert_eq!(
            registry.instrument_id_by_symbol("BTC", LighterProductType::Perp),
            Some(perp),
        );
        assert_eq!(
            registry.instrument_id_by_symbol("BTC", LighterProductType::Spot),
            Some(spot),
        );
        assert_eq!(registry.market_index(&perp), Some(1));
        assert_eq!(registry.market_index(&spot), Some(2049));
    }

    #[rstest]
    fn registry_insert_overwrites_existing_index() {
        let registry = MarketRegistry::new();
        let old_id = registry.insert(5, "OLD", LighterProductType::Perp);
        let new_id = registry.insert(5, "NEW", LighterProductType::Perp);

        assert_eq!(registry.instrument_id(5), Some(new_id));
        assert_eq!(
            registry.instrument_id_by_symbol("NEW", LighterProductType::Perp),
            Some(new_id),
        );

        // Stale entries for the displaced InstrumentId must not survive.
        assert_eq!(registry.market_index(&old_id), None);
        assert_eq!(
            registry.instrument_id_by_symbol("OLD", LighterProductType::Perp),
            None,
        );
        assert_eq!(registry.len(), 1);
    }

    #[rstest]
    fn registry_insert_canonicalizes_symbol_case() {
        let registry = MarketRegistry::new();
        let lower = registry.insert(5, "eth", LighterProductType::Perp);
        let _new_id = registry.insert(5, "NEW", LighterProductType::Perp);

        // Lookup with the lowercased form must miss because the prior entry
        // was evicted from the canonical (uppercased) by_raw_symbol slot.
        assert_eq!(
            registry.instrument_id_by_symbol("eth", LighterProductType::Perp),
            None,
        );
        assert_eq!(
            registry.instrument_id_by_symbol("ETH", LighterProductType::Perp),
            None,
        );
        assert_eq!(registry.market_index(&lower), None);
        assert_eq!(registry.len(), 1);
    }

    #[rstest]
    fn registry_lookup_is_case_insensitive() {
        let registry = MarketRegistry::new();
        let id = registry.insert(0, "btc", LighterProductType::Perp);
        assert_eq!(
            registry.instrument_id_by_symbol("BTC", LighterProductType::Perp),
            Some(id),
        );
        assert_eq!(
            registry.instrument_id_by_symbol("  btc ", LighterProductType::Perp),
            Some(id),
        );
    }

    #[rstest]
    fn registry_insert_remaps_symbol_to_new_index() {
        let registry = MarketRegistry::new();
        let original = registry.insert(0, "ETH", LighterProductType::Perp);
        let remapped = registry.insert(7, "ETH", LighterProductType::Perp);

        assert_eq!(original, remapped);
        assert_eq!(registry.market_index(&original), Some(7));
        assert_eq!(registry.instrument_id(7), Some(original));

        // The previous market_index slot must no longer point at this id.
        assert_eq!(registry.instrument_id(0), None);
        assert_eq!(registry.len(), 1);
    }

    #[rstest]
    fn registry_clear_empties_all_indices() {
        let registry = MarketRegistry::new();
        registry.insert(0, "ETH", LighterProductType::Perp);
        registry.insert(2048, "USDC", LighterProductType::Spot);
        assert_eq!(registry.len(), 2);

        registry.clear();
        assert!(registry.is_empty());
        assert_eq!(registry.instrument_id(0), None);
    }

    #[rstest]
    fn registry_lookup_misses_return_none() {
        let registry = MarketRegistry::new();
        let unknown = InstrumentId::new(Symbol::from_str_unchecked("XYZ-PERP"), *LIGHTER_VENUE);
        assert_eq!(registry.instrument_id(99), None);
        assert_eq!(registry.market_index(&unknown), None);
        assert_eq!(
            registry.instrument_id_by_symbol("XYZ", LighterProductType::Perp),
            None,
        );
    }

    proptest! {
        /// Round-tripping a canonical (uppercase) venue symbol through
        /// `format_instrument_id` and `format_venue_symbol` returns the
        /// same string, and the suffix encodes the original product type.
        #[rstest]
        fn prop_instrument_id_roundtrips_through_venue_symbol(
            symbol in "[A-Z][A-Z0-9]{0,7}",
            is_perp in any::<bool>(),
        ) {
            let product = if is_perp {
                LighterProductType::Perp
            } else {
                LighterProductType::Spot
            };
            let id = format_instrument_id(&symbol, product);
            prop_assert_eq!(format_venue_symbol(&id), symbol.as_str());
            prop_assert_eq!(product_type_from_instrument_id(&id), Some(product));
        }
    }
}
