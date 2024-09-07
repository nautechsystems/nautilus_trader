// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

//! A common in-memory `Cache` for market and execution related data.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

pub mod database;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use database::CacheDatabaseAdapter;
use nautilus_core::correctness::{
    check_key_not_in_map, check_predicate_false, check_slice_not_empty, check_valid_string, FAILED,
};
use nautilus_model::{
    accounts::any::AccountAny,
    data::{
        bar::{Bar, BarType},
        quote::QuoteTick,
        trade::TradeTick,
    },
    enums::{AggregationSource, OmsType, OrderSide, PositionSide, PriceType, TriggerType},
    identifiers::{
        AccountId, ClientId, ClientOrderId, ComponentId, ExecAlgorithmId, InstrumentId,
        OrderListId, PositionId, StrategyId, Venue, VenueOrderId,
    },
    instruments::{any::InstrumentAny, synthetic::SyntheticInstrument},
    orderbook::book::OrderBook,
    orders::{any::OrderAny, list::OrderList},
    position::Position,
    types::{currency::Currency, price::Price, quantity::Quantity},
};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{enums::SerializationEncoding, msgbus::database::DatabaseConfig};

/// Configuration for `Cache` instances.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheConfig {
    /// The configuration for the cache backing database.
    pub database: Option<DatabaseConfig>,
    /// The encoding for database operations, controls the type of serializer used.
    pub encoding: SerializationEncoding,
    /// If timestamps should be persisted as ISO 8601 strings.
    pub timestamps_as_iso8601: bool,
    /// The buffer interval (milliseconds) between pipelined/batched transactions.
    pub buffer_interval_ms: Option<usize>,
    /// If a 'trader-' prefix is used for keys.
    pub use_trader_prefix: bool,
    /// If the trader's instance ID is used for keys.
    pub use_instance_id: bool,
    /// If the database should be flushed on start.
    pub flush_on_start: bool,
    /// If instrument data should be dropped from the cache's memory on reset.
    pub drop_instruments_on_reset: bool,
    /// The maximum length for internal tick deques.
    pub tick_capacity: usize,
    /// The maximum length for internal bar deques.
    pub bar_capacity: usize,
    /// If market data should be persisted to disk.
    pub save_market_data: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            database: None,
            encoding: SerializationEncoding::MsgPack,
            timestamps_as_iso8601: false,
            buffer_interval_ms: None,
            use_trader_prefix: true,
            use_instance_id: false,
            flush_on_start: false,
            drop_instruments_on_reset: true,
            tick_capacity: 10_000,
            bar_capacity: 10_000,
            save_market_data: false,
        }
    }
}

impl CacheConfig {
    /// Creates a new [`CacheConfig`] instance.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        database: Option<DatabaseConfig>,
        encoding: SerializationEncoding,
        timestamps_as_iso8601: bool,
        buffer_interval_ms: Option<usize>,
        use_trader_prefix: bool,
        use_instance_id: bool,
        flush_on_start: bool,
        drop_instruments_on_reset: bool,
        tick_capacity: usize,
        bar_capacity: usize,
        save_market_data: bool,
    ) -> Self {
        Self {
            database,
            encoding,
            timestamps_as_iso8601,
            buffer_interval_ms,
            use_trader_prefix,
            use_instance_id,
            flush_on_start,
            drop_instruments_on_reset,
            tick_capacity,
            bar_capacity,
            save_market_data,
        }
    }
}

/// A key-value lookup index for a `Cache`.
pub struct CacheIndex {
    venue_account: HashMap<Venue, AccountId>,
    venue_orders: HashMap<Venue, HashSet<ClientOrderId>>,
    venue_positions: HashMap<Venue, HashSet<PositionId>>,
    venue_order_ids: HashMap<VenueOrderId, ClientOrderId>,
    client_order_ids: HashMap<ClientOrderId, VenueOrderId>,
    order_position: HashMap<ClientOrderId, PositionId>,
    order_strategy: HashMap<ClientOrderId, StrategyId>,
    order_client: HashMap<ClientOrderId, ClientId>,
    position_strategy: HashMap<PositionId, StrategyId>,
    position_orders: HashMap<PositionId, HashSet<ClientOrderId>>,
    instrument_orders: HashMap<InstrumentId, HashSet<ClientOrderId>>,
    instrument_positions: HashMap<InstrumentId, HashSet<PositionId>>,
    strategy_orders: HashMap<StrategyId, HashSet<ClientOrderId>>,
    strategy_positions: HashMap<StrategyId, HashSet<PositionId>>,
    exec_algorithm_orders: HashMap<ExecAlgorithmId, HashSet<ClientOrderId>>,
    exec_spawn_orders: HashMap<ClientOrderId, HashSet<ClientOrderId>>,
    orders: HashSet<ClientOrderId>,
    orders_open: HashSet<ClientOrderId>,
    orders_closed: HashSet<ClientOrderId>,
    orders_emulated: HashSet<ClientOrderId>,
    orders_inflight: HashSet<ClientOrderId>,
    orders_pending_cancel: HashSet<ClientOrderId>,
    positions: HashSet<PositionId>,
    positions_open: HashSet<PositionId>,
    positions_closed: HashSet<PositionId>,
    actors: HashSet<ComponentId>,
    strategies: HashSet<StrategyId>,
    exec_algorithms: HashSet<ExecAlgorithmId>,
}

impl CacheIndex {
    /// Clears the index which will clear/reset all internal state.
    pub fn clear(&mut self) {
        self.venue_account.clear();
        self.venue_orders.clear();
        self.venue_positions.clear();
        self.venue_order_ids.clear();
        self.client_order_ids.clear();
        self.order_position.clear();
        self.order_strategy.clear();
        self.order_client.clear();
        self.position_strategy.clear();
        self.position_orders.clear();
        self.instrument_orders.clear();
        self.instrument_positions.clear();
        self.strategy_orders.clear();
        self.strategy_positions.clear();
        self.exec_algorithm_orders.clear();
        self.exec_spawn_orders.clear();
        self.orders.clear();
        self.orders_open.clear();
        self.orders_closed.clear();
        self.orders_emulated.clear();
        self.orders_inflight.clear();
        self.orders_pending_cancel.clear();
        self.positions.clear();
        self.positions_open.clear();
        self.positions_closed.clear();
        self.actors.clear();
        self.strategies.clear();
        self.exec_algorithms.clear();
    }
}

/// A common in-memory `Cache` for market and execution related data.
pub struct Cache {
    config: CacheConfig,
    index: CacheIndex,
    database: Option<Box<dyn CacheDatabaseAdapter>>,
    general: HashMap<String, Bytes>,
    quotes: HashMap<InstrumentId, VecDeque<QuoteTick>>,
    trades: HashMap<InstrumentId, VecDeque<TradeTick>>,
    books: HashMap<InstrumentId, OrderBook>,
    bars: HashMap<BarType, VecDeque<Bar>>,
    currencies: HashMap<Ustr, Currency>,
    instruments: HashMap<InstrumentId, InstrumentAny>,
    synthetics: HashMap<InstrumentId, SyntheticInstrument>,
    accounts: HashMap<AccountId, AccountAny>,
    orders: HashMap<ClientOrderId, OrderAny>,
    order_lists: HashMap<OrderListId, OrderList>,
    positions: HashMap<PositionId, Position>,
    position_snapshots: HashMap<PositionId, Bytes>,
}

// SAFETY: Cache is not meant to be passed between threads
unsafe impl Send for Cache {}
unsafe impl Sync for Cache {}

impl Default for Cache {
    /// Creates a new default [`Cache`] instance.
    fn default() -> Self {
        Self::new(Some(CacheConfig::default()), None)
    }
}

impl Cache {
    /// Creates a new [`Cache`] instance.
    #[must_use]
    pub fn new(
        config: Option<CacheConfig>,
        database: Option<Box<dyn CacheDatabaseAdapter>>,
    ) -> Self {
        let index = CacheIndex {
            venue_account: HashMap::new(),
            venue_orders: HashMap::new(),
            venue_positions: HashMap::new(),
            venue_order_ids: HashMap::new(),
            client_order_ids: HashMap::new(),
            order_position: HashMap::new(),
            order_strategy: HashMap::new(),
            order_client: HashMap::new(),
            position_strategy: HashMap::new(),
            position_orders: HashMap::new(),
            instrument_orders: HashMap::new(),
            instrument_positions: HashMap::new(),
            strategy_orders: HashMap::new(),
            strategy_positions: HashMap::new(),
            exec_algorithm_orders: HashMap::new(),
            exec_spawn_orders: HashMap::new(),
            orders: HashSet::new(),
            orders_open: HashSet::new(),
            orders_closed: HashSet::new(),
            orders_emulated: HashSet::new(),
            orders_inflight: HashSet::new(),
            orders_pending_cancel: HashSet::new(),
            positions: HashSet::new(),
            positions_open: HashSet::new(),
            positions_closed: HashSet::new(),
            actors: HashSet::new(),
            strategies: HashSet::new(),
            exec_algorithms: HashSet::new(),
        };

        Self {
            config: config.unwrap_or_default(),
            index,
            database,
            general: HashMap::new(),
            quotes: HashMap::new(),
            trades: HashMap::new(),
            books: HashMap::new(),
            bars: HashMap::new(),
            currencies: HashMap::new(),
            instruments: HashMap::new(),
            synthetics: HashMap::new(),
            accounts: HashMap::new(),
            orders: HashMap::new(),
            order_lists: HashMap::new(),
            positions: HashMap::new(),
            position_snapshots: HashMap::new(),
        }
    }

    /// Returns the cache instances memory address.
    #[must_use]
    pub fn memory_address(&self) -> String {
        format!("{:?}", std::ptr::from_ref(self))
    }

    // -- COMMANDS --------------------------------------------------------------------------------

    /// Clears the current general cache and loads the general objects from the cache database.
    pub fn cache_general(&mut self) -> anyhow::Result<()> {
        self.general = match &mut self.database {
            Some(db) => db.load()?,
            None => HashMap::new(),
        };

        log::info!(
            "Cached {} general object(s) from database",
            self.general.len()
        );
        Ok(())
    }

