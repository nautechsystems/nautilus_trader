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

use std::collections::{HashMap, HashSet, VecDeque};

use log::{debug, info};
use nautilus_core::correctness::{check_key_not_in_map, check_slice_not_empty, check_valid_string};
use nautilus_model::{
    data::{
        bar::{Bar, BarType},
        quote::QuoteTick,
        trade::TradeTick,
    },
    enums::{AggregationSource, OrderSide, PositionSide, PriceType},
    identifiers::{
        account_id::AccountId, client_id::ClientId, client_order_id::ClientOrderId,
        component_id::ComponentId, exec_algorithm_id::ExecAlgorithmId, instrument_id::InstrumentId,
        order_list_id::OrderListId, position_id::PositionId, strategy_id::StrategyId, venue::Venue,
        venue_order_id::VenueOrderId,
    },
    instruments::{synthetic::SyntheticInstrument, InstrumentAny},
    orderbook::book::OrderBook,
    orders::{base::OrderAny, list::OrderList},
    polymorphism::{
        GetClientOrderId, GetExecAlgorithmId, GetExecSpawnId, GetInstrumentId, GetOrderFilledQty,
        GetOrderLeavesQty, GetOrderQuantity, GetOrderSide, GetStrategyId, IsClosed,
    },
    position::Position,
    types::{currency::Currency, price::Price, quantity::Quantity},
};
use ustr::Ustr;

use self::database::CacheDatabaseAdapter;
use crate::{enums::SerializationEncoding, interface::account::Account};

/// The configuration for `Cache` instances.
pub struct CacheConfig {
    pub encoding: SerializationEncoding,
    pub timestamps_as_iso8601: bool,
    pub use_trader_prefix: bool,
    pub use_instance_id: bool,
    pub flush_on_start: bool,
    pub drop_instruments_on_reset: bool,
    pub tick_capacity: usize,
    pub bar_capacity: usize,
}

impl CacheConfig {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        encoding: SerializationEncoding,
        timestamps_as_iso8601: bool,
        use_trader_prefix: bool,
        use_instance_id: bool,
        flush_on_start: bool,
        drop_instruments_on_reset: bool,
        tick_capacity: usize,
        bar_capacity: usize,
    ) -> Self {
        Self {
            encoding,
            timestamps_as_iso8601,
            use_trader_prefix,
            use_instance_id,
            flush_on_start,
            drop_instruments_on_reset,
            tick_capacity,
            bar_capacity,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self::new(
            SerializationEncoding::MsgPack,
            false,
            true,
            false,
            false,
            true,
            10_000,
            10_000,
        )
    }
}

