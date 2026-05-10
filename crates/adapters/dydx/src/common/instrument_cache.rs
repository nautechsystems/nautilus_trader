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

//! Thread-safe instrument cache for dYdX adapter.
//!
//! This module provides a centralized cache for instrument data that is shared
//! between HTTP client, WebSocket client, and execution client via `Arc`.
//!
//! # Design
//!
//! dYdX uses different identifiers in different contexts:
//! - **InstrumentId** ("BTC-USD-PERP.DYDX"): Nautilus internal identifier (primary key)
//! - **Market ticker** ("BTC-USD"): Used in public WebSocket channels
//! - **clob_pair_id** (0, 1, 2...): Used in blockchain transactions and order messages
//!
//! This cache provides O(1) lookups by any of these identifiers through internal indices.
//! Using `InstrumentId` as the primary key provides better type safety and eliminates
//! redundant conversions.
//!
//! # Thread Safety
//!
//! All operations use `DashMap` for lock-free concurrent access. The cache can be
//! safely shared across multiple async tasks via `Arc<InstrumentCache>`.

use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::DashMap;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use ustr::Ustr;

use crate::{grpc::OrderMarketParams, http::models::PerpetualMarket};

/// Thread-safe instrument cache with multiple lookup indices.
///
/// Shared between HTTP client, WebSocket client, and execution client via `Arc`.
/// Provides O(1) lookups by `InstrumentId`, market ticker, or clob_pair_id.

#[derive(Debug, Default)]
pub struct InstrumentCache {
    /// Primary storage: InstrumentId → InstrumentAny
    instruments: DashMap<InstrumentId, InstrumentAny>,
    /// Index: clob_pair_id (0, 1, 2...) → InstrumentId (direct lookup)
    clob_pair_id_index: DashMap<u32, InstrumentId>,
    /// Index: market ticker ("BTC-USD") → InstrumentId (direct lookup)
    market_index: DashMap<Ustr, InstrumentId>,
    /// Market parameters: InstrumentId → PerpetualMarket
    market_params: DashMap<InstrumentId, PerpetualMarket>,
    /// Whether cache has been initialized with instrument data
    initialized: AtomicBool,
}

impl InstrumentCache {
    /// Creates a new empty instrument cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts an instrument with its market data.
    ///
    /// This populates the primary storage and all lookup indices.
    pub fn insert(&self, instrument: InstrumentAny, market: PerpetualMarket) {
        let instrument_id = instrument.id();
        let ticker = Ustr::from(&market.ticker);
        let clob_pair_id = market.clob_pair_id;

        // Primary storage
        self.instruments.insert(instrument_id, instrument);

        // Build indices for reverse lookups (now point directly to InstrumentId)
        self.clob_pair_id_index.insert(clob_pair_id, instrument_id);
        self.market_index.insert(ticker, instrument_id);

        // Store full market params for order building
        self.market_params.insert(instrument_id, market);
    }

    /// Bulk inserts instruments with their market data.
    ///
    /// Marks the cache as initialized after insertion.
    pub fn insert_many(&self, items: Vec<(InstrumentAny, PerpetualMarket)>) {
        for (instrument, market) in items {
            self.insert(instrument, market);
        }
        self.initialized.store(true, Ordering::Release);
    }

    /// Clears all cached data.
    ///
    /// Useful for refreshing instruments from the API.
    pub fn clear(&self) {
        self.instruments.clear();
        self.clob_pair_id_index.clear();
        self.market_index.clear();
        self.market_params.clear();
        self.initialized.store(false, Ordering::Release);
    }

    /// Inserts an instrument without market data.
    ///
    /// Derives the market ticker from the instrument symbol by stripping the
    /// "-PERP" suffix, so `get_by_market()` works. `get_by_clob_id()` requires
    /// full market params and won't work for instruments inserted this way.
    pub fn insert_instrument_only(&self, instrument: InstrumentAny) {
        let instrument_id = instrument.id();
        let symbol = instrument_id.symbol.as_str();
        let ticker = symbol.strip_suffix("-PERP").unwrap_or(symbol);
        self.market_index.insert(Ustr::from(ticker), instrument_id);
        self.instruments.insert(instrument_id, instrument);
    }

    /// Bulk inserts instruments without market data (derives market tickers).
    ///
    /// Marks the cache as initialized after insertion.
    pub fn insert_instruments_only(&self, instruments: Vec<InstrumentAny>) {
        for instrument in instruments {
            self.insert_instrument_only(instrument);
        }
        self.initialized.store(true, Ordering::Release);
    }

    /// Gets an instrument by InstrumentId.
    #[must_use]
    pub fn get(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.instruments.get(instrument_id).map(|r| r.clone())
    }

