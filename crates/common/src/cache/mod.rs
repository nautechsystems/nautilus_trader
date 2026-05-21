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

//! In-memory cache for market and execution data, with optional persistent backing.
//!
//! Provides methods to load, query, and update cached data such as instruments, orders, and prices.

pub mod config;
pub mod database;
pub mod fifo;
pub mod quote;
pub mod refs;

mod bounded;
mod index;

#[cfg(test)]
mod tests;

use std::{
    borrow::Cow,
    cell::{Ref, RefCell},
    fmt::{Debug, Display},
    rc::Rc,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use ahash::{AHashMap, AHashSet};
use bounded::BoundedVecDeque;
use bytes::Bytes;
pub use config::CacheConfig; // Re-export
use database::{CacheDatabaseAdapter, CacheMap};
use index::CacheIndex;
use nautilus_core::{
    SharedCell, UUID4, UnixNanos,
    correctness::{
        check_key_not_in_map, check_predicate_false, check_slice_not_empty,
        check_valid_string_ascii,
    },
    datetime::secs_to_nanos_unchecked,
};
use nautilus_model::{
    accounts::{Account, AccountAny},
    data::{
        Bar, BarType, FundingRateUpdate, GreeksData, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, QuoteTick, TradeTick, YieldCurveData, option_chain::OptionGreeks,
    },
    enums::{
        AggregationSource, ContingencyType, InstrumentClass, OmsType, OrderSide, PositionSide,
        PriceType, TriggerType,
    },
    events::{AccountState, OrderEventAny},
    identifiers::{
        AccountId, ClientId, ClientOrderId, ComponentId, ExecAlgorithmId, InstrumentId,
        OrderListId, PositionId, StrategyId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, SyntheticInstrument},
    orderbook::{
        OrderBook,
        own::{OwnOrderBook, should_handle_own_book_order},
    },
    orders::{Order, OrderAny, OrderError, OrderList},
    position::Position,
    types::{Currency, Money, Price, Quantity},
};
pub use refs::{AccountRef, AccountRefMut, OrderRef, OrderRefMut, PositionRef, PositionRefMut};
use ustr::Ustr;

use crate::xrate::get_exchange_rate;

/// Cache-owned reference to a snapshot blob.
///
/// The cache writes and later fetches the blob; external systems persist this opaque reference
/// and may hash the bytes before recording a durable anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheSnapshotRef {
    /// Opaque cache-owned snapshot location.
    pub blob_ref: String,
    /// Snapshot bytes stored under [`Self::blob_ref`].
    pub blob: Bytes,
}

impl CacheSnapshotRef {
    /// Creates a new [`CacheSnapshotRef`].
    #[must_use]
    pub fn new(blob_ref: impl Into<String>, blob: impl Into<Bytes>) -> Self {
        Self {
            blob_ref: blob_ref.into(),
            blob: blob.into(),
        }
    }
}

/// Read-only view over the platform cache.
///
/// Adapter-facing code receives this type instead of the mutable cache handle so cache writes stay
/// owned by the data and execution engines.
#[derive(Clone, Debug)]
pub struct CacheView {
    inner: Rc<RefCell<Cache>>,
}

impl CacheView {
    /// Creates a new [`CacheView`] from a cache handle.
    #[must_use]
    pub fn new(inner: Rc<RefCell<Cache>>) -> Self {
        Self { inner }
    }

    /// Borrows the cache immutably.
    ///
    /// # Panics
    ///
    /// Panics if the cache is already mutably borrowed.
    pub fn borrow(&self) -> Ref<'_, Cache> {
        self.inner.borrow()
    }
}

impl From<Rc<RefCell<Cache>>> for CacheView {
    fn from(inner: Rc<RefCell<Cache>>) -> Self {
        Self::new(inner)
    }
}

// Filter sources resolved from an order or position query.
//
// Captures the three states of a multi-key index intersection without committing to an owned
// result set: no filters at all (the caller iterates the bucket directly), one or more filter
// sources resolved successfully (intersect them lazily), or one filter resolved to no entries
// at all (the result is unconditionally empty).
enum FilterSources<'a, K> {
    Unfiltered,
    Empty,
    Sets(Vec<&'a AHashSet<K>>),
}

// Intersects a non-empty collection of filter sources by sorting them ascending by length and
// driving the loop from the smallest set, collecting one `AHashSet` of matching keys.
//
// Single-source inputs short-circuit to a direct `AHashSet::clone` (memcopy of the bucket
// table) rather than rehashing each entry through `iter().copied().collect()`.
fn intersect_filter_sources<K>(mut sources: Vec<&AHashSet<K>>) -> AHashSet<K>
where
    K: Copy + Eq + std::hash::Hash,
{
    debug_assert!(!sources.is_empty());
    sources.sort_unstable_by_key(|s| s.len());
    let driver = sources[0];
    let rest = &sources[1..];

    if rest.is_empty() {
        return driver.clone();
    }

    driver
        .iter()
        .filter(|id| rest.iter().all(|s| s.contains(id)))
        .copied()
        .collect()
}

// Intersects `bucket` with one or more filter sources.
//
// For exactly one filter source, iterates the larger of (bucket, filter) and looks up in the
// smaller. The larger set scans linearly (HW-prefetcher friendly) and the smaller stays hot in
// cache, which empirically beats the size-ordered approach when the smaller filter is too
// large to fit in L1 (e.g., a 20k-entry venue filter against a 100k-entry bucket). For two or
// more filters the size-ordered driver is reinstated and the bucket joins the source list.
fn intersect_pair_or_many<'a, K>(
    bucket: &'a AHashSet<K>,
    mut sources: Vec<&'a AHashSet<K>>,
) -> AHashSet<K>
where
    K: Copy + Eq + std::hash::Hash,
{
    debug_assert!(!sources.is_empty());
    if sources.len() == 1 {
        let filter = sources[0];
        let (larger, smaller) = if bucket.len() >= filter.len() {
            (bucket, filter)
        } else {
            (filter, bucket)
        };
        return larger.intersection(smaller).copied().collect();
    }

    sources.push(bucket);
    intersect_filter_sources(sources)
}

/// A common in-memory `Cache` for market and execution related data.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common", unsendable)
)]
pub struct Cache {
    config: CacheConfig,
    index: CacheIndex,
    database: Option<Box<dyn CacheDatabaseAdapter>>,
    general: AHashMap<String, Bytes>,
    currencies: AHashMap<Ustr, Currency>,
    instruments: AHashMap<InstrumentId, InstrumentAny>,
    synthetics: AHashMap<InstrumentId, SyntheticInstrument>,
    books: AHashMap<InstrumentId, OrderBook>,
    own_books: AHashMap<InstrumentId, OwnOrderBook>,
    quotes: AHashMap<InstrumentId, BoundedVecDeque<QuoteTick>>,
    trades: AHashMap<InstrumentId, BoundedVecDeque<TradeTick>>,
    mark_xrates: AHashMap<(Currency, Currency), f64>,
    mark_prices: AHashMap<InstrumentId, BoundedVecDeque<MarkPriceUpdate>>,
    index_prices: AHashMap<InstrumentId, BoundedVecDeque<IndexPriceUpdate>>,
    funding_rates: AHashMap<InstrumentId, BoundedVecDeque<FundingRateUpdate>>,
    instrument_statuses: AHashMap<InstrumentId, BoundedVecDeque<InstrumentStatus>>,
    bars: AHashMap<BarType, BoundedVecDeque<Bar>>,
    greeks: AHashMap<InstrumentId, GreeksData>,
    option_greeks: AHashMap<InstrumentId, OptionGreeks>,
    yield_curves: AHashMap<String, YieldCurveData>,
    accounts: AHashMap<AccountId, SharedCell<AccountAny>>,
    orders: AHashMap<ClientOrderId, SharedCell<OrderAny>>,
    order_lists: AHashMap<OrderListId, OrderList>,
    positions: AHashMap<PositionId, SharedCell<Position>>,
    position_snapshots: AHashMap<PositionId, Vec<Bytes>>,
    #[cfg(feature = "defi")]
    pub(crate) defi: crate::defi::cache::DefiCache,
}

impl Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Cache))
            .field("config", &self.config)
            .field("index", &self.index)
            .field("general", &self.general)
            .field("currencies", &self.currencies)
            .field("instruments", &self.instruments)
            .field("synthetics", &self.synthetics)
            .field("books", &self.books)
            .field("own_books", &self.own_books)
            .field("quotes", &self.quotes)
            .field("trades", &self.trades)
            .field("mark_xrates", &self.mark_xrates)
            .field("mark_prices", &self.mark_prices)
            .field("index_prices", &self.index_prices)
            .field("funding_rates", &self.funding_rates)
            .field("instrument_statuses", &self.instrument_statuses)
            .field("bars", &self.bars)
            .field("greeks", &self.greeks)
            .field("option_greeks", &self.option_greeks)
            .field("yield_curves", &self.yield_curves)
            .field("accounts", &self.accounts)
            .field("orders", &self.orders)
            .field("order_lists", &self.order_lists)
            .field("positions", &self.positions)
            .field("position_snapshots", &self.position_snapshots)
            .finish()
    }
}

impl Default for Cache {
    /// Creates a new default [`Cache`] instance.
    fn default() -> Self {
        Self::new(Some(CacheConfig::default()), None)
    }
}

impl Cache {
    /// Creates a new [`Cache`] instance with optional configuration and database adapter.
    #[must_use]
    /// # Note
    ///
    /// Uses provided `CacheConfig` or defaults, and optional `CacheDatabaseAdapter` for persistence.
    ///
    /// # Panics
    ///
    /// Panics if the cache config has a zero tick or bar capacity.
    pub fn new(
        config: Option<CacheConfig>,
        database: Option<Box<dyn CacheDatabaseAdapter>>,
    ) -> Self {
        let config = config.unwrap_or_default();
        config.validate().expect("invalid `CacheConfig`");

        Self {
            config,
            index: CacheIndex::default(),
            database,
            general: AHashMap::new(),
            currencies: AHashMap::new(),
            instruments: AHashMap::new(),
            synthetics: AHashMap::new(),
            books: AHashMap::new(),
            own_books: AHashMap::new(),
            quotes: AHashMap::new(),
            trades: AHashMap::new(),
            mark_xrates: AHashMap::new(),
            mark_prices: AHashMap::new(),
            index_prices: AHashMap::new(),
            funding_rates: AHashMap::new(),
            instrument_statuses: AHashMap::new(),
            bars: AHashMap::new(),
            greeks: AHashMap::new(),
            option_greeks: AHashMap::new(),
            yield_curves: AHashMap::new(),
            accounts: AHashMap::new(),
            orders: AHashMap::new(),
            order_lists: AHashMap::new(),
            positions: AHashMap::new(),
            position_snapshots: AHashMap::new(),
            #[cfg(feature = "defi")]
            defi: crate::defi::cache::DefiCache::default(),
        }
    }

    /// Returns the cache instances memory address.
    #[must_use]
    pub fn memory_address(&self) -> String {
        format!("{:?}", std::ptr::from_ref(self))
    }

    /// Sets the cache database adapter for persistence.
    ///
    /// This allows setting or replacing the database adapter after cache construction.
    pub fn set_database(&mut self, database: Box<dyn CacheDatabaseAdapter>) {
        let type_name = std::any::type_name_of_val(&*database);
        log::info!("Cache database adapter set: {type_name}");
        self.database = Some(database);
    }

    // -- COMMANDS --------------------------------------------------------------------------------

    /// Clears and reloads general entries from the database into the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading general cache data fails.
    pub fn cache_general(&mut self) -> anyhow::Result<()> {
        self.general = match &mut self.database {
            Some(db) => db.load()?,
            None => AHashMap::new(),
        };

        log::info!(
            "Cached {} general object(s) from database",
            self.general.len()
        );
        Ok(())
    }

    /// Loads all core caches (currencies, instruments, accounts, orders, positions) from the database.
    ///
    /// # Errors
    ///
    /// Returns an error if loading all cache data fails.
    pub async fn cache_all(&mut self) -> anyhow::Result<()> {
        let cache_map = match &self.database {
            Some(db) => db.load_all().await?,
            None => CacheMap::default(),
        };

        self.currencies = cache_map.currencies;
        self.instruments = cache_map.instruments;
        self.synthetics = cache_map.synthetics;
        self.accounts = cache_map
            .accounts
            .into_iter()
            .map(|(id, account)| (id, SharedCell::new(account)))
            .collect();
        self.orders = cache_map
            .orders
            .into_iter()
            .map(|(id, order)| (id, SharedCell::new(order)))
            .collect();
        self.positions = cache_map
            .positions
            .into_iter()
            .map(|(id, position)| (id, SharedCell::new(position)))
            .collect();

        self.assign_position_ids_to_contingencies();
        Ok(())
    }

    /// Clears and reloads the currency cache from the database.
    ///
    /// # Errors
    ///
    /// Returns an error if loading currencies cache fails.
    pub async fn cache_currencies(&mut self) -> anyhow::Result<()> {
        self.currencies = match &mut self.database {
            Some(db) => db.load_currencies().await?,
            None => AHashMap::new(),
        };

        log::info!("Cached {} currencies from database", self.general.len());
        Ok(())
    }

    /// Clears and reloads the instrument cache from the database.
    ///
    /// # Errors
    ///
    /// Returns an error if loading instruments cache fails.
    pub async fn cache_instruments(&mut self) -> anyhow::Result<()> {
        self.instruments = match &mut self.database {
            Some(db) => db.load_instruments().await?,
            None => AHashMap::new(),
        };

        log::info!("Cached {} instruments from database", self.general.len());
        Ok(())
    }

    /// Clears and reloads the synthetic instrument cache from the database.
    ///
    /// # Errors
    ///
    /// Returns an error if loading synthetic instruments cache fails.
    pub async fn cache_synthetics(&mut self) -> anyhow::Result<()> {
        self.synthetics = match &mut self.database {
            Some(db) => db.load_synthetics().await?,
            None => AHashMap::new(),
        };

        log::info!(
            "Cached {} synthetic instruments from database",
            self.general.len()
        );
        Ok(())
    }

    /// Clears and reloads the account cache from the database.
    ///
    /// # Errors
    ///
    /// Returns an error if loading accounts cache fails.
    pub async fn cache_accounts(&mut self) -> anyhow::Result<()> {
        self.accounts = match &mut self.database {
            Some(db) => db
                .load_accounts()
                .await?
                .into_iter()
                .map(|(id, account)| (id, SharedCell::new(account)))
                .collect(),
            None => AHashMap::new(),
        };

        log::info!(
            "Cached {} synthetic instruments from database",
            self.general.len()
        );
        Ok(())
    }

    /// Clears and reloads the order cache from the database.
    ///
    /// # Errors
    ///
    /// Returns an error if loading orders cache fails.
    pub async fn cache_orders(&mut self) -> anyhow::Result<()> {
        self.orders = match &mut self.database {
            Some(db) => db
                .load_orders()
                .await?
                .into_iter()
                .map(|(id, order)| (id, SharedCell::new(order)))
                .collect(),
            None => AHashMap::new(),
        };

        log::info!("Cached {} orders from database", self.general.len());

        self.assign_position_ids_to_contingencies();
        Ok(())
    }

    /// Clears and reloads the position cache from the database.
    ///
    /// # Errors
    ///
    /// Returns an error if loading positions cache fails.
    pub async fn cache_positions(&mut self) -> anyhow::Result<()> {
        self.positions = match &mut self.database {
            Some(db) => db
                .load_positions()
                .await?
                .into_iter()
                .map(|(id, position)| (id, SharedCell::new(position)))
                .collect(),
            None => AHashMap::new(),
        };

        log::info!("Cached {} positions from database", self.general.len());
        Ok(())
    }