    /// Clears the current currencies cache and loads currencies from the cache database.
    pub fn cache_currencies(&mut self) -> anyhow::Result<()> {
        self.currencies = match &mut self.database {
            Some(db) => db.load_currencies()?,
            None => HashMap::new(),
        };

        log::info!("Cached {} currencies from database", self.general.len());
        Ok(())
    }

    /// Clears the current instruments cache and loads instruments from the cache database.
    pub fn cache_instruments(&mut self) -> anyhow::Result<()> {
        self.instruments = match &mut self.database {
            Some(db) => db.load_instruments()?,
            None => HashMap::new(),
        };

        log::info!("Cached {} instruments from database", self.general.len());
        Ok(())
    }

    /// Clears the current synthetic instruments cache and loads synthetic instruments from the cache
    /// database.
    pub fn cache_synthetics(&mut self) -> anyhow::Result<()> {
        self.synthetics = match &mut self.database {
            Some(db) => db.load_synthetics()?,
            None => HashMap::new(),
        };

        log::info!(
            "Cached {} synthetic instruments from database",
            self.general.len()
        );
        Ok(())
    }

    /// Clears the current accounts cache and loads accounts from the cache database.
    pub fn cache_accounts(&mut self) -> anyhow::Result<()> {
        self.accounts = match &mut self.database {
            Some(db) => db.load_accounts()?,
            None => HashMap::new(),
        };

        log::info!(
            "Cached {} synthetic instruments from database",
            self.general.len()
        );
        Ok(())
    }

    /// Clears the current orders cache and loads orders from the cache database.
    pub fn cache_orders(&mut self) -> anyhow::Result<()> {
        self.orders = match &mut self.database {
            Some(db) => db.load_orders()?,
            None => HashMap::new(),
        };

        log::info!("Cached {} orders from database", self.general.len());
        Ok(())
    }

    /// Clears the current positions cache and loads positions from the cache database.
    pub fn cache_positions(&mut self) -> anyhow::Result<()> {
        self.positions = match &mut self.database {
            Some(db) => db.load_positions()?,
            None => HashMap::new(),
        };

        log::info!("Cached {} positions from database", self.general.len());
        Ok(())
    }

    /// Clears the current cache index and re-build.
    pub fn build_index(&mut self) {
        self.index.clear();
        log::debug!("Building index");

        // Index accounts
        for account_id in self.accounts.keys() {
            self.index
                .venue_account
                .insert(account_id.get_issuer(), *account_id);
        }

        // Index orders
        for (client_order_id, order) in &self.orders {
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

            // 7: Build index.exec_algorithm_orders -> {ExecAlgorithmId, {ClientOrderId}}
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

            // 10: Build index.orders_open -> {ClientOrderId}
            if order.is_open() {
                self.index.orders_open.insert(*client_order_id);
            }

            // 11: Build index.orders_closed -> {ClientOrderId}
            if order.is_closed() {
                self.index.orders_closed.insert(*client_order_id);
            }

            // 12: Build index.orders_emulated -> {ClientOrderId}
            if let Some(emulation_trigger) = order.emulation_trigger() {
                if emulation_trigger != TriggerType::NoTrigger && !order.is_closed() {
                    self.index.orders_emulated.insert(*client_order_id);
                }
            }

            // 13: Build index.orders_inflight -> {ClientOrderId}
            if order.is_inflight() {
                self.index.orders_inflight.insert(*client_order_id);
            }

            // 14: Build index.strategies -> {StrategyId}
            self.index.strategies.insert(strategy_id);

            // 15: Build index.strategies -> {ExecAlgorithmId}
            if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
                self.index.exec_algorithms.insert(exec_algorithm_id);
            }
        }