    /// Gets an instrument by market ticker (e.g., "BTC-USD").
    ///
    /// This is the identifier used in public WebSocket channels.
    #[must_use]
    pub fn get_by_market(&self, ticker: &str) -> Option<InstrumentAny> {
        let ticker_ustr = Ustr::from(ticker);
        self.market_index
            .get(&ticker_ustr)
            .and_then(|instrument_id| self.instruments.get(&*instrument_id).map(|r| r.clone()))
    }

    /// Gets an instrument by clob_pair_id (e.g., 0, 1, 2).
    ///
    /// This is the identifier used in blockchain transactions and order messages.
    #[must_use]
    pub fn get_by_clob_id(&self, clob_pair_id: u32) -> Option<InstrumentAny> {
        self.clob_pair_id_index
            .get(&clob_pair_id)
            .and_then(|instrument_id| self.instruments.get(&*instrument_id).map(|r| r.clone()))
    }

    /// Gets an InstrumentId by clob_pair_id.
    ///
    /// Returns directly from index without cloning full instrument.
    #[must_use]
    pub fn get_id_by_clob_id(&self, clob_pair_id: u32) -> Option<InstrumentId> {
        self.clob_pair_id_index.get(&clob_pair_id).map(|r| *r)
    }

    /// Gets an InstrumentId by market ticker.
    ///
    /// Returns directly from index without cloning full instrument.
    #[must_use]
    pub fn get_id_by_market(&self, ticker: &str) -> Option<InstrumentId> {
        let ticker_ustr = Ustr::from(ticker);
        self.market_index.get(&ticker_ustr).map(|r| *r)
    }

    /// Gets full market parameters by InstrumentId.
    ///
    /// Returns the complete `PerpetualMarket` data including margin requirements,
    /// quantization parameters, and current oracle price.
    #[must_use]
    pub fn get_market_params(&self, instrument_id: &InstrumentId) -> Option<PerpetualMarket> {
        self.market_params.get(instrument_id).map(|r| r.clone())
    }

    /// Gets order market parameters for order building.
    ///
    /// Returns the subset of market data needed for constructing orders
    /// (quantization, clob_pair_id, etc.).
    #[must_use]
    pub fn get_order_market_params(
        &self,
        instrument_id: &InstrumentId,
    ) -> Option<OrderMarketParams> {
        self.get_market_params(instrument_id)
            .map(|market| OrderMarketParams {
                atomic_resolution: market.atomic_resolution,
                clob_pair_id: market.clob_pair_id,
                oracle_price: market.oracle_price,
                quantum_conversion_exponent: market.quantum_conversion_exponent,
                step_base_quantums: market.step_base_quantums,
                subticks_per_tick: market.subticks_per_tick,
            })
    }

    /// Updates oracle price for a market.
    ///
    /// Called when receiving price updates via WebSocket `v4_markets` channel.
    pub fn update_oracle_price(&self, ticker: &str, oracle_price: rust_decimal::Decimal) {
        let ticker_ustr = Ustr::from(ticker);
        if let Some(instrument_id) = self.market_index.get(&ticker_ustr)
            && let Some(mut market) = self.market_params.get_mut(&*instrument_id)
        {
            market.oracle_price = Some(oracle_price);
        }
    }

    /// Returns whether the cache has been initialized with instrument data.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    /// Returns the number of cached instruments.
    #[must_use]
    pub fn len(&self) -> usize {
        self.instruments.len()
    }

