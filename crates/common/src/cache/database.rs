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

//! Provides a `Cache` database backing.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use ahash::AHashMap;
use bytes::Bytes;
use nautilus_core::UnixNanos;
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, DataType, GreeksData, QuoteTick, TradeTick, YieldCurveData},
    events::{OrderEventAny, OrderSnapshot, position::snapshot::PositionSnapshot},
    identifiers::{
        AccountId, ClientId, ClientOrderId, ComponentId, InstrumentId, PositionId, StrategyId,
        VenueOrderId,
    },
    instruments::{InstrumentAny, SyntheticInstrument},
    orderbook::OrderBook,
    orders::OrderAny,
    position::Position,
    types::Currency,
};
use ustr::Ustr;

use crate::{custom::CustomData, signal::Signal};

#[derive(Debug, Default)]
pub struct CacheMap {
    pub currencies: AHashMap<Ustr, Currency>,
    pub instruments: AHashMap<InstrumentId, InstrumentAny>,
    pub synthetics: AHashMap<InstrumentId, SyntheticInstrument>,
    pub accounts: AHashMap<AccountId, AccountAny>,
    pub orders: AHashMap<ClientOrderId, OrderAny>,
    pub positions: AHashMap<PositionId, Position>,
    pub greeks: AHashMap<InstrumentId, GreeksData>,
    pub yield_curves: AHashMap<String, YieldCurveData>,
}

#[async_trait::async_trait]
pub trait CacheDatabaseAdapter {
    /// Closes the cache database connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the database fails to close properly.
    fn close(&mut self) -> anyhow::Result<()>;

    /// Flushes any pending changes to the database.
    ///
    /// # Errors
    ///
    /// Returns an error if flushing changes fails.
    fn flush(&mut self) -> anyhow::Result<()>;

    /// Loads all cached data into memory.
    ///
    /// # Errors
    ///
    /// Returns an error if loading data from the database fails.
    async fn load_all(&self) -> anyhow::Result<CacheMap>;

    /// Loads raw key-value data from the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the load operation fails.
    fn load(&self) -> anyhow::Result<AHashMap<String, Bytes>>;

    /// Loads all currencies from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading currencies fails.
    async fn load_currencies(&self) -> anyhow::Result<AHashMap<Ustr, Currency>>;

    /// Loads all instruments from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading instruments fails.
    async fn load_instruments(&self) -> anyhow::Result<AHashMap<InstrumentId, InstrumentAny>>;

    /// Loads all synthetic instruments from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading synthetic instruments fails.
    async fn load_synthetics(&self) -> anyhow::Result<AHashMap<InstrumentId, SyntheticInstrument>>;

    /// Loads all accounts from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading accounts fails.
    async fn load_accounts(&self) -> anyhow::Result<AHashMap<AccountId, AccountAny>>;

    /// Loads all orders from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading orders fails.
    async fn load_orders(&self) -> anyhow::Result<AHashMap<ClientOrderId, OrderAny>>;

    /// Loads all positions from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading positions fails.
    async fn load_positions(&self) -> anyhow::Result<AHashMap<PositionId, Position>>;

    /// Loads all [`GreeksData`] from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading greeks data fails.
    async fn load_greeks(&self) -> anyhow::Result<AHashMap<InstrumentId, GreeksData>> {
        Ok(AHashMap::new())
    }

    /// Loads all [`YieldCurveData`] from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if loading yield curve data fails.
    async fn load_yield_curves(&self) -> anyhow::Result<AHashMap<String, YieldCurveData>> {
        Ok(AHashMap::new())
    }

    /// Loads mapping from order IDs to position IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the index order-position mapping fails.
    fn load_index_order_position(&self) -> anyhow::Result<AHashMap<ClientOrderId, Position>>;

    /// Loads mapping from order IDs to client IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the index order-client mapping fails.
    fn load_index_order_client(&self) -> anyhow::Result<AHashMap<ClientOrderId, ClientId>>;

    /// Loads a single currency by code.
    ///
    /// # Errors
    ///
    /// Returns an error if loading a single currency fails.
    async fn load_currency(&self, code: &Ustr) -> anyhow::Result<Option<Currency>>;