        // Index positions
        for (position_id, position) in &self.positions {
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
                .extend(position.client_order_ids().into_iter());

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

            // 6: Build index.positions -> {PositionId}
            self.index.positions.insert(*position_id);

            // 7: Build index.positions_open -> {PositionId}
            if position.is_open() {
                self.index.positions_open.insert(*position_id);
            }

            // 8: Build index.positions_closed -> {PositionId}
            if position.is_closed() {
                self.index.positions_closed.insert(*position_id);
            }

            // 9: Build index.strategies -> {StrategyId}
            self.index.strategies.insert(strategy_id);
        }
    }

    /// Checks integrity of data within the cache.
    ///
    /// All data should be loaded from the database prior to this call.
    /// If an error is found then a log error message will also be produced.
    #[must_use]
    fn check_integrity(&mut self) -> bool {
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
                    "{} in accounts: {} not found in `self.index.venue_account`",
                    failure,
                    account_id
                );
                error_count += 1;
            }
        }

        for (client_order_id, order) in &self.orders {
            if !self.index.order_strategy.contains_key(client_order_id) {
                log::error!(
                    "{} in orders: {} not found in `self.index.order_strategy`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
            if !self.index.orders.contains(client_order_id) {
                log::error!(
                    "{} in orders: {} not found in `self.index.orders`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
            if order.is_inflight() && !self.index.orders_inflight.contains(client_order_id) {
                log::error!(
                    "{} in orders: {} not found in `self.index.orders_inflight`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
            if order.is_open() && !self.index.orders_open.contains(client_order_id) {
                log::error!(
                    "{} in orders: {} not found in `self.index.orders_open`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
            if order.is_closed() && !self.index.orders_closed.contains(client_order_id) {
                log::error!(
                    "{} in orders: {} not found in `self.index.orders_closed`",
                    failure,
                    client_order_id
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
                        "{} in orders: {} not found in `self.index.exec_algorithm_orders`",
                        failure,
                        exec_algorithm_id
                    );
                    error_count += 1;
                }
                if order.exec_spawn_id().is_none()
                    && !self.index.exec_spawn_orders.contains_key(client_order_id)
                {
                    log::error!(
                        "{} in orders: {} not found in `self.index.exec_spawn_orders`",
                        failure,
                        exec_algorithm_id
                    );
                    error_count += 1;
                }
            }
        }

        for (position_id, position) in &self.positions {
            if !self.index.position_strategy.contains_key(position_id) {
                log::error!(
                    "{} in positions: {} not found in `self.index.position_strategy`",
                    failure,
                    position_id
                );
                error_count += 1;
            }
            if !self.index.position_orders.contains_key(position_id) {
                log::error!(
                    "{} in positions: {} not found in `self.index.position_orders`",
                    failure,
                    position_id
                );
                error_count += 1;
            }
            if !self.index.positions.contains(position_id) {
                log::error!(
                    "{} in positions: {} not found in `self.index.positions`",
                    failure,
                    position_id
                );
                error_count += 1;
            }
            if position.is_open() && !self.index.positions_open.contains(position_id) {
                log::error!(
                    "{} in positions: {} not found in `self.index.positions_open`",
                    failure,
                    position_id
                );
                error_count += 1;
            }
            if position.is_closed() && !self.index.positions_closed.contains(position_id) {
                log::error!(
                    "{} in positions: {} not found in `self.index.positions_closed`",
                    failure,
                    position_id
                );
                error_count += 1;
            }
        }

        // Check indexes
        for account_id in self.index.venue_account.values() {
            if !self.accounts.contains_key(account_id) {
                log::error!(
                    "{} in `index.venue_account`: {} not found in `self.accounts`",
                    failure,
                    account_id
                );
                error_count += 1;
            }
        }

        for client_order_id in self.index.venue_order_ids.values() {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{} in `index.venue_order_ids`: {} not found in `self.orders`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
        }

        for client_order_id in self.index.client_order_ids.keys() {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{} in `index.client_order_ids`: {} not found in `self.orders`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
        }

        for client_order_id in self.index.order_position.keys() {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{} in `index.order_position`: {} not found in `self.orders`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
        }

        // Check indexes
        for client_order_id in self.index.order_strategy.keys() {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{} in `index.order_strategy`: {} not found in `self.orders`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
        }

        for position_id in self.index.position_strategy.keys() {
            if !self.positions.contains_key(position_id) {
                log::error!(
                    "{} in `index.position_strategy`: {} not found in `self.positions`",
                    failure,
                    position_id
                );
                error_count += 1;
            }
        }

        for position_id in self.index.position_orders.keys() {
            if !self.positions.contains_key(position_id) {
                log::error!(
                    "{} in `index.position_orders`: {} not found in `self.positions`",
                    failure,
                    position_id
                );
                error_count += 1;
            }
        }

        for (instrument_id, client_order_ids) in &self.index.instrument_orders {
            for client_order_id in client_order_ids {
                if !self.orders.contains_key(client_order_id) {
                    log::error!(
                        "{} in `index.instrument_orders`: {} not found in `self.orders`",
                        failure,
                        instrument_id
                    );
                    error_count += 1;
                }
            }
        }

        for instrument_id in self.index.instrument_positions.keys() {
            if !self.index.instrument_orders.contains_key(instrument_id) {
                log::error!(
                    "{} in `index.instrument_positions`: {} not found in `index.instrument_orders`",
                    failure,
                    instrument_id
                );
                error_count += 1;
            }
        }

        for client_order_ids in self.index.strategy_orders.values() {
            for client_order_id in client_order_ids {
                if !self.orders.contains_key(client_order_id) {
                    log::error!(
                        "{} in `index.strategy_orders`: {} not found in `self.orders`",
                        failure,
                        client_order_id
                    );
                    error_count += 1;
                }
            }
        }

        for position_ids in self.index.strategy_positions.values() {
            for position_id in position_ids {
                if !self.positions.contains_key(position_id) {
                    log::error!(
                        "{} in `index.strategy_positions`: {} not found in `self.positions`",
                        failure,
                        position_id
                    );
                    error_count += 1;
                }
            }
        }

        for client_order_id in &self.index.orders {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{} in `index.orders`: {} not found in `self.orders`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
        }

        for client_order_id in &self.index.orders_emulated {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{} in `index.orders_emulated`: {} not found in `self.orders`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
        }

        for client_order_id in &self.index.orders_inflight {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{} in `index.orders_inflight`: {} not found in `self.orders`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
        }

        for client_order_id in &self.index.orders_open {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{} in `index.orders_open`: {} not found in `self.orders`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
        }

        for client_order_id in &self.index.orders_closed {
            if !self.orders.contains_key(client_order_id) {
                log::error!(
                    "{} in `index.orders_closed`: {} not found in `self.orders`",
                    failure,
                    client_order_id
                );
                error_count += 1;
            }
        }

        for position_id in &self.index.positions {
            if !self.positions.contains_key(position_id) {
                log::error!(
                    "{} in `index.positions`: {} not found in `self.positions`",
                    failure,
                    position_id
                );
                error_count += 1;
            }
        }

        for position_id in &self.index.positions_open {
            if !self.positions.contains_key(position_id) {
                log::error!(
                    "{} in `index.positions_open`: {} not found in `self.positions`",
                    failure,
                    position_id
                );
                error_count += 1;
            }
        }

        for position_id in &self.index.positions_closed {
            if !self.positions.contains_key(position_id) {
                log::error!(
                    "{} in `index.positions_closed`: {} not found in `self.positions`",
                    failure,
                    position_id
                );
                error_count += 1;
            }
        }

        for strategy_id in &self.index.strategies {
            if !self.index.strategy_orders.contains_key(strategy_id) {
                log::error!(
                    "{} in `index.strategies`: {} not found in `index.strategy_orders`",
                    failure,
                    strategy_id
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
                    "{} in `index.exec_algorithms`: {} not found in `index.exec_algorithm_orders`",
                    failure,
                    exec_algorithm_id
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
            log::info!("Integrity check passed in {}μs", total_us);
            true
        } else {
            log::error!(
                "Integrity check failed with {} error{} in {}μs",
                error_count,
                if error_count == 1 { "" } else { "s" },
                total_us
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
        for order in self.orders_open(None, None, None, None) {
            residuals = true;
            log::warn!("Residual {:?}", order);
        }

        // Check for any open positions
        for position in self.positions_open(None, None, None, None) {
            residuals = true;
            log::warn!("Residual {}", position);
        }

        residuals
    }

    /// Clears the caches index.
    pub fn clear_index(&mut self) {
        self.index.clear();
        log::debug!("Cleared index");
    }

    /// Resets the cache.
    ///
    /// All stateful fields are reset to their initial value.
    pub fn reset(&mut self) {
        log::debug!("Resetting cache");

        self.general.clear();
        self.quotes.clear();
        self.trades.clear();
        self.books.clear();
        self.bars.clear();
        self.currencies.clear();
        self.instruments.clear();
        self.synthetics.clear();
        self.accounts.clear();
        self.orders.clear();
        self.order_lists.clear();
        self.positions.clear();
        self.position_snapshots.clear();

        self.clear_index();

        log::info!("Reset cache");
    }

    /// Dispose of the cache which will close any underlying database adapter.
    pub fn dispose(&mut self) {
        if let Some(database) = &mut self.database {
            database.close();
        }
    }

    /// Flushes the caches database which permanently removes all persisted data.
    pub fn flush_db(&mut self) {
        if let Some(database) = &mut self.database {
            database.flush();
        }
    }

    /// Adds a general object `value` (as bytes) to the cache at the given `key`.
    ///
    /// The cache is agnostic to what the bytes actually represent (and how it may be serialized),
    /// which provides maximum flexibility.
    pub fn add(&mut self, key: &str, value: Bytes) -> anyhow::Result<()> {
        check_valid_string(key, stringify!(key)).expect(FAILED);
        check_predicate_false(value.is_empty(), stringify!(value)).expect(FAILED);

        log::debug!("Adding general {key}");
        self.general.insert(key.to_string(), value.clone());

        if let Some(database) = &mut self.database {
            database.add(key.to_string(), value)?;
        }
        Ok(())
    }

    /// Adds the given order `book` to the cache.
    pub fn add_order_book(&mut self, book: OrderBook) -> anyhow::Result<()> {
        log::debug!("Adding `OrderBook` {}", book.instrument_id);

        if self.config.save_market_data {
            if let Some(database) = &mut self.database {
                database.add_order_book(&book)?;
            }
        }

        self.books.insert(book.instrument_id, book);
        Ok(())
    }

    /// Adds the given `quote` tick to the cache.
    pub fn add_quote(&mut self, quote: QuoteTick) -> anyhow::Result<()> {
        log::debug!("Adding `QuoteTick` {}", quote.instrument_id);

        if self.config.save_market_data {
            if let Some(database) = &mut self.database {
                database.add_quote(&quote)?;
            }
        }

        let quotes_deque = self
            .quotes
            .entry(quote.instrument_id)
            .or_insert_with(|| VecDeque::with_capacity(self.config.tick_capacity));
        quotes_deque.push_front(quote);
        Ok(())
    }

    /// Adds the given `quotes` to the cache.
    pub fn add_quotes(&mut self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        check_slice_not_empty(quotes, stringify!(quotes)).unwrap();

        let instrument_id = quotes[0].instrument_id;
        log::debug!("Adding `QuoteTick`[{}] {}", quotes.len(), instrument_id);

        if self.config.save_market_data {
            if let Some(database) = &mut self.database {
                for quote in quotes {
                    database.add_quote(quote).unwrap();
                }
            }
        }

        let quotes_deque = self
            .quotes
            .entry(instrument_id)
            .or_insert_with(|| VecDeque::with_capacity(self.config.tick_capacity));

        for quote in quotes {
            quotes_deque.push_front(*quote);
        }
        Ok(())
    }

    /// Adds the given `trade` tick to the cache.
    pub fn add_trade(&mut self, trade: TradeTick) -> anyhow::Result<()> {
        log::debug!("Adding `TradeTick` {}", trade.instrument_id);

        if self.config.save_market_data {
            if let Some(database) = &mut self.database {
                database.add_trade(&trade)?;
            }
        }

        let trades_deque = self
            .trades
            .entry(trade.instrument_id)
            .or_insert_with(|| VecDeque::with_capacity(self.config.tick_capacity));
        trades_deque.push_front(trade);
        Ok(())
    }

    /// Adds the give `trades` to the cache.
    pub fn add_trades(&mut self, trades: &[TradeTick]) -> anyhow::Result<()> {
        check_slice_not_empty(trades, stringify!(trades)).unwrap();

        let instrument_id = trades[0].instrument_id;
        log::debug!("Adding `TradeTick`[{}] {}", trades.len(), instrument_id);

        if self.config.save_market_data {
            if let Some(database) = &mut self.database {
                for trade in trades {
                    database.add_trade(trade).unwrap();
                }
            }
        }

        let trades_deque = self
            .trades
            .entry(instrument_id)
            .or_insert_with(|| VecDeque::with_capacity(self.config.tick_capacity));

        for trade in trades {
            trades_deque.push_front(*trade);
        }
        Ok(())
    }

    /// Adds the given `bar` to the cache.
    pub fn add_bar(&mut self, bar: Bar) -> anyhow::Result<()> {
        log::debug!("Adding `Bar` {}", bar.bar_type);

        if self.config.save_market_data {
            if let Some(database) = &mut self.database {
                database.add_bar(&bar)?;
            }
        }

        let bars = self
            .bars
            .entry(bar.bar_type)
            .or_insert_with(|| VecDeque::with_capacity(self.config.bar_capacity));
        bars.push_front(bar);
        Ok(())
    }

    /// Adds the given `bars` to the cache.
    pub fn add_bars(&mut self, bars: &[Bar]) -> anyhow::Result<()> {
        check_slice_not_empty(bars, stringify!(bars)).unwrap();

        let bar_type = bars[0].bar_type;
        log::debug!("Adding `Bar`[{}] {}", bars.len(), bar_type);

        if self.config.save_market_data {
            if let Some(database) = &mut self.database {
                for bar in bars {
                    database.add_bar(bar).unwrap();
                }
            }
        }

        let bars_deque = self
            .bars
            .entry(bar_type)
            .or_insert_with(|| VecDeque::with_capacity(self.config.tick_capacity));

        for bar in bars {
            bars_deque.push_front(*bar);
        }
        Ok(())
    }

    /// Adds the given `currency` to the cache.
    pub fn add_currency(&mut self, currency: Currency) -> anyhow::Result<()> {
        log::debug!("Adding `Currency` {}", currency.code);

        if let Some(database) = &mut self.database {
            database.add_currency(&currency)?;
        }

        self.currencies.insert(currency.code, currency);
        Ok(())
    }

    /// Adds the given `instrument` to the cache.
    pub fn add_instrument(&mut self, instrument: InstrumentAny) -> anyhow::Result<()> {
        log::debug!("Adding `Instrument` {}", instrument.id());

        if let Some(database) = &mut self.database {
            database.add_instrument(&instrument)?;
        }

        self.instruments.insert(instrument.id(), instrument);
        Ok(())
    }

    /// Adds the given `synthetic` instrument to the cache.
    pub fn add_synthetic(&mut self, synthetic: SyntheticInstrument) -> anyhow::Result<()> {
        log::debug!("Adding `SyntheticInstrument` {}", synthetic.id);

        if let Some(database) = &mut self.database {
            database.add_synthetic(&synthetic)?;
        }

        self.synthetics.insert(synthetic.id, synthetic);
        Ok(())
    }

    /// Adds the given `account` to the cache.
    pub fn add_account(&mut self, account: AccountAny) -> anyhow::Result<()> {
        log::debug!("Adding `Account` {}", account.id());

        if let Some(database) = &mut self.database {
            database.add_account(&account)?;
        }

        let account_id = account.id();
        self.accounts.insert(account_id, account);
        self.index
            .venue_account
            .insert(account_id.get_issuer(), account_id);
        Ok(())
    }

    /// Indexes the given `client_order_id` with the given `venue_order_id`.
    ///
    /// The `overwrite` parameter determines whether to overwrite any existing cached identifier.
    pub fn add_venue_order_id(
        &mut self,
        client_order_id: &ClientOrderId,
        venue_order_id: &VenueOrderId,
        overwrite: bool,
    ) -> anyhow::Result<()> {
        if let Some(existing_venue_order_id) = self.index.client_order_ids.get(client_order_id) {
            if !overwrite && existing_venue_order_id != venue_order_id {
                anyhow::bail!(
                    "Existing {existing_venue_order_id} for {client_order_id}
                    did not match the given {venue_order_id}.
                    If you are writing a test then try a different `venue_order_id`,
                    otherwise this is probably a bug."
                );
            }
        };

        self.index
            .client_order_ids
            .insert(*client_order_id, *venue_order_id);
        self.index
            .venue_order_ids
            .insert(*venue_order_id, *client_order_id);

        Ok(())
    }

    /// Adds the given `order` to the cache indexed with any given identifiers.
    ///
    /// # Parameters
    ///
    /// `override_existing`: If the added order should 'override' any existing order and replace
    /// it in the cache. This is currently used for emulated orders which are
    /// being released and transformed into another type.
    ///
    /// # Errors
    ///
    /// This function returns an error:
    /// If not `replace_existing` and the `order.client_order_id` is already contained in the cache.
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
            )
            .expect(FAILED);
            check_key_not_in_map(
                &client_order_id,
                &self.orders,
                stringify!(client_order_id),
                stringify!(orders),
            )
            .expect(FAILED);
            check_key_not_in_map(
                &client_order_id,
                &self.orders,
                stringify!(client_order_id),
                stringify!(orders),
            )
            .expect(FAILED);
            check_key_not_in_map(
                &client_order_id,
                &self.orders,
                stringify!(client_order_id),
                stringify!(orders),
            )
            .expect(FAILED);
        };

        log::debug!("Adding {:?}", order);

        self.index.orders.insert(client_order_id);
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

        // Update exec_algorithm -> orders index
        if let Some(exec_algorithm_id) = exec_algorithm_id {
            self.index.exec_algorithms.insert(exec_algorithm_id);

            self.index
                .exec_algorithm_orders
                .entry(exec_algorithm_id)
                .or_default()
                .insert(client_order_id);

            self.index
                .exec_spawn_orders
                .entry(exec_spawn_id.expect("`exec_spawn_id` is guaranteed to exist"))
                .or_default()
                .insert(client_order_id);
        }

        // Update emulation index
        match order.emulation_trigger() {
            Some(_) => {
                self.index.orders_emulated.remove(&client_order_id);
            }
            None => {
                self.index.orders_emulated.insert(client_order_id);
            }
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
            log::debug!("Indexed {:?}", client_id);
        }

        if let Some(database) = &mut self.database {
            database.add_order(&order, client_id)?;
            // TODO: Implement
            // if self.config.snapshot_orders {
            //     database.snapshot_order_state(order)?;
            // }
        }

        self.orders.insert(client_order_id, order);

        Ok(())
    }

    /// Indexes the given `position_id` with the other given IDs.
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

        Ok(())
    }

    /// Adds the given `position` to the cache.
    pub fn add_position(&mut self, position: Position, oms_type: OmsType) -> anyhow::Result<()> {
        self.positions.insert(position.id, position.clone());
        self.index.positions.insert(position.id);
        self.index.positions_open.insert(position.id);

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

        // Index: InstrumentId -> HashSet
        let instrument_id = position.instrument_id;
        let instrument_positions = self
            .index
            .instrument_positions
            .entry(instrument_id)
            .or_default();
        instrument_positions.insert(position.id);

        if let Some(database) = &mut self.database {
            database.add_position(&position)?;
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

    /// Updates the given `account` in the cache.
    pub fn update_account(&mut self, account: AccountAny) -> anyhow::Result<()> {
        if let Some(database) = &mut self.database {
            database.update_account(&account)?;
        }
        Ok(())
    }

    /// Updates the given `order` in the cache.
    pub fn update_order(&mut self, order: &OrderAny) -> anyhow::Result<()> {
        let client_order_id = order.client_order_id();

        // Update venue order ID
        if let Some(venue_order_id) = order.venue_order_id() {
            // If the order is being modified then we allow a changing `VenueOrderId` to accommodate
            // venues which use a cancel+replace update strategy.
            if !self.index.venue_order_ids.contains_key(&venue_order_id) {
                // TODO: If the last event was `OrderUpdated` then overwrite should be true
                self.add_venue_order_id(&order.client_order_id(), &venue_order_id, false)?;
            };
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

        // Update emulation
        if let Some(emulation_trigger) = order.emulation_trigger() {
            match emulation_trigger {
                TriggerType::NoTrigger => self.index.orders_emulated.remove(&client_order_id),
                _ => self.index.orders_emulated.insert(client_order_id),
            };
        }

        if let Some(database) = &mut self.database {
            database.update_order(order)?;
            // TODO: Implement order snapshots
            // if self.snapshot_orders {
            //     database.snapshot_order_state(order)?;
            // }
        }

        Ok(())
    }

    /// Updates the given `order` as pending cancel locally.
    pub fn update_order_pending_cancel_local(&mut self, order: &OrderAny) {
        self.index
            .orders_pending_cancel
            .insert(order.client_order_id());
    }

    /// Updates the given `position` in the cache.
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
        Ok(())
    }

    // -- IDENTIFIER QUERIES ----------------------------------------------------------------------

    fn build_order_query_filter_set(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
    ) -> Option<HashSet<ClientOrderId>> {
        let mut query: Option<HashSet<ClientOrderId>> = None;

        if let Some(venue) = venue {
            query = Some(
                self.index
                    .venue_orders
                    .get(venue)
                    .map_or(HashSet::new(), |o| o.iter().copied().collect()),
            );
        };

        if let Some(instrument_id) = instrument_id {
            let instrument_orders = self
                .index
                .instrument_orders
                .get(instrument_id)
                .map_or(HashSet::new(), |o| o.iter().copied().collect());

            if let Some(existing_query) = &mut query {
                *existing_query = existing_query
                    .intersection(&instrument_orders)
                    .copied()
                    .collect();
            } else {
                query = Some(instrument_orders);
            };
        };

        if let Some(strategy_id) = strategy_id {
            let strategy_orders = self
                .index
                .strategy_orders
                .get(strategy_id)
                .map_or(HashSet::new(), |o| o.iter().copied().collect());

            if let Some(existing_query) = &mut query {
                *existing_query = existing_query
                    .intersection(&strategy_orders)
                    .copied()
                    .collect();
            } else {
                query = Some(strategy_orders);
            };
        };

        query
    }

    fn build_position_query_filter_set(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
    ) -> Option<HashSet<PositionId>> {
        let mut query: Option<HashSet<PositionId>> = None;

        if let Some(venue) = venue {
            query = Some(
                self.index
                    .venue_positions
                    .get(venue)
                    .map_or(HashSet::new(), |p| p.iter().copied().collect()),
            );
        };

        if let Some(instrument_id) = instrument_id {
            let instrument_positions = self
                .index
                .instrument_positions
                .get(instrument_id)
                .map_or(HashSet::new(), |p| p.iter().copied().collect());

            if let Some(existing_query) = query {
                query = Some(
                    existing_query
                        .intersection(&instrument_positions)
                        .copied()
                        .collect(),
                );
            } else {
                query = Some(instrument_positions);
            };
        };

        if let Some(strategy_id) = strategy_id {
            let strategy_positions = self
                .index
                .strategy_positions
                .get(strategy_id)
                .map_or(HashSet::new(), |p| p.iter().copied().collect());

            if let Some(existing_query) = query {
                query = Some(
                    existing_query
                        .intersection(&strategy_positions)
                        .copied()
                        .collect(),
                );
            } else {
                query = Some(strategy_positions);
            };
        };

        query
    }

    fn get_orders_for_ids(
        &self,
        client_order_ids: &HashSet<ClientOrderId>,
        side: Option<OrderSide>,
    ) -> Vec<&OrderAny> {
        let side = side.unwrap_or(OrderSide::NoOrderSide);
        let mut orders = Vec::new();

        for client_order_id in client_order_ids {
            let order = self
                .orders
                .get(client_order_id)
                .unwrap_or_else(|| panic!("Order {client_order_id} not found"));
            if side == OrderSide::NoOrderSide || side == order.order_side() {
                orders.push(order);
            };
        }

        orders
    }

    fn get_positions_for_ids(
        &self,
        position_ids: &HashSet<PositionId>,
        side: Option<PositionSide>,
    ) -> Vec<&Position> {
        let side = side.unwrap_or(PositionSide::NoPositionSide);
        let mut positions = Vec::new();

        for position_id in position_ids {
            let position = self
                .positions
                .get(position_id)
                .unwrap_or_else(|| panic!("Position {position_id} not found"));
            if side == PositionSide::NoPositionSide || side == position.side {
                positions.push(position);
            };
        }

        positions
    }

    /// Returns the `ClientOrderId`s of all orders.
    #[must_use]
    pub fn client_order_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
    ) -> HashSet<ClientOrderId> {
        let query = self.build_order_query_filter_set(venue, instrument_id, strategy_id);
        match query {
            Some(query) => self.index.orders.intersection(&query).copied().collect(),
            None => self.index.orders.clone(),
        }
    }

    /// Returns the `ClientOrderId`s of all open orders.
    #[must_use]
    pub fn client_order_ids_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
    ) -> HashSet<ClientOrderId> {
        let query = self.build_order_query_filter_set(venue, instrument_id, strategy_id);
        match query {
            Some(query) => self
                .index
                .orders_open
                .intersection(&query)
                .copied()
                .collect(),
            None => self.index.orders_open.clone(),
        }
    }

    /// Returns the `ClientOrderId`s of all closed orders.
    #[must_use]
    pub fn client_order_ids_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
    ) -> HashSet<ClientOrderId> {
        let query = self.build_order_query_filter_set(venue, instrument_id, strategy_id);
        match query {
            Some(query) => self
                .index
                .orders_closed
                .intersection(&query)
                .copied()
                .collect(),
            None => self.index.orders_closed.clone(),
        }
    }

    /// Returns the `ClientOrderId`s of all emulated orders.
    #[must_use]
    pub fn client_order_ids_emulated(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
    ) -> HashSet<ClientOrderId> {
        let query = self.build_order_query_filter_set(venue, instrument_id, strategy_id);
        match query {
            Some(query) => self
                .index
                .orders_emulated
                .intersection(&query)
                .copied()
                .collect(),
            None => self.index.orders_emulated.clone(),
        }
    }

    /// Returns the `ClientOrderId`s of all in-flight orders.
    #[must_use]
    pub fn client_order_ids_inflight(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
    ) -> HashSet<ClientOrderId> {
        let query = self.build_order_query_filter_set(venue, instrument_id, strategy_id);
        match query {
            Some(query) => self
                .index
                .orders_inflight
                .intersection(&query)
                .copied()
                .collect(),
            None => self.index.orders_inflight.clone(),
        }
    }

    /// Returns `PositionId`s of all positions.
    #[must_use]
    pub fn position_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
    ) -> HashSet<PositionId> {
        let query = self.build_position_query_filter_set(venue, instrument_id, strategy_id);
        match query {
            Some(query) => self.index.positions.intersection(&query).copied().collect(),
            None => self.index.positions.clone(),
        }
    }

    /// Returns the `PositionId`s of all open positions.
    #[must_use]
    pub fn position_open_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
    ) -> HashSet<PositionId> {
        let query = self.build_position_query_filter_set(venue, instrument_id, strategy_id);
        match query {
            Some(query) => self
                .index
                .positions_open
                .intersection(&query)
                .copied()
                .collect(),
            None => self.index.positions_open.clone(),
        }
    }

    /// Returns the `PositionId`s of all closed positions.
    #[must_use]
    pub fn position_closed_ids(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
    ) -> HashSet<PositionId> {
        let query = self.build_position_query_filter_set(venue, instrument_id, strategy_id);
        match query {
            Some(query) => self
                .index
                .positions_closed
                .intersection(&query)
                .copied()
                .collect(),
            None => self.index.positions_closed.clone(),
        }
    }

    /// Returns the `ComponentId`s of all actors.
    #[must_use]
    pub fn actor_ids(&self) -> HashSet<ComponentId> {
        self.index.actors.clone()
    }

    /// Returns the `StrategyId`s of all strategies.
    #[must_use]
    pub fn strategy_ids(&self) -> HashSet<StrategyId> {
        self.index.strategies.clone()
    }

    /// Returns the `ExecAlgorithmId`s of all execution algorithms.
    #[must_use]
    pub fn exec_algorithm_ids(&self) -> HashSet<ExecAlgorithmId> {
        self.index.exec_algorithms.clone()
    }

    // -- ORDER QUERIES ---------------------------------------------------------------------------

    /// Gets a reference to the order with the given `client_order_id` (if found).
    #[must_use]
    pub fn order(&self, client_order_id: &ClientOrderId) -> Option<&OrderAny> {
        self.orders.get(client_order_id)
    }

    /// Gets a reference to the client order ID for given `venue_order_id` (if found).
    #[must_use]
    pub fn client_order_id(&self, venue_order_id: &VenueOrderId) -> Option<&ClientOrderId> {
        self.index.venue_order_ids.get(venue_order_id)
    }

    /// Gets a reference to the venue order ID for given `client_order_id` (if found).
    #[must_use]
    pub fn venue_order_id(&self, client_order_id: &ClientOrderId) -> Option<&VenueOrderId> {
        self.index.client_order_ids.get(client_order_id)
    }

    /// Gets a reference to the client ID indexed for given `client_order_id` (if found).
    #[must_use]
    pub fn client_id(&self, client_order_id: &ClientOrderId) -> Option<&ClientId> {
        self.index.order_client.get(client_order_id)
    }

    /// Returns references to all orders matching the given optional filter parameters.
    #[must_use]
    pub fn orders(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<OrderSide>,
    ) -> Vec<&OrderAny> {
        let client_order_ids = self.client_order_ids(venue, instrument_id, strategy_id);
        self.get_orders_for_ids(&client_order_ids, side)
    }

    /// Returns references to all open orders matching the given optional filter parameters.
    #[must_use]
    pub fn orders_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<OrderSide>,
    ) -> Vec<&OrderAny> {
        let client_order_ids = self.client_order_ids_open(venue, instrument_id, strategy_id);
        self.get_orders_for_ids(&client_order_ids, side)
    }

    /// Returns references to all closed orders matching the given optional filter parameters.
    #[must_use]
    pub fn orders_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<OrderSide>,
    ) -> Vec<&OrderAny> {
        let client_order_ids = self.client_order_ids_closed(venue, instrument_id, strategy_id);
        self.get_orders_for_ids(&client_order_ids, side)
    }

    /// Returns references to all emulated orders matching the given optional filter parameters.
    #[must_use]
    pub fn orders_emulated(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<OrderSide>,
    ) -> Vec<&OrderAny> {
        let client_order_ids = self.client_order_ids_emulated(venue, instrument_id, strategy_id);
        self.get_orders_for_ids(&client_order_ids, side)
    }

    /// Returns references to all in-flight orders matching the given optional filter parameters.
    #[must_use]
    pub fn orders_inflight(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<OrderSide>,
    ) -> Vec<&OrderAny> {
        let client_order_ids = self.client_order_ids_inflight(venue, instrument_id, strategy_id);
        self.get_orders_for_ids(&client_order_ids, side)
    }

    /// Returns references to all orders for the given `position_id`.
    #[must_use]
    pub fn orders_for_position(&self, position_id: &PositionId) -> Vec<&OrderAny> {
        let client_order_ids = self.index.position_orders.get(position_id);
        match client_order_ids {
            Some(client_order_ids) => {
                self.get_orders_for_ids(&client_order_ids.iter().copied().collect(), None)
            }
            None => Vec::new(),
        }
    }

    /// Returns whether an order with the given `client_order_id` exists.
    #[must_use]
    pub fn order_exists(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders.contains(client_order_id)
    }

    /// Returns whether an order with the given `client_order_id` is open.
    #[must_use]
    pub fn is_order_open(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_open.contains(client_order_id)
    }

    /// Returns whether an order with the given `client_order_id` is closed.
    #[must_use]
    pub fn is_order_closed(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_closed.contains(client_order_id)
    }

    /// Returns whether an order with the given `client_order_id` is emulated.
    #[must_use]
    pub fn is_order_emulated(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_emulated.contains(client_order_id)
    }

    /// Returns whether an order with the given `client_order_id` is in-flight.
    #[must_use]
    pub fn is_order_inflight(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_inflight.contains(client_order_id)
    }

    /// Returns whether an order with the given `client_order_id` is `PENDING_CANCEL` locally.
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
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_open(venue, instrument_id, strategy_id, side)
            .len()
    }

    /// Returns the count of all closed orders.
    #[must_use]
    pub fn orders_closed_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_closed(venue, instrument_id, strategy_id, side)
            .len()
    }

    /// Returns the count of all emulated orders.
    #[must_use]
    pub fn orders_emulated_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_emulated(venue, instrument_id, strategy_id, side)
            .len()
    }

    /// Returns the count of all in-flight orders.
    #[must_use]
    pub fn orders_inflight_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_inflight(venue, instrument_id, strategy_id, side)
            .len()
    }

    /// Returns the count of all orders.
    #[must_use]
    pub fn orders_total_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders(venue, instrument_id, strategy_id, side).len()
    }

    /// Returns the order list for the given `order_list_id`.
    #[must_use]
    pub fn order_list(&self, order_list_id: &OrderListId) -> Option<&OrderList> {
        self.order_lists.get(order_list_id)
    }

    /// Returns all order lists matching the given optional filter parameters.
    #[must_use]
    pub fn order_lists(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
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

        order_lists
    }

    /// Returns whether an order list with the given `order_list_id` exists.
    #[must_use]
    pub fn order_list_exists(&self, order_list_id: &OrderListId) -> bool {
        self.order_lists.contains_key(order_list_id)
    }

    // -- EXEC ALGORITHM QUERIES ------------------------------------------------------------------

    /// Returns references to all orders associated with the given `exec_algorithm_id` matching the given
    /// optional filter parameters.
    #[must_use]
    pub fn orders_for_exec_algorithm(
        &self,
        exec_algorithm_id: &ExecAlgorithmId,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<OrderSide>,
    ) -> Vec<&OrderAny> {
        let query = self.build_order_query_filter_set(venue, instrument_id, strategy_id);
        let exec_algorithm_order_ids = self.index.exec_algorithm_orders.get(exec_algorithm_id);

        if let Some(query) = query {
            if let Some(exec_algorithm_order_ids) = exec_algorithm_order_ids {
                let exec_algorithm_order_ids = exec_algorithm_order_ids.intersection(&query);
            }
        }

        if let Some(exec_algorithm_order_ids) = exec_algorithm_order_ids {
            self.get_orders_for_ids(exec_algorithm_order_ids, side)
        } else {
            Vec::new()
        }
    }

    /// Returns references to all orders with the given `exec_spawn_id`.
    #[must_use]
    pub fn orders_for_exec_spawn(&self, exec_spawn_id: &ClientOrderId) -> Vec<&OrderAny> {
        self.get_orders_for_ids(
            self.index
                .exec_spawn_orders
                .get(exec_spawn_id)
                .unwrap_or(&HashSet::new()),
            None,
        )
    }

    /// Returns the total order quantity for the given `exec_spawn_id`.
    #[must_use]
    pub fn exec_spawn_total_quantity(
        &self,
        exec_spawn_id: &ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        let exec_spawn_orders = self.orders_for_exec_spawn(exec_spawn_id);

        let mut total_quantity: Option<Quantity> = None;

        for spawn_order in exec_spawn_orders {
            if !active_only || !spawn_order.is_closed() {
                if let Some(mut total_quantity) = total_quantity {
                    total_quantity += spawn_order.quantity();
                }
            } else {
                total_quantity = Some(spawn_order.quantity());
            }
        }

        total_quantity
    }

    /// Returns the total filled quantity for all orders with the given `exec_spawn_id`.
    #[must_use]
    pub fn exec_spawn_total_filled_qty(
        &self,
        exec_spawn_id: &ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        let exec_spawn_orders = self.orders_for_exec_spawn(exec_spawn_id);

        let mut total_quantity: Option<Quantity> = None;

        for spawn_order in exec_spawn_orders {
            if !active_only || !spawn_order.is_closed() {
                if let Some(mut total_quantity) = total_quantity {
                    total_quantity += spawn_order.filled_qty();
                }
            } else {
                total_quantity = Some(spawn_order.filled_qty());
            }
        }

        total_quantity
    }

    /// Returns the total leaves quantity for all orders with the given `exec_spawn_id`.
    #[must_use]
    pub fn exec_spawn_total_leaves_qty(
        &self,
        exec_spawn_id: &ClientOrderId,
        active_only: bool,
    ) -> Option<Quantity> {
        let exec_spawn_orders = self.orders_for_exec_spawn(exec_spawn_id);

        let mut total_quantity: Option<Quantity> = None;

        for spawn_order in exec_spawn_orders {
            if !active_only || !spawn_order.is_closed() {
                if let Some(mut total_quantity) = total_quantity {
                    total_quantity += spawn_order.leaves_qty();
                }
            } else {
                total_quantity = Some(spawn_order.leaves_qty());
            }
        }

        total_quantity
    }

    // -- POSITION QUERIES ------------------------------------------------------------------------

    /// Returns a reference to the position with the given `position_id` (if found).
    #[must_use]
    pub fn position(&self, position_id: &PositionId) -> Option<&Position> {
        self.positions.get(position_id)
    }

    /// Returns a reference to the position for the given `client_order_id` (if found).
    #[must_use]
    pub fn position_for_order(&self, client_order_id: &ClientOrderId) -> Option<&Position> {
        self.index
            .order_position
            .get(client_order_id)
            .and_then(|position_id| self.positions.get(position_id))
    }

    /// Returns a reference to the position ID for the given `client_order_id` (if found).
    #[must_use]
    pub fn position_id(&self, client_order_id: &ClientOrderId) -> Option<&PositionId> {
        self.index.order_position.get(client_order_id)
    }

    /// Returns a reference to all positions matching the given optional filter parameters.
    #[must_use]
    pub fn positions(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<PositionSide>,
    ) -> Vec<&Position> {
        let position_ids = self.position_ids(venue, instrument_id, strategy_id);
        self.get_positions_for_ids(&position_ids, side)
    }

    /// Returns a reference to all open positions matching the given optional filter parameters.
    #[must_use]
    pub fn positions_open(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<PositionSide>,
    ) -> Vec<&Position> {
        let position_ids = self.position_open_ids(venue, instrument_id, strategy_id);
        self.get_positions_for_ids(&position_ids, side)
    }

    /// Returns a reference to all closed positions matching the given optional filter parameters.
    #[must_use]
    pub fn positions_closed(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<PositionSide>,
    ) -> Vec<&Position> {
        let position_ids = self.position_closed_ids(venue, instrument_id, strategy_id);
        self.get_positions_for_ids(&position_ids, side)
    }

    /// Returns whether a position with the given `position_id` exists.
    #[must_use]
    pub fn position_exists(&self, position_id: &PositionId) -> bool {
        self.index.positions.contains(position_id)
    }

    /// Returns whether a position with the given `position_id` is open.
    #[must_use]
    pub fn is_position_open(&self, position_id: &PositionId) -> bool {
        self.index.positions_open.contains(position_id)
    }

    /// Returns whether a position with the given `position_id` is closed.
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
        side: Option<PositionSide>,
    ) -> usize {
        self.positions_open(venue, instrument_id, strategy_id, side)
            .len()
    }

    /// Returns the count of all closed positions.
    #[must_use]
    pub fn positions_closed_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.positions_closed(venue, instrument_id, strategy_id, side)
            .len()
    }

    /// Returns the count of all positions.
    #[must_use]
    pub fn positions_total_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.positions(venue, instrument_id, strategy_id, side)
            .len()
    }

    // -- STRATEGY QUERIES ------------------------------------------------------------------------

    /// Gets a reference to the strategy ID for the given `client_order_id` (if found).
    #[must_use]
    pub fn strategy_id_for_order(&self, client_order_id: &ClientOrderId) -> Option<&StrategyId> {
        self.index.order_strategy.get(client_order_id)
    }

    /// Gets a reference to the strategy ID for the given `position_id` (if found).
    #[must_use]
    pub fn strategy_id_for_position(&self, position_id: &PositionId) -> Option<&StrategyId> {
        self.index.position_strategy.get(position_id)
    }

    // -- GENERAL ---------------------------------------------------------------------------------

    /// Gets a reference to the general object value for the given `key` (if found).
    pub fn get(&self, key: &str) -> anyhow::Result<Option<&Bytes>> {
        check_valid_string(key, stringify!(key)).expect(FAILED);

        Ok(self.general.get(key))
    }

    // -- DATA QUERIES ----------------------------------------------------------------------------

    /// Returns the price for the given `instrument_id` and `price_type` (if found).
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
                        (quote.ask_price.as_f64() + quote.bid_price.as_f64()) / 2.0,
                        quote.bid_price.precision + 1,
                    )
                })
            }),
            PriceType::Last => self
                .trades
                .get(instrument_id)
                .and_then(|trades| trades.front().map(|trade| trade.price)),
        }
    }

    /// Gets all quote ticks for the given `instrument_id`.
    #[must_use]
    pub fn quote_ticks(&self, instrument_id: &InstrumentId) -> Option<Vec<QuoteTick>> {
        self.quotes
            .get(instrument_id)
            .map(|quotes| quotes.iter().copied().collect())
    }

    /// Gets all trade ticks for the given `instrument_id`.
    #[must_use]
    pub fn trade_ticks(&self, instrument_id: &InstrumentId) -> Option<Vec<TradeTick>> {
        self.trades
            .get(instrument_id)
            .map(|trades| trades.iter().copied().collect())
    }

    /// Gets all bars for the given `bar_type`.
    #[must_use]
    pub fn bars(&self, bar_type: &BarType) -> Option<Vec<Bar>> {
        self.bars
            .get(bar_type)
            .map(|bars| bars.iter().copied().collect())
    }

    /// Gets a reference to the order book for the given `instrument_id`.
    #[must_use]
    pub fn order_book(&mut self, instrument_id: &InstrumentId) -> Option<&mut OrderBook> {
        self.books.get_mut(instrument_id)
    }

    /// Gets a reference to the latest quote tick for the given `instrument_id`.
    #[must_use]
    pub fn quote_tick(&self, instrument_id: &InstrumentId) -> Option<&QuoteTick> {
        self.quotes
            .get(instrument_id)
            .and_then(|quotes| quotes.front())
    }

    /// Gets a refernece to the latest trade tick for the given `instrument_id`.
    #[must_use]
    pub fn trade_tick(&self, instrument_id: &InstrumentId) -> Option<&TradeTick> {
        self.trades
            .get(instrument_id)
            .and_then(|trades| trades.front())
    }

    /// Gets a reference to the latest bar for the given `bar_type`.
    #[must_use]
    pub fn bar(&self, bar_type: &BarType) -> Option<&Bar> {
        self.bars.get(bar_type).and_then(|bars| bars.front())
    }

    /// Gets the order book update count for the given `instrument_id`.
    #[must_use]
    pub fn book_update_count(&self, instrument_id: &InstrumentId) -> usize {
        self.books.get(instrument_id).map_or(0, |book| book.count) as usize
    }

    /// Gets the quote tick count for the given `instrument_id`.
    #[must_use]
    pub fn quote_tick_count(&self, instrument_id: &InstrumentId) -> usize {
        self.quotes
            .get(instrument_id)
            .map_or(0, std::collections::VecDeque::len)
    }

    /// Gets the trade tick count for the given `instrument_id`.
    #[must_use]
    pub fn trade_tick_count(&self, instrument_id: &InstrumentId) -> usize {
        self.trades
            .get(instrument_id)
            .map_or(0, std::collections::VecDeque::len)
    }

    /// Gets the bar count for the given `instrument_id`.
    #[must_use]
    pub fn bar_count(&self, bar_type: &BarType) -> usize {
        self.bars
            .get(bar_type)
            .map_or(0, std::collections::VecDeque::len)
    }

    /// Returns whether the cache contains an order book for the given `instrument_id`.
    #[must_use]
    pub fn has_order_book(&self, instrument_id: &InstrumentId) -> bool {
        self.books.contains_key(instrument_id)
    }

    /// Returns whether the cache contains quote ticks for the given `instrument_id`.
    #[must_use]
    pub fn has_quote_ticks(&self, instrument_id: &InstrumentId) -> bool {
        self.quote_tick_count(instrument_id) > 0
    }

    /// Returns whether the cache contains trade ticks for the given `instrument_id`.
    #[must_use]
    pub fn has_trade_ticks(&self, instrument_id: &InstrumentId) -> bool {
        self.trade_tick_count(instrument_id) > 0
    }

    /// Returns whether the cache contains bars for the given `bar_type`.
    #[must_use]
    pub fn has_bars(&self, bar_type: &BarType) -> bool {
        self.bar_count(bar_type) > 0
    }

    // -- INSTRUMENT QUERIES ----------------------------------------------------------------------

    /// Returns a reference to the instrument for the given `instrument_id` (if found).
    #[must_use]
    pub fn instrument(&self, instrument_id: &InstrumentId) -> Option<&InstrumentAny> {
        self.instruments.get(instrument_id)
    }

    /// Returns references to all instrument IDs for the given `venue`.
    #[must_use]
    pub fn instrument_ids(&self, venue: Option<&Venue>) -> Vec<&InstrumentId> {
        self.instruments
            .keys()
            .filter(|i| venue.is_none() || &i.venue == venue.unwrap())
            .collect()
    }

    /// Returns references to all instruments for the given `venue`.
    #[must_use]
    pub fn instruments(&self, venue: &Venue) -> Vec<&InstrumentAny> {
        self.instruments
            .values()
            .filter(|i| &i.id().venue == venue)
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

    /// Returns a reference to the synthetic instrument for the given `instrument_id` (if found).
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

    /// Returns a reference to the account for the given `account_id` (if found).
    #[must_use]
    pub fn account(&self, account_id: &AccountId) -> Option<&AccountAny> {
        self.accounts.get(account_id)
    }

    /// Returns a reference to the account for the given `venue` (if found).
    #[must_use]
    pub fn account_for_venue(&self, venue: &Venue) -> Option<&AccountAny> {
        self.index
            .venue_account
            .get(venue)
            .and_then(|account_id| self.accounts.get(account_id))
    }

    /// Returns a reference to the account ID for the given `venue` (if found).
    #[must_use]
    pub fn account_id(&self, venue: &Venue) -> Option<&AccountId> {
        self.index.venue_account.get(venue)
    }

    /// Returns references to all accounts for the given `account_id`.
    #[must_use]
    pub fn accounts(&self, account_id: &AccountId) -> Vec<&AccountAny> {
        self.accounts
            .values()
            .filter(|account| &account.id() == account_id)
            .collect()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use nautilus_model::{
        accounts::any::AccountAny,
        data::{bar::Bar, quote::QuoteTick, trade::TradeTick},
        enums::{BookType, OmsType, OrderSide, OrderStatus},
        events::order::{OrderAccepted, OrderEventAny, OrderRejected, OrderSubmitted},
        identifiers::{AccountId, ClientOrderId, PositionId, Venue},
        instruments::{
            any::InstrumentAny, currency_pair::CurrencyPair, stubs::*,
            synthetic::SyntheticInstrument,
        },
        orderbook::book::OrderBook,
        orders::stubs::{TestOrderEventStubs, TestOrderStubs},
        position::Position,
        types::{price::Price, quantity::Quantity},
    };
    use rstest::{fixture, rstest};

    use super::Cache;

    #[fixture]
    fn cache() -> Cache {
        Cache::default()
    }

    #[rstest]
    fn test_build_index_when_empty(mut cache: Cache) {
        cache.build_index();
    }

    #[rstest]
    fn test_check_integrity_when_empty(mut cache: Cache) {
        let result = cache.check_integrity();
        assert!(result);
    }

    #[rstest]
    fn test_check_residuals_when_empty(cache: Cache) {
        let result = cache.check_residuals();
        assert!(!result);
    }

    #[rstest]
    fn test_clear_index_when_empty(mut cache: Cache) {
        cache.clear_index();
    }

    #[rstest]
    fn test_reset_when_empty(mut cache: Cache) {
        cache.reset();
    }

    #[rstest]
    fn test_dispose_when_empty(mut cache: Cache) {
        cache.dispose();
    }

    #[rstest]
    fn test_flush_db_when_empty(mut cache: Cache) {
        cache.flush_db();
    }

    #[rstest]
    fn test_cache_general_when_no_database(mut cache: Cache) {
        assert!(cache.cache_general().is_ok());
    }

    // -- EXECUTION -------------------------------------------------------------------------------

    #[rstest]
    fn test_cache_orders_when_no_database(mut cache: Cache) {
        assert!(cache.cache_orders().is_ok());
    }

    #[rstest]
    fn test_order_when_empty(cache: Cache) {
        let client_order_id = ClientOrderId::default();
        let result = cache.order(&client_order_id);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_order_when_initialized(mut cache: Cache, audusd_sim: CurrencyPair) {
        let order = TestOrderStubs::limit_order(
            audusd_sim.id,
            OrderSide::Buy,
            Price::from("1.00000"),
            Quantity::from(100_000),
            None,
            None,
        );
        let client_order_id = order.client_order_id();
        cache.add_order(order, None, None, false).unwrap();

        let order = cache.order(&client_order_id).unwrap();
        assert_eq!(cache.orders(None, None, None, None), vec![order]);
        assert!(cache.orders_open(None, None, None, None).is_empty());
        assert!(cache.orders_closed(None, None, None, None).is_empty());
        assert!(cache.orders_emulated(None, None, None, None).is_empty());
        assert!(cache.orders_inflight(None, None, None, None).is_empty());
        assert!(cache.order_exists(&order.client_order_id()));
        assert!(!cache.is_order_open(&order.client_order_id()));
        assert!(!cache.is_order_closed(&order.client_order_id()));
        assert!(!cache.is_order_emulated(&order.client_order_id()));
        assert!(!cache.is_order_inflight(&order.client_order_id()));
        assert!(!cache.is_order_pending_cancel_local(&order.client_order_id()));
        assert_eq!(cache.orders_open_count(None, None, None, None), 0);
        assert_eq!(cache.orders_closed_count(None, None, None, None), 0);
        assert_eq!(cache.orders_emulated_count(None, None, None, None), 0);
        assert_eq!(cache.orders_inflight_count(None, None, None, None), 0);
        assert_eq!(cache.orders_total_count(None, None, None, None), 1);
        assert_eq!(cache.venue_order_id(&order.client_order_id()), None);
    }

    #[rstest]
    fn test_order_when_submitted(mut cache: Cache, audusd_sim: CurrencyPair) {
        let mut order = TestOrderStubs::limit_order(
            audusd_sim.id,
            OrderSide::Buy,
            Price::from("1.00000"),
            Quantity::from(100_000),
            None,
            None,
        );
        let client_order_id = order.client_order_id();
        cache.add_order(order.clone(), None, None, false).unwrap();

        let submitted = OrderSubmitted::default();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        cache.update_order(&order).unwrap();

        let result = cache.order(&order.client_order_id()).unwrap();

        assert_eq!(order.status(), OrderStatus::Submitted);
        assert_eq!(result, &order);
        assert_eq!(cache.orders(None, None, None, None), vec![&order]);
        assert!(cache.orders_open(None, None, None, None).is_empty());
        assert!(cache.orders_closed(None, None, None, None).is_empty());
        assert!(cache.orders_emulated(None, None, None, None).is_empty());
        assert!(!cache.orders_inflight(None, None, None, None).is_empty());
        assert!(cache.order_exists(&order.client_order_id()));
        assert!(!cache.is_order_open(&order.client_order_id()));
        assert!(!cache.is_order_closed(&order.client_order_id()));
        assert!(!cache.is_order_emulated(&order.client_order_id()));
        assert!(cache.is_order_inflight(&order.client_order_id()));
        assert!(!cache.is_order_pending_cancel_local(&order.client_order_id()));
        assert_eq!(cache.orders_open_count(None, None, None, None), 0);
        assert_eq!(cache.orders_closed_count(None, None, None, None), 0);
        assert_eq!(cache.orders_emulated_count(None, None, None, None), 0);
        assert_eq!(cache.orders_inflight_count(None, None, None, None), 1);
        assert_eq!(cache.orders_total_count(None, None, None, None), 1);
        assert_eq!(cache.venue_order_id(&order.client_order_id()), None);
    }

    #[rstest]
    fn test_order_when_rejected(mut cache: Cache, audusd_sim: CurrencyPair) {
        let mut order = TestOrderStubs::market_order(
            audusd_sim.id,
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            None,
        );
        cache.add_order(order.clone(), None, None, false).unwrap();

        let submitted = OrderSubmitted::default();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        cache.update_order(&order).unwrap();

        let rejected = OrderRejected::default();
        order.apply(OrderEventAny::Rejected(rejected)).unwrap();
        cache.update_order(&order).unwrap();

        let result = cache.order(&order.client_order_id()).unwrap();

        assert!(order.is_closed());
        assert_eq!(result, &order);
        assert_eq!(cache.orders(None, None, None, None), vec![&order]);
        assert!(cache.orders_open(None, None, None, None).is_empty());
        assert_eq!(cache.orders_closed(None, None, None, None), vec![&order]);
        assert!(cache.orders_emulated(None, None, None, None).is_empty());
        assert!(cache.orders_inflight(None, None, None, None).is_empty());
        assert!(cache.order_exists(&order.client_order_id()));
        assert!(!cache.is_order_open(&order.client_order_id()));
        assert!(cache.is_order_closed(&order.client_order_id()));
        assert!(!cache.is_order_emulated(&order.client_order_id()));
        assert!(!cache.is_order_inflight(&order.client_order_id()));
        assert!(!cache.is_order_pending_cancel_local(&order.client_order_id()));
        assert_eq!(cache.orders_open_count(None, None, None, None), 0);
        assert_eq!(cache.orders_closed_count(None, None, None, None), 1);
        assert_eq!(cache.orders_emulated_count(None, None, None, None), 0);
        assert_eq!(cache.orders_inflight_count(None, None, None, None), 0);
        assert_eq!(cache.orders_total_count(None, None, None, None), 1);
    }

    #[rstest]
    fn test_order_when_accepted(mut cache: Cache, audusd_sim: CurrencyPair) {
        let mut order = TestOrderStubs::limit_order(
            audusd_sim.id,
            OrderSide::Buy,
            Price::from("1.00000"),
            Quantity::from(100_000),
            None,
            None,
        );
        cache.add_order(order.clone(), None, None, false).unwrap();

        let submitted = OrderSubmitted::default();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        cache.update_order(&order).unwrap();

        let accepted = OrderAccepted::default();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        cache.update_order(&order).unwrap();

        let result = cache.order(&order.client_order_id()).unwrap();

        assert!(order.is_open());
        assert_eq!(result, &order);
        assert_eq!(cache.orders(None, None, None, None), vec![&order]);
        assert_eq!(cache.orders_open(None, None, None, None), vec![&order]);
        assert!(cache.orders_closed(None, None, None, None).is_empty());
        assert!(cache.orders_emulated(None, None, None, None).is_empty());
        assert!(cache.orders_inflight(None, None, None, None).is_empty());
        assert!(cache.order_exists(&order.client_order_id()));
        assert!(cache.is_order_open(&order.client_order_id()));
        assert!(!cache.is_order_closed(&order.client_order_id()));
        assert!(!cache.is_order_emulated(&order.client_order_id()));
        assert!(!cache.is_order_inflight(&order.client_order_id()));
        assert!(!cache.is_order_pending_cancel_local(&order.client_order_id()));
        assert_eq!(cache.orders_open_count(None, None, None, None), 1);
        assert_eq!(cache.orders_closed_count(None, None, None, None), 0);
        assert_eq!(cache.orders_emulated_count(None, None, None, None), 0);
        assert_eq!(cache.orders_inflight_count(None, None, None, None), 0);
        assert_eq!(cache.orders_total_count(None, None, None, None), 1);
        assert_eq!(
            cache.client_order_id(&order.venue_order_id().unwrap()),
            Some(&order.client_order_id())
        );
        assert_eq!(
            cache.venue_order_id(&order.client_order_id()),
            Some(&order.venue_order_id().unwrap())
        );
    }

    #[rstest]
    fn test_order_when_filled(mut cache: Cache, audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let mut order = TestOrderStubs::market_order(
            audusd_sim.id(),
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            None,
        );
        cache.add_order(order.clone(), None, None, false).unwrap();

        let submitted = OrderSubmitted::default();
        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        cache.update_order(&order).unwrap();

        let accepted = OrderAccepted::default();
        order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        cache.update_order(&order).unwrap();

        let filled = TestOrderEventStubs::order_filled(
            &order,
            &audusd_sim,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        order.apply(filled).unwrap();
        cache.update_order(&order).unwrap();

        let result = cache.order(&order.client_order_id()).unwrap();

        assert!(order.is_closed());
        assert_eq!(result, &order);
        assert_eq!(cache.orders(None, None, None, None), vec![&order]);
        assert_eq!(cache.orders_closed(None, None, None, None), vec![&order]);
        assert!(cache.orders_open(None, None, None, None).is_empty());
        assert!(cache.orders_emulated(None, None, None, None).is_empty());
        assert!(cache.orders_inflight(None, None, None, None).is_empty());
        assert!(cache.order_exists(&order.client_order_id()));
        assert!(!cache.is_order_open(&order.client_order_id()));
        assert!(cache.is_order_closed(&order.client_order_id()));
        assert!(!cache.is_order_emulated(&order.client_order_id()));
        assert!(!cache.is_order_inflight(&order.client_order_id()));
        assert!(!cache.is_order_pending_cancel_local(&order.client_order_id()));
        assert_eq!(cache.orders_open_count(None, None, None, None), 0);
        assert_eq!(cache.orders_closed_count(None, None, None, None), 1);
        assert_eq!(cache.orders_emulated_count(None, None, None, None), 0);
        assert_eq!(cache.orders_inflight_count(None, None, None, None), 0);
        assert_eq!(cache.orders_total_count(None, None, None, None), 1);
        assert_eq!(
            cache.client_order_id(&order.venue_order_id().unwrap()),
            Some(&order.client_order_id())
        );
        assert_eq!(
            cache.venue_order_id(&order.client_order_id()),
            Some(&order.venue_order_id().unwrap())
        );
    }

    #[rstest]
    fn test_get_general_when_empty(cache: Cache) {
        let result = cache.get("A").unwrap();
        assert!(result.is_none());
    }

    #[rstest]
    fn test_add_general_when_value(mut cache: Cache) {
        let key = "A";
        let value = Bytes::from_static(&[0_u8]);
        cache.add(key, value.clone()).unwrap();
        let result = cache.get(key).unwrap();
        assert_eq!(result, Some(&value));
    }

    #[rstest]
    fn test_orders_for_position(mut cache: Cache, audusd_sim: CurrencyPair) {
        let order = TestOrderStubs::limit_order(
            audusd_sim.id,
            OrderSide::Buy,
            Price::from("1.00000"),
            Quantity::from(100_000),
            None,
            None,
        );
        let position_id = PositionId::default();
        cache
            .add_order(order.clone(), Some(position_id), None, false)
            .unwrap();
        let result = cache.order(&order.client_order_id()).unwrap();
        assert_eq!(result, &order);
        assert_eq!(cache.orders_for_position(&position_id), vec![&order]);
    }

    #[rstest]
    fn test_cache_positions_when_no_database(mut cache: Cache) {
        assert!(cache.cache_positions().is_ok());
    }

    #[rstest]
    fn test_position_when_empty(cache: Cache) {
        let position_id = PositionId::from("1");
        let result = cache.position(&position_id);
        assert!(result.is_none());
        assert!(!cache.position_exists(&position_id));
    }

    #[rstest]
    fn test_position_when_some(mut cache: Cache, audusd_sim: CurrencyPair) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = TestOrderStubs::market_order(
            audusd_sim.id(),
            OrderSide::Buy,
            Quantity::from("1000000"),
            None,
            None,
        );
        let fill = TestOrderEventStubs::order_filled(
            &order,
            &audusd_sim,
            None,
            Some(PositionId::new("P-123456")),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let position = Position::new(&audusd_sim, fill.into());
        cache
            .add_position(position.clone(), OmsType::Netting)
            .unwrap();

        let result = cache.position(&position.id);
        assert_eq!(result, Some(&position));
        assert!(cache.position_exists(&position.id));
        assert_eq!(
            cache.position_id(&order.client_order_id()),
            Some(&position.id)
        );
        assert_eq!(
            cache.positions_open(None, None, None, None),
            vec![&position]
        );
        assert_eq!(
            cache.positions_closed(None, None, None, None),
            Vec::<&Position>::new()
        );
        assert_eq!(cache.positions_open_count(None, None, None, None), 1);
        assert_eq!(cache.positions_closed_count(None, None, None, None), 0);
    }

    // -- DATA ------------------------------------------------------------------------------------

    #[rstest]
    fn test_cache_currencies_when_no_database(mut cache: Cache) {
        assert!(cache.cache_currencies().is_ok());
    }

    #[rstest]
    fn test_cache_instruments_when_no_database(mut cache: Cache) {
        assert!(cache.cache_instruments().is_ok());
    }

    #[rstest]
    fn test_instrument_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
        let result = cache.instrument(&audusd_sim.id);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_instrument_when_some(mut cache: Cache, audusd_sim: CurrencyPair) {
        cache
            .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
            .unwrap();

        let result = cache.instrument(&audusd_sim.id);
        assert_eq!(result, Some(&InstrumentAny::CurrencyPair(audusd_sim)));
    }

    #[rstest]
    fn test_cache_synthetics_when_no_database(mut cache: Cache) {
        assert!(cache.cache_synthetics().is_ok());
    }

    #[rstest]
    fn test_synthetic_when_empty(cache: Cache) {
        let synth = SyntheticInstrument::default();
        let result = cache.synthetic(&synth.id);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_synthetic_when_some(cache: Cache) {
        let mut cache = Cache::default();
        let synth = SyntheticInstrument::default();
        cache.add_synthetic(synth.clone()).unwrap();
        let result = cache.synthetic(&synth.id);
        assert_eq!(result, Some(&synth));
    }

    #[rstest]
    fn test_order_book_when_empty(mut cache: Cache, audusd_sim: CurrencyPair) {
        let result = cache.order_book(&audusd_sim.id);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_order_book_when_some(mut cache: Cache, audusd_sim: CurrencyPair) {
        let mut book = OrderBook::new(audusd_sim.id, BookType::L2_MBP);
        cache.add_order_book(book.clone()).unwrap();
        let result = cache.order_book(&audusd_sim.id);
        assert_eq!(result, Some(&mut book));
    }

    #[rstest]
    fn test_quote_tick_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
        let result = cache.quote_tick(&audusd_sim.id);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_quote_tick_when_some(mut cache: Cache) {
        let quote = QuoteTick::default();
        cache.add_quote(quote).unwrap();
        let result = cache.quote_tick(&quote.instrument_id);
        assert_eq!(result, Some(&quote));
    }

    #[rstest]
    fn test_quote_ticks_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
        let result = cache.quote_ticks(&audusd_sim.id);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_quote_ticks_when_some(mut cache: Cache) {
        let quotes = vec![
            QuoteTick::default(),
            QuoteTick::default(),
            QuoteTick::default(),
        ];
        cache.add_quotes(&quotes).unwrap();
        let result = cache.quote_ticks(&quotes[0].instrument_id);
        assert_eq!(result, Some(quotes));
    }

    #[rstest]
    fn test_trade_tick_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
        let result = cache.trade_tick(&audusd_sim.id);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_trade_tick_when_some(mut cache: Cache) {
        let trade = TradeTick::default();
        cache.add_trade(trade).unwrap();
        let result = cache.trade_tick(&trade.instrument_id);
        assert_eq!(result, Some(&trade));
    }

    #[rstest]
    fn test_trade_ticks_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
        let result = cache.trade_ticks(&audusd_sim.id);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_trade_ticks_when_some(mut cache: Cache) {
        let trades = vec![
            TradeTick::default(),
            TradeTick::default(),
            TradeTick::default(),
        ];
        cache.add_trades(&trades).unwrap();
        let result = cache.trade_ticks(&trades[0].instrument_id);
        assert_eq!(result, Some(trades));
    }

    #[rstest]
    fn test_bar_when_empty(cache: Cache) {
        let bar = Bar::default();
        let result = cache.bar(&bar.bar_type);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_bar_when_some(mut cache: Cache) {
        let bar = Bar::default();
        cache.add_bar(bar).unwrap();
        let result = cache.bar(&bar.bar_type);
        assert_eq!(result, Some(&bar));
    }

    #[rstest]
    fn test_bars_when_empty(cache: Cache) {
        let bar = Bar::default();
        let result = cache.bars(&bar.bar_type);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_bars_when_some(mut cache: Cache) {
        let bars = vec![Bar::default(), Bar::default(), Bar::default()];
        cache.add_bars(&bars).unwrap();
        let result = cache.bars(&bars[0].bar_type);
        assert_eq!(result, Some(bars));
    }

    // -- ACCOUNT ---------------------------------------------------------------------------------

    #[rstest]
    fn test_cache_accounts_when_no_database(mut cache: Cache) {
        assert!(cache.cache_accounts().is_ok());
    }

    #[rstest]
    fn test_cache_add_account(mut cache: Cache) {
        let account = AccountAny::default();
        cache.add_account(account.clone()).unwrap();
        let result = cache.account(&account.id());
        assert!(result.is_some());
        assert_eq!(*result.unwrap(), account);
    }

    #[rstest]
    fn test_cache_accounts_when_no_accounts_returns_empty(cache: Cache) {
        let result = cache.accounts(&AccountId::default());
        assert!(result.is_empty());
    }

    #[rstest]
    fn test_cache_account_for_venue_returns_empty(cache: Cache) {
        let venue = Venue::default();
        let result = cache.account_for_venue(&venue);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_cache_account_for_venue_return_correct(mut cache: Cache) {
        let account = AccountAny::default();
        let venue = account.last_event().unwrap().account_id.get_issuer();
        cache.add_account(account.clone()).unwrap();
        let result = cache.account_for_venue(&venue);
        assert!(result.is_some());
        assert_eq!(*result.unwrap(), account);
    }
}