    /// Returns whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instruments.is_empty()
    }

    /// Returns all cached instruments.
    ///
    /// Useful for WebSocket handler initialization and instrument replay.
    #[must_use]
    pub fn all_instruments(&self) -> Vec<InstrumentAny> {
        self.instruments.iter().map(|r| r.clone()).collect()
    }

    /// Returns all InstrumentIds.
    #[must_use]
    pub fn all_instrument_ids(&self) -> Vec<InstrumentId> {
        self.instruments.iter().map(|r| r.value().id()).collect()
    }

    /// Checks if an instrument exists by InstrumentId.
    #[must_use]
    pub fn contains(&self, instrument_id: &InstrumentId) -> bool {
        self.instruments.contains_key(instrument_id)
    }

    /// Checks if an instrument exists by clob_pair_id.
    #[must_use]
    pub fn contains_clob_id(&self, clob_pair_id: u32) -> bool {
        self.clob_pair_id_index.contains_key(&clob_pair_id)
    }

    /// Checks if an instrument exists by market ticker (e.g., "BTC-USD").
    #[must_use]
    pub fn contains_market(&self, ticker: &str) -> bool {
        let ticker_ustr = Ustr::from(ticker);
        self.market_index.contains_key(&ticker_ustr)
    }

    /// Returns a HashMap of all instruments keyed by InstrumentId.
    ///
    /// This is useful for parsing functions that expect `HashMap<InstrumentId, InstrumentAny>`.
    /// Note: Creates a snapshot copy, so frequent calls should be avoided.
    #[must_use]
    pub fn to_instrument_id_map(&self) -> std::collections::HashMap<InstrumentId, InstrumentAny> {
        self.instruments
            .iter()
            .map(|entry| (entry.value().id(), entry.value().clone()))
            .collect()
    }

    /// Returns a HashMap of oracle prices keyed by InstrumentId.
    ///
    /// This is useful for parsing functions like `parse_account_state` that need oracle prices.
    /// Note: Creates a snapshot copy, so frequent calls should be avoided.
    #[must_use]
    pub fn to_oracle_prices_map(
        &self,
    ) -> std::collections::HashMap<InstrumentId, rust_decimal::Decimal> {
        self.market_params
            .iter()
            .filter_map(|entry| entry.value().oracle_price.map(|p| (*entry.key(), p)))
            .collect()
    }

    /// Logs a warning about a missing instrument for a clob_pair_id, listing known mappings.
    pub fn log_missing_clob_pair_id(&self, clob_pair_id: u32) {
        let known: Vec<(u32, String)> = self
            .clob_pair_id_index
            .iter()
            .map(|entry| (*entry.key(), entry.value().symbol.as_str().to_string()))
            .collect();

        log::warn!(
            "Instrument for clob_pair_id {clob_pair_id} not found in cache. \
             Known CLOB pair IDs and symbols: {known:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::{InstrumentId, Symbol, Venue},
        instruments::{CryptoPerpetual, InstrumentAny},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::common::enums::DydxMarketStatus;

    fn create_test_instrument(symbol: &str) -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new(symbol), Venue::new("DYDX"));
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            instrument_id.symbol,
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false,
            1,                       // price_precision
            3,                       // size_precision
            Price::new(0.1, 1),      // price_increment
            Quantity::new(0.001, 3), // size_increment
            None,                    // multiplier
            None,                    // lot_size
            None,                    // max_quantity
            None,                    // min_quantity
            None,                    // max_notional
            None,                    // min_notional
            None,                    // max_price
            None,                    // min_price
            None,                    // margin_init
            None,                    // margin_maint
            None,                    // maker_fee
            None,                    // taker_fee
            None,                    // info: Option<Params>
            UnixNanos::default(),    // ts_event
            UnixNanos::default(),    // ts_init
        ))
    }

    fn create_test_market(ticker: &str, clob_pair_id: u32) -> PerpetualMarket {
        PerpetualMarket {
            clob_pair_id,
            ticker: Ustr::from(ticker),
            status: DydxMarketStatus::Active,
            base_asset: Some(Ustr::from("BTC")),
            quote_asset: Some(Ustr::from("USD")),
            step_size: dec!(0.001),
            tick_size: dec!(0.1),
            index_price: Some(dec!(50000)),
            oracle_price: Some(dec!(50000)),
            price_change_24h: dec!(0),
            next_funding_rate: dec!(0),
            next_funding_at: None,
            min_order_size: Some(dec!(0.001)),
            market_type: None,
            initial_margin_fraction: dec!(0.05),
            maintenance_margin_fraction: dec!(0.03),
            base_position_notional: None,
            incremental_position_size: None,
            incremental_initial_margin_fraction: None,
            max_position_size: None,
            open_interest: dec!(1000),
            atomic_resolution: -10,
            quantum_conversion_exponent: -9,
            subticks_per_tick: 1000000,
            step_base_quantums: 1000000,
            is_reduce_only: false,
        }
    }

    #[rstest]
    fn test_insert_and_get() {
        let cache = InstrumentCache::new();
        let instrument = create_test_instrument("BTC-USD-PERP");
        let instrument_id = instrument.id();
        let market = create_test_market("BTC-USD", 0);

        cache.insert(instrument, market);

        // Get by InstrumentId
        let retrieved = cache.get(&instrument_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id().symbol.as_str(), "BTC-USD-PERP");
    }

    #[rstest]
    fn test_get_by_market() {
        let cache = InstrumentCache::new();
        let instrument = create_test_instrument("BTC-USD-PERP");
        let market = create_test_market("BTC-USD", 0);

        cache.insert(instrument, market);

        // Get by market ticker
        let retrieved = cache.get_by_market("BTC-USD");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id().symbol.as_str(), "BTC-USD-PERP");
    }

    #[rstest]
    fn test_get_by_clob_id() {
        let cache = InstrumentCache::new();
        let instrument = create_test_instrument("BTC-USD-PERP");
        let market = create_test_market("BTC-USD", 0);

        cache.insert(instrument, market);

        // Get by clob_pair_id
        let retrieved = cache.get_by_clob_id(0);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id().symbol.as_str(), "BTC-USD-PERP");

        // Non-existent clob_pair_id
        assert!(cache.get_by_clob_id(999).is_none());
    }

    #[rstest]
    fn test_insert_many() {
        let cache = InstrumentCache::new();

        let items = vec![
            (
                create_test_instrument("BTC-USD-PERP"),
                create_test_market("BTC-USD", 0),
            ),
            (
                create_test_instrument("ETH-USD-PERP"),
                create_test_market("ETH-USD", 1),
            ),
        ];

        assert!(!cache.is_initialized());
        cache.insert_many(items);
        assert!(cache.is_initialized());

        assert_eq!(cache.len(), 2);
        assert!(cache.get_by_market("BTC-USD").is_some());
        assert!(cache.get_by_market("ETH-USD").is_some());
        assert!(cache.get_by_clob_id(0).is_some());
        assert!(cache.get_by_clob_id(1).is_some());
    }

    #[rstest]
    fn test_clear() {
        let cache = InstrumentCache::new();
        let instrument = create_test_instrument("BTC-USD-PERP");
        let market = create_test_market("BTC-USD", 0);

        cache.insert(instrument, market);
        assert_eq!(cache.len(), 1);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(!cache.is_initialized());
    }

    #[rstest]
    fn test_get_market_params() {
        let cache = InstrumentCache::new();
        let instrument = create_test_instrument("BTC-USD-PERP");
        let market = create_test_market("BTC-USD", 0);

        cache.insert(instrument.clone(), market);

        let params = cache.get_market_params(&instrument.id());
        assert!(params.is_some());
        let params = params.unwrap();
        assert_eq!(params.clob_pair_id, 0);
        assert_eq!(params.ticker, "BTC-USD");
    }

    #[rstest]
    fn test_update_oracle_price() {
        let cache = InstrumentCache::new();
        let instrument = create_test_instrument("BTC-USD-PERP");
        let market = create_test_market("BTC-USD", 0);

        cache.insert(instrument.clone(), market);

        // Initial oracle price
        let params = cache.get_market_params(&instrument.id()).unwrap();
        assert_eq!(params.oracle_price, Some(dec!(50000)));

        // Update oracle price
        cache.update_oracle_price("BTC-USD", dec!(55000));

        let params = cache.get_market_params(&instrument.id()).unwrap();
        assert_eq!(params.oracle_price, Some(dec!(55000)));
    }

    #[rstest]
    fn test_to_oracle_prices_map() {
        let cache = InstrumentCache::new();

        let items = vec![
            (
                create_test_instrument("BTC-USD-PERP"),
                create_test_market("BTC-USD", 0),
            ),
            (
                create_test_instrument("ETH-USD-PERP"),
                create_test_market("ETH-USD", 1),
            ),
        ];

        cache.insert_many(items);

        // Update one oracle price
        cache.update_oracle_price("ETH-USD", dec!(3000));

        let oracle_map = cache.to_oracle_prices_map();
        assert_eq!(oracle_map.len(), 2);

        // BTC-USD should have default 50000
        let btc_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
        assert_eq!(oracle_map.get(&btc_id), Some(&dec!(50000)));

        // ETH-USD should have updated price 3000
        let eth_id = InstrumentId::new(Symbol::new("ETH-USD-PERP"), Venue::new("DYDX"));
        assert_eq!(oracle_map.get(&eth_id), Some(&dec!(3000)));
    }

    #[rstest]
    fn test_get_order_market_params_with_none_oracle_price() {
        let cache = InstrumentCache::new();
        let instrument = create_test_instrument("WTI-USD-PERP");
        let instrument_id = instrument.id();
        let mut market = create_test_market("WTI-USD", 99);
        market.oracle_price = None;

        cache.insert(instrument, market);

        let params = cache.get_order_market_params(&instrument_id).unwrap();
        assert_eq!(params.oracle_price, None);
        assert_eq!(params.clob_pair_id, 99);
    }

    #[rstest]
    fn test_to_oracle_prices_map_excludes_none() {
        let cache = InstrumentCache::new();

        let mut market_no_oracle = create_test_market("WTI-USD", 99);
        market_no_oracle.oracle_price = None;

        let items = vec![
            (
                create_test_instrument("BTC-USD-PERP"),
                create_test_market("BTC-USD", 0),
            ),
            (create_test_instrument("WTI-USD-PERP"), market_no_oracle),
        ];

        cache.insert_many(items);

        let oracle_map = cache.to_oracle_prices_map();
        assert_eq!(oracle_map.len(), 1);

        let btc_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
        assert_eq!(oracle_map.get(&btc_id), Some(&dec!(50000)));

        let wti_id = InstrumentId::new(Symbol::new("WTI-USD-PERP"), Venue::new("DYDX"));
        assert_eq!(oracle_map.get(&wti_id), None);
    }
}