/// A key-value lookup index for a `Cache`.
pub struct CacheIndex {
    venue_account: HashMap<Venue, AccountId>,
    venue_orders: HashMap<Venue, HashSet<ClientOrderId>>,
    venue_positions: HashMap<Venue, HashSet<PositionId>>,
    order_ids: HashMap<VenueOrderId, ClientOrderId>,
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
    /// Clear the index which will clear/reset all internal state.
    pub fn clear(&mut self) {
        self.venue_account.clear();
        self.venue_orders.clear();
        self.venue_positions.clear();
        self.order_ids.clear();
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
    database: Option<CacheDatabaseAdapter>,
    general: HashMap<String, Vec<u8>>,
    quotes: HashMap<InstrumentId, VecDeque<QuoteTick>>,
    trades: HashMap<InstrumentId, VecDeque<TradeTick>>,
    books: HashMap<InstrumentId, OrderBook>,
    bars: HashMap<BarType, VecDeque<Bar>>,
    currencies: HashMap<Ustr, Currency>,
    instruments: HashMap<InstrumentId, InstrumentAny>,
    synthetics: HashMap<InstrumentId, SyntheticInstrument>,
    accounts: HashMap<AccountId, Box<dyn Account>>,
    orders: HashMap<ClientOrderId, OrderAny>,
    order_lists: HashMap<OrderListId, OrderList>,
    positions: HashMap<PositionId, Position>,
    position_snapshots: HashMap<PositionId, Vec<u8>>,
}

impl Default for Cache {
    fn default() -> Self {
        Self::new(CacheConfig::default(), None)
    }
}

impl Cache {
    #[must_use]
    pub fn new(config: CacheConfig, database: Option<CacheDatabaseAdapter>) -> Self {
        let index = CacheIndex {
            venue_account: HashMap::new(),
            venue_orders: HashMap::new(),
            venue_positions: HashMap::new(),
            order_ids: HashMap::new(),
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
            config,
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

    // -- COMMANDS --------------------------------------------------------------------------------

    pub fn cache_general(&mut self) -> anyhow::Result<()> {
        self.general = match &self.database {
            Some(db) => db.load()?,
            None => HashMap::new(),
        };

        info!(
            "Cached {} general object(s) from database",
            self.general.len()
        );
        Ok(())
    }

    pub fn cache_currencies(&mut self) -> anyhow::Result<()> {
        self.currencies = match &self.database {
            Some(db) => db.load_currencies()?,
            None => HashMap::new(),
        };

        info!("Cached {} currencies from database", self.general.len());
        Ok(())
    }

    pub fn cache_instruments(&mut self) -> anyhow::Result<()> {
        self.instruments = match &self.database {
            Some(db) => db.load_instruments()?,
            None => HashMap::new(),
        };

        info!("Cached {} instruments from database", self.general.len());
        Ok(())
    }

    pub fn cache_synthetics(&mut self) -> anyhow::Result<()> {
        self.synthetics = match &self.database {
            Some(db) => db.load_synthetics()?,
            None => HashMap::new(),
        };

        info!(
            "Cached {} synthetic instruments from database",
            self.general.len()
        );
        Ok(())
    }

    pub fn cache_accounts(&mut self) -> anyhow::Result<()> {
        self.accounts = match &self.database {
            Some(db) => db.load_accounts()?,
            None => HashMap::new(),
        };

        info!(
            "Cached {} synthetic instruments from database",
            self.general.len()
        );
        Ok(())
    }

    pub fn cache_orders(&mut self) -> anyhow::Result<()> {
        self.orders = match &self.database {
            Some(db) => db.load_orders()?,
            None => HashMap::new(),
        };

        info!("Cached {} orders from database", self.general.len());
        Ok(())
    }

    pub fn cache_positions(&mut self) -> anyhow::Result<()> {
        self.positions = match &self.database {
            Some(db) => db.load_positions()?,
            None => HashMap::new(),
        };

        info!("Cached {} positions from database", self.general.len());
        Ok(())
    }

    pub fn build_index(&self) {
        todo!() // Needs order query methods
    }

    #[must_use]
    pub fn check_integrity(&self) -> bool {
        true // TODO
    }

    pub fn check_residuals(&self) {
        todo!() // Needs order query methods
    }

    pub fn clear_index(&mut self) {
        self.index.clear();
        debug!("Cleared index");
    }

    /// Reset the cache.
    ///
    /// All stateful fields are reset to their initial value.
    pub fn reset(&mut self) {
        debug!("Resetting cache");

        self.general.clear();
        self.quotes.clear();
        self.trades.clear();
        self.books.clear();
        self.bars.clear();
        self.instruments.clear();
        self.synthetics.clear();
        self.accounts.clear();
        self.orders.clear();
        // self.order_lists.clear();  // TODO
        self.positions.clear();
        self.position_snapshots.clear();

        self.clear_index();

        info!("Reset cache");
    }

    pub fn dispose(&self) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            // TODO: Log operations in database adapter
            database.close()?;
        }
        Ok(())
    }