    /// Loads a single instrument by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading a single instrument fails.
    async fn load_instrument(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>>;

    /// Loads a single synthetic instrument by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading a single synthetic instrument fails.
    async fn load_synthetic(
        &self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<SyntheticInstrument>>;

    /// Loads a single account by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading a single account fails.
    async fn load_account(&self, account_id: &AccountId) -> anyhow::Result<Option<AccountAny>>;

    /// Loads a single order by client order ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading a single order fails.
    async fn load_order(&self, client_order_id: &ClientOrderId)
    -> anyhow::Result<Option<OrderAny>>;

    /// Loads a single position by position ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading a single position fails.
    async fn load_position(&self, position_id: &PositionId) -> anyhow::Result<Option<Position>>;

    /// Loads actor state by component ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading actor state fails.
    fn load_actor(&self, component_id: &ComponentId) -> anyhow::Result<AHashMap<String, Bytes>>;

    /// Loads strategy state by strategy ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading strategy state fails.
    fn load_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<AHashMap<String, Bytes>>;

    /// Loads signals by name.
    ///
    /// # Errors
    ///
    /// Returns an error if loading signals fails.
    fn load_signals(&self, name: &str) -> anyhow::Result<Vec<Signal>>;

    /// Loads custom data by data type.
    ///
    /// # Errors
    ///
    /// Returns an error if loading custom data fails.
    fn load_custom_data(&self, data_type: &DataType) -> anyhow::Result<Vec<CustomData>>;

    /// Loads an order snapshot by client order ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the order snapshot fails.
    fn load_order_snapshot(
        &self,
        client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderSnapshot>>;

    /// Loads a position snapshot by position ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading the position snapshot fails.
    fn load_position_snapshot(
        &self,
        position_id: &PositionId,
    ) -> anyhow::Result<Option<PositionSnapshot>>;

    /// Loads quote ticks by instrument ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading quotes fails.
    fn load_quotes(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<QuoteTick>>;

    /// Loads trade ticks by instrument ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading trades fails.
    fn load_trades(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<TradeTick>>;

    /// Loads bars by instrument ID.
    ///
    /// # Errors
    ///
    /// Returns an error if loading bars fails.
    fn load_bars(&self, instrument_id: &InstrumentId) -> anyhow::Result<Vec<Bar>>;

    /// Adds a generic key-value pair to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding a generic key/value fails.
    fn add(&self, key: String, value: Bytes) -> anyhow::Result<()>;

    /// Adds a currency to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding a currency fails.
    fn add_currency(&self, currency: &Currency) -> anyhow::Result<()>;

    /// Adds an instrument to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding an instrument fails.
    fn add_instrument(&self, instrument: &InstrumentAny) -> anyhow::Result<()>;

    /// Adds a synthetic instrument to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding a synthetic instrument fails.
    fn add_synthetic(&self, synthetic: &SyntheticInstrument) -> anyhow::Result<()>;

    /// Adds an account to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding an account fails.
    fn add_account(&self, account: &AccountAny) -> anyhow::Result<()>;

    /// Adds an order to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding an order fails.
    fn add_order(&self, order: &OrderAny, client_id: Option<ClientId>) -> anyhow::Result<()>;

    /// Adds an order snapshot to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding an order snapshot fails.
    fn add_order_snapshot(&self, snapshot: &OrderSnapshot) -> anyhow::Result<()>;

    /// Adds a position to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding a position fails.
    fn add_position(&self, position: &Position) -> anyhow::Result<()>;

    /// Adds a position snapshot to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding a position snapshot fails.
    fn add_position_snapshot(&self, snapshot: &PositionSnapshot) -> anyhow::Result<()>;

    /// Adds an order book to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding an order book fails.
    fn add_order_book(&self, order_book: &OrderBook) -> anyhow::Result<()>;

    /// Adds a signal to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding a signal fails.
    fn add_signal(&self, signal: &Signal) -> anyhow::Result<()>;

    /// Adds custom data to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding custom data fails.
    fn add_custom_data(&self, data: &CustomData) -> anyhow::Result<()>;

    /// Adds a quote tick to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding a quote tick fails.
    fn add_quote(&self, quote: &QuoteTick) -> anyhow::Result<()>;

    /// Adds a trade tick to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding a trade tick fails.
    fn add_trade(&self, trade: &TradeTick) -> anyhow::Result<()>;

    /// Adds a bar to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding a bar fails.
    fn add_bar(&self, bar: &Bar) -> anyhow::Result<()>;

    /// Adds greeks data to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding greeks data fails.
    fn add_greeks(&self, greeks: &GreeksData) -> anyhow::Result<()> {
        Ok(())
    }

    /// Adds yield curve data to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding yield curve data fails.
    fn add_yield_curve(&self, yield_curve: &YieldCurveData) -> anyhow::Result<()> {
        Ok(())
    }

    /// Deletes actor state from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if deleting actor state fails.
    fn delete_actor(&self, component_id: &ComponentId) -> anyhow::Result<()>;

    /// Deletes strategy state from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if deleting strategy state fails.
    fn delete_strategy(&self, component_id: &StrategyId) -> anyhow::Result<()>;

    /// Deletes an order from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if deleting an order fails.
    fn delete_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<()>;

    /// Deletes a position from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if deleting a position fails.
    fn delete_position(&self, position_id: &PositionId) -> anyhow::Result<()>;

    /// Deletes an account event from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if deleting account events fails.
    fn delete_account_event(&self, account_id: &AccountId, event_id: &str) -> anyhow::Result<()>;

    /// Indexes a venue order ID with its client order ID.
    ///
    /// # Errors
    ///
    /// Returns an error if indexing venue order ID fails.
    fn index_venue_order_id(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) -> anyhow::Result<()>;

    /// Indexes an order-position mapping.
    ///
    /// # Errors
    ///
    /// Returns an error if indexing order-position mapping fails.
    fn index_order_position(
        &self,
        client_order_id: ClientOrderId,
        position_id: PositionId,
    ) -> anyhow::Result<()>;

    /// Updates actor state in the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if updating actor state fails.
    fn update_actor(&self) -> anyhow::Result<()>;

    /// Updates strategy state in the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if updating strategy state fails.
    fn update_strategy(&self) -> anyhow::Result<()>;

    /// Updates an account in the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if updating an account fails.
    fn update_account(&self, account: &AccountAny) -> anyhow::Result<()>;

    /// Updates an order in the cache with an order event.
    ///
    /// # Errors
    ///
    /// Returns an error if updating an order fails.
    fn update_order(&self, order_event: &OrderEventAny) -> anyhow::Result<()>;

    /// Updates a position in the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if updating a position fails.
    fn update_position(&self, position: &Position) -> anyhow::Result<()>;

    /// Creates a snapshot of order state.
    ///
    /// # Errors
    ///
    /// Returns an error if snapshotting order state fails.
    fn snapshot_order_state(&self, order: &OrderAny) -> anyhow::Result<()>;

    /// Creates a snapshot of position state.
    ///
    /// # Errors
    ///
    /// Returns an error if snapshotting position state fails.
    fn snapshot_position_state(&self, position: &Position) -> anyhow::Result<()>;

    /// Records a heartbeat timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error if heartbeat recording fails.
    fn heartbeat(&self, timestamp: UnixNanos) -> anyhow::Result<()>;
}