    /// Clears the current cache index and re-build.
    pub fn build_index(&mut self) {
        log::debug!("Building index");

        // Index accounts
        for account_id in self.accounts.keys() {
            self.index
                .venue_account
                .insert(account_id.get_issuer(), *account_id);
        }

        // Index orders
        for (client_order_id, order_cell) in &self.orders {
            let order = order_cell.borrow();
            let instrument_id = order.instrument_id();
            let venue = instrument_id.venue;
            let strategy_id = order.strategy_id();

            // 1: Build index.venue_orders -> {Venue, {ClientOrderId}}
            self.index
                .venue_orders
                .entry(venue)
                .or_default()
                .insert(*client_order_id);

            // 2: Build index.order_ids -> {VenueOrderId, ClientOrderId}
            if let Some(venue_order_id) = order.venue_order_id() {
                self.index
                    .venue_order_ids
                    .insert(venue_order_id, *client_order_id);
            }

            // 3: Build index.order_position -> {ClientOrderId, PositionId}
            if let Some(position_id) = order.position_id() {
                self.index
                    .order_position
                    .insert(*client_order_id, position_id);
            }

            // 4: Build index.order_strategy -> {ClientOrderId, StrategyId}
            self.index
                .order_strategy
                .insert(*client_order_id, order.strategy_id());

            // 5: Build index.instrument_orders -> {InstrumentId, {ClientOrderId}}
            self.index
                .instrument_orders
                .entry(instrument_id)
                .or_default()
                .insert(*client_order_id);

            // 6: Build index.strategy_orders -> {StrategyId, {ClientOrderId}}
            self.index
                .strategy_orders
                .entry(strategy_id)
                .or_default()
                .insert(*client_order_id);

            // 7: Build index.account_orders -> {AccountId, {ClientOrderId}}
            if let Some(account_id) = order.account_id() {
                self.index
                    .account_orders
                    .entry(account_id)
                    .or_default()
                    .insert(*client_order_id);
            }

            // 8: Build index.exec_algorithm_orders -> {ExecAlgorithmId, {ClientOrderId}}
            if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
                self.index
                    .exec_algorithm_orders
                    .entry(exec_algorithm_id)
                    .or_default()
                    .insert(*client_order_id);
            }

            // 8: Build index.exec_spawn_orders -> {ClientOrderId, {ClientOrderId}}
            if let Some(exec_spawn_id) = order.exec_spawn_id() {
                self.index
                    .exec_spawn_orders
                    .entry(exec_spawn_id)
                    .or_default()
                    .insert(*client_order_id);
            }

            // 9: Build index.orders -> {ClientOrderId}
            self.index.orders.insert(*client_order_id);

            // 10: Build index.orders_active_local -> {ClientOrderId}
            if order.is_active_local() {
                self.index.orders_active_local.insert(*client_order_id);
            }

            // 11: Build index.orders_open -> {ClientOrderId}
            if order.is_open() {
                self.index.orders_open.insert(*client_order_id);
            }

            // 12: Build index.orders_closed -> {ClientOrderId}
            if order.is_closed() {
                self.index.orders_closed.insert(*client_order_id);
            }

            // 13: Build index.orders_emulated -> {ClientOrderId}
            if let Some(emulation_trigger) = order.emulation_trigger()
                && emulation_trigger != TriggerType::NoTrigger
                && !order.is_closed()
            {
                self.index.orders_emulated.insert(*client_order_id);
            }

            // 14: Build index.orders_inflight -> {ClientOrderId}
            if order.is_inflight() {
                self.index.orders_inflight.insert(*client_order_id);
            }

            // 15: Build index.strategies -> {StrategyId}
            self.index.strategies.insert(strategy_id);

            // 16: Build index.strategies -> {ExecAlgorithmId}
            if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
                self.index.exec_algorithms.insert(exec_algorithm_id);
            }
        }

        // Index positions
        for (position_id, position_cell) in &self.positions {
            let position = position_cell.borrow();
            let instrument_id = position.instrument_id;
            let venue = instrument_id.venue;
            let strategy_id = position.strategy_id;

            // 1: Build index.venue_positions -> {Venue, {PositionId}}
            self.index
                .venue_positions
                .entry(venue)
                .or_default()
                .insert(*position_id);

            // 2: Build index.position_strategy -> {PositionId, StrategyId}
            self.index
                .position_strategy
                .insert(*position_id, position.strategy_id);

            // 3: Build index.position_orders -> {PositionId, {ClientOrderId}}
            self.index
                .position_orders
                .entry(*position_id)
                .or_default()
                .extend(position.client_order_ids());

            // 4: Build index.instrument_positions -> {InstrumentId, {PositionId}}
            self.index
                .instrument_positions
                .entry(instrument_id)
                .or_default()
                .insert(*position_id);

            // 5: Build index.strategy_positions -> {StrategyId, {PositionId}}
            self.index
                .strategy_positions
                .entry(strategy_id)
                .or_default()
                .insert(*position_id);

            // 6: Build index.account_positions -> {AccountId, {PositionId}}
            self.index
                .account_positions
                .entry(position.account_id)
                .or_default()
                .insert(*position_id);

            // 7: Build index.positions -> {PositionId}
            self.index.positions.insert(*position_id);

            // 8: Build index.positions_open -> {PositionId}
            if position.is_open() {
                self.index.positions_open.insert(*position_id);
            }

            // 9: Build index.positions_closed -> {PositionId}
            if position.is_closed() {
                self.index.positions_closed.insert(*position_id);
            }

            // 10: Build index.strategies -> {StrategyId}
            self.index.strategies.insert(strategy_id);
        }
    }

    /// Returns whether the cache has a backing database.
    #[must_use]
    pub const fn has_backing(&self) -> bool {
        self.database.is_some()
    }

    // Calculate the unrealized profit and loss (PnL) for `position`.
    #[must_use]
    pub fn calculate_unrealized_pnl(&self, position: &Position) -> Option<Money> {
        let Some(quote) = self.quote(&position.instrument_id) else {
            log::warn!(
                "Cannot calculate unrealized PnL for {}, no quotes for {}",
                position.id,
                position.instrument_id
            );
            return None;
        };

        // Use exit price for mark-to-market: longs exit at bid, shorts exit at ask
        let last = match position.side {
            PositionSide::Flat | PositionSide::NoPositionSide => {
                return Some(Money::new(0.0, position.settlement_currency));
            }
            PositionSide::Long => quote.bid_price,
            PositionSide::Short => quote.ask_price,
        };

        Some(position.unrealized_pnl(last))
    }

    /// Checks integrity of data within the cache.
    ///
    /// All data should be loaded from the database prior to this call.
    /// If an error is found then a log error message will also be produced.
    ///
    /// # Panics
    ///
    /// Panics if failure calling system clock.
    #[must_use]
    pub fn check_integrity(&mut self) -> bool {
        let mut error_count = 0;
        let failure = "Integrity failure";

        // Get current timestamp in microseconds
        let timestamp_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_micros();

        log::info!("Checking data integrity");

        // Check object caches
        for account_id in self.accounts.keys() {
            if !self
                .index
                .venue_account
                .contains_key(&account_id.get_issuer())
            {
                log::error!(
                    "{failure} in accounts: {account_id} not found in `self.index.venue_account`",
                );
                error_count += 1;
            }
        }

        for (client_order_id, order_cell) in &self.orders {
            let order = order_cell.borrow();

            if !self.index.order_strategy.contains_key(client_order_id) {
                log::error!(
                    "{failure} in orders: {client_order_id} not found in `self.index.order_strategy`"
                );
                error_count += 1;
            }

            if !self.index.orders.contains(client_order_id) {
                log::error!(
                    "{failure} in orders: {client_order_id} not found in `self.index.orders`",
                );
                error_count += 1;
            }

            if order.is_inflight() && !self.index.orders_inflight.contains(client_order_id) {
                log::error!(
                    "{failure} in orders: {client_order_id} not found in `self.index.orders_inflight`",
                );
                error_count += 1;
            }

            if order.is_active_local() && !self.index.orders_active_local.contains(client_order_id)
            {
                log::error!(
                    "{failure} in orders: {client_order_id} not found in `self.index.orders_active_local`",
                );
                error_count += 1;
            }

            if order.is_open() && !self.index.orders_open.contains(client_order_id) {
                log::error!(
                    "{failure} in orders: {client_order_id} not found in `self.index.orders_open`",
                );
                error_count += 1;
            }

            if order.is_closed() && !self.index.orders_closed.contains(client_order_id) {
                log::error!(
                    "{failure} in orders: {client_order_id} not found in `self.index.orders_closed`",
                );
                error_count += 1;
            }

            if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
                if !self
                    .index
                    .exec_algorithm_orders
                    .contains_key(&exec_algorithm_id)
                {
                    log::error!(
                        "{failure} in orders: {client_order_id} not found in `self.index.exec_algorithm_orders`",
                    );
                    error_count += 1;
                }

                if order.exec_spawn_id().is_none()
                    && !self.index.exec_spawn_orders.contains_key(client_order_id)
                {
                    log::error!(
                        "{failure} in orders: {client_order_id} not found in `self.index.exec_spawn_orders`",
                    );
                    error_count += 1;
                }
            }
        }

        for (position_id, position_cell) in &self.positions {
            let position = position_cell.borrow();

            if !self.index.position_strategy.contains_key(position_id) {
                log::error!(
                    "{failure} in positions: {position_id} not found in `self.index.position_strategy`",
                );
                error_count += 1;
            }

            if !self.index.position_orders.contains_key(position_id) {
                log::error!(
                    "{failure} in positions: {position_id} not found in `self.index.position_orders`",
                );
                error_count += 1;
            }

            if !self.index.positions.contains(position_id) {
                log::error!(
                    "{failure} in positions: {position_id} not found in `self.index.positions`",
                );
                error_count += 1;
            }

            if position.is_open() && !self.index.positions_open.contains(position_id) {
                log::error!(
                    "{failure} in positions: {position_id} not found in `self.index.positions_open`",
                );
                error_count += 1;
            }

            if position.is_closed() && !self.index.positions_closed.contains(position_id) {
                log::error!(
                    "{failure} in positions: {position_id} not found in `self.index.positions_closed`",
                );
                error_count += 1;
            }
        }

        // Check indexes
        for account_id in self.index.venue_account.values() {
            if !self.accounts.contains_key(account_id) {
                log::error!(
                    "{failure} in `index.venue_account`: {account_id} not found in `self.accounts`",
                );
                error_count += 1;
            }
        }

        for client_order_id in self.index.venue_order_ids.values() {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{failure} in `index.venue_order_ids`: {client_order_id} not found in `self.orders`",
                );
                error_count += 1;
            }
        }

        for client_order_id in self.index.client_order_ids.keys() {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{failure} in `index.client_order_ids`: {client_order_id} not found in `self.orders`",
                );
                error_count += 1;
            }
        }

        for client_order_id in self.index.order_position.keys() {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{failure} in `index.order_position`: {client_order_id} not found in `self.orders`",
                );
                error_count += 1;
            }
        }

        // Check indexes
        for client_order_id in self.index.order_strategy.keys() {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{failure} in `index.order_strategy`: {client_order_id} not found in `self.orders`",
                );
                error_count += 1;
            }
        }

        for position_id in self.index.position_strategy.keys() {
            if !self.positions.contains_key(position_id) {
                log::error!(
                    "{failure} in `index.position_strategy`: {position_id} not found in `self.positions`",
                );
                error_count += 1;
            }
        }

        for position_id in self.index.position_orders.keys() {
            if !self.positions.contains_key(position_id) {
                log::error!(
                    "{failure} in `index.position_orders`: {position_id} not found in `self.positions`",
                );
                error_count += 1;
            }
        }

        for (instrument_id, client_order_ids) in &self.index.instrument_orders {
            for client_order_id in client_order_ids {
                if !self.orders.contains_key(client_order_id) {
                    log::error!(
                        "{failure} in `index.instrument_orders`: {instrument_id} not found in `self.orders`",
                    );
                    error_count += 1;
                }
            }
        }

        for instrument_id in self.index.instrument_positions.keys() {
            if !self.index.instrument_orders.contains_key(instrument_id) {
                log::error!(
                    "{failure} in `index.instrument_positions`: {instrument_id} not found in `index.instrument_orders`",
                );
                error_count += 1;
            }
        }

        for client_order_ids in self.index.strategy_orders.values() {
            for client_order_id in client_order_ids {
                if !self.orders.contains_key(client_order_id) {
                    log::error!(
                        "{failure} in `index.strategy_orders`: {client_order_id} not found in `self.orders`",
                    );
                    error_count += 1;
                }
            }
        }

        for position_ids in self.index.strategy_positions.values() {
            for position_id in position_ids {
                if !self.positions.contains_key(position_id) {
                    log::error!(
                        "{failure} in `index.strategy_positions`: {position_id} not found in `self.positions`",
                    );
                    error_count += 1;
                }
            }
        }

        for client_order_id in &self.index.orders {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{failure} in `index.orders`: {client_order_id} not found in `self.orders`",
                );
                error_count += 1;
            }
        }

        for client_order_id in &self.index.orders_emulated {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{failure} in `index.orders_emulated`: {client_order_id} not found in `self.orders`",
                );
                error_count += 1;
            }
        }

        for client_order_id in &self.index.orders_active_local {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{failure} in `index.orders_active_local`: {client_order_id} not found in `self.orders`",
                );
                error_count += 1;
            }
        }

        for client_order_id in &self.index.orders_inflight {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{failure} in `index.orders_inflight`: {client_order_id} not found in `self.orders`",
                );
                error_count += 1;
            }
        }

        for client_order_id in &self.index.orders_open {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{failure} in `index.orders_open`: {client_order_id} not found in `self.orders`",
                );
                error_count += 1;
            }
        }

        for client_order_id in &self.index.orders_closed {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{failure} in `index.orders_closed`: {client_order_id} not found in `self.orders`",
                );
                error_count += 1;
            }
        }

        for position_id in &self.index.positions {
            if !self.positions.contains_key(position_id) {
                log::error!(
                    "{failure} in `index.positions`: {position_id} not found in `self.positions`",
                );
                error_count += 1;
            }
        }

        for position_id in &self.index.positions_open {
            if !self.positions.contains_key(position_id) {
                log::error!(
                    "{failure} in `index.positions_open`: {position_id} not found in `self.positions`",
                );
                error_count += 1;
            }
        }

        for position_id in &self.index.positions_closed {
            if !self.positions.contains_key(position_id) {
                log::error!(
                    "{failure} in `index.positions_closed`: {position_id} not found in `self.positions`",
                );
                error_count += 1;
            }
        }

        for strategy_id in &self.index.strategies {
            if !self.index.strategy_orders.contains_key(strategy_id) {
                log::error!(
                    "{failure} in `index.strategies`: {strategy_id} not found in `index.strategy_orders`",
                );
                error_count += 1;
            }
        }

        for exec_algorithm_id in &self.index.exec_algorithms {
            if !self
                .index
                .exec_algorithm_orders
                .contains_key(exec_algorithm_id)
            {
                log::error!(
                    "{failure} in `index.exec_algorithms`: {exec_algorithm_id} not found in `index.exec_algorithm_orders`",
                );
                error_count += 1;
            }
        }

        let total_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_micros()
            - timestamp_us;

        if error_count == 0 {
            log::info!("Integrity check passed in {total_us}μs");
            true
        } else {
            log::error!(
                "Integrity check failed with {error_count} error{} in {total_us}μs",
                if error_count == 1 { "" } else { "s" },
            );
            false
        }
    }

    /// Checks for any residual open state and log warnings if any are found.
    ///
    ///'Open state' is considered to be open orders and open positions.
    #[must_use]
    pub fn check_residuals(&self) -> bool {
        log::debug!("Checking residuals");

        let mut residuals = false;

        // Check for any open orders
        for order in self.orders_open(None, None, None, None, None) {
            residuals = true;
            log::warn!("Residual {order}");
        }

        // Check for any open positions
        for position in self.positions_open(None, None, None, None, None) {
            residuals = true;
            log::warn!("Residual {position}");
        }

        residuals
    }

    /// Purges all closed orders from the cache that are older than `buffer_secs`.
    ///
    ///
    /// Only orders that have been closed for at least this amount of time will be purged.
    /// A value of 0 means purge all closed orders regardless of when they were closed.
    pub fn purge_closed_orders(&mut self, ts_now: UnixNanos, buffer_secs: u64) {
        log::debug!(
            "Purging closed orders{}",
            if buffer_secs > 0 {
                format!(" with buffer_secs={buffer_secs}")
            } else {
                String::new()
            }
        );

        let buffer_ns = secs_to_nanos_unchecked(buffer_secs as f64);

        let mut affected_order_list_ids: AHashSet<OrderListId> = AHashSet::new();

        'outer: for client_order_id in self.index.orders_closed.clone() {
            let purge_target = self.orders.get(&client_order_id).and_then(|order_cell| {
                let order = order_cell.borrow();
                if order.is_closed()
                    && let Some(ts_closed) = order.ts_closed()
                    && ts_closed + buffer_ns <= ts_now
                {
                    let linked = order.linked_order_ids().map(<[_]>::to_vec);
                    let order_list_id = order.order_list_id();
                    Some((linked, order_list_id))
                } else {
                    None
                }
            });

            let Some((linked, order_list_id)) = purge_target else {
                continue;
            };

            // Check any linked orders (contingency orders)
            if let Some(linked_order_ids) = linked {
                for linked_order_id in &linked_order_ids {
                    if let Some(linked_order_cell) = self.orders.get(linked_order_id)
                        && linked_order_cell.borrow().is_open()
                    {
                        // Do not purge if linked order still open
                        continue 'outer;
                    }
                }
            }

            if let Some(order_list_id) = order_list_id {
                affected_order_list_ids.insert(order_list_id);
            }

            self.purge_order(client_order_id);
        }

        for order_list_id in affected_order_list_ids {
            if let Some(order_list) = self.order_lists.get(&order_list_id) {
                let all_purged = order_list
                    .client_order_ids
                    .iter()
                    .all(|id| !self.orders.contains_key(id));

                if all_purged {
                    self.order_lists.remove(&order_list_id);
                    log::info!("Purged {order_list_id}");
                }
            }
        }
    }

    /// Purges all closed positions from the cache that are older than `buffer_secs`.
    pub fn purge_closed_positions(&mut self, ts_now: UnixNanos, buffer_secs: u64) {
        log::debug!(
            "Purging closed positions{}",
            if buffer_secs > 0 {
                format!(" with buffer_secs={buffer_secs}")
            } else {
                String::new()
            }
        );

        let buffer_ns = secs_to_nanos_unchecked(buffer_secs as f64);

        for position_id in self.index.positions_closed.clone() {
            let should_purge = self.positions.get(&position_id).is_some_and(|cell| {
                let position = cell.borrow();
                position.is_closed()
                    && position
                        .ts_closed
                        .is_some_and(|ts_closed| ts_closed + buffer_ns <= ts_now)
            });

            if should_purge {
                self.purge_position(position_id);
            }
        }
    }

    /// Purges the order with the `client_order_id` from the cache (if found).
    ///
    /// For safety, an order is prevented from being purged if it's open.
    pub fn purge_order(&mut self, client_order_id: ClientOrderId) {
        // Check if order exists and is safe to purge before removing
        let order_cell = self.orders.get(&client_order_id).cloned();

        // Prevent purging open orders
        if let Some(ref order_cell) = order_cell
            && order_cell.borrow().is_open()
        {
            log::warn!("Order {client_order_id} found open when purging, skipping purge");
            return;
        }

        // If order exists in cache, remove it and clean up order-specific indices
        if let Some(ref order_cell) = order_cell {
            let order = order_cell.borrow();
            // Safe to purge
            self.orders.remove(&client_order_id);

            // Remove order from venue index
            if let Some(venue_orders) = self
                .index
                .venue_orders
                .get_mut(&order.instrument_id().venue)
            {
                venue_orders.remove(&client_order_id);
                if venue_orders.is_empty() {
                    self.index.venue_orders.remove(&order.instrument_id().venue);
                }
            }

            // Remove venue order ID index if exists
            if let Some(venue_order_id) = order.venue_order_id() {
                self.index.venue_order_ids.remove(&venue_order_id);
            }

            // Remove from instrument orders index
            if let Some(instrument_orders) =
                self.index.instrument_orders.get_mut(&order.instrument_id())
            {
                instrument_orders.remove(&client_order_id);
                if instrument_orders.is_empty() {
                    self.index.instrument_orders.remove(&order.instrument_id());
                }
            }

            // Remove from position orders index if associated with a position
            if let Some(position_id) = order.position_id()
                && let Some(position_orders) = self.index.position_orders.get_mut(&position_id)
            {
                position_orders.remove(&client_order_id);
                if position_orders.is_empty() {
                    self.index.position_orders.remove(&position_id);
                }
            }

            // Remove from exec algorithm orders index if it has an exec algorithm
            if let Some(exec_algorithm_id) = order.exec_algorithm_id()
                && let Some(exec_algorithm_orders) =
                    self.index.exec_algorithm_orders.get_mut(&exec_algorithm_id)
            {
                exec_algorithm_orders.remove(&client_order_id);
                if exec_algorithm_orders.is_empty() {
                    self.index.exec_algorithm_orders.remove(&exec_algorithm_id);
                }
            }

            // Clean up strategy orders reverse index
            if let Some(strategy_orders) = self.index.strategy_orders.get_mut(&order.strategy_id())
            {
                strategy_orders.remove(&client_order_id);
                if strategy_orders.is_empty() {
                    self.index.strategy_orders.remove(&order.strategy_id());
                }
            }

            // Clean up account orders index
            if let Some(account_id) = order.account_id()
                && let Some(account_orders) = self.index.account_orders.get_mut(&account_id)
            {
                account_orders.remove(&client_order_id);
                if account_orders.is_empty() {
                    self.index.account_orders.remove(&account_id);
                }
            }

            // Clean up exec spawn reverse index (if this order is a spawned child)
            if let Some(exec_spawn_id) = order.exec_spawn_id()
                && let Some(spawn_orders) = self.index.exec_spawn_orders.get_mut(&exec_spawn_id)
            {
                spawn_orders.remove(&client_order_id);
                if spawn_orders.is_empty() {
                    self.index.exec_spawn_orders.remove(&exec_spawn_id);
                }
            }

            log::info!("Purged order {client_order_id}");
        } else {
            log::warn!("Order {client_order_id} not found when purging");
        }

        // Always clean up order indices (even if order was not in cache)
        self.index.order_position.remove(&client_order_id);
        let strategy_id = self.index.order_strategy.remove(&client_order_id);
        self.index.order_client.remove(&client_order_id);
        self.index.client_order_ids.remove(&client_order_id);

        // Clean up reverse index when order not in cache (using forward index)
        if let Some(strategy_id) = strategy_id
            && let Some(strategy_orders) = self.index.strategy_orders.get_mut(&strategy_id)
        {
            strategy_orders.remove(&client_order_id);
            if strategy_orders.is_empty() {
                self.index.strategy_orders.remove(&strategy_id);
            }
        }

        // Remove spawn parent entry if this order was a spawn root
        self.index.exec_spawn_orders.remove(&client_order_id);

        self.index.orders.remove(&client_order_id);
        self.index.orders_active_local.remove(&client_order_id);
        self.index.orders_open.remove(&client_order_id);
        self.index.orders_closed.remove(&client_order_id);
        self.index.orders_emulated.remove(&client_order_id);
        self.index.orders_inflight.remove(&client_order_id);
        self.index.orders_pending_cancel.remove(&client_order_id);
    }

    /// Purges the position with the `position_id` from the cache (if found).
    ///
    /// For safety, a position is prevented from being purged if it's open.
    pub fn purge_position(&mut self, position_id: PositionId) {
        // Snapshot the position so we can release the borrow before mutating indexes.
        let position = self
            .positions
            .get(&position_id)
            .map(|cell| cell.borrow().clone());

        // Prevent purging open positions
        if let Some(ref pos) = position
            && pos.is_open()
        {
            log::warn!("Position {position_id} found open when purging, skipping purge");
            return;
        }

        // If position exists in cache, remove it and clean up position-specific indices
        if let Some(ref pos) = position {
            self.positions.remove(&position_id);

            // Remove from venue positions index
            if let Some(venue_positions) =
                self.index.venue_positions.get_mut(&pos.instrument_id.venue)
            {
                venue_positions.remove(&position_id);
                if venue_positions.is_empty() {
                    self.index.venue_positions.remove(&pos.instrument_id.venue);
                }
            }

            // Remove from instrument positions index
            if let Some(instrument_positions) =
                self.index.instrument_positions.get_mut(&pos.instrument_id)
            {
                instrument_positions.remove(&position_id);
                if instrument_positions.is_empty() {
                    self.index.instrument_positions.remove(&pos.instrument_id);
                }
            }

            // Remove from strategy positions index
            if let Some(strategy_positions) =
                self.index.strategy_positions.get_mut(&pos.strategy_id)
            {
                strategy_positions.remove(&position_id);
                if strategy_positions.is_empty() {
                    self.index.strategy_positions.remove(&pos.strategy_id);
                }
            }

            // Remove from account positions index
            if let Some(account_positions) = self.index.account_positions.get_mut(&pos.account_id) {
                account_positions.remove(&position_id);
                if account_positions.is_empty() {
                    self.index.account_positions.remove(&pos.account_id);
                }
            }

            // Remove position ID from orders that reference it
            for client_order_id in pos.client_order_ids() {
                self.index.order_position.remove(&client_order_id);
            }

            log::info!("Purged position {position_id}");
        } else {
            log::warn!("Position {position_id} not found when purging");
        }

        // Always clean up position indices (even if position not in cache)
        self.index.position_strategy.remove(&position_id);
        self.index.position_orders.remove(&position_id);
        self.index.positions.remove(&position_id);
        self.index.positions_open.remove(&position_id);
        self.index.positions_closed.remove(&position_id);

        // Always clean up position snapshots (even if position not in cache)
        self.position_snapshots.remove(&position_id);
    }

    /// Purges the instrument with the `instrument_id` from the cache (if found).
    ///
    /// All cache-owned data keyed by the instrument is removed: the instrument record,
    /// any synthetic with the same id, order book and own-order-book state, quote/trade
    /// histories, mark/index/funding price histories, instrument status, bars for any
    /// `BarType` referencing the instrument, and the `instrument_orders` /
    /// `instrument_positions` index entries.
    ///
    /// For safety, an instrument is prevented from being purged while any associated
    /// order is non-terminal (anything not in `orders_closed`, including
    /// initialized, submitted, accepted, emulated, released, or inflight states) or
    /// any associated position is non-closed.
    ///
    /// Active subscriptions and other live data-engine state are not touched here;
    /// those belong to the data and execution engines.
    ///
    /// # Warning
    ///
    /// Intended for actors and strategies that have their own lifecycle logic for
    /// deciding when an instrument is no longer needed. Purging an instrument that any
    /// other actor, strategy, or engine still relies on may cause incorrect behavior
    /// (missing instrument lookups, lost market-data history). The caller is
    /// responsible for ensuring the instrument is no longer in use before purging.
    pub fn purge_instrument(&mut self, instrument_id: InstrumentId) {
        #[cfg(feature = "defi")]
        let defi_found = self.defi.pools.contains_key(&instrument_id)
            || self.defi.pool_profilers.contains_key(&instrument_id);
        #[cfg(not(feature = "defi"))]
        let defi_found = false;

        let found = self.instruments.contains_key(&instrument_id)
            || self.synthetics.contains_key(&instrument_id)
            || defi_found;

        if !found {
            log::warn!("Instrument {instrument_id} not found when purging");
            return;
        }

        if let Some(orders) = self.index.instrument_orders.get(&instrument_id) {
            let has_non_terminal = orders
                .iter()
                .any(|client_order_id| !self.index.orders_closed.contains(client_order_id));

            if has_non_terminal {
                log::warn!(
                    "Instrument {instrument_id} has non-terminal orders when purging, skipping purge"
                );
                return;
            }
        }

        if let Some(positions) = self.index.instrument_positions.get(&instrument_id) {
            let has_non_closed = positions
                .iter()
                .any(|position_id| !self.index.positions_closed.contains(position_id));

            if has_non_closed {
                log::warn!(
                    "Instrument {instrument_id} has non-closed positions when purging, skipping purge"
                );
                return;
            }
        }

        self.instruments.remove(&instrument_id);
        self.synthetics.remove(&instrument_id);
        self.books.remove(&instrument_id);
        self.own_books.remove(&instrument_id);
        self.quotes.remove(&instrument_id);
        self.trades.remove(&instrument_id);
        self.mark_prices.remove(&instrument_id);
        self.index_prices.remove(&instrument_id);
        self.funding_rates.remove(&instrument_id);
        self.instrument_statuses.remove(&instrument_id);
        self.greeks.remove(&instrument_id);
        self.option_greeks.remove(&instrument_id);

        self.bars
            .retain(|bar_type, _| bar_type.instrument_id() != instrument_id);

        #[cfg(feature = "defi")]
        {
            self.defi.pools.remove(&instrument_id);
            self.defi.pool_profilers.remove(&instrument_id);
        }

        self.index.instrument_orders.remove(&instrument_id);
        self.index.instrument_positions.remove(&instrument_id);

        log::info!("Purged instrument {instrument_id}");
    }

    /// Purges all account state events which are outside the lookback window.
    ///
    /// Only events which are outside the lookback window will be purged.
    /// A value of 0 means purge all account state events.
    pub fn purge_account_events(&mut self, ts_now: UnixNanos, lookback_secs: u64) {
        log::debug!(
            "Purging account events{}",
            if lookback_secs > 0 {
                format!(" with lookback_secs={lookback_secs}")
            } else {
                String::new()
            }
        );

        for account_cell in self.accounts.values() {
            let mut account = account_cell.borrow_mut();
            let event_count = account.event_count();
            account.purge_account_events(ts_now, lookback_secs);
            let count_diff = event_count - account.event_count();
            if count_diff > 0 {
                log::info!(
                    "Purged {} event(s) from account {}",
                    count_diff,
                    account.id()
                );
            }
        }
    }

    /// Clears the caches index.
    pub fn clear_index(&mut self) {
        self.index.clear();
        log::debug!("Cleared index");
    }

    /// Resets the cache.
    ///
    /// All stateful fields are reset to their initial value. Instruments,
    /// currencies and synthetics are retained when `drop_instruments_on_reset`
    /// is `false` so that repeated backtest runs can reuse the same dataset.
    pub fn reset(&mut self) {
        log::debug!("Resetting cache");

        self.general.clear();
        self.books.clear();
        self.own_books.clear();
        self.quotes.clear();
        self.trades.clear();
        self.mark_xrates.clear();
        self.mark_prices.clear();
        self.index_prices.clear();
        self.funding_rates.clear();
        self.instrument_statuses.clear();
        self.bars.clear();
        self.accounts.clear();
        self.orders.clear();
        self.order_lists.clear();
        self.positions.clear();
        self.position_snapshots.clear();
        self.greeks.clear();
        self.yield_curves.clear();

        if self.config.drop_instruments_on_reset {
            self.currencies.clear();
            self.instruments.clear();
            self.synthetics.clear();
        }

        #[cfg(feature = "defi")]
        {
            self.defi.pools.clear();
            self.defi.pool_profilers.clear();
        }

        self.clear_index();

        log::info!("Reset cache");
    }

    /// Dispose of the cache which will close any underlying database adapter.
    ///
    /// If closing the database connection fails, an error is logged.
    pub fn dispose(&mut self) {
        self.reset();

        if let Some(database) = &mut self.database
            && let Err(e) = database.close()
        {
            log::error!("Failed to close database during dispose: {e}");
        }
    }

    /// Flushes the caches database which permanently removes all persisted data.
    ///
    /// If flushing the database connection fails, an error is logged.
    pub fn flush_db(&mut self) {
        if let Some(database) = &mut self.database
            && let Err(e) = database.flush()
        {
            log::error!("Failed to flush database: {e}");
        }
    }

    /// Adds a raw bytes `value` to the cache under the `key`.
    ///
    /// The cache stores only raw bytes; interpretation is the caller's responsibility.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the entry to the backing database fails.
    pub fn add(&mut self, key: &str, value: Bytes) -> anyhow::Result<()> {
        check_valid_string_ascii(key, stringify!(key))?;
        check_predicate_false(value.is_empty(), stringify!(value))?;

        log::debug!("Adding general {key}");
        self.general.insert(key.to_string(), value.clone());

        if let Some(database) = &mut self.database {
            database.add(key.to_string(), value)?;
        }
        Ok(())
    }

    /// Adds an `OrderBook` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the order book to the backing database fails.
    pub fn add_order_book(&mut self, book: OrderBook) -> anyhow::Result<()> {
        log::debug!("Adding `OrderBook` {}", book.instrument_id);

        if self.config.save_market_data
            && let Some(database) = &mut self.database
        {
            database.add_order_book(&book)?;
        }

        self.books.insert(book.instrument_id, book);
        Ok(())
    }

    /// Adds an `OwnOrderBook` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the own order book fails.
    pub fn add_own_order_book(&mut self, own_book: OwnOrderBook) -> anyhow::Result<()> {
        log::debug!("Adding `OwnOrderBook` {}", own_book.instrument_id);

        self.own_books.insert(own_book.instrument_id, own_book);
        Ok(())
    }

    /// Adds the `mark_price` update to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the mark price to the backing database fails.
    pub fn add_mark_price(&mut self, mark_price: MarkPriceUpdate) -> anyhow::Result<()> {
        log::debug!("Adding `MarkPriceUpdate` for {}", mark_price.instrument_id);

        if self.config.save_market_data {
            // TODO: Placeholder and return Result for consistency
        }

        let mark_prices_deque = self
            .mark_prices
            .entry(mark_price.instrument_id)
            .or_insert_with(|| BoundedVecDeque::new(self.config.tick_capacity));
        mark_prices_deque.push_front(mark_price);
        Ok(())
    }

    /// Adds the `index_price` update to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the index price to the backing database fails.
    pub fn add_index_price(&mut self, index_price: IndexPriceUpdate) -> anyhow::Result<()> {
        log::debug!(
            "Adding `IndexPriceUpdate` for {}",
            index_price.instrument_id
        );

        if self.config.save_market_data {
            // TODO: Placeholder and return Result for consistency
        }

        let index_prices_deque = self
            .index_prices
            .entry(index_price.instrument_id)
            .or_insert_with(|| BoundedVecDeque::new(self.config.tick_capacity));
        index_prices_deque.push_front(index_price);
        Ok(())
    }

    /// Adds the `funding_rate` update to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the funding rate update to the backing database fails.
    pub fn add_funding_rate(&mut self, funding_rate: FundingRateUpdate) -> anyhow::Result<()> {
        log::debug!(
            "Adding `FundingRateUpdate` for {}",
            funding_rate.instrument_id
        );

        if self.config.save_market_data {
            // TODO: Placeholder and return Result for consistency
        }

        let funding_rates_deque = self
            .funding_rates
            .entry(funding_rate.instrument_id)
            .or_insert_with(|| BoundedVecDeque::new(self.config.tick_capacity));
        funding_rates_deque.push_front(funding_rate);
        Ok(())
    }

    /// Adds the given `funding rates` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the trade ticks to the backing database fails.
    pub fn add_funding_rates(&mut self, funding_rates: &[FundingRateUpdate]) -> anyhow::Result<()> {
        check_slice_not_empty(funding_rates, stringify!(funding_rates))?;

        let instrument_id = funding_rates[0].instrument_id;
        log::debug!(
            "Adding `FundingRateUpdate`[{}] {instrument_id}",
            funding_rates.len()
        );

        if self.config.save_market_data
            && let Some(database) = &mut self.database
        {
            for funding_rate in funding_rates {
                database.add_funding_rate(funding_rate)?;
            }
        }

        let funding_rate_deque = self
            .funding_rates
            .entry(instrument_id)
            .or_insert_with(|| BoundedVecDeque::new(self.config.tick_capacity));

        for funding_rate in funding_rates {
            funding_rate_deque.push_front(*funding_rate);
        }
        Ok(())
    }

    /// Adds the `instrument_status` update to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the instrument status to the backing database fails.
    pub fn add_instrument_status(&mut self, status: InstrumentStatus) -> anyhow::Result<()> {
        log::debug!("Adding `InstrumentStatus` for {}", status.instrument_id);

        if self.config.save_market_data {
            // TODO: Placeholder and return Result for consistency
        }

        let statuses_deque = self
            .instrument_statuses
            .entry(status.instrument_id)
            .or_insert_with(|| BoundedVecDeque::new(self.config.tick_capacity));
        statuses_deque.push_front(status);
        Ok(())
    }

    /// Adds the `quote` tick to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the quote tick to the backing database fails.
    pub fn add_quote(&mut self, quote: QuoteTick) -> anyhow::Result<()> {
        log::debug!("Adding `QuoteTick` {}", quote.instrument_id);

        if self.config.save_market_data
            && let Some(database) = &mut self.database
        {
            database.add_quote(&quote)?;
        }

        let quotes_deque = self
            .quotes
            .entry(quote.instrument_id)
            .or_insert_with(|| BoundedVecDeque::new(self.config.tick_capacity));
        quotes_deque.push_front(quote);
        Ok(())
    }

    /// Adds the `quotes` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the quote ticks to the backing database fails.
    pub fn add_quotes(&mut self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        check_slice_not_empty(quotes, stringify!(quotes))?;

        let instrument_id = quotes[0].instrument_id;
        log::debug!("Adding `QuoteTick`[{}] {instrument_id}", quotes.len());

        if self.config.save_market_data
            && let Some(database) = &mut self.database
        {
            for quote in quotes {
                database.add_quote(quote)?;
            }
        }

        let quotes_deque = self
            .quotes
            .entry(instrument_id)
            .or_insert_with(|| BoundedVecDeque::new(self.config.tick_capacity));

        for quote in quotes {
            quotes_deque.push_front(*quote);
        }
        Ok(())
    }

    /// Adds the `trade` tick to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the trade tick to the backing database fails.
    pub fn add_trade(&mut self, trade: TradeTick) -> anyhow::Result<()> {
        log::debug!("Adding `TradeTick` {}", trade.instrument_id);

        if self.config.save_market_data
            && let Some(database) = &mut self.database
        {
            database.add_trade(&trade)?;
        }

        let trades_deque = self
            .trades
            .entry(trade.instrument_id)
            .or_insert_with(|| BoundedVecDeque::new(self.config.tick_capacity));
        trades_deque.push_front(trade);
        Ok(())
    }

    /// Adds the give `trades` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the trade ticks to the backing database fails.
    pub fn add_trades(&mut self, trades: &[TradeTick]) -> anyhow::Result<()> {
        check_slice_not_empty(trades, stringify!(trades))?;

        let instrument_id = trades[0].instrument_id;
        log::debug!("Adding `TradeTick`[{}] {instrument_id}", trades.len());

        if self.config.save_market_data
            && let Some(database) = &mut self.database
        {
            for trade in trades {
                database.add_trade(trade)?;
            }
        }

        let trades_deque = self
            .trades
            .entry(instrument_id)
            .or_insert_with(|| BoundedVecDeque::new(self.config.tick_capacity));

        for trade in trades {
            trades_deque.push_front(*trade);
        }
        Ok(())
    }

    /// Adds the `bar` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the bar to the backing database fails.
    pub fn add_bar(&mut self, bar: Bar) -> anyhow::Result<()> {
        log::debug!("Adding `Bar` {}", bar.bar_type);

        if self.config.save_market_data
            && let Some(database) = &mut self.database
        {
            database.add_bar(&bar)?;
        }

        let bars = self
            .bars
            .entry(bar.bar_type)
            .or_insert_with(|| BoundedVecDeque::new(self.config.bar_capacity));
        bars.push_front(bar);
        Ok(())
    }

    /// Adds the `bars` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the bars to the backing database fails.
    pub fn add_bars(&mut self, bars: &[Bar]) -> anyhow::Result<()> {
        check_slice_not_empty(bars, stringify!(bars))?;

        let bar_type = bars[0].bar_type;
        log::debug!("Adding `Bar`[{}] {bar_type}", bars.len());

        if self.config.save_market_data
            && let Some(database) = &mut self.database
        {
            for bar in bars {
                database.add_bar(bar)?;
            }
        }

        let bars_deque = self
            .bars
            .entry(bar_type)
            .or_insert_with(|| BoundedVecDeque::new(self.config.bar_capacity));

        for bar in bars {
            bars_deque.push_front(*bar);
        }
        Ok(())
    }

    /// Adds the `greeks` data to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the greeks data to the backing database fails.
    pub fn add_greeks(&mut self, greeks: GreeksData) -> anyhow::Result<()> {
        log::debug!("Adding `GreeksData` {}", greeks.instrument_id);

        if self.config.save_market_data
            && let Some(_database) = &mut self.database
        {
            // TODO: Implement database.add_greeks(&greeks) when database adapter is updated
        }

        self.greeks.insert(greeks.instrument_id, greeks);
        Ok(())
    }

    /// Gets the greeks data for the `instrument_id`.
    pub fn greeks(&self, instrument_id: &InstrumentId) -> Option<GreeksData> {
        self.greeks.get(instrument_id).cloned()
    }

    /// Adds exchange-provided option greeks to the cache.
    pub fn add_option_greeks(&mut self, greeks: OptionGreeks) {
        log::debug!("Adding `OptionGreeks` {}", greeks.instrument_id);
        self.option_greeks.insert(greeks.instrument_id, greeks);
    }

    /// Gets a reference to the exchange-provided option greeks for the `instrument_id`.
    #[must_use]
    pub fn option_greeks(&self, instrument_id: &InstrumentId) -> Option<&OptionGreeks> {
        self.option_greeks.get(instrument_id)
    }

    /// Adds the `yield_curve` data to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the yield curve data to the backing database fails.
    pub fn add_yield_curve(&mut self, yield_curve: YieldCurveData) -> anyhow::Result<()> {
        log::debug!("Adding `YieldCurveData` {}", yield_curve.curve_name);

        if self.config.save_market_data
            && let Some(_database) = &mut self.database
        {
            // TODO: Implement database.add_yield_curve(&yield_curve) when database adapter is updated
        }

        self.yield_curves
            .insert(yield_curve.curve_name.clone(), yield_curve);
        Ok(())
    }

    /// Gets the yield curve for the `key`.
    pub fn yield_curve(&self, key: &str) -> Option<Box<dyn Fn(f64) -> f64>> {
        self.yield_curves.get(key).map(|curve| {
            let curve_clone = curve.clone();
            Box::new(move |expiry_in_years: f64| curve_clone.get_rate(expiry_in_years))
                as Box<dyn Fn(f64) -> f64>
        })
    }

    /// Adds the `currency` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the currency to the backing database fails.
    pub fn add_currency(&mut self, currency: Currency) -> anyhow::Result<()> {
        if self.currencies.contains_key(&currency.code) {
            return Ok(());
        }
        log::debug!("Adding `Currency` {}", currency.code);

        if let Some(database) = &mut self.database {
            database.add_currency(&currency)?;
        }

        self.currencies.insert(currency.code, currency);
        Ok(())
    }

    /// Adds the `instrument` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the instrument to the backing database fails.
    pub fn add_instrument(&mut self, instrument: InstrumentAny) -> anyhow::Result<()> {
        log::debug!("Adding `Instrument` {}", instrument.id());

        // Ensure currencies exist in cache - safe to call repeatedly as add_currency is idempotent
        if let Some(base_currency) = instrument.base_currency() {
            self.add_currency(base_currency)?;
        }
        self.add_currency(instrument.quote_currency())?;
        self.add_currency(instrument.settlement_currency())?;

        if let Some(database) = &mut self.database {
            database.add_instrument(&instrument)?;
        }

        self.instruments.insert(instrument.id(), instrument);
        Ok(())
    }

    /// Adds the `synthetic` instrument to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the synthetic instrument to the backing database fails.
    pub fn add_synthetic(&mut self, synthetic: SyntheticInstrument) -> anyhow::Result<()> {
        log::debug!("Adding `SyntheticInstrument` {}", synthetic.id);

        if let Some(database) = &mut self.database {
            database.add_synthetic(&synthetic)?;
        }

        self.synthetics.insert(synthetic.id, synthetic);
        Ok(())
    }

    /// Adds the `account` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the account to the backing database fails.
    pub fn add_account(&mut self, account: AccountAny) -> anyhow::Result<()> {
        log::debug!("Adding `Account` {}", account.id());

        if let Some(database) = &mut self.database {
            database.add_account(&account)?;
        }

        let account_id = account.id();
        self.accounts.insert(account_id, SharedCell::new(account));
        self.index
            .venue_account
            .insert(account_id.get_issuer(), account_id);
        Ok(())
    }

    /// Indexes the `client_order_id` with the `venue_order_id`.
    ///
    /// The `overwrite` parameter determines whether to overwrite any existing cached identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the existing venue order ID conflicts and overwrite is false.
    pub fn add_venue_order_id(
        &mut self,
        client_order_id: &ClientOrderId,
        venue_order_id: &VenueOrderId,
        overwrite: bool,
    ) -> anyhow::Result<()> {
        if let Some(existing_venue_order_id) = self.index.client_order_ids.get(client_order_id)
            && !overwrite
            && existing_venue_order_id != venue_order_id
        {
            anyhow::bail!(
                "Existing {existing_venue_order_id} for {client_order_id}
                    did not match the given {venue_order_id}.
                    If you are writing a test then try a different `venue_order_id`,
                    otherwise this is probably a bug."
            );
        }

        self.index
            .client_order_ids
            .insert(*client_order_id, *venue_order_id);
        self.index
            .venue_order_ids
            .insert(*venue_order_id, *client_order_id);

        Ok(())
    }

    /// Adds the `order` to the cache indexed with any given identifiers.
    ///
    /// # Parameters
    ///
    /// `override_existing`: If the added order should 'override' any existing order and replace
    /// it in the cache. This is currently used for emulated orders which are
    /// being released and transformed into another type.
    ///
    /// # Errors
    ///
    /// Returns an error if not `replace_existing` and the `order.client_order_id` is already contained in the cache.
    pub fn add_order(
        &mut self,
        order: OrderAny,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        replace_existing: bool,
    ) -> anyhow::Result<()> {
        let instrument_id = order.instrument_id();
        let venue = instrument_id.venue;
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let exec_algorithm_id = order.exec_algorithm_id();
        let exec_spawn_id = order.exec_spawn_id();

        if !replace_existing {
            check_key_not_in_map(
                &client_order_id,
                &self.orders,
                stringify!(client_order_id),
                stringify!(orders),
            )?;
        }

        log::debug!("Adding {order:?}");

        self.index.orders.insert(client_order_id);

        if order.is_active_local() {
            self.index.orders_active_local.insert(client_order_id);
        }
        self.index
            .order_strategy
            .insert(client_order_id, strategy_id);
        self.index.strategies.insert(strategy_id);

        // Update venue -> orders index
        self.index
            .venue_orders
            .entry(venue)
            .or_default()
            .insert(client_order_id);

        // Update instrument -> orders index
        self.index
            .instrument_orders
            .entry(instrument_id)
            .or_default()
            .insert(client_order_id);

        // Update strategy -> orders index
        self.index
            .strategy_orders
            .entry(strategy_id)
            .or_default()
            .insert(client_order_id);

        // Update account -> orders index (if account_id known at creation)
        if let Some(account_id) = order.account_id() {
            self.index
                .account_orders
                .entry(account_id)
                .or_default()
                .insert(client_order_id);
        }

        // Update exec_algorithm -> orders index
        if let Some(exec_algorithm_id) = exec_algorithm_id {
            self.index.exec_algorithms.insert(exec_algorithm_id);

            self.index
                .exec_algorithm_orders
                .entry(exec_algorithm_id)
                .or_default()
                .insert(client_order_id);
        }

        // Update exec_spawn -> orders index
        if let Some(exec_spawn_id) = exec_spawn_id {
            self.index
                .exec_spawn_orders
                .entry(exec_spawn_id)
                .or_default()
                .insert(client_order_id);
        }

        // Update emulation index
        if let Some(emulation_trigger) = order.emulation_trigger()
            && emulation_trigger != TriggerType::NoTrigger
        {
            self.index.orders_emulated.insert(client_order_id);
        }

        // Index position ID if provided
        if let Some(position_id) = position_id {
            self.add_position_id(
                &position_id,
                &order.instrument_id().venue,
                &client_order_id,
                &strategy_id,
            )?;
        }

        // Index client ID if provided
        if let Some(client_id) = client_id {
            self.index.order_client.insert(client_order_id, client_id);
            log::debug!("Indexed {client_id:?}");
        }

        if let Some(database) = &mut self.database {
            database.add_order(&order, client_id)?;
            // TODO: Implement
            // if self.config.snapshot_orders {
            //     database.snapshot_order_state(order)?;
            // }
        }

        match self.orders.get(&client_order_id) {
            // Reuse the existing cell on replace so the canonical entry stays in place
            // rather than orphaning a stale cell.
            Some(order_cell) => *order_cell.borrow_mut() = order,
            None => {
                self.orders.insert(client_order_id, SharedCell::new(order));
            }
        }

        Ok(())
    }

    /// Adds the `order_list` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the order list ID is already contained in the cache.
    pub fn add_order_list(&mut self, order_list: OrderList) -> anyhow::Result<()> {
        let order_list_id = order_list.id;
        check_key_not_in_map(
            &order_list_id,
            &self.order_lists,
            stringify!(order_list_id),
            stringify!(order_lists),
        )?;

        log::debug!("Adding {order_list:?}");
        self.order_lists.insert(order_list_id, order_list);
        Ok(())
    }

    /// Indexes the `position_id` with the other given IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if indexing position ID in the backing database fails.
    pub fn add_position_id(
        &mut self,
        position_id: &PositionId,
        venue: &Venue,
        client_order_id: &ClientOrderId,
        strategy_id: &StrategyId,
    ) -> anyhow::Result<()> {
        self.index
            .order_position
            .insert(*client_order_id, *position_id);

        // Index: ClientOrderId -> PositionId
        if let Some(database) = &mut self.database {
            database.index_order_position(*client_order_id, *position_id)?;
        }

        // Index: PositionId -> StrategyId
        self.index
            .position_strategy
            .insert(*position_id, *strategy_id);

        // Index: PositionId -> set[ClientOrderId]
        self.index
            .position_orders
            .entry(*position_id)
            .or_default()
            .insert(*client_order_id);

        // Index: StrategyId -> set[PositionId]
        self.index
            .strategy_positions
            .entry(*strategy_id)
            .or_default()
            .insert(*position_id);

        // Index: Venue -> set[PositionId]
        self.index
            .venue_positions
            .entry(*venue)
            .or_default()
            .insert(*position_id);

        Ok(())
    }

    // Propagates parent OTO `position_id` to contingent children that are missing one.
    //
    // Recovers from a partial-write window during fill handling: the fill-time path in the
    // execution engine assigns `position_id` to each contingent child in a non-atomic loop
    // (`set_position_id` then `add_position_id`), so a crash mid-loop can leave the database
    // with the parent updated and some children un-updated. This pass re-applies any missing
    // assignments after load. Mirrors the Cython behaviour at
    // `nautilus_trader/cache/cache.pyx::_assign_position_id_to_contingencies`.
    fn assign_position_ids_to_contingencies(&mut self) {
        let mut assignments: Vec<(PositionId, ClientOrderId)> = Vec::new();

        for parent_order_cell in self.orders.values() {
            let parent = parent_order_cell.borrow();
            if parent.contingency_type() != Some(ContingencyType::Oto) {
                continue;
            }
            let Some(parent_position_id) = parent.position_id() else {
                continue;
            };
            let Some(linked_order_ids) = parent.linked_order_ids() else {
                continue;
            };

            for client_order_id in linked_order_ids {
                match self.orders.get(client_order_id) {
                    None => {
                        log::error!("Contingency order {client_order_id} not found");
                    }
                    Some(contingent_order_cell) => {
                        if contingent_order_cell.borrow().position_id().is_none() {
                            assignments.push((parent_position_id, *client_order_id));
                        }
                    }
                }
            }
        }

        for (position_id, client_order_id) in assignments {
            let Some((venue, strategy_id)) = self.orders.get(&client_order_id).map(|order_cell| {
                let mut contingent = order_cell.borrow_mut();
                contingent.set_position_id(Some(position_id));
                (contingent.instrument_id().venue, contingent.strategy_id())
            }) else {
                continue;
            };

            // In-memory index updates only. The persistent index entry (if any) was written by
            // the original fill-time `add_position_id` call; replaying the database write here
            // would invoke `CacheDatabaseAdapter::index_order_position`, which is currently
            // `todo!()` on both the Redis and SQL adapters. Until those land, the load-time
            // recovery is in-memory-only: sufficient for the current process to operate, but
            // not durable across another restart.
            self.index
                .order_position
                .insert(client_order_id, position_id);
            self.index
                .position_strategy
                .insert(position_id, strategy_id);
            self.index
                .position_orders
                .entry(position_id)
                .or_default()
                .insert(client_order_id);
            self.index
                .strategy_positions
                .entry(strategy_id)
                .or_default()
                .insert(position_id);
            self.index
                .venue_positions
                .entry(venue)
                .or_default()
                .insert(position_id);
        }
    }

    /// Adds the `position` to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if persisting the position to the backing database fails.
    pub fn add_position(&mut self, position: &Position, _oms_type: OmsType) -> anyhow::Result<()> {
        self.positions
            .insert(position.id, SharedCell::new(position.clone()));
        self.index.positions.insert(position.id);
        self.index.positions_open.insert(position.id);
        self.index.positions_closed.remove(&position.id); // Cleanup for NETTING reopen

        log::debug!("Adding {position}");

        self.add_position_id(
            &position.id,
            &position.instrument_id.venue,
            &position.opening_order_id,
            &position.strategy_id,
        )?;

        let venue = position.instrument_id.venue;
        let venue_positions = self.index.venue_positions.entry(venue).or_default();
        venue_positions.insert(position.id);

        // Index: InstrumentId -> AHashSet
        let instrument_id = position.instrument_id;
        let instrument_positions = self
            .index
            .instrument_positions
            .entry(instrument_id)
            .or_default();
        instrument_positions.insert(position.id);

        // Index: AccountId -> AHashSet<PositionId>
        self.index
            .account_positions
            .entry(position.account_id)
            .or_default()
            .insert(position.id);

        if let Some(database) = &mut self.database {
            database.add_position(position)?;
            // TODO: Implement position snapshots
            // if self.snapshot_positions {
            //     database.snapshot_position_state(
            //         position,
            //         position.ts_last,
            //         self.calculate_unrealized_pnl(&position),
            //     )?;
            // }
        }

        Ok(())
    }

    /// Updates the `account` in the cache.
    ///
    /// Reuses the existing cell when present so any held [`AccountRef`] handles continue to point
    /// at the canonical entry; only inserts a new cell when the account is unknown.
    ///
    /// # Errors
    ///
    /// Returns an error if updating the account in the database fails.
    pub fn update_account(&mut self, account: &AccountAny) -> anyhow::Result<()> {
        let account_id = account.id();
        match self.accounts.get(&account_id) {
            Some(account_cell) => *account_cell.borrow_mut() = account.clone(),
            None => {
                self.accounts
                    .insert(account_id, SharedCell::new(account.clone()));
            }
        }

        if let Some(database) = &mut self.database {
            database.update_account(account)?;
        }
        Ok(())
    }

    /// Removes the `account` from the cache and returns it.
    ///
    /// This supports hot paths which need owned account mutation without
    /// cloning the account event history. The cache is the sole owner of the
    /// account cell (the field is private and accessors only hand out
    /// lifetime-scoped [`AccountRef`] borrows), so the value is moved out of
    /// its cell rather than cloned.
    ///
    /// # Panics
    ///
    /// Panics if the cache no longer holds the only strong handle to the
    /// account cell. This indicates an internal invariant violation: some
    /// component cloned the underlying [`SharedCell`] and held it past the
    /// scope of a single cache method.
    #[must_use]
    pub fn take_account(&mut self, account_id: &AccountId) -> Option<AccountAny> {
        self.accounts.remove(account_id).map(|cell| {
            let rc: Rc<RefCell<AccountAny>> = cell.into();
            Rc::try_unwrap(rc).map_or_else(
                |_| panic!("take_account: cache must be sole owner of {account_id} cell"),
                RefCell::into_inner,
            )
        })
    }

    /// Caches the `account` in memory without updating the database.
    pub fn cache_account_owned(&mut self, account: AccountAny) {
        let account_id = account.id();
        self.index
            .venue_account
            .insert(account_id.get_issuer(), account_id);
        match self.accounts.get(&account_id) {
            Some(account_cell) => *account_cell.borrow_mut() = account,
            None => {
                self.accounts.insert(account_id, SharedCell::new(account));
            }
        }
    }

    /// Updates the `account` in the cache, taking ownership of the updated account.
    ///
    /// # Errors
    ///
    /// Returns an error if updating the account in the database fails.
    pub fn update_account_owned(&mut self, account: AccountAny) -> anyhow::Result<()> {
        let account_id = account.id();
        self.cache_account_owned(account);

        if let Some(database) = &mut self.database {
            let Some(account_cell) = self.accounts.get(&account_id) else {
                anyhow::bail!("Account {account_id} not found after cache update");
            };
            database.update_account(&account_cell.borrow())?;
        }
        Ok(())
    }

    /// Applies an account state event to the cached account.
    ///
    /// Mutates the cached account in place to avoid cloning the account event
    /// history on the hot path; long-running sessions accumulate many events
    /// per account, so a snapshot-clone here would be O(history) per update.
    ///
    /// # Errors
    ///
    /// Returns an error if applying or persisting the account state fails.
    pub fn update_account_state(&mut self, event: &AccountState) -> anyhow::Result<()> {
        let Some(cell) = self.accounts.get(&event.account_id) else {
            return self.add_account(AccountAny::from_events(std::slice::from_ref(event))?);
        };

        cell.borrow_mut().apply(event.clone())?;

        if let Some(database) = &mut self.database {
            database.update_account(&cell.borrow())?;
        }
        Ok(())
    }

    /// Replaces the cached `order` from a non-event snapshot.
    ///
    /// Prefer [`Self::update_order`] for lifecycle state changes. Use this only for order state
    /// that is not represented by [`OrderEventAny`].
    ///
    /// # Errors
    ///
    /// Returns an error if updating the order indexes or database fails.
    pub fn replace_order(&mut self, order: &OrderAny) -> anyhow::Result<()> {
        self.refresh_order(order)?;

        let client_order_id = order.client_order_id();
        match self.orders.get(&client_order_id) {
            // Reuse the existing cell so the canonical entry stays in place rather than
            // orphaning a stale cell.
            Some(order_cell) => *order_cell.borrow_mut() = order.clone(),
            None => {
                self.orders
                    .insert(client_order_id, SharedCell::new(order.clone()));
            }
        }

        Ok(())
    }

    /// Updates the cached order by applying an event and refreshing derived cache state.
    ///
    /// # Errors
    ///
    /// Returns an error if the order is not found or rejects the event.
    pub fn update_order(&mut self, event: &OrderEventAny) -> anyhow::Result<OrderAny> {
        let event_client_order_id = event.client_order_id();
        let client_order_id = if self.order_exists(&event_client_order_id) {
            event_client_order_id
        } else if let Some(venue_order_id) = event.venue_order_id() {
            self.index
                .venue_order_ids
                .get(&venue_order_id)
                .copied()
                .ok_or(OrderError::NotFound(event_client_order_id))?
        } else {
            return Err(OrderError::NotFound(event_client_order_id).into());
        };

        let order_cell = self
            .orders
            .get(&client_order_id)
            .cloned()
            .ok_or(OrderError::NotFound(client_order_id))?;

        // Apply on a snapshot first so a fallible `apply` (e.g. invalid state
        // transition) leaves the canonical cell untouched. On success we swap the
        // post-event value back into the cell so subsequent reads see the new state.
        let mut snapshot = order_cell.borrow().clone();
        snapshot.apply(event.clone())?;
        *order_cell.borrow_mut() = snapshot.clone();

        if let Err(e) = self.refresh_order(&snapshot) {
            log::error!("Error updating order in cache: {e}");
        }

        Ok(snapshot)
    }

    fn refresh_order(&mut self, order: &OrderAny) -> anyhow::Result<()> {
        let client_order_id = order.client_order_id();

        if order.is_active_local() {
            self.index.orders_active_local.insert(client_order_id);
        } else {
            self.index.orders_active_local.remove(&client_order_id);
        }

        // Update venue order ID
        if let Some(venue_order_id) = order.venue_order_id() {
            // If the order is being modified then we allow a changing `VenueOrderId` to accommodate
            // venues which use a cancel+replace update strategy.
            if !self.index.venue_order_ids.contains_key(&venue_order_id) {
                let overwrite = matches!(order.last_event(), OrderEventAny::Updated(_));
                if let Err(e) =
                    self.add_venue_order_id(&order.client_order_id(), &venue_order_id, overwrite)
                {
                    log::error!("Error indexing venue order ID in cache: {e}");
                }
            }
        }

        // Update in-flight state
        if order.is_inflight() {
            self.index.orders_inflight.insert(client_order_id);
        } else {
            self.index.orders_inflight.remove(&client_order_id);
        }

        // Update open/closed state
        if order.is_open() {
            self.index.orders_closed.remove(&client_order_id);
            self.index.orders_open.insert(client_order_id);
        } else if order.is_closed() {
            self.index.orders_open.remove(&client_order_id);
            self.index.orders_pending_cancel.remove(&client_order_id);
            self.index.orders_closed.insert(client_order_id);
        }

        // Update emulation index
        if let Some(emulation_trigger) = order.emulation_trigger()
            && emulation_trigger != TriggerType::NoTrigger
            && !order.is_closed()
        {
            self.index.orders_emulated.insert(client_order_id);
        } else {
            self.index.orders_emulated.remove(&client_order_id);
        }

        // Update account orders index when account_id becomes available
        if let Some(account_id) = order.account_id() {
            self.index
                .account_orders
                .entry(account_id)
                .or_default()
                .insert(client_order_id);
        }

        // Update own book
        if !self.own_books.is_empty() {
            let own_book = self.own_order_book(&order.instrument_id());
            if (own_book.is_some() && order.is_closed()) || should_handle_own_book_order(order) {
                self.update_own_order_book(order);
            }
        }

        if let Some(database) = &mut self.database {
            database.update_order(order.last_event())?;
            // TODO: Implement order snapshots
            // if self.snapshot_orders {
            //     database.snapshot_order_state(order)?;
            // }
        }

        Ok(())
    }

    /// Updates the `order` as pending cancel locally.
    pub fn update_order_pending_cancel_local(&mut self, order: &OrderAny) {
        self.index
            .orders_pending_cancel
            .insert(order.client_order_id());
    }

    /// Updates the `position` in the cache.
    ///
    /// Reuses the existing cell when present so any held [`PositionRef`] handles continue to point
    /// at the canonical entry; only inserts a new cell when the position is unknown.
    ///
    /// # Errors
    ///
    /// Returns an error if updating the position in the database fails.
    pub fn update_position(&mut self, position: &Position) -> anyhow::Result<()> {
        // Update open/closed state

        if position.is_open() {
            self.index.positions_open.insert(position.id);
            self.index.positions_closed.remove(&position.id);
        } else {
            self.index.positions_closed.insert(position.id);
            self.index.positions_open.remove(&position.id);
        }

        if let Some(database) = &mut self.database {
            database.update_position(position)?;
            // TODO: Implement order snapshots
            // if self.snapshot_orders {
            //     database.snapshot_order_state(order)?;
            // }
        }

        match self.positions.get(&position.id) {
            Some(position_cell) => *position_cell.borrow_mut() = position.clone(),
            None => {
                self.positions
                    .insert(position.id, SharedCell::new(position.clone()));
            }
        }

        Ok(())
    }

    /// Creates a snapshot of the `position` by cloning it, assigning a new ID,
    /// serializing it, and storing it in the position snapshots.
    ///
    /// # Errors
    ///
    /// Returns an error if serializing or storing the position snapshot fails.
    pub fn snapshot_position(&mut self, position: &Position) -> anyhow::Result<CacheSnapshotRef> {
        let position_id = position.id;

        let mut copied_position = position.clone();
        let new_id = format!("{}-{}", position_id.as_str(), UUID4::new());
        copied_position.id = PositionId::new(new_id);

        // Serialize the position (TODO: temporarily just to JSON to remove a dependency)
        let position_serialized = serde_json::to_vec(&copied_position)?;
        let snapshot_index = self.position_snapshot_count(&position_id);
        let blob_ref = format!(
            "cache://position-snapshots/{}/{}",
            position_id.as_str(),
            snapshot_index,
        );
        let snapshot_blob = Bytes::from(position_serialized);

        self.add(&blob_ref, snapshot_blob.clone())?;
        self.position_snapshots
            .entry(position_id)
            .or_default()
            .push(snapshot_blob.clone());

        log::debug!("Snapshot {copied_position}");
        Ok(CacheSnapshotRef::new(blob_ref, snapshot_blob))
    }

    /// Loads the cache-owned snapshot blob stored under `blob_ref`.
    ///
    /// The cache first checks in-memory snapshot state. When the blob is not present and a
    /// database adapter exists, the generic cache entries are loaded and checked for the same
    /// opaque reference.
    ///
    /// # Errors
    ///
    /// Returns an error if loading generic cache entries from the backing database fails.
    pub fn load_snapshot_blob(&mut self, blob_ref: &str) -> anyhow::Result<Option<Bytes>> {
        if let Some(blob) = self.snapshot_blob(blob_ref) {
            return Ok(Some(blob));
        }

        if self.database.is_some() {
            self.cache_general()?;
        }

        Ok(self.snapshot_blob(blob_ref))
    }

    /// Restores the cache-owned snapshot blob stored under `blob_ref`.
    ///
    /// Only cache-owned `cache://position-snapshots/...` blobs are currently supported.
    ///
    /// # Errors
    ///
    /// Returns an error if the blob reference is unsupported, malformed, skips earlier
    /// snapshot frames, conflicts with an existing frame, or does not decode to the expected
    /// position snapshot.
    pub fn restore_snapshot_blob(&mut self, blob_ref: &str, blob: Bytes) -> anyhow::Result<()> {
        let (position_id, snapshot_index) = parse_position_snapshot_blob_ref(blob_ref)?;
        validate_position_snapshot_blob(&position_id, blob.as_ref())?;

        let frames = self.position_snapshots.entry(position_id).or_default();
        match frames.get(snapshot_index) {
            Some(existing) if existing == &blob => {}
            Some(_) => {
                anyhow::bail!(
                    "position snapshot frame {snapshot_index} for {position_id} already exists with different bytes"
                );
            }
            None if frames.len() == snapshot_index => frames.push(blob.clone()),
            None => {
                anyhow::bail!(
                    "position snapshot blob_ref {blob_ref} skips missing frame {}",
                    frames.len()
                );
            }
        }

        self.general.insert(blob_ref.to_string(), blob);
        Ok(())
    }

    fn snapshot_blob(&self, blob_ref: &str) -> Option<Bytes> {
        if let Some(blob) = self.general.get(blob_ref) {
            return Some(blob.clone());
        }

        let (position_id, snapshot_index) = parse_position_snapshot_blob_ref(blob_ref).ok()?;
        self.position_snapshots
            .get(&position_id)
            .and_then(|frames| frames.get(snapshot_index))
            .cloned()
    }

    /// Creates a snapshot of the `position` state in the database.
    ///
    /// # Errors
    ///
    /// Returns an error if snapshotting the position state fails.
    pub fn snapshot_position_state(
        &mut self,
        position: &Position,
        // ts_snapshot: u64,
        // unrealized_pnl: Option<Money>,
        open_only: Option<bool>,
    ) -> anyhow::Result<()> {
        let open_only = open_only.unwrap_or(true);

        if open_only && !position.is_open() {
            return Ok(());
        }

        if let Some(database) = &mut self.database {
            database.snapshot_position_state(position).map_err(|e| {
                log::error!(
                    "Failed to snapshot position state for {}: {e:?}",
                    position.id
                );
                e
            })?;
        } else {
            log::warn!(
                "Cannot snapshot position state for {} (no database configured)",
                position.id
            );
        }

        // Ok(())
        todo!()
    }

    /// Gets the OMS type for the `position_id`.
    #[must_use]
    pub fn oms_type(&self, position_id: &PositionId) -> Option<OmsType> {
        // Get OMS type from the index
        if self.index.position_strategy.contains_key(position_id) {
            // For now, we'll default to NETTING
            // TODO: Store and retrieve actual OMS type per position
            Some(OmsType::Netting)
        } else {
            None
        }
    }

    /// Gets the serialized position snapshot frames for the `position_id`.
    ///
    /// Each element in the returned vector is one JSON-encoded [`Position`] snapshot,
    /// in the order they were taken.
    #[must_use]
    pub fn position_snapshot_bytes(&self, position_id: &PositionId) -> Option<Vec<Vec<u8>>> {
        self.position_snapshots
            .get(position_id)
            .map(|frames| frames.iter().map(|b| b.to_vec()).collect())
    }

    /// Returns the number of stored snapshot frames for the `position_id`.
    ///
    /// Returns `0` when no frames are stored. Does not allocate or copy frame bytes.
    #[must_use]
    pub fn position_snapshot_count(&self, position_id: &PositionId) -> usize {
        self.position_snapshots.get(position_id).map_or(0, Vec::len)
    }

    /// Returns all position snapshots with the given optional filters.
    ///
    /// When `position_id` is `Some`, only snapshots for that position are returned.
    /// When `account_id` is `Some`, snapshots are filtered to that account.
    /// Frames that fail to deserialize are skipped with a warning.
    #[must_use]
    pub fn position_snapshots(
        &self,
        position_id: Option<&PositionId>,
        account_id: Option<&AccountId>,
    ) -> Vec<Position> {
        let frames: Box<dyn Iterator<Item = &Bytes> + '_> = match position_id {
            Some(pid) => match self.position_snapshots.get(pid) {
                Some(v) => Box::new(v.iter()),
                None => Box::new(std::iter::empty()),
            },
            None => Box::new(self.position_snapshots.values().flat_map(|v| v.iter())),
        };

        let mut results: Vec<Position> = frames
            .filter_map(|bytes| match serde_json::from_slice::<Position>(bytes) {
                Ok(position) => Some(position),
                Err(e) => {
                    log::warn!("Failed to decode position snapshot: {e}");
                    None
                }
            })
            .collect();

        if let Some(aid) = account_id {
            results.retain(|p| p.account_id == *aid);
        }

        results
    }

    /// Returns position snapshots for `position_id` starting from the `skip`th frame.
    ///
    /// Use this to deserialize only newly appended snapshots when the caller already
    /// processed earlier frames. Returns an empty vector when no frames or fewer than
    /// `skip` frames are stored. Frames that fail to deserialize are skipped with a warning.
    #[must_use]
    pub fn position_snapshots_from(&self, position_id: &PositionId, skip: usize) -> Vec<Position> {
        let Some(frames) = self.position_snapshots.get(position_id) else {
            return Vec::new();
        };

        frames
            .iter()
            .skip(skip)
            .filter_map(|bytes| match serde_json::from_slice::<Position>(bytes) {
                Ok(position) => Some(position),
                Err(e) => {
                    log::warn!("Failed to decode position snapshot: {e}");
                    None
                }
            })
            .collect()
    }

    /// Gets position snapshot IDs for the `instrument_id`.
    #[must_use]
    pub fn position_snapshot_ids(&self, instrument_id: &InstrumentId) -> AHashSet<PositionId> {
        // Get snapshot position IDs that match the instrument
        let mut result = AHashSet::new();

        for (position_id, _) in &self.position_snapshots {
            // Check if this position is for the requested instrument
            if let Some(position_cell) = self.positions.get(position_id)
                && position_cell.borrow().instrument_id == *instrument_id
            {
                result.insert(*position_id);
            }
        }
        result
    }

    /// Snapshots the `order` state in the database.
    ///
    /// # Errors
    ///
    /// Returns an error if snapshotting the order state fails.
    pub fn snapshot_order_state(&self, order: &OrderAny) -> anyhow::Result<()> {
        let Some(database) = &self.database else {
            log::warn!(
                "Cannot snapshot order state for {} (no database configured)",
                order.client_order_id()
            );
            return Ok(());
        };

        database.snapshot_order_state(order)
    }

    // -- IDENTIFIER QUERIES ----------------------------------------------------------------------

    // Collects references to the index sets that constrain an order query.
    //
    // Returns:
    // - `FilterSources::Unfiltered` when no filter is provided (the caller should iterate
    //   the full bucket).
    // - `FilterSources::Empty` when a filter is provided but the index has no entry for it
    //   (the resolved set is unconditionally empty, no further work needed).
    // - `FilterSources::Sets` with borrowed references to each filter source set.
    fn collect_order_filter_sources<'a>(
        &'a self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> FilterSources<'a, ClientOrderId> {
        let mut sources: Vec<&AHashSet<ClientOrderId>> = Vec::with_capacity(4);

        if let Some(venue) = venue {
            match self.index.venue_orders.get(venue) {
                Some(set) => sources.push(set),
                None => return FilterSources::Empty,
            }
        }

        if let Some(instrument_id) = instrument_id {
            match self.index.instrument_orders.get(instrument_id) {
                Some(set) => sources.push(set),
                None => return FilterSources::Empty,
            }
        }

        if let Some(strategy_id) = strategy_id {
            match self.index.strategy_orders.get(strategy_id) {
                Some(set) => sources.push(set),
                None => return FilterSources::Empty,
            }
        }

        if let Some(account_id) = account_id {
            match self.index.account_orders.get(account_id) {
                Some(set) => sources.push(set),
                None => return FilterSources::Empty,
            }
        }

        if sources.is_empty() {
            FilterSources::Unfiltered
        } else {
            FilterSources::Sets(sources)
        }
    }

    fn collect_position_filter_sources<'a>(
        &'a self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> FilterSources<'a, PositionId> {
        let mut sources: Vec<&AHashSet<PositionId>> = Vec::with_capacity(4);

        if let Some(venue) = venue {
            match self.index.venue_positions.get(venue) {
                Some(set) => sources.push(set),
                None => return FilterSources::Empty,
            }
        }

        if let Some(instrument_id) = instrument_id {
            match self.index.instrument_positions.get(instrument_id) {
                Some(set) => sources.push(set),
                None => return FilterSources::Empty,
            }
        }

        if let Some(strategy_id) = strategy_id {
            match self.index.strategy_positions.get(strategy_id) {
                Some(set) => sources.push(set),
                None => return FilterSources::Empty,
            }
        }

        if let Some(account_id) = account_id {
            match self.index.account_positions.get(account_id) {
                Some(set) => sources.push(set),
                None => return FilterSources::Empty,
            }
        }

        if sources.is_empty() {
            FilterSources::Unfiltered
        } else {
            FilterSources::Sets(sources)
        }
    }

    // Materializes the `ClientOrderId`s in `bucket` matching the optional filter parameters.
    //
    // Folds the bucket into the filter sources and runs a single size-ordered intersection,
    // avoiding the legacy two-step build-filter-set + bucket-intersection that allocated and
    // rehashed twice.
    fn query_orders_in_bucket(
        &self,
        bucket: &AHashSet<ClientOrderId>,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        match self.collect_order_filter_sources(venue, instrument_id, strategy_id, account_id) {
            FilterSources::Empty => AHashSet::new(),
            FilterSources::Unfiltered => bucket.clone(),
            FilterSources::Sets(sources) => intersect_pair_or_many(bucket, sources),
        }
    }

    fn query_positions_in_bucket(
        &self,
        bucket: &AHashSet<PositionId>,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<PositionId> {
        match self.collect_position_filter_sources(venue, instrument_id, strategy_id, account_id) {
            FilterSources::Empty => AHashSet::new(),
            FilterSources::Unfiltered => bucket.clone(),
            FilterSources::Sets(sources) => intersect_pair_or_many(bucket, sources),
        }
    }

    // Returns a borrowed or owned view of the orders in `bucket` matching the optional filter
    // parameters. Avoids cloning the bucket when no filter narrows it.
    fn view_orders_in_bucket<'a>(
        &'a self,
        bucket: &'a AHashSet<ClientOrderId>,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Cow<'a, AHashSet<ClientOrderId>> {
        match self.collect_order_filter_sources(venue, instrument_id, strategy_id, account_id) {
            FilterSources::Empty => Cow::Owned(AHashSet::new()),
            FilterSources::Unfiltered => Cow::Borrowed(bucket),
            FilterSources::Sets(sources) => Cow::Owned(intersect_pair_or_many(bucket, sources)),
        }
    }

    fn view_positions_in_bucket<'a>(
        &'a self,
        bucket: &'a AHashSet<PositionId>,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Cow<'a, AHashSet<PositionId>> {
        match self.collect_position_filter_sources(venue, instrument_id, strategy_id, account_id) {
            FilterSources::Empty => Cow::Owned(AHashSet::new()),
            FilterSources::Unfiltered => Cow::Borrowed(bucket),
            FilterSources::Sets(sources) => Cow::Owned(intersect_pair_or_many(bucket, sources)),
        }
    }

    // Returns a lazy iterator yielding the [`ClientOrderId`]s in `bucket` matching the optional
    // filter parameters. Avoids any [`Vec`] or [`AHashSet`] materialization in the result path,
    // and (for multi-filter calls) drives intersection from the smallest source while looking
    // up membership in the rest.
    fn iter_orders_in_bucket<'a>(
        &'a self,
        bucket: &'a AHashSet<ClientOrderId>,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Box<dyn Iterator<Item = ClientOrderId> + 'a> {
        match self.collect_order_filter_sources(venue, instrument_id, strategy_id, account_id) {
            FilterSources::Empty => Box::new(std::iter::empty()),
            FilterSources::Unfiltered => Box::new(bucket.iter().copied()),
            FilterSources::Sets(mut sources) => {
                sources.push(bucket);
                sources.sort_unstable_by_key(|s| s.len());
                let driver = sources[0];
                let rest: Vec<&'a AHashSet<ClientOrderId>> = sources[1..].to_vec();
                Box::new(
                    driver
                        .iter()
                        .copied()
                        .filter(move |id| rest.iter().all(|s| s.contains(id))),
                )
            }
        }
    }

    fn iter_positions_in_bucket<'a>(
        &'a self,
        bucket: &'a AHashSet<PositionId>,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Box<dyn Iterator<Item = PositionId> + 'a> {
        match self.collect_position_filter_sources(venue, instrument_id, strategy_id, account_id) {
            FilterSources::Empty => Box::new(std::iter::empty()),
            FilterSources::Unfiltered => Box::new(bucket.iter().copied()),
            FilterSources::Sets(mut sources) => {
                sources.push(bucket);
                sources.sort_unstable_by_key(|s| s.len());
                let driver = sources[0];
                let rest: Vec<&'a AHashSet<PositionId>> = sources[1..].to_vec();
                Box::new(
                    driver
                        .iter()
                        .copied()
                        .filter(move |id| rest.iter().all(|s| s.contains(id))),
                )
            }
        }
    }

    // Counts orders in `bucket` matching the optional filter parameters.
    //
    // Drives intersection from the smallest filter source (or the bucket itself when no filter
    // is provided) and short-circuits by counting rather than collecting. With a side filter,
    // each candidate order is borrowed via its cell only long enough to inspect the side.
    fn count_orders_in_bucket(
        &self,
        bucket: &AHashSet<ClientOrderId>,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        let side = side.unwrap_or(OrderSide::NoOrderSide);

        match self.collect_order_filter_sources(venue, instrument_id, strategy_id, account_id) {
            FilterSources::Empty => 0,
            FilterSources::Unfiltered => {
                if side == OrderSide::NoOrderSide {
                    bucket.len()
                } else {
                    bucket
                        .iter()
                        .filter(|id| self.order_side_matches(id, side))
                        .count()
                }
            }
            FilterSources::Sets(mut sources) => {
                sources.push(bucket);
                sources.sort_unstable_by_key(|s| s.len());
                let driver = sources[0];
                let rest = &sources[1..];

                driver
                    .iter()
                    .filter(|id| rest.iter().all(|s| s.contains(id)))
                    .filter(|id| {
                        side == OrderSide::NoOrderSide || self.order_side_matches(id, side)
                    })
                    .count()
            }
        }
    }

    fn count_positions_in_bucket(
        &self,
        bucket: &AHashSet<PositionId>,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        let side = side.unwrap_or(PositionSide::NoPositionSide);

        match self.collect_position_filter_sources(venue, instrument_id, strategy_id, account_id) {
            FilterSources::Empty => 0,
            FilterSources::Unfiltered => {
                if side == PositionSide::NoPositionSide {
                    bucket.len()
                } else {
                    bucket
                        .iter()
                        .filter(|id| self.position_side_matches(id, side))
                        .count()
                }
            }
            FilterSources::Sets(mut sources) => {
                sources.push(bucket);
                sources.sort_unstable_by_key(|s| s.len());
                let driver = sources[0];
                let rest = &sources[1..];

                driver
                    .iter()
                    .filter(|id| rest.iter().all(|s| s.contains(id)))
                    .filter(|id| {
                        side == PositionSide::NoPositionSide || self.position_side_matches(id, side)
                    })
                    .count()
            }
        }
    }

    // Returns whether any order in `bucket` matches the optional filter parameters.
    //
    // Mirrors `count_orders_in_bucket` but short-circuits on the first match. Useful for
    // `is_empty`-style gating in hot paths where the caller only needs to know whether at
    // least one matching order exists.
    fn any_orders_in_bucket(
        &self,
        bucket: &AHashSet<ClientOrderId>,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        let side = side.unwrap_or(OrderSide::NoOrderSide);

        match self.collect_order_filter_sources(venue, instrument_id, strategy_id, account_id) {
            FilterSources::Empty => false,
            FilterSources::Unfiltered => {
                if side == OrderSide::NoOrderSide {
                    !bucket.is_empty()
                } else {
                    bucket.iter().any(|id| self.order_side_matches(id, side))
                }
            }
            FilterSources::Sets(mut sources) => {
                sources.push(bucket);
                sources.sort_unstable_by_key(|s| s.len());
                let driver = sources[0];
                let rest = &sources[1..];

                driver
                    .iter()
                    .filter(|id| rest.iter().all(|s| s.contains(id)))
                    .any(|id| side == OrderSide::NoOrderSide || self.order_side_matches(id, side))
            }
        }
    }

    fn any_positions_in_bucket(
        &self,
        bucket: &AHashSet<PositionId>,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> bool {
        let side = side.unwrap_or(PositionSide::NoPositionSide);

        match self.collect_position_filter_sources(venue, instrument_id, strategy_id, account_id) {
            FilterSources::Empty => false,
            FilterSources::Unfiltered => {
                if side == PositionSide::NoPositionSide {
                    !bucket.is_empty()
                } else {
                    bucket.iter().any(|id| self.position_side_matches(id, side))
                }
            }
            FilterSources::Sets(mut sources) => {
                sources.push(bucket);
                sources.sort_unstable_by_key(|s| s.len());
                let driver = sources[0];
                let rest = &sources[1..];

                driver
                    .iter()
                    .filter(|id| rest.iter().all(|s| s.contains(id)))
                    .any(|id| {
                        side == PositionSide::NoPositionSide || self.position_side_matches(id, side)
                    })
            }
        }
    }

    fn order_side_matches(&self, client_order_id: &ClientOrderId, side: OrderSide) -> bool {
        self.orders
            .get(client_order_id)
            .is_some_and(|cell| cell.borrow().order_side() == side)
    }

    fn position_side_matches(&self, position_id: &PositionId, side: PositionSide) -> bool {
        self.positions
            .get(position_id)
            .is_some_and(|cell| cell.borrow().side == side)
    }

    /// Retrieves orders corresponding to the `client_order_ids`, optionally filtering by `side`.
    ///
    /// # Panics
    ///
    /// Panics if any `client_order_id` in the set is not found in the cache.
    fn get_orders_for_ids(
        &self,
        client_order_ids: &AHashSet<ClientOrderId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderRef<'_>> {
        let side = side.unwrap_or(OrderSide::NoOrderSide);
        let mut orders = Vec::new();

        for client_order_id in client_order_ids {
            let order_cell = self
                .orders
                .get(client_order_id)
                .unwrap_or_else(|| panic!("Order {client_order_id} not found"));
            let order = OrderRef::new(order_cell.borrow());

            if side == OrderSide::NoOrderSide || side == order.order_side() {
                orders.push(order);
            }
        }

        // Sort so callers receive a deterministic Vec across runs; the
        // underlying client_order_ids set is AHash-backed.
        orders.sort_by_key(|o| o.client_order_id());
        orders
    }

    /// Retrieves positions corresponding to the `position_ids`, optionally filtering by `side`.
    ///
    /// Each [`PositionRef`] in the returned vector borrows its underlying cell; mutating any of
    /// those positions while the vector is alive will panic at runtime. Drop the vector before
    /// issuing writes.
    ///
    /// # Panics
    ///
    /// Panics if any `position_id` in the set is not found in the cache.
    fn get_positions_for_ids(
        &self,
        position_ids: &AHashSet<PositionId>,
        side: Option<PositionSide>,
    ) -> Vec<PositionRef<'_>> {
        let side = side.unwrap_or(PositionSide::NoPositionSide);
        let mut positions = Vec::new();

        for position_id in position_ids {
            let position_cell = self
                .positions
                .get(position_id)
                .unwrap_or_else(|| panic!("Position {position_id} not found"));
            let position = PositionRef::new(position_cell.borrow());

            if side == PositionSide::NoPositionSide || side == position.side {
                positions.push(position);
            }
        }

        // Sort so callers receive a deterministic Vec across runs; the
        // underlying position_ids set is AHash-backed.
        positions.sort_by_key(|p| p.id);
        positions
    }

    /// Returns the `ClientOrderId`s of all orders.
    #[must_use]
    pub fn client_order_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.query_orders_in_bucket(
            &self.index.orders,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns the `ClientOrderId`s of all open orders.
    #[must_use]
    pub fn client_order_ids_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.query_orders_in_bucket(
            &self.index.orders_open,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns the `ClientOrderId`s of all closed orders.
    #[must_use]
    pub fn client_order_ids_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.query_orders_in_bucket(
            &self.index.orders_closed,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns the `ClientOrderId`s of all locally active orders.
    ///
    /// Locally active orders are in the `INITIALIZED`, `EMULATED`, or `RELEASED` state
    /// (a superset of emulated orders).
    #[must_use]
    pub fn client_order_ids_active_local(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.query_orders_in_bucket(
            &self.index.orders_active_local,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns the `ClientOrderId`s of all emulated orders.
    #[must_use]
    pub fn client_order_ids_emulated(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.query_orders_in_bucket(
            &self.index.orders_emulated,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns the `ClientOrderId`s of all in-flight orders.
    #[must_use]
    pub fn client_order_ids_inflight(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<ClientOrderId> {
        self.query_orders_in_bucket(
            &self.index.orders_inflight,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns `PositionId`s of all positions.
    #[must_use]
    pub fn position_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<PositionId> {
        self.query_positions_in_bucket(
            &self.index.positions,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns the `PositionId`s of all open positions.
    #[must_use]
    pub fn position_open_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<PositionId> {
        self.query_positions_in_bucket(
            &self.index.positions_open,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns the `PositionId`s of all closed positions.
    #[must_use]
    pub fn position_closed_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> AHashSet<PositionId> {
        self.query_positions_in_bucket(
            &self.index.positions_closed,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a borrowed view over the [`ClientOrderId`]s of all orders matching the optional
    /// filter parameters.
    ///
    /// The returned [`Cow`] borrows the underlying index when no filter is provided and only
    /// allocates an owned [`AHashSet`] when an intersection is required. Prefer this over
    /// [`Self::client_order_ids`] when the caller only needs to iterate or read membership.
    #[must_use]
    pub fn client_order_ids_view(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Cow<'_, AHashSet<ClientOrderId>> {
        self.view_orders_in_bucket(
            &self.index.orders,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a borrowed view over the [`ClientOrderId`]s of all open orders.
    #[must_use]
    pub fn client_order_ids_open_view(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Cow<'_, AHashSet<ClientOrderId>> {
        self.view_orders_in_bucket(
            &self.index.orders_open,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a borrowed view over the [`ClientOrderId`]s of all closed orders.
    #[must_use]
    pub fn client_order_ids_closed_view(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Cow<'_, AHashSet<ClientOrderId>> {
        self.view_orders_in_bucket(
            &self.index.orders_closed,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a borrowed view over the [`ClientOrderId`]s of all locally active orders.
    #[must_use]
    pub fn client_order_ids_active_local_view(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Cow<'_, AHashSet<ClientOrderId>> {
        self.view_orders_in_bucket(
            &self.index.orders_active_local,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a borrowed view over the [`ClientOrderId`]s of all emulated orders.
    #[must_use]
    pub fn client_order_ids_emulated_view(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Cow<'_, AHashSet<ClientOrderId>> {
        self.view_orders_in_bucket(
            &self.index.orders_emulated,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a borrowed view over the [`ClientOrderId`]s of all in-flight orders.
    #[must_use]
    pub fn client_order_ids_inflight_view(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Cow<'_, AHashSet<ClientOrderId>> {
        self.view_orders_in_bucket(
            &self.index.orders_inflight,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a borrowed view over the [`PositionId`]s of all positions.
    #[must_use]
    pub fn position_ids_view(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Cow<'_, AHashSet<PositionId>> {
        self.view_positions_in_bucket(
            &self.index.positions,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a borrowed view over the [`PositionId`]s of all open positions.
    #[must_use]
    pub fn position_open_ids_view(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Cow<'_, AHashSet<PositionId>> {
        self.view_positions_in_bucket(
            &self.index.positions_open,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a borrowed view over the [`PositionId`]s of all closed positions.
    #[must_use]
    pub fn position_closed_ids_view(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Cow<'_, AHashSet<PositionId>> {
        self.view_positions_in_bucket(
            &self.index.positions_closed,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a lazy iterator yielding [`ClientOrderId`]s of all orders matching the optional
    /// filter parameters.
    ///
    /// Avoids the [`AHashSet`] allocation performed by [`Self::client_order_ids`]. Useful when
    /// the caller iterates the result once and discards it.
    pub fn iter_client_order_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Box<dyn Iterator<Item = ClientOrderId> + '_> {
        self.iter_orders_in_bucket(
            &self.index.orders,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a lazy iterator yielding [`ClientOrderId`]s of all open orders.
    pub fn iter_client_order_ids_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Box<dyn Iterator<Item = ClientOrderId> + '_> {
        self.iter_orders_in_bucket(
            &self.index.orders_open,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a lazy iterator yielding [`ClientOrderId`]s of all closed orders.
    pub fn iter_client_order_ids_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Box<dyn Iterator<Item = ClientOrderId> + '_> {
        self.iter_orders_in_bucket(
            &self.index.orders_closed,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a lazy iterator yielding [`ClientOrderId`]s of all locally active orders.
    pub fn iter_client_order_ids_active_local(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Box<dyn Iterator<Item = ClientOrderId> + '_> {
        self.iter_orders_in_bucket(
            &self.index.orders_active_local,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a lazy iterator yielding [`ClientOrderId`]s of all emulated orders.
    pub fn iter_client_order_ids_emulated(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Box<dyn Iterator<Item = ClientOrderId> + '_> {
        self.iter_orders_in_bucket(
            &self.index.orders_emulated,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a lazy iterator yielding [`ClientOrderId`]s of all in-flight orders.
    pub fn iter_client_order_ids_inflight(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Box<dyn Iterator<Item = ClientOrderId> + '_> {
        self.iter_orders_in_bucket(
            &self.index.orders_inflight,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a lazy iterator yielding [`PositionId`]s of all positions matching the filters.
    pub fn iter_position_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Box<dyn Iterator<Item = PositionId> + '_> {
        self.iter_positions_in_bucket(
            &self.index.positions,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a lazy iterator yielding [`PositionId`]s of all open positions.
    pub fn iter_position_open_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Box<dyn Iterator<Item = PositionId> + '_> {
        self.iter_positions_in_bucket(
            &self.index.positions_open,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns a lazy iterator yielding [`PositionId`]s of all closed positions.
    pub fn iter_position_closed_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Box<dyn Iterator<Item = PositionId> + '_> {
        self.iter_positions_in_bucket(
            &self.index.positions_closed,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        )
    }

    /// Returns the `ComponentId`s of all actors.
    #[must_use]
    pub fn actor_ids(&self) -> AHashSet<ComponentId> {
        self.index.actors.clone()
    }

    /// Returns the `StrategyId`s of all strategies.
    #[must_use]
    pub fn strategy_ids(&self) -> AHashSet<StrategyId> {
        self.index.strategies.clone()
    }

    /// Returns the `ExecAlgorithmId`s of all execution algorithms.
    #[must_use]
    pub fn exec_algorithm_ids(&self) -> AHashSet<ExecAlgorithmId> {
        self.index.exec_algorithms.clone()
    }

    // -- ORDER QUERIES ---------------------------------------------------------------------------

    /// Gets a borrow of the order with the `client_order_id` (if found).
    ///
    /// The returned [`OrderRef`] is tied to the cache borrow's scope and panics at runtime if
    /// held across a mutation of the same order. Drop the borrow before dispatching events; if
    /// post-event state is required, perform a fresh lookup. Use [`Self::order_owned`] when an
    /// owned snapshot is needed for a boundary handover.
    #[must_use]
    pub fn order(&self, client_order_id: &ClientOrderId) -> Option<OrderRef<'_>> {
        self.orders
            .get(client_order_id)
            .map(|order_cell| OrderRef::new(order_cell.borrow()))
    }

    /// Gets an exclusive write borrow of the order with the `client_order_id` (if found).
    ///
    /// Requires `&mut Cache` so cache writes are reachable only by privileged crates that hold
    /// `Rc<RefCell<Cache>>` directly. Adapter-facing code receives [`CacheView`], which only
    /// exposes immutable cache borrows and therefore cannot reach this method.
    ///
    /// While the returned [`OrderRefMut`] is alive, no other read or write of the same order is
    /// permitted. Drop the borrow before dispatching events or taking any other cache borrow that
    /// may re-enter the same order.
    #[must_use]
    pub fn order_mut(&mut self, client_order_id: &ClientOrderId) -> Option<OrderRefMut<'_>> {
        self.orders
            .get(client_order_id)
            .map(|order_cell| OrderRefMut::new(order_cell.borrow_mut()))
    }

    /// Gets an owned snapshot of the order with the `client_order_id` (if found).
    ///
    /// Use when downstream needs an owned [`OrderAny`] that crosses a boundary (for example, an
    /// adapter `get_order` API). The snapshot will not reflect later cache mutations.
    #[must_use]
    pub fn order_owned(&self, client_order_id: &ClientOrderId) -> Option<OrderAny> {
        self.orders
            .get(client_order_id)
            .map(|order_cell| order_cell.borrow().clone())
    }

    /// Gets cloned orders for the given `client_order_ids`, logging an error for any missing.
    #[must_use]
    pub fn orders_for_ids(
        &self,
        client_order_ids: &[ClientOrderId],
        context: &dyn Display,
    ) -> Vec<OrderAny> {
        let mut orders = Vec::with_capacity(client_order_ids.len());
        for id in client_order_ids {
            match self.orders.get(id) {
                Some(order_cell) => orders.push(order_cell.borrow().clone()),
                None => log::error!("Order {id} not found in cache for {context}"),
            }
        }
        orders
    }

    /// Gets a reference to the client order ID for the `venue_order_id` (if found).
    #[must_use]
    pub fn client_order_id(&self, venue_order_id: &VenueOrderId) -> Option<&ClientOrderId> {
        self.index.venue_order_ids.get(venue_order_id)
    }

    /// Gets a reference to the venue order ID for the `client_order_id` (if found).
    #[must_use]
    pub fn venue_order_id(&self, client_order_id: &ClientOrderId) -> Option<&VenueOrderId> {
        self.index.client_order_ids.get(client_order_id)
    }

    /// Gets a reference to the client ID indexed for then `client_order_id` (if found).
    #[must_use]
    pub fn client_id(&self, client_order_id: &ClientOrderId) -> Option<&ClientId> {
        self.index.order_client.get(client_order_id)
    }

    /// Returns borrows of all orders matching the optional filter parameters.
    ///
    /// Each [`Ref`] in the returned vector borrows its underlying cell; mutating any of
    /// those orders while the vector is alive will panic at runtime. Drop the vector
    /// before issuing writes.
    #[must_use]
    pub fn orders(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderRef<'_>> {
        let client_order_ids = self.client_order_ids(venue, instrument_id, strategy_id, account_id);
        self.get_orders_for_ids(&client_order_ids, side)
    }

    /// Returns borrows of all open orders matching the optional filter parameters.
    #[must_use]
    pub fn orders_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderRef<'_>> {
        let client_order_ids =
            self.client_order_ids_open(venue, instrument_id, strategy_id, account_id);
        self.get_orders_for_ids(&client_order_ids, side)
    }

    /// Returns borrows of all closed orders matching the optional filter parameters.
    #[must_use]
    pub fn orders_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderRef<'_>> {
        let client_order_ids =
            self.client_order_ids_closed(venue, instrument_id, strategy_id, account_id);
        self.get_orders_for_ids(&client_order_ids, side)
    }

    /// Returns borrows of all locally active orders matching the optional filter parameters.
    ///
    /// Locally active orders are in the `INITIALIZED`, `EMULATED`, or `RELEASED` state
    /// (a superset of emulated orders).
    #[must_use]
    pub fn orders_active_local(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderRef<'_>> {
        let client_order_ids =
            self.client_order_ids_active_local(venue, instrument_id, strategy_id, account_id);
        self.get_orders_for_ids(&client_order_ids, side)
    }

    /// Returns borrows of all emulated orders matching the optional filter parameters.
    #[must_use]
    pub fn orders_emulated(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderRef<'_>> {
        let client_order_ids =
            self.client_order_ids_emulated(venue, instrument_id, strategy_id, account_id);
        self.get_orders_for_ids(&client_order_ids, side)
    }

    /// Returns borrows of all in-flight orders matching the optional filter parameters.
    #[must_use]
    pub fn orders_inflight(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderRef<'_>> {
        let client_order_ids =
            self.client_order_ids_inflight(venue, instrument_id, strategy_id, account_id);
        self.get_orders_for_ids(&client_order_ids, side)
    }

    /// Returns borrows of all orders for the `position_id`.
    #[must_use]
    pub fn orders_for_position(&self, position_id: &PositionId) -> Vec<OrderRef<'_>> {
        match self.index.position_orders.get(position_id) {
            Some(client_order_ids) => self.get_orders_for_ids(client_order_ids, None),
            None => Vec::new(),
        }
    }

    /// Returns whether an order with the `client_order_id` exists.
    #[must_use]
    pub fn order_exists(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders.contains(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is open.
    #[must_use]
    pub fn is_order_open(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_open.contains(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is closed.
    #[must_use]
    pub fn is_order_closed(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_closed.contains(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is locally active.
    ///
    /// Locally active orders are in the `INITIALIZED`, `EMULATED`, or `RELEASED` state
    /// (a superset of emulated orders).
    #[must_use]
    pub fn is_order_active_local(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_active_local.contains(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is emulated.
    #[must_use]
    pub fn is_order_emulated(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_emulated.contains(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is in-flight.
    #[must_use]
    pub fn is_order_inflight(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_inflight.contains(client_order_id)
    }

    /// Returns whether an order with the `client_order_id` is `PENDING_CANCEL` locally.
    #[must_use]
    pub fn is_order_pending_cancel_local(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_pending_cancel.contains(client_order_id)
    }

    /// Returns the count of all open orders.
    #[must_use]
    pub fn orders_open_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.count_orders_in_bucket(
            &self.index.orders_open,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns the count of all closed orders.
    #[must_use]
    pub fn orders_closed_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.count_orders_in_bucket(
            &self.index.orders_closed,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns the count of all locally active orders.
    ///
    /// Locally active orders are in the `INITIALIZED`, `EMULATED`, or `RELEASED` state
    /// (a superset of emulated orders).
    #[must_use]
    pub fn orders_active_local_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.count_orders_in_bucket(
            &self.index.orders_active_local,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns the count of all emulated orders.
    #[must_use]
    pub fn orders_emulated_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.count_orders_in_bucket(
            &self.index.orders_emulated,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns the count of all in-flight orders.
    #[must_use]
    pub fn orders_inflight_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.count_orders_in_bucket(
            &self.index.orders_inflight,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns the count of all orders.
    #[must_use]
    pub fn orders_total_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.count_orders_in_bucket(
            &self.index.orders,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns whether any open order matches the optional filter parameters.
    ///
    /// Short-circuits on the first match, avoiding the full intersection walk performed by
    /// [`Self::orders_open_count`]. Prefer this over `orders_open_count(...) > 0` when only
    /// existence matters.
    #[must_use]
    pub fn has_orders_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.any_orders_in_bucket(
            &self.index.orders_open,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns whether any closed order matches the optional filter parameters.
    #[must_use]
    pub fn has_orders_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.any_orders_in_bucket(
            &self.index.orders_closed,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns whether any locally active order matches the optional filter parameters.
    ///
    /// Locally active orders are in the `INITIALIZED`, `EMULATED`, or `RELEASED` state.
    #[must_use]
    pub fn has_orders_active_local(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.any_orders_in_bucket(
            &self.index.orders_active_local,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns whether any emulated order matches the optional filter parameters.
    #[must_use]
    pub fn has_orders_emulated(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.any_orders_in_bucket(
            &self.index.orders_emulated,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns whether any in-flight order matches the optional filter parameters.
    #[must_use]
    pub fn has_orders_inflight(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.any_orders_in_bucket(
            &self.index.orders_inflight,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns whether any order (in any state) matches the optional filter parameters.
    #[must_use]
    pub fn has_orders(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> bool {
        self.any_orders_in_bucket(
            &self.index.orders,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns the order list for the `order_list_id`.
    #[must_use]
    pub fn order_list(&self, order_list_id: &OrderListId) -> Option<&OrderList> {
        self.order_lists.get(order_list_id)
    }

    /// Returns all order lists matching the optional filter parameters.
    #[must_use]
    pub fn order_lists(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
    ) -> Vec<&OrderList> {
        let mut order_lists = self.order_lists.values().collect::<Vec<&OrderList>>();

        if let Some(venue) = venue {
            order_lists.retain(|ol| &ol.instrument_id.venue == venue);
        }

        if let Some(instrument_id) = instrument_id {
            order_lists.retain(|ol| &ol.instrument_id == instrument_id);
        }

        if let Some(strategy_id) = strategy_id {
            order_lists.retain(|ol| &ol.strategy_id == strategy_id);
        }

        if let Some(account_id) = account_id {
            order_lists.retain(|ol| {
                ol.client_order_ids.iter().any(|client_order_id| {
                    self.orders.get(client_order_id).is_some_and(|order_cell| {
                        order_cell.borrow().account_id().as_ref() == Some(account_id)
                    })
                })
            });
        }

        order_lists
    }

    /// Returns whether an order list with the `order_list_id` exists.
    #[must_use]
    pub fn order_list_exists(&self, order_list_id: &OrderListId) -> bool {
        self.order_lists.contains_key(order_list_id)
    }

    // -- EXEC ALGORITHM QUERIES ------------------------------------------------------------------

    /// Returns references to all orders associated with the `exec_algorithm_id` matching the
    /// optional filter parameters.
    #[must_use]
    pub fn orders_for_exec_algorithm(
        &self,
        exec_algorithm_id: &ExecAlgorithmId,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<OrderSide>,
    ) -> Vec<OrderRef<'_>> {
        let Some(exec_algorithm_order_ids) =
            self.index.exec_algorithm_orders.get(exec_algorithm_id)
        else {
            return Vec::new();
        };

        let filtered = self.query_orders_in_bucket(
            exec_algorithm_order_ids,
            venue,
            instrument_id,
            strategy_id,
            account_id,
        );
        self.get_orders_for_ids(&filtered, side)
    }

    /// Returns references to all orders with the `exec_spawn_id`.
    #[must_use]
    pub fn orders_for_exec_spawn(&self, exec_spawn_id: &ClientOrderId) -> Vec<OrderRef<'_>> {
        match self.index.exec_spawn_orders.get(exec_spawn_id) {
            Some(ids) => self.get_orders_for_ids(ids, None),
            None => Vec::new(),
        }
    }

    /// Returns the total order quantity for the `exec_spawn_id`.
    #[must_use]
    pub fn exec_spawn_total_quantity(
        &self,
        exec_spawn_id: &ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        let exec_spawn_orders = self.orders_for_exec_spawn(exec_spawn_id);

        let mut total_quantity: Option<Quantity> = None;

        for spawn_order in exec_spawn_orders {
            if active_only && spawn_order.is_closed() {
                continue;
            }

            match total_quantity.as_mut() {
                Some(total) => *total = *total + spawn_order.quantity(),
                None => total_quantity = Some(spawn_order.quantity()),
            }
        }

        total_quantity
    }

    /// Returns the total filled quantity for all orders with the `exec_spawn_id`.
    #[must_use]
    pub fn exec_spawn_total_filled_qty(
        &self,
        exec_spawn_id: &ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        let exec_spawn_orders = self.orders_for_exec_spawn(exec_spawn_id);

        let mut total_quantity: Option<Quantity> = None;

        for spawn_order in exec_spawn_orders {
            if active_only && spawn_order.is_closed() {
                continue;
            }

            match total_quantity.as_mut() {
                Some(total) => *total = *total + spawn_order.filled_qty(),
                None => total_quantity = Some(spawn_order.filled_qty()),
            }
        }

        total_quantity
    }

    /// Returns the total leaves quantity for all orders with the `exec_spawn_id`.
    #[must_use]
    pub fn exec_spawn_total_leaves_qty(
        &self,
        exec_spawn_id: &ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        let exec_spawn_orders = self.orders_for_exec_spawn(exec_spawn_id);

        let mut total_quantity: Option<Quantity> = None;

        for spawn_order in exec_spawn_orders {
            if active_only && spawn_order.is_closed() {
                continue;
            }

            match total_quantity.as_mut() {
                Some(total) => *total = *total + spawn_order.leaves_qty(),
                None => total_quantity = Some(spawn_order.leaves_qty()),
            }
        }

        total_quantity
    }

    // -- POSITION QUERIES ------------------------------------------------------------------------

    /// Returns a borrow of the position with the `position_id` (if found).
    #[must_use]
    pub fn position(&self, position_id: &PositionId) -> Option<PositionRef<'_>> {
        self.positions
            .get(position_id)
            .map(|position_cell| PositionRef::new(position_cell.borrow()))
    }

    /// Gets an exclusive write borrow of the position with the `position_id` (if found).
    ///
    /// Requires `&mut Cache` so cache writes are reachable only by privileged crates that hold
    /// `Rc<RefCell<Cache>>` directly. Adapter-facing code receives [`CacheView`], which only
    /// exposes immutable cache borrows and therefore cannot reach this method.
    ///
    /// While the returned [`PositionRefMut`] is alive, no other read or write of the same position
    /// is permitted. Drop the borrow before dispatching events or taking any other cache borrow
    /// that may re-enter the same position.
    #[must_use]
    pub fn position_mut(&mut self, position_id: &PositionId) -> Option<PositionRefMut<'_>> {
        self.positions
            .get(position_id)
            .map(|position_cell| PositionRefMut::new(position_cell.borrow_mut()))
    }

    /// Gets an owned snapshot of the position with the `position_id` (if found).
    ///
    /// Use when downstream needs an owned [`Position`] that crosses a boundary. The snapshot will
    /// not reflect later cache mutations.
    #[must_use]
    pub fn position_owned(&self, position_id: &PositionId) -> Option<Position> {
        self.positions
            .get(position_id)
            .map(|position_cell| position_cell.borrow().clone())
    }

    /// Returns a borrow of the position for the `client_order_id` (if found).
    #[must_use]
    pub fn position_for_order(&self, client_order_id: &ClientOrderId) -> Option<PositionRef<'_>> {
        self.index
            .order_position
            .get(client_order_id)
            .and_then(|position_id| self.positions.get(position_id))
            .map(|position_cell| PositionRef::new(position_cell.borrow()))
    }

    /// Returns a reference to the position ID for the `client_order_id` (if found).
    #[must_use]
    pub fn position_id(&self, client_order_id: &ClientOrderId) -> Option<&PositionId> {
        self.index.order_position.get(client_order_id)
    }

    /// Returns borrows of all positions matching the optional filter parameters.
    ///
    /// Each [`PositionRef`] in the returned vector borrows its underlying cell; mutating any of
    /// those positions while the vector is alive will panic at runtime. Drop the vector before
    /// issuing writes.
    #[must_use]
    pub fn positions(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> Vec<PositionRef<'_>> {
        let position_ids = self.position_ids(venue, instrument_id, strategy_id, account_id);
        self.get_positions_for_ids(&position_ids, side)
    }

    /// Returns borrows of all open positions matching the optional filter parameters.
    #[must_use]
    pub fn positions_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> Vec<PositionRef<'_>> {
        let position_ids = self.position_open_ids(venue, instrument_id, strategy_id, account_id);
        self.get_positions_for_ids(&position_ids, side)
    }

    /// Returns borrows of all closed positions matching the optional filter parameters.
    #[must_use]
    pub fn positions_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> Vec<PositionRef<'_>> {
        let position_ids = self.position_closed_ids(venue, instrument_id, strategy_id, account_id);
        self.get_positions_for_ids(&position_ids, side)
    }

    /// Returns whether a position with the `position_id` exists.
    #[must_use]
    pub fn position_exists(&self, position_id: &PositionId) -> bool {
        self.index.positions.contains(position_id)
    }

    /// Returns whether a position with the `position_id` is open.
    #[must_use]
    pub fn is_position_open(&self, position_id: &PositionId) -> bool {
        self.index.positions_open.contains(position_id)
    }

    /// Returns whether a position with the `position_id` is closed.
    #[must_use]
    pub fn is_position_closed(&self, position_id: &PositionId) -> bool {
        self.index.positions_closed.contains(position_id)
    }

    /// Returns the count of all open positions.
    #[must_use]
    pub fn positions_open_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.count_positions_in_bucket(
            &self.index.positions_open,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns the count of all closed positions.
    #[must_use]
    pub fn positions_closed_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.count_positions_in_bucket(
            &self.index.positions_closed,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns the count of all positions.
    #[must_use]
    pub fn positions_total_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.count_positions_in_bucket(
            &self.index.positions,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns whether any open position matches the optional filter parameters.
    ///
    /// Short-circuits on the first match, avoiding the full intersection walk performed by
    /// [`Self::positions_open_count`]. Prefer this over `positions_open_count(...) > 0` when
    /// only existence matters.
    #[must_use]
    pub fn has_positions_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> bool {
        self.any_positions_in_bucket(
            &self.index.positions_open,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns whether any closed position matches the optional filter parameters.
    #[must_use]
    pub fn has_positions_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> bool {
        self.any_positions_in_bucket(
            &self.index.positions_closed,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    /// Returns whether any position (open or closed) matches the optional filter parameters.
    #[must_use]
    pub fn has_positions(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        account_id: Option<&AccountId>,
        side: Option<PositionSide>,
    ) -> bool {
        self.any_positions_in_bucket(
            &self.index.positions,
            venue,
            instrument_id,
            strategy_id,
            account_id,
            side,
        )
    }

    // -- STRATEGY QUERIES ------------------------------------------------------------------------

    /// Gets a reference to the strategy ID for the `client_order_id` (if found).
    #[must_use]
    pub fn strategy_id_for_order(&self, client_order_id: &ClientOrderId) -> Option<&StrategyId> {
        self.index.order_strategy.get(client_order_id)
    }

    /// Gets a reference to the strategy ID for the `position_id` (if found).
    #[must_use]
    pub fn strategy_id_for_position(&self, position_id: &PositionId) -> Option<&StrategyId> {
        self.index.position_strategy.get(position_id)
    }

    // -- GENERAL ---------------------------------------------------------------------------------

    /// Gets a reference to the general value for the `key` (if found).
    ///
    /// # Errors
    ///
    /// Returns an error if the `key` is invalid.
    pub fn get(&self, key: &str) -> anyhow::Result<Option<&Bytes>> {
        check_valid_string_ascii(key, stringify!(key))?;

        Ok(self.general.get(key))
    }

    // -- DATA QUERIES ----------------------------------------------------------------------------

    /// Returns the price for the `instrument_id` and `price_type` (if found).
    #[must_use]
    pub fn price(&self, instrument_id: &InstrumentId, price_type: PriceType) -> Option<Price> {
        match price_type {
            PriceType::Bid => self
                .quotes
                .get(instrument_id)
                .and_then(|quotes| quotes.front().map(|quote| quote.bid_price)),
            PriceType::Ask => self
                .quotes
                .get(instrument_id)
                .and_then(|quotes| quotes.front().map(|quote| quote.ask_price)),
            PriceType::Mid => self.quotes.get(instrument_id).and_then(|quotes| {
                quotes.front().map(|quote| {
                    Price::new(
                        f64::midpoint(quote.ask_price.as_f64(), quote.bid_price.as_f64()),
                        quote.bid_price.precision + 1,
                    )
                })
            }),
            PriceType::Last => self
                .trades
                .get(instrument_id)
                .and_then(|trades| trades.front().map(|trade| trade.price)),
            PriceType::Mark => self
                .mark_prices
                .get(instrument_id)
                .and_then(|marks| marks.front().map(|mark| mark.value)),
        }
    }

    /// Gets all quotes for the `instrument_id`.
    #[must_use]
    pub fn quotes(&self, instrument_id: &InstrumentId) -> Option<Vec<QuoteTick>> {
        self.quotes
            .get(instrument_id)
            .map(|quotes| quotes.iter().copied().collect())
    }

    /// Gets all trades for the `instrument_id`.
    #[must_use]
    pub fn trades(&self, instrument_id: &InstrumentId) -> Option<Vec<TradeTick>> {
        self.trades
            .get(instrument_id)
            .map(|trades| trades.iter().copied().collect())
    }

    /// Gets all mark price updates for the `instrument_id`.
    #[must_use]
    pub fn mark_prices(&self, instrument_id: &InstrumentId) -> Option<Vec<MarkPriceUpdate>> {
        self.mark_prices
            .get(instrument_id)
            .map(|mark_prices| mark_prices.iter().copied().collect())
    }

    /// Gets all index price updates for the `instrument_id`.
    #[must_use]
    pub fn index_prices(&self, instrument_id: &InstrumentId) -> Option<Vec<IndexPriceUpdate>> {
        self.index_prices
            .get(instrument_id)
            .map(|index_prices| index_prices.iter().copied().collect())
    }

    /// Gets all funding rate updates for the `instrument_id`.
    #[must_use]
    pub fn funding_rates(&self, instrument_id: &InstrumentId) -> Option<Vec<FundingRateUpdate>> {
        self.funding_rates
            .get(instrument_id)
            .map(|funding_rates| funding_rates.iter().copied().collect())
    }

    /// Gets all instrument status updates for the `instrument_id`.
    #[must_use]
    pub fn instrument_statuses(
        &self,
        instrument_id: &InstrumentId,
    ) -> Option<Vec<InstrumentStatus>> {
        self.instrument_statuses
            .get(instrument_id)
            .map(|statuses| statuses.iter().copied().collect())
    }

    /// Gets all bars for the `bar_type`.
    #[must_use]
    pub fn bars(&self, bar_type: &BarType) -> Option<Vec<Bar>> {
        self.bars
            .get(bar_type)
            .map(|bars| bars.iter().copied().collect())
    }

    /// Gets a reference to the order book for the `instrument_id`.
    #[must_use]
    pub fn order_book(&self, instrument_id: &InstrumentId) -> Option<&OrderBook> {
        self.books.get(instrument_id)
    }

    /// Gets a reference to the order book for the `instrument_id`.
    #[must_use]
    pub fn order_book_mut(&mut self, instrument_id: &InstrumentId) -> Option<&mut OrderBook> {
        self.books.get_mut(instrument_id)
    }

    /// Gets a reference to the own order book for the `instrument_id`.
    #[must_use]
    pub fn own_order_book(&self, instrument_id: &InstrumentId) -> Option<&OwnOrderBook> {
        self.own_books.get(instrument_id)
    }

    /// Gets a reference to the own order book for the `instrument_id`.
    #[must_use]
    pub fn own_order_book_mut(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> Option<&mut OwnOrderBook> {
        self.own_books.get_mut(instrument_id)
    }

    /// Gets a reference to the latest quote for the `instrument_id`.
    #[must_use]
    pub fn quote(&self, instrument_id: &InstrumentId) -> Option<&QuoteTick> {
        self.quotes
            .get(instrument_id)
            .and_then(|quotes| quotes.front())
    }

    /// Gets a reference to the quote at `index` for the `instrument_id`.
    ///
    /// Index 0 is the most recent.
    #[must_use]
    pub fn quote_at_index(&self, instrument_id: &InstrumentId, index: usize) -> Option<&QuoteTick> {
        self.quotes
            .get(instrument_id)
            .and_then(|quotes| quotes.get(index))
    }

    /// Gets a reference to the latest trade for the `instrument_id`.
    #[must_use]
    pub fn trade(&self, instrument_id: &InstrumentId) -> Option<&TradeTick> {
        self.trades
            .get(instrument_id)
            .and_then(|trades| trades.front())
    }

    /// Gets a reference to the trade at `index` for the `instrument_id`.
    ///
    /// Index 0 is the most recent.
    #[must_use]
    pub fn trade_at_index(&self, instrument_id: &InstrumentId, index: usize) -> Option<&TradeTick> {
        self.trades
            .get(instrument_id)
            .and_then(|trades| trades.get(index))
    }

    /// Gets a reference to the latest mark price update for the `instrument_id`.
    #[must_use]
    pub fn mark_price(&self, instrument_id: &InstrumentId) -> Option<&MarkPriceUpdate> {
        self.mark_prices
            .get(instrument_id)
            .and_then(|mark_prices| mark_prices.front())
    }

    /// Gets a reference to the latest index price update for the `instrument_id`.
    #[must_use]
    pub fn index_price(&self, instrument_id: &InstrumentId) -> Option<&IndexPriceUpdate> {
        self.index_prices
            .get(instrument_id)
            .and_then(|index_prices| index_prices.front())
    }

    /// Gets a reference to the latest funding rate update for the `instrument_id`.
    #[must_use]
    pub fn funding_rate(&self, instrument_id: &InstrumentId) -> Option<&FundingRateUpdate> {
        self.funding_rates
            .get(instrument_id)
            .and_then(|funding_rates| funding_rates.front())
    }

    /// Gets a reference to the latest instrument status update for the `instrument_id`.
    #[must_use]
    pub fn instrument_status(&self, instrument_id: &InstrumentId) -> Option<&InstrumentStatus> {
        self.instrument_statuses
            .get(instrument_id)
            .and_then(|statuses| statuses.front())
    }

    /// Gets a reference to the latest bar for the `bar_type`.
    #[must_use]
    pub fn bar(&self, bar_type: &BarType) -> Option<&Bar> {
        self.bars.get(bar_type).and_then(|bars| bars.front())
    }

    /// Gets a reference to the bar at `index` for the `bar_type`.
    ///
    /// Index 0 is the most recent.
    #[must_use]
    pub fn bar_at_index(&self, bar_type: &BarType, index: usize) -> Option<&Bar> {
        self.bars.get(bar_type).and_then(|bars| bars.get(index))
    }

    /// Gets the order book update count for the `instrument_id`.
    #[must_use]
    pub fn book_update_count(&self, instrument_id: &InstrumentId) -> usize {
        self.books
            .get(instrument_id)
            .map_or(0, |book| book.update_count) as usize
    }

    /// Gets the quote tick count for the `instrument_id`.
    #[must_use]
    pub fn quote_count(&self, instrument_id: &InstrumentId) -> usize {
        self.quotes
            .get(instrument_id)
            .map_or(0, BoundedVecDeque::len)
    }

    /// Gets the trade tick count for the `instrument_id`.
    #[must_use]
    pub fn trade_count(&self, instrument_id: &InstrumentId) -> usize {
        self.trades
            .get(instrument_id)
            .map_or(0, BoundedVecDeque::len)
    }

    /// Gets the bar count for the `instrument_id`.
    #[must_use]
    pub fn bar_count(&self, bar_type: &BarType) -> usize {
        self.bars.get(bar_type).map_or(0, BoundedVecDeque::len)
    }

    /// Returns whether the cache contains an order book for the `instrument_id`.
    #[must_use]
    pub fn has_order_book(&self, instrument_id: &InstrumentId) -> bool {
        self.books.contains_key(instrument_id)
    }

    /// Returns whether the cache contains quotes for the `instrument_id`.
    #[must_use]
    pub fn has_quote_ticks(&self, instrument_id: &InstrumentId) -> bool {
        self.quote_count(instrument_id) > 0
    }

    /// Returns whether the cache contains trades for the `instrument_id`.
    #[must_use]
    pub fn has_trade_ticks(&self, instrument_id: &InstrumentId) -> bool {
        self.trade_count(instrument_id) > 0
    }

    /// Returns whether the cache contains bars for the `bar_type`.
    #[must_use]
    pub fn has_bars(&self, bar_type: &BarType) -> bool {
        self.bar_count(bar_type) > 0
    }

    #[must_use]
    pub fn get_xrate(
        &self,
        venue: Venue,
        from_currency: Currency,
        to_currency: Currency,
        price_type: PriceType,
    ) -> Option<f64> {
        if from_currency == to_currency {
            // When the source and target currencies are identical,
            // no conversion is needed; return an exchange rate of 1.0.
            return Some(1.0);
        }

        let (bid_quote, ask_quote) = self.build_quote_table(&venue);

        match get_exchange_rate(
            from_currency.code,
            to_currency.code,
            price_type,
            bid_quote,
            ask_quote,
        ) {
            Ok(rate) => rate,
            Err(e) => {
                log::error!("Failed to calculate xrate: {e}");
                None
            }
        }
    }

    fn build_quote_table(&self, venue: &Venue) -> (AHashMap<String, f64>, AHashMap<String, f64>) {
        let mut bid_quotes = AHashMap::new();
        let mut ask_quotes = AHashMap::new();

        for instrument_id in self.instruments.keys() {
            if instrument_id.venue != *venue {
                continue;
            }

            let (bid_price, ask_price) = if let Some(ticks) = self.quotes.get(instrument_id) {
                if let Some(tick) = ticks.front() {
                    (tick.bid_price, tick.ask_price)
                } else {
                    continue; // Empty ticks vector
                }
            } else {
                let bid_bar = self
                    .bars
                    .iter()
                    .find(|(k, _)| {
                        k.instrument_id() == *instrument_id
                            && matches!(k.spec().price_type, PriceType::Bid)
                    })
                    .map(|(_, v)| v);

                let ask_bar = self
                    .bars
                    .iter()
                    .find(|(k, _)| {
                        k.instrument_id() == *instrument_id
                            && matches!(k.spec().price_type, PriceType::Ask)
                    })
                    .map(|(_, v)| v);

                match (bid_bar, ask_bar) {
                    (Some(bid), Some(ask)) => {
                        match (bid.front(), ask.front()) {
                            (Some(bid_bar), Some(ask_bar)) => (bid_bar.close, ask_bar.close),
                            _ => {
                                // Empty bar VecDeques
                                continue;
                            }
                        }
                    }
                    _ => continue,
                }
            };

            bid_quotes.insert(instrument_id.symbol.to_string(), bid_price.as_f64());
            ask_quotes.insert(instrument_id.symbol.to_string(), ask_price.as_f64());
        }

        (bid_quotes, ask_quotes)
    }

    /// Returns the mark exchange rate for the given currency pair, or `None` if not set.
    #[must_use]
    pub fn get_mark_xrate(&self, from_currency: Currency, to_currency: Currency) -> Option<f64> {
        self.mark_xrates.get(&(from_currency, to_currency)).copied()
    }

    /// Sets the mark exchange rate for the given currency pair and automatically sets the inverse rate.
    ///
    /// # Panics
    ///
    /// Panics if `xrate` is not positive.
    pub fn set_mark_xrate(&mut self, from_currency: Currency, to_currency: Currency, xrate: f64) {
        assert!(xrate > 0.0, "xrate was zero");
        self.mark_xrates.insert((from_currency, to_currency), xrate);
        self.mark_xrates
            .insert((to_currency, from_currency), 1.0 / xrate);
    }

    /// Clears the mark exchange rate for the given currency pair.
    pub fn clear_mark_xrate(&mut self, from_currency: Currency, to_currency: Currency) {
        let _ = self.mark_xrates.remove(&(from_currency, to_currency));
    }

    /// Clears all mark exchange rates.
    pub fn clear_mark_xrates(&mut self) {
        self.mark_xrates.clear();
    }

    // -- INSTRUMENT QUERIES ----------------------------------------------------------------------

    /// Returns a reference to the instrument for the `instrument_id` (if found).
    #[must_use]
    pub fn instrument(&self, instrument_id: &InstrumentId) -> Option<&InstrumentAny> {
        self.instruments.get(instrument_id)
    }

    /// Returns references to all instrument IDs for the `venue`.
    #[must_use]
    pub fn instrument_ids(&self, venue: Option<&Venue>) -> Vec<&InstrumentId> {
        match venue {
            Some(v) => self.instruments.keys().filter(|i| &i.venue == v).collect(),
            None => self.instruments.keys().collect(),
        }
    }

    /// Returns references to all instruments for the `venue`.
    #[must_use]
    pub fn instruments(&self, venue: &Venue, underlying: Option<&Ustr>) -> Vec<&InstrumentAny> {
        self.instruments
            .values()
            .filter(|i| &i.id().venue == venue)
            .filter(|i| underlying.is_none_or(|u| i.underlying() == Some(*u)))
            .collect()
    }

    /// Returns references to all instruments for the `venue` whose underlying
    /// equals `root` and whose [`InstrumentClass`] equals `class`.
    ///
    /// Use when expanding a parent-symbol subscription: filtering by class as
    /// well as root prevents leaves of a different class (e.g. options when
    /// the user asked for futures, or vice versa) from being pulled in.
    #[must_use]
    pub fn instruments_by_parent(
        &self,
        venue: &Venue,
        root: &Ustr,
        class: InstrumentClass,
    ) -> Vec<&InstrumentAny> {
        self.instruments
            .values()
            .filter(|i| &i.id().venue == venue)
            .filter(|i| i.underlying() == Some(*root))
            .filter(|i| i.instrument_class() == class)
            .collect()
    }

    /// Returns references to all bar types contained in the cache.
    #[must_use]
    pub fn bar_types(
        &self,
        instrument_id: Option<&InstrumentId>,
        price_type: Option<&PriceType>,
        aggregation_source: AggregationSource,
    ) -> Vec<&BarType> {
        let mut bar_types = self
            .bars
            .keys()
            .filter(|bar_type| bar_type.aggregation_source() == aggregation_source)
            .collect::<Vec<&BarType>>();

        if let Some(instrument_id) = instrument_id {
            bar_types.retain(|bar_type| bar_type.instrument_id() == *instrument_id);
        }

        if let Some(price_type) = price_type {
            bar_types.retain(|bar_type| &bar_type.spec().price_type == price_type);
        }

        bar_types
    }

    // -- SYNTHETIC QUERIES -----------------------------------------------------------------------

    /// Returns a reference to the synthetic instrument for the `instrument_id` (if found).
    #[must_use]
    pub fn synthetic(&self, instrument_id: &InstrumentId) -> Option<&SyntheticInstrument> {
        self.synthetics.get(instrument_id)
    }

    /// Returns references to instrument IDs for all synthetic instruments contained in the cache.
    #[must_use]
    pub fn synthetic_ids(&self) -> Vec<&InstrumentId> {
        self.synthetics.keys().collect()
    }

    /// Returns references to all synthetic instruments contained in the cache.
    #[must_use]
    pub fn synthetics(&self) -> Vec<&SyntheticInstrument> {
        self.synthetics.values().collect()
    }

    // -- ACCOUNT QUERIES -----------------------------------------------------------------------

    /// Returns a borrow of the account for the `account_id` (if found).
    #[must_use]
    pub fn account(&self, account_id: &AccountId) -> Option<AccountRef<'_>> {
        self.accounts
            .get(account_id)
            .map(|account_cell| AccountRef::new(account_cell.borrow()))
    }

    /// Gets an exclusive write borrow of the account with the `account_id` (if found).
    ///
    /// Requires `&mut Cache` so cache writes are reachable only by privileged crates that hold
    /// `Rc<RefCell<Cache>>` directly. Adapter-facing code receives [`CacheView`], which only
    /// exposes immutable cache borrows and therefore cannot reach this method.
    ///
    /// While the returned [`AccountRefMut`] is alive, no other read or write of the same account
    /// is permitted. Drop the borrow before dispatching events or taking any other cache borrow
    /// that may re-enter the same account.
    #[must_use]
    pub fn account_mut(&mut self, account_id: &AccountId) -> Option<AccountRefMut<'_>> {
        self.accounts
            .get(account_id)
            .map(|account_cell| AccountRefMut::new(account_cell.borrow_mut()))
    }

    /// Gets an owned snapshot of the account with the `account_id` (if found).
    ///
    /// Use when downstream needs an owned [`AccountAny`] that crosses a boundary. The snapshot
    /// will not reflect later cache mutations.
    #[must_use]
    pub fn account_owned(&self, account_id: &AccountId) -> Option<AccountAny> {
        self.accounts
            .get(account_id)
            .map(|account_cell| account_cell.borrow().clone())
    }

    /// Returns a borrow of the account for the `venue` (if found).
    #[must_use]
    pub fn account_for_venue(&self, venue: &Venue) -> Option<AccountRef<'_>> {
        self.index
            .venue_account
            .get(venue)
            .and_then(|account_id| self.accounts.get(account_id))
            .map(|account_cell| AccountRef::new(account_cell.borrow()))
    }

    /// Returns an owned snapshot of the account for the `venue` (if found).
    ///
    /// Use when downstream needs an owned [`AccountAny`] that crosses a boundary. The snapshot
    /// will not reflect later cache mutations.
    #[must_use]
    pub fn account_for_venue_owned(&self, venue: &Venue) -> Option<AccountAny> {
        self.index
            .venue_account
            .get(venue)
            .and_then(|account_id| self.accounts.get(account_id))
            .map(|account_cell| account_cell.borrow().clone())
    }

    /// Returns a reference to the account ID for the `venue` (if found).
    #[must_use]
    pub fn account_id(&self, venue: &Venue) -> Option<&AccountId> {
        self.index.venue_account.get(venue)
    }

    /// Returns borrows of all accounts for the `account_id`.
    ///
    /// Each [`AccountRef`] in the returned vector borrows its underlying cell; mutating any of
    /// those accounts while the vector is alive will panic at runtime. Drop the vector before
    /// issuing writes.
    #[must_use]
    pub fn accounts(&self, account_id: &AccountId) -> Vec<AccountRef<'_>> {
        self.accounts
            .values()
            .filter(|account_cell| &account_cell.borrow().id() == account_id)
            .map(|account_cell| AccountRef::new(account_cell.borrow()))
            .collect()
    }

    /// Updates the own order book with an order.
    ///
    /// This method adds, updates, or removes an order from the own order book
    /// based on the order's current state.
    ///
    /// Orders without prices (MARKET, etc.) are skipped as they cannot be
    /// represented in own books.
    pub fn update_own_order_book(&mut self, order: &OrderAny) {
        if !order.has_price() {
            return;
        }

        let instrument_id = order.instrument_id();

        if !self.own_books.contains_key(&instrument_id) {
            if order.is_closed() {
                return;
            }

            self.own_books
                .insert(instrument_id, OwnOrderBook::new(instrument_id));
        }

        let Some(own_book) = self.own_books.get_mut(&instrument_id) else {
            return;
        };

        let own_book_order = order.to_own_book_order();

        if order.is_closed() {
            if let Err(e) = own_book.delete(own_book_order) {
                log::debug!(
                    "Failed to delete order {} from own book: {e}",
                    order.client_order_id(),
                );
            } else {
                log::debug!("Deleted order {} from own book", order.client_order_id());
            }
        } else {
            // Add or update the order in the own book
            if let Err(e) = own_book.update(own_book_order) {
                log::debug!(
                    "Failed to update order {} in own book: {e}; inserting instead",
                    order.client_order_id(),
                );
                own_book.add(own_book_order);
            }
            log::debug!("Updated order {} in own book", order.client_order_id());
        }
    }

    /// Force removal of an order from own order books and clean up all indexes.
    ///
    /// This method is used when order event application fails and we need to ensure
    /// terminal orders are properly cleaned up from own books and all relevant indexes.
    /// Replicates the index cleanup that `update_order` performs for closed orders.
    pub fn force_remove_from_own_order_book(&mut self, client_order_id: &ClientOrderId) {
        let Some(order_cell) = self.orders.get(client_order_id) else {
            return;
        };
        let order = order_cell.borrow();
        let instrument_id = order.instrument_id();
        let own_book_order = if order.has_price() {
            Some(order.to_own_book_order())
        } else {
            None
        };
        drop(order);

        self.index.orders_open.remove(client_order_id);
        self.index.orders_pending_cancel.remove(client_order_id);
        self.index.orders_inflight.remove(client_order_id);
        self.index.orders_emulated.remove(client_order_id);
        self.index.orders_active_local.remove(client_order_id);

        if let Some(own_book) = self.own_books.get_mut(&instrument_id)
            && let Some(own_book_order) = own_book_order
        {
            if let Err(e) = own_book.delete(own_book_order) {
                log::debug!("Could not force delete {client_order_id} from own book: {e}");
            } else {
                log::debug!("Force deleted {client_order_id} from own book");
            }
        }

        self.index.orders_closed.insert(*client_order_id);
    }

    /// Audit all own order books against open and inflight order indexes.
    ///
    /// Ensures closed orders are removed from own order books. This includes both
    /// orders tracked in `orders_open` (`ACCEPTED`, `TRIGGERED`, `PENDING_*`, `PARTIALLY_FILLED`)
    /// and `orders_inflight` (`INITIALIZED`, `SUBMITTED`) to prevent false positives
    /// during venue latency windows.
    pub fn audit_own_order_books(&mut self) {
        log::debug!("Starting own books audit");
        let start = std::time::Instant::now();

        // Build union of open and inflight orders for audit,
        // this prevents false positives for SUBMITTED orders during venue latency.
        let valid_order_ids: AHashSet<ClientOrderId> = self
            .index
            .orders_open
            .union(&self.index.orders_inflight)
            .copied()
            .collect();

        for own_book in self.own_books.values_mut() {
            own_book.audit_open_orders(&valid_order_ids);
        }

        log::debug!("Completed own books audit in {:?}", start.elapsed());
    }
}

fn parse_position_snapshot_blob_ref(blob_ref: &str) -> anyhow::Result<(PositionId, usize)> {
    let Some(rest) = blob_ref.strip_prefix("cache://position-snapshots/") else {
        anyhow::bail!("unsupported cache snapshot blob_ref {blob_ref}");
    };

    let Some((position_id, snapshot_index)) = rest.rsplit_once('/') else {
        anyhow::bail!("malformed position snapshot blob_ref {blob_ref}");
    };

    if position_id.is_empty() {
        anyhow::bail!("position snapshot blob_ref {blob_ref} has empty position id");
    }

    let snapshot_index = snapshot_index.parse::<usize>().map_err(|e| {
        anyhow::anyhow!("position snapshot blob_ref {blob_ref} has invalid frame index: {e}")
    })?;

    Ok((PositionId::new(position_id), snapshot_index))
}

fn validate_position_snapshot_blob(position_id: &PositionId, blob: &[u8]) -> anyhow::Result<()> {
    let snapshot = serde_json::from_slice::<Position>(blob)?;
    let expected_prefix = format!("{}-", position_id.as_str());

    let Some(snapshot_uuid) = snapshot.id.as_str().strip_prefix(&expected_prefix) else {
        anyhow::bail!(
            "position snapshot id {} does not match blob_ref position {position_id}",
            snapshot.id
        );
    };

    if UUID4::from_str(snapshot_uuid).is_err() {
        anyhow::bail!(
            "position snapshot id {} does not match blob_ref position {position_id}",
            snapshot.id
        );
    }

    Ok(())
}