    pub fn flush_db(&self) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            // TODO: Log operations in database adapter
            database.flush()?;
        }
        Ok(())
    }

    pub fn add(&mut self, key: &str, value: Vec<u8>) -> anyhow::Result<()> {
        check_valid_string(key, stringify!(key))?;
        check_slice_not_empty(value.as_slice(), stringify!(value))?;

        debug!("Add general {key}");
        self.general.insert(key.to_string(), value.clone());

        if let Some(database) = &self.database {
            database.add(key.to_string(), value)?;
        }
        Ok(())
    }

    pub fn add_order_book(&mut self, book: OrderBook) -> anyhow::Result<()> {
        debug!("Add `OrderBook` {}", book.instrument_id);
        self.books.insert(book.instrument_id, book);
        Ok(())
    }

    pub fn add_quote(&mut self, quote: QuoteTick) -> anyhow::Result<()> {
        debug!("Add `QuoteTick` {}", quote.instrument_id);
        let quotes_deque = self
            .quotes
            .entry(quote.instrument_id)
            .or_insert_with(|| VecDeque::with_capacity(self.config.tick_capacity));
        quotes_deque.push_front(quote);
        Ok(())
    }

    pub fn add_quotes(&mut self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        check_slice_not_empty(quotes, stringify!(quotes))?;

        let instrument_id = quotes[0].instrument_id;
        debug!("Add `QuoteTick`[{}] {}", quotes.len(), instrument_id);
        let quotes_deque = self
            .quotes
            .entry(instrument_id)
            .or_insert_with(|| VecDeque::with_capacity(self.config.tick_capacity));

        for quote in quotes {
            quotes_deque.push_front(*quote);
        }
        Ok(())
    }

    pub fn add_trade(&mut self, trade: TradeTick) -> anyhow::Result<()> {
        debug!("Add `TradeTick` {}", trade.instrument_id);
        let trades_deque = self
            .trades
            .entry(trade.instrument_id)
            .or_insert_with(|| VecDeque::with_capacity(self.config.tick_capacity));
        trades_deque.push_front(trade);
        Ok(())
    }

    pub fn add_trades(&mut self, trades: &[TradeTick]) -> anyhow::Result<()> {
        check_slice_not_empty(trades, stringify!(trades))?;

        let instrument_id = trades[0].instrument_id;
        debug!("Add `TradeTick`[{}] {}", trades.len(), instrument_id);
        let trades_deque = self
            .trades
            .entry(instrument_id)
            .or_insert_with(|| VecDeque::with_capacity(self.config.tick_capacity));

        for trade in trades {
            trades_deque.push_front(*trade);
        }
        Ok(())
    }

    pub fn add_bar(&mut self, bar: Bar) -> anyhow::Result<()> {
        debug!("Add `Bar` {}", bar.bar_type);
        let bars = self
            .bars
            .entry(bar.bar_type)
            .or_insert_with(|| VecDeque::with_capacity(self.config.bar_capacity));
        bars.push_front(bar);
        Ok(())
    }

    pub fn add_bars(&mut self, bars: &[Bar]) -> anyhow::Result<()> {
        check_slice_not_empty(bars, stringify!(bars))?;

        let bar_type = bars[0].bar_type;
        debug!("Add `Bar`[{}] {}", bars.len(), bar_type);
        let bars_deque = self
            .bars
            .entry(bar_type)
            .or_insert_with(|| VecDeque::with_capacity(self.config.tick_capacity));

        for bar in bars {
            bars_deque.push_front(*bar);
        }
        Ok(())
    }

    pub fn add_currency(&mut self, currency: Currency) -> anyhow::Result<()> {
        debug!("Add `Currency` {}", currency.code);

        if let Some(database) = &self.database {
            database.add_currency(&currency)?;
        }

        self.currencies.insert(currency.code, currency);
        Ok(())
    }

    pub fn add_instrument(&mut self, instrument: InstrumentAny) -> anyhow::Result<()> {
        debug!("Add `Instrument` {}", instrument.id());

        if let Some(database) = &self.database {
            database.add_instrument(&instrument)?;
        }

        self.instruments.insert(instrument.id(), instrument);
        Ok(())
    }

    pub fn add_synthetic(&mut self, synthetic: SyntheticInstrument) -> anyhow::Result<()> {
        debug!("Add `SyntheticInstrument` {}", synthetic.id);

        if let Some(database) = &self.database {
            database.add_synthetic(&synthetic)?;
        }

        self.synthetics.insert(synthetic.id, synthetic);
        Ok(())
    }

    pub fn add_account(&mut self, account: Box<dyn Account>) -> anyhow::Result<()> {
        debug!("Add `Account` {}", account.id());

        if let Some(database) = &self.database {
            database.add_account(account.as_ref())?;
        }

        self.accounts.insert(account.id(), account);
        Ok(())
    }

    /// Add the order to the cache indexed with any given identifiers.
    ///
    /// # Parameters
    ///
    /// `override_existing`: If the added order should 'override' any existing order and replace
    /// it in the cache. This is currently used for emulated orders which are
    /// being released and transformed into another type.
    ///
    /// # Errors
    ///
    /// If not `replace_existing` and the `order.client_order_id` is already contained in the cache.
    pub fn add_order(
        &mut self,
        order: OrderAny,
        _position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        replace_existing: bool,
    ) -> anyhow::Result<()> {
        let instrument_id = order.instrument_id();
        let venue = instrument_id.venue;
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let exec_algorithm_id = order.exec_algorithm_id();
        let _exec_spawn_id = order.exec_spawn_id();

        if !replace_existing {
            check_key_not_in_map(
                &client_order_id,
                &self.orders,
                stringify!(client_order_id),
                stringify!(orders),
            )?;
            check_key_not_in_map(
                &client_order_id,
                &self.orders,
                stringify!(client_order_id),
                stringify!(orders),
            )?;
            check_key_not_in_map(
                &client_order_id,
                &self.orders,
                stringify!(client_order_id),
                stringify!(orders),
            )?;
            check_key_not_in_map(
                &client_order_id,
                &self.orders,
                stringify!(client_order_id),
                stringify!(orders),
            )?;
        };

        debug!("Added {:?}", order);

        self.index.orders.insert(client_order_id);
        self.index
            .order_strategy
            .insert(client_order_id, strategy_id);
        self.index.strategies.insert(strategy_id);

        // Update venue -> orders index
        if let Some(venue_orders) = self.index.venue_orders.get_mut(&venue) {
            venue_orders.insert(client_order_id);
        } else {
            let mut new_set = HashSet::new();
            new_set.insert(client_order_id);
            self.index.venue_orders.insert(venue, new_set);
        }

        // Update instrument -> orders index
        if let Some(instrument_orders) = self.index.instrument_orders.get_mut(&instrument_id) {
            instrument_orders.insert(client_order_id);
        } else {
            let mut new_set = HashSet::new();
            new_set.insert(client_order_id);
            self.index.instrument_orders.insert(instrument_id, new_set);
        }

        // Update strategy -> orders index
        if let Some(strategy_orders) = self.index.strategy_orders.get_mut(&strategy_id) {
            strategy_orders.insert(client_order_id);
        } else {
            let mut new_set = HashSet::new();
            new_set.insert(client_order_id);
            self.index.strategy_orders.insert(strategy_id, new_set);
        }

        // Update exec_algorithm -> orders index
        if let Some(exec_algorithm_id) = exec_algorithm_id {
            self.index.exec_algorithms.insert(exec_algorithm_id);

            if let Some(exec_algorithm_orders) =
                self.index.exec_algorithm_orders.get_mut(&exec_algorithm_id)
            {
                exec_algorithm_orders.insert(client_order_id);
            } else {
                let mut new_set = HashSet::new();
                new_set.insert(client_order_id);
                self.index
                    .exec_algorithm_orders
                    .insert(exec_algorithm_id, new_set);
            }

            // TODO: Implement
            // if let Some(exec_spawn_orders) = self.index.exec_spawn_orders.get_mut(&exec_spawn_id) {
            //     exec_spawn_orders.insert(client_order_id.clone());
            // } else {
            //     let mut new_set = HashSet::new();
            //     new_set.insert(client_order_id.clone());
            //     self.index.exec_spawn_orders.insert(exec_spawn_id, new_set);
            // }
        }

        // TODO: Change emulation trigger setup
        // Update emulation index
        // match order.emulation_trigger() {
        //     TriggerType::NoTrigger => {
        //         self.index.orders_emulated.remove(&client_order_id);
        //     }
        //     _ => {
        //         self.index.orders_emulated.insert(client_order_id.clone());
        //     }
        // }

        // TODO: Implement
        // Index position ID if provided
        // if let Some(position_id) = position_id {
        //     self.add_position_id(
        //         position_id,
        //         order.instrument_id().venue,
        //         client_order_id.clone(),
        //         strategy_id,
        //     );
        // }

        // Index client ID if provided
        if let Some(client_id) = client_id {
            self.index.order_client.insert(client_order_id, client_id);
            log::debug!("Indexed {:?}", client_id);
        }

        // Update database if available
        if let Some(database) = &mut self.database {
            database.add_order(&order)?;
            // TODO: Implement
            // if self.config.snapshot_orders {
            //     database.snapshot_order_state(order)?;
            // }
        }

        self.orders.insert(client_order_id, order);

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

    #[must_use]
    pub fn actor_ids(&self) -> HashSet<ComponentId> {
        self.index.actors.clone()
    }

    #[must_use]
    pub fn strategy_ids(&self) -> HashSet<StrategyId> {
        self.index.strategies.clone()
    }

    #[must_use]
    pub fn exec_algorithm_ids(&self) -> HashSet<ExecAlgorithmId> {
        self.index.exec_algorithms.clone()
    }

    // -- ORDER QUERIES ---------------------------------------------------------------------------

    #[must_use]
    pub fn order(&self, client_order_id: &ClientOrderId) -> Option<&OrderAny> {
        self.orders.get(client_order_id)
    }

    #[must_use]
    pub fn client_order_id(&self, venue_order_id: &VenueOrderId) -> Option<&ClientOrderId> {
        self.index.order_ids.get(venue_order_id)
    }

    #[must_use]
    pub fn venue_order_id(&self, client_order_id: &ClientOrderId) -> Option<VenueOrderId> {
        self.orders
            .get(client_order_id)
            .and_then(nautilus_model::polymorphism::GetVenueOrderId::venue_order_id)
    }

    #[must_use]
    pub fn client_id(&self, client_order_id: &ClientOrderId) -> Option<&ClientId> {
        self.index.order_client.get(client_order_id)
    }

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

    #[must_use]
    pub fn orders_for_position(&self, position_id: PositionId) -> Vec<&OrderAny> {
        let client_order_ids = self.index.position_orders.get(&position_id);
        match client_order_ids {
            Some(client_order_ids) => {
                self.get_orders_for_ids(&client_order_ids.iter().copied().collect(), None)
            }
            None => Vec::new(),
        }
    }

    #[must_use]
    pub fn order_exists(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders.contains(client_order_id)
    }

    #[must_use]
    pub fn is_order_open(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_open.contains(client_order_id)
    }

    #[must_use]
    pub fn is_order_closed(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_closed.contains(client_order_id)
    }

    #[must_use]
    pub fn is_order_emulated(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_emulated.contains(client_order_id)
    }

    #[must_use]
    pub fn is_order_inflight(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_inflight.contains(client_order_id)
    }

    #[must_use]
    pub fn is_order_pending_cancel_local(&self, client_order_id: &ClientOrderId) -> bool {
        self.index.orders_pending_cancel.contains(client_order_id)
    }

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

    #[must_use]
    pub fn order_list(&self, order_list_id: &OrderListId) -> Option<&OrderList> {
        self.order_lists.get(order_list_id)
    }

    #[must_use]
    pub fn order_lists(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
    ) -> Vec<&OrderList> {
        let mut order_lists = self.order_lists.values().collect::<Vec<&OrderList>>();

        if let Some(venue) = venue {
            order_lists.retain(|ol| ol.instrument_id.venue == *venue);
        }

        if let Some(instrument_id) = instrument_id {
            order_lists.retain(|ol| &ol.instrument_id == instrument_id);
        }

        if let Some(strategy_id) = strategy_id {
            order_lists.retain(|ol| &ol.strategy_id == strategy_id);
        }

        order_lists
    }

    #[must_use]
    pub fn order_list_exists(&self, order_list_id: &OrderListId) -> bool {
        self.order_lists.contains_key(order_list_id)
    }

    // -- EXEC ALGORITHM QUERIES ------------------------------------------------------------------

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

    #[must_use]
    pub fn position(&self, position_id: &PositionId) -> Option<&Position> {
        self.positions.get(position_id)
    }

    #[must_use]
    pub fn position_for_order(&self, client_order_id: &ClientOrderId) -> Option<&Position> {
        self.index
            .order_position
            .get(client_order_id)
            .and_then(|position_id| self.positions.get(position_id))
    }

    #[must_use]
    pub fn position_id(&self, client_order_id: &ClientOrderId) -> Option<&PositionId> {
        self.index.order_position.get(client_order_id)
    }

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

    #[must_use]
    pub fn position_exists(&self, position_id: &PositionId) -> bool {
        self.index.positions.contains(position_id)
    }

    #[must_use]
    pub fn is_position_open(&self, position_id: &PositionId) -> bool {
        self.index.positions_open.contains(position_id)
    }

    #[must_use]
    pub fn is_position_closed(&self, position_id: &PositionId) -> bool {
        self.index.positions_closed.contains(position_id)
    }

    #[must_use]
    pub fn positions_open_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<PositionSide>,
    ) -> u64 {
        self.positions_open(venue, instrument_id, strategy_id, side)
            .len() as u64
    }

    #[must_use]
    pub fn positions_closed_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<PositionSide>,
    ) -> u64 {
        self.positions_closed(venue, instrument_id, strategy_id, side)
            .len() as u64
    }

    #[must_use]
    pub fn positions_total_count(
        &self,
        venue: Option<&Venue>,
        instrument_id: Option<&InstrumentId>,
        strategy_id: Option<&StrategyId>,
        side: Option<PositionSide>,
    ) -> u64 {
        self.positions(venue, instrument_id, strategy_id, side)
            .len() as u64
    }

    // -- STRATEGY QUERIES ------------------------------------------------------------------------

    #[must_use]
    pub fn strategy_id_for_order(&self, client_order_id: &ClientOrderId) -> Option<&StrategyId> {
        self.index.order_strategy.get(client_order_id)
    }

    #[must_use]
    pub fn strategy_id_for_position(&self, position_id: &PositionId) -> Option<&StrategyId> {
        self.index.position_strategy.get(position_id)
    }

    // -- GENERAL ---------------------------------------------------------------------------------

    pub fn get(&self, key: &str) -> anyhow::Result<Option<&[u8]>> {
        check_valid_string(key, stringify!(key))?;

        Ok(self.general.get(key).map(std::vec::Vec::as_slice))
    }

    // -- DATA QUERIES ----------------------------------------------------------------------------

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
                    .expect("Error calculating mid price")
                })
            }),
            PriceType::Last => self
                .trades
                .get(instrument_id)
                .and_then(|trades| trades.front().map(|trade| trade.price)),
        }
    }

    #[must_use]
    pub fn quote_ticks(&self, instrument_id: &InstrumentId) -> Option<Vec<QuoteTick>> {
        self.quotes
            .get(instrument_id)
            .map(|quotes| quotes.iter().copied().collect())
    }

    #[must_use]
    pub fn trade_ticks(&self, instrument_id: &InstrumentId) -> Option<Vec<TradeTick>> {
        self.trades
            .get(instrument_id)
            .map(|trades| trades.iter().copied().collect())
    }

    #[must_use]
    pub fn bars(&self, bar_type: &BarType) -> Option<Vec<Bar>> {
        self.bars
            .get(bar_type)
            .map(|bars| bars.iter().copied().collect())
    }

    #[must_use]
    pub fn order_book(&self, instrument_id: &InstrumentId) -> Option<&OrderBook> {
        self.books.get(instrument_id)
    }

    #[must_use]
    pub fn quote_tick(&self, instrument_id: &InstrumentId) -> Option<&QuoteTick> {
        self.quotes
            .get(instrument_id)
            .and_then(|quotes| quotes.front())
    }

    #[must_use]
    pub fn trade_tick(&self, instrument_id: &InstrumentId) -> Option<&TradeTick> {
        self.trades
            .get(instrument_id)
            .and_then(|trades| trades.front())
    }

    #[must_use]
    pub fn bar(&self, bar_type: &BarType) -> Option<&Bar> {
        self.bars.get(bar_type).and_then(|bars| bars.front())
    }

    #[must_use]
    pub fn book_update_count(&self, instrument_id: &InstrumentId) -> u64 {
        self.books.get(instrument_id).map_or(0, |book| book.count)
    }

    #[must_use]
    pub fn quote_tick_count(&self, instrument_id: &InstrumentId) -> u64 {
        self.quotes
            .get(instrument_id)
            .map_or(0, std::collections::VecDeque::len) as u64
    }

    #[must_use]
    pub fn trade_tick_count(&self, instrument_id: &InstrumentId) -> u64 {
        self.trades
            .get(instrument_id)
            .map_or(0, std::collections::VecDeque::len) as u64
    }

    #[must_use]
    pub fn bar_count(&self, bar_type: &BarType) -> u64 {
        self.bars
            .get(bar_type)
            .map_or(0, std::collections::VecDeque::len) as u64
    }

    #[must_use]
    pub fn has_order_book(&self, instrument_id: &InstrumentId) -> bool {
        self.books.contains_key(instrument_id)
    }

    #[must_use]
    pub fn has_quote_ticks(&self, instrument_id: &InstrumentId) -> bool {
        self.quote_tick_count(instrument_id) > 0
    }

    #[must_use]
    pub fn has_trade_ticks(&self, instrument_id: &InstrumentId) -> bool {
        self.trade_tick_count(instrument_id) > 0
    }

    #[must_use]
    pub fn has_bars(&self, bar_type: &BarType) -> bool {
        self.bar_count(bar_type) > 0
    }

    // -- INSTRUMENT QUERIES ----------------------------------------------------------------------

    #[must_use]
    pub fn instrument(&self, instrument_id: &InstrumentId) -> Option<&InstrumentAny> {
        self.instruments.get(instrument_id)
    }

    #[must_use]
    pub fn instrument_ids(&self, venue: &Venue) -> Vec<&InstrumentId> {
        self.instruments
            .keys()
            .filter(|i| &i.venue == venue)
            .collect()
    }

    #[must_use]
    pub fn instruments(&self, venue: &Venue) -> Vec<&InstrumentAny> {
        self.instruments
            .values()
            .filter(|i| &i.id().venue == venue)
            .collect()
    }

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
            .filter(|bar_type| bar_type.aggregation_source == aggregation_source)
            .collect::<Vec<&BarType>>();

        if let Some(instrument_id) = instrument_id {
            bar_types.retain(|bar_type| &bar_type.instrument_id == instrument_id);
        }

        if let Some(price_type) = price_type {
            bar_types.retain(|bar_type| &bar_type.spec.price_type == price_type);
        }

        bar_types
    }

    // -- SYNTHETIC QUERIES -----------------------------------------------------------------------

    #[must_use]
    pub fn synthetic(&self, instrument_id: &InstrumentId) -> Option<&SyntheticInstrument> {
        self.synthetics.get(instrument_id)
    }

    #[must_use]
    pub fn synthetic_ids(&self) -> Vec<&InstrumentId> {
        self.synthetics.keys().collect()
    }

    #[must_use]
    pub fn synthetics(&self) -> Vec<&SyntheticInstrument> {
        self.synthetics.values().collect()
    }

    // -- ACCOUNT QUERIES -----------------------------------------------------------------------

    #[must_use]
    pub fn account(&self, account_id: &AccountId) -> Option<&dyn Account> {
        self.accounts
            .get(account_id)
            .map(std::convert::AsRef::as_ref)
    }

    #[must_use]
    pub fn account_for_venue(&self, venue: &Venue) -> Option<&dyn Account> {
        self.index
            .venue_account
            .get(venue)
            .and_then(|account_id| self.accounts.get(account_id))
            .map(std::convert::AsRef::as_ref)
    }

    #[must_use]
    pub fn account_id(&self, venue: &Venue) -> Option<&AccountId> {
        self.index.venue_account.get(venue)
    }

    #[must_use]
    pub fn accounts(&self, account_id: &AccountId) -> Vec<&dyn Account> {
        self.accounts
            .values()
            .map(std::convert::AsRef::as_ref)
            .collect()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::*;

    use super::Cache;

    #[rstest]
    fn test_reset_index() {
        let mut cache = Cache::default();
        cache.clear_index();
    }

    #[rstest]
    fn test_reset() {
        let mut cache = Cache::default();
        cache.reset();
    }

    #[rstest]
    fn test_dispose() {
        let cache = Cache::default();
        let result = cache.dispose();
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_flushdb() {
        let cache = Cache::default();
        let result = cache.flush_db();
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_general_when_no_value() {
        let cache = Cache::default();
        let result = cache.get("A").unwrap();
        assert_eq!(result, None);
    }

    #[rstest]
    fn test_general_when_value() {
        let mut cache = Cache::default();

        let key = "A";
        let value = vec![0_u8];
        cache.add(key, value.clone()).unwrap();

        let result = cache.get(key).unwrap();
        assert_eq!(result, Some(&value.as_slice()).copied());
    }
}
